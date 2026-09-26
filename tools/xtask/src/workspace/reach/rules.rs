//! The forbidden sets: the root manifest's `[workspace.metadata.flui.reach]`.
//!
//! ```toml
//! [workspace.metadata.flui.reach]
//! generic-ffi = [{ name = "windows-sys", reason = "…" }, …]
//! [workspace.metadata.flui.reach.tier.K]
//! forbid = ["winit", …]
//! [workspace.metadata.flui.reach.tier.V]
//! extends = "S"
//! forbid = ["tokio"]
//! ```
//!
//! A tier's set is the set of the tier it `extends` plus its `forbid`; a tier
//! with no table forbids nothing. A tier never drops an inherited name: the
//! one crate that may reach one says so with its own `grant` in
//! `reach-exceptions`, which leaves the name forbidden to the rest of its
//! tier. A pattern is a package
//! name or a glob whose only wildcard is `*`, matched against the whole name.
//! A name on the `generic-ffi` allowlist matches no pattern: those crates name
//! no windowing backend (ADR-0081 §2), and a pattern that names one exactly
//! is a configuration error rather than a rule that never fires.

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{Context, bail, ensure};
use serde_json::{Map, Value as Json};

/// A package name, or a glob with `*` as its only wildcard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pattern {
    Exact(String),
    Glob(String),
}

impl Pattern {
    pub(super) fn parse(text: &str) -> anyhow::Result<Self> {
        ensure!(
            !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '*')),
            "`{text}` is not a package name or a `*` glob"
        );
        Ok(if text.contains('*') {
            Self::Glob(text.to_owned())
        } else {
            Self::Exact(text.to_owned())
        })
    }

    fn text(&self) -> &str {
        match self {
            Self::Exact(text) | Self::Glob(text) => text,
        }
    }

    pub(super) fn matches(&self, name: &str) -> bool {
        match self {
            Self::Exact(text) => text == name,
            Self::Glob(text) => glob_matches(text, name),
        }
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

/// Whether `name` is all of `glob`, each `*` standing for any run of
/// characters.
fn glob_matches(glob: &str, name: &str) -> bool {
    let mut parts = glob.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let Some(last) = parts.pop() else {
        // no `*`: the whole name
        return rest.is_empty();
    };
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// The forbidden set of each tier and the generic-FFI allowlist.
#[derive(Debug)]
pub(super) struct Rules {
    tiers: BTreeMap<String, Vec<Pattern>>,
    generic_ffi: Vec<(Pattern, String)>,
}

const REACH_KEYS: [&str; 2] = ["generic-ffi", "tier"];
const TIER_KEYS: [&str; 2] = ["extends", "forbid"];

impl Rules {
    /// Reads `[workspace.metadata.flui.reach]`; `tiers` are the root's tier
    /// names. An unknown key, an unknown or cyclic `extends`, and an exact
    /// pattern naming a generic-FFI crate are errors.
    pub(super) fn from_workspace_metadata(
        workspace_metadata: &Json,
        tiers: &[String],
    ) -> anyhow::Result<Self> {
        let reach = workspace_metadata["flui"]["reach"]
            .as_object()
            .context("Cargo.toml needs `[workspace.metadata.flui.reach]`")?;
        refuse_unknown(reach, &REACH_KEYS, "[workspace.metadata.flui.reach]")?;

        let malformed_ffi = "`[workspace.metadata.flui.reach] generic-ffi` must be a list of \
                             `{ name = \"<package or glob>\", reason = \"<text>\" }`";
        let mut generic_ffi = Vec::new();
        for entry in reach
            .get("generic-ffi")
            .map_or(Some(&[][..]), |list| list.as_array().map(Vec::as_slice))
            .context(malformed_ffi)?
        {
            let table = entry.as_object().context(malformed_ffi)?;
            let field = |key: &str| table.get(key).and_then(Json::as_str);
            let (Some(name), Some(reason), 2) = (field("name"), field("reason"), table.len())
            else {
                bail!(malformed_ffi);
            };
            ensure!(
                !reason.trim().is_empty(),
                "generic-ffi `{name}` needs a reason"
            );
            generic_ffi.push((Pattern::parse(name)?, reason.to_owned()));
        }

        let tables = match reach.get("tier") {
            None => Map::new(),
            Some(value) => value
                .as_object()
                .cloned()
                .context("`[workspace.metadata.flui.reach.tier]` must be a table of tiers")?,
        };
        for (tier, table) in &tables {
            ensure!(
                tiers.contains(tier),
                "`[workspace.metadata.flui.reach.tier.{tier}]` names no tier of `tiers`"
            );
            let table = table
                .as_object()
                .with_context(|| format!("`reach.tier.{tier}` must be a table"))?;
            refuse_unknown(
                table,
                &TIER_KEYS,
                &format!("[workspace.metadata.flui.reach.tier.{tier}]"),
            )?;
        }

        let mut rules = Self {
            tiers: BTreeMap::new(),
            generic_ffi,
        };
        for tier in tiers {
            let set = expand(tier, &tables, &mut Vec::new())?;
            rules.refuse_generic_ffi(&set, &format!("tier {tier}'s forbidden set"))?;
            rules.tiers.insert(tier.clone(), set);
        }
        Ok(rules)
    }

    /// `texts` as patterns: a package's `reach-forbid`.
    pub(super) fn patterns(&self, texts: &[String], whose: &str) -> anyhow::Result<Vec<Pattern>> {
        let patterns = texts
            .iter()
            .map(|text| Pattern::parse(text))
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| format!("{whose}'s `reach-forbid`"))?;
        self.refuse_generic_ffi(&patterns, &format!("{whose}'s `reach-forbid`"))?;
        Ok(patterns)
    }

    fn refuse_generic_ffi(&self, set: &[Pattern], what: &str) -> anyhow::Result<()> {
        for pattern in set {
            if let Pattern::Exact(name) = pattern
                && self.is_generic_ffi(name)
            {
                bail!(
                    "{what} names {name}, which is on the `generic-ffi` allowlist and so never \
                     matches; remove one of the two"
                );
            }
        }
        Ok(())
    }

    /// The expanded set of `tier`; empty for a tier without a table.
    pub(super) fn tier_set(&self, tier: &str) -> &[Pattern] {
        self.tiers.get(tier).map_or(&[], Vec::as_slice)
    }

    /// The generic-FFI allowlist, with each entry's reason.
    #[cfg(test)]
    pub(super) fn generic_ffi(&self) -> &[(Pattern, String)] {
        &self.generic_ffi
    }

    pub(super) fn is_generic_ffi(&self, name: &str) -> bool {
        self.generic_ffi
            .iter()
            .any(|(pattern, _)| pattern.matches(name))
    }

    /// Whether `tier`'s own set forbids `name`.
    pub(super) fn tier_forbids(&self, tier: &str, name: &str) -> bool {
        self.forbidden(tier, &[], name)
    }

    /// Whether `name` is forbidden to a package of `tier` whose
    /// `reach-forbid` is `extra`.
    pub(super) fn forbidden(&self, tier: &str, extra: &[Pattern], name: &str) -> bool {
        !self.is_generic_ffi(name)
            && self
                .tier_set(tier)
                .iter()
                .chain(extra)
                .any(|pattern| pattern.matches(name))
    }
}

fn refuse_unknown(table: &Map<String, Json>, known: &[&str], what: &str) -> anyhow::Result<()> {
    if let Some(key) = table.keys().find(|key| !known.contains(&key.as_str())) {
        bail!(
            "`{what}` has unknown key `{key}` (known: {})",
            known.join(", ")
        );
    }
    Ok(())
}

/// `tier`'s set: its `extends` expanded, plus `forbid`.
/// `visiting` holds the chain being expanded, to refuse a cycle.
fn expand(
    tier: &str,
    tables: &Map<String, Json>,
    visiting: &mut Vec<String>,
) -> anyhow::Result<Vec<Pattern>> {
    let Some(table) = tables.get(tier) else {
        return Ok(Vec::new());
    };
    ensure!(
        !visiting.iter().any(|seen| seen == tier),
        "`reach.tier` `extends` forms a cycle: {} -> {tier}",
        visiting.join(" -> ")
    );
    visiting.push(tier.to_owned());
    let list = |key: &str| -> anyhow::Result<Vec<String>> {
        match table.get(key) {
            None => Ok(Vec::new()),
            Some(value) => value
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_str().map(str::to_owned))
                        .collect()
                })
                .with_context(|| format!("`reach.tier.{tier}.{key}` must be a list of strings")),
        }
    };
    let mut set = match table.get("extends") {
        None => Vec::new(),
        Some(parent) => {
            let parent = parent
                .as_str()
                .with_context(|| format!("`reach.tier.{tier}.extends` must be a tier name"))?;
            ensure!(
                tables.contains_key(parent),
                "`reach.tier.{tier}` extends `{parent}`, which has no `reach.tier` table"
            );
            expand(parent, tables, visiting)?
        }
    };
    for text in list("forbid")? {
        let pattern =
            Pattern::parse(&text).with_context(|| format!("`reach.tier.{tier}.forbid`"))?;
        if !set.contains(&pattern) {
            set.push(pattern);
        }
    }
    visiting.pop();
    Ok(set)
}
