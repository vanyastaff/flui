//! `[package.metadata.flui] globals`: the reviewed entries, and what the gate
//! checks about each class.
//!
//! An entry is debt or a grant. Debt names the ADR whose change removes the
//! global (`exit`); a grant is permanent and names ADR-0097 with the class of
//! state it is (`grant`, `class`). The schema is strict: a malformed entry is
//! an error, not a finding, like `edge-exceptions`.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::LazyLock;

use anyhow::{Context, bail};
use regex::Regex;
use serde_json::Value as Json;

use super::scan::{Def, Shape};

/// The only ADR a grant names.
pub(super) const GRANT_ADR: &str = "ADR-0097";

static ADR_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ADR-\d{4}$").expect("BUG: static regex is valid"));

/// The keys an entry may carry.
const KEYS: [&str; 5] = ["item", "exit", "grant", "class", "reason"];

/// A permanent kind of global state (ADR-0097 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Class {
    /// The one thread-local an OS callback without a user-data pointer reads
    /// to find its owner: one in the host, one per platform backend.
    Trampoline,
    /// An atomic ID counter advanced through a reference (`try_update`
    /// behind `&NAME`), never stored, loaded or decremented directly.
    Counter,
    /// Data that never changes once built: no lock, cell or atomic inside,
    /// a write-once wrapper only as the outermost type.
    Immutable,
    /// Mirrors a resource the process has once: an OS registration, the
    /// system clipboard, a GCD queue, tracing's dispatcher, the start instant.
    Process,
    /// A warn-once flag or a debug-only record that no behavior reads.
    Diagnostic,
}

impl Class {
    const ALL: [(&'static str, Self); 5] = [
        ("trampoline", Self::Trampoline),
        ("counter", Self::Counter),
        ("immutable", Self::Immutable),
        ("process", Self::Process),
        ("diagnostic", Self::Diagnostic),
    ];

    fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|&(_, class)| class)
    }

    pub(super) fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, class)| *class == self)
            .map(|&(name, _)| name)
            .expect("BUG: every class is in ALL")
    }
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a global may exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Standing {
    /// Debt: the ADR whose change removes it.
    Exit(String),
    /// Permanent, under ADR-0097.
    Grant(Class),
}

/// One `globals` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) item: String,
    pub(super) standing: Standing,
    pub(super) reason: String,
}

/// Parses `[package.metadata.flui] globals` of the manifest at `rel`; `adrs`
/// are the `ADR-NNNN` numbers with a file under `docs/adr`.
pub(super) fn parse(flui: &Json, rel: &str, adrs: &BTreeSet<String>) -> anyhow::Result<Vec<Entry>> {
    let shape = || {
        format!(
            "{rel}: `globals` must be a list of `{{ item = \"<path>\", exit = \"ADR-NNNN\", \
             reason = \"<text>\" }}` or `{{ item = \"<path>\", grant = \"{GRANT_ADR}\", class = \
             \"<class>\", reason = \"<text>\" }}`"
        )
    };
    let list = match &flui["globals"] {
        Json::Null => return Ok(Vec::new()),
        value => value.as_array().with_context(shape)?,
    };
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for value in list {
        let table = value.as_object().with_context(shape)?;
        if let Some(key) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
            bail!(
                "{rel}: `globals` entry has unknown key `{key}` (known: {})",
                KEYS.join(", ")
            );
        }
        let text = |key: &str| -> anyhow::Result<Option<String>> {
            match table.get(key) {
                None => Ok(None),
                Some(value) => value
                    .as_str()
                    .map(|text| Some(text.to_owned()))
                    .with_context(|| format!("{rel}: `globals` entry `{key}` must be a string")),
            }
        };
        let item = text("item")?
            .filter(|item| !item.trim().is_empty())
            .with_context(|| format!("{rel}: every `globals` entry names its `item`"))?;
        let reason = text("reason")?
            .filter(|reason| !reason.trim().is_empty())
            .with_context(|| {
                format!("{rel}: `globals` entry `{item}` needs a non-empty `reason`")
            })?;
        let standing = match (text("exit")?, text("grant")?, text("class")?) {
            (Some(exit), None, None) => {
                if !ADR_NUMBER.is_match(&exit) || !adrs.contains(&exit) {
                    bail!(
                        "{rel}: `globals` entry `{item}` names exit \"{exit}\", which is not an \
                         ADR with a file under docs/adr"
                    );
                }
                Standing::Exit(exit)
            }
            (Some(_), None, Some(_)) => bail!(
                "{rel}: `globals` entry `{item}` has an `exit` and a `class`: an entry with an exit \
                 is debt, and only a grant has a class"
            ),
            (None, Some(grant), Some(class)) => {
                if grant != GRANT_ADR {
                    bail!(
                        "{rel}: `globals` entry `{item}` grants under \"{grant}\"; grants are made \
                         by {GRANT_ADR}"
                    );
                }
                Standing::Grant(Class::parse(&class).with_context(|| {
                    format!(
                        "{rel}: `globals` entry `{item}` has class \"{class}\"; the classes are {}",
                        Class::ALL.map(|(name, _)| name).join(", ")
                    )
                })?)
            }
            (None, Some(_), None) => {
                bail!("{rel}: `globals` entry `{item}` has a `grant` but no `class`")
            }
            (Some(_), Some(_), _) => bail!(
                "{rel}: `globals` entry `{item}` has both `exit` and `grant`; it is one or the other"
            ),
            (None, None, _) => bail!(
                "{rel}: `globals` entry `{item}` needs an `exit` (the ADR that removes it) or a \
                 `grant` (\"{GRANT_ADR}\", with a `class`)"
            ),
        };
        if !seen.insert(item.clone()) {
            bail!("{rel}: `globals` lists `{item}` twice");
        }
        entries.push(Entry {
            item,
            standing,
            reason,
        });
    }
    Ok(entries)
}

/// Methods that make an atomic more than an ID source.
const NOT_A_COUNTER: [&str; 13] = [
    "store",
    "swap",
    "load",
    "fetch_sub",
    "fetch_and",
    "fetch_or",
    "fetch_xor",
    "fetch_nand",
    "fetch_min",
    "get_mut",
    "into_inner",
    "as_ptr",
    "compare_exchange",
];

/// Types that make state mutable through a shared reference.
const INTERIOR: [&str; 10] = [
    "Cell",
    "RefCell",
    "UnsafeCell",
    "SyncUnsafeCell",
    "OnceCell",
    "Mutex",
    "RwLock",
    "Condvar",
    "Once",
    "Barrier",
];

/// Write-once wrappers, allowed as the outermost type of an immutable value.
const WRITE_ONCE: [&str; 3] = ["OnceLock", "LazyLock", "LazyCell"];

/// Why `defs` do not fit `class`, or `None` when they do. Where a
/// trampoline may live is checked across crates, not here.
pub(super) fn class_problem(class: Class, defs: &[Def]) -> Option<String> {
    defs.iter().find_map(|def| match class {
        Class::Trampoline => (def.shape != Shape::ThreadLocal)
            .then(|| "a trampoline is a `thread_local!` entry".to_owned()),
        Class::Counter => {
            if def.shape != Shape::Static || !def.ty.atomic_int {
                return Some(
                    "a counter is a non-`mut` `static` of an atomic integer type".to_owned(),
                );
            }
            let used: Vec<&str> = def
                .uses
                .iter()
                .flat_map(|uses| uses.methods.iter())
                .map(String::as_str)
                .filter(|method| {
                    NOT_A_COUNTER
                        .iter()
                        .any(|refused| method.starts_with(refused))
                })
                .collect();
            (!used.is_empty()).then(|| {
                format!(
                    "a counter is only advanced, but it is also used with `{}`",
                    used.join("`, `")
                )
            })
        }
        Class::Immutable => {
            if def.shape == Shape::StaticMut {
                return Some("a `static mut` is never immutable".to_owned());
            }
            let interior: Vec<&str> = def
                .ty
                .idents
                .iter()
                .map(String::as_str)
                .filter(|ident| INTERIOR.contains(ident) || ident.starts_with("Atomic"))
                .collect();
            if !interior.is_empty() {
                return Some(format!(
                    "an immutable value holds no `{}`",
                    interior.join("`, `")
                ));
            }
            let wrappers = def
                .ty
                .idents
                .iter()
                .filter(|ident| WRITE_ONCE.contains(&ident.as_str()))
                .count();
            let outer_wrapper = def
                .ty
                .outer
                .as_deref()
                .is_some_and(|outer| WRITE_ONCE.contains(&outer));
            (wrappers > usize::from(outer_wrapper)).then(|| {
                format!(
                    "`{}` is allowed only as the outermost type of an immutable value",
                    WRITE_ONCE.join("`, `")
                )
            })
        }
        Class::Process => None,
        Class::Diagnostic => {
            let flag = def
                .ty
                .outer
                .as_deref()
                .is_some_and(|outer| outer == "Once" || outer == "AtomicBool");
            (!flag && !def.cfg.contains("debug_assertions")).then(|| {
                "a diagnostic is a `Once` or `AtomicBool` flag, or sits under a \
                 `debug_assertions` cfg"
                    .to_owned()
            })
        }
    })
}
