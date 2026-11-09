//! The tier table of `docs/crates.md`, generated from the manifests: the
//! tier names of the root manifest's `[workspace.metadata.flui]`, and each
//! crate's `order` and `tier-kind` from its `[package.metadata.flui]`.
//! `cargo xtask workspace` fails when the committed table differs;
//! `cargo xtask workspace --write-crates-map` regenerates it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Context;
use serde_json::Value as Json;

use super::{Members, tiers};

const BEGIN: &str = "<!-- BEGIN crates-map -->";
const END: &str = "<!-- END crates-map -->";
const DOC: &str = "docs/crates.md";

/// One crate of a tier, as the table prints it.
struct Entry<'a> {
    name: &'a str,
    order: u64,
    kind: &'a str,
    /// An official package under `packages/`, which builds on `flui-sdk`.
    in_packages: bool,
}

/// The table: a row per tier, bottom to top, naming each crate with its
/// `order` and kind.
fn generate(members: &Members, workspace_metadata: &Json) -> anyhow::Result<String> {
    let tier_names = tiers::names(workspace_metadata)?;
    let titles = tiers::titles(workspace_metadata)?;
    let mut by_tier: BTreeMap<&str, Vec<Entry<'_>>> = BTreeMap::new();
    for member in &members.0 {
        if let (Some(tier), Some(order)) = (member.tier.as_deref(), member.order) {
            by_tier.entry(tier).or_default().push(Entry {
                name: member.name(),
                order,
                kind: member.tier_kind.as_deref().unwrap_or_default(),
                in_packages: member.rel.starts_with("packages/"),
            });
        }
    }
    let mut out = String::from(
        "| Tier | Crates, by `order` and `tier-kind` |\n|------|------------------|\n",
    );
    for tier in &tier_names {
        let mut crates = by_tier.remove(tier.as_str()).unwrap_or_default();
        crates.sort_by_key(|entry| entry.order);
        let list = crates.iter().map(entry_text).collect::<Vec<_>>().join(", ");
        let title = &titles[tier];
        writeln!(out, "| {tier} {title} | {list} |").expect("writing to a String never fails");
    }
    Ok(out)
}

fn entry_text(entry: &Entry<'_>) -> String {
    let mut parts = vec![entry.order.to_string()];
    if !entry.kind.is_empty() {
        parts.push(entry.kind.to_owned());
    }
    if entry.in_packages {
        parts.push("under packages/, on `flui-sdk`".to_owned());
    }
    format!("`{}` ({})", entry.name, parts.join(", "))
}

/// The text of `doc` between the markers, and the byte range it occupies.
fn committed(doc: &str) -> anyhow::Result<(std::ops::Range<usize>, &str)> {
    let start = doc
        .find(BEGIN)
        .with_context(|| format!("{DOC} has no `{BEGIN}` marker"))?
        + BEGIN.len();
    let end = doc
        .find(END)
        .with_context(|| format!("{DOC} has no `{END}` marker"))?;
    anyhow::ensure!(start <= end, "{DOC}: `{END}` comes before `{BEGIN}`");
    Ok((start..end, doc[start..end].trim()))
}

/// Verifies `docs/crates.md`'s table agrees with the manifests. A missing
/// file or marker is an error, not a pass: a moved document would otherwise
/// stop being checked.
pub(super) fn check(
    root: &Path,
    members: &Members,
    workspace_metadata: &Json,
    findings: &mut Vec<String>,
) -> anyhow::Result<()> {
    let path = root.join(DOC);
    let doc = std::fs::read_to_string(&path)
        .with_context(|| format!("{DOC} is not readable at {}", path.display()))?;
    let (_, committed) = committed(&doc)?;
    if committed != generate(members, workspace_metadata)?.trim() {
        findings.push(format!(
            "{DOC}'s tier table drifted from the manifests; run `cargo xtask workspace --write-crates-map`"
        ));
    }
    Ok(())
}

/// Regenerates the table in `docs/crates.md`.
pub(super) fn write(
    root: &Path,
    members: &Members,
    workspace_metadata: &Json,
) -> anyhow::Result<()> {
    let path = root.join(DOC);
    let doc = std::fs::read_to_string(&path)
        .with_context(|| format!("{DOC} is not readable at {}", path.display()))?;
    let (range, _) = committed(&doc)?;
    let updated = format!(
        "{}\n{}{}",
        &doc[..range.start],
        generate(members, workspace_metadata)?,
        &doc[range.end..]
    );
    std::fs::write(&path, updated).with_context(|| format!("writing {}", path.display()))
}
