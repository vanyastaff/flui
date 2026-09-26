//! The self-test's crates: source files in memory and their entries, with
//! one planted violation per rule and one silent case per exemption.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::scan::{Source, Target};
use super::{HOST, Krate, PLATFORM};

/// In-memory files, repository-relative.
pub(super) struct Memory(pub(super) BTreeMap<String, String>);

impl Source for Memory {
    fn read(&self, rel: &str) -> Option<String> {
        self.0.get(rel).cloned()
    }
}

/// The ADRs the fixture entries may name.
pub(super) fn adrs() -> BTreeSet<String> {
    ["ADR-0094", "ADR-0097"].map(str::to_owned).into()
}

const HOST_LIB: &str = r#"
use std::sync::atomic::{AtomicU64, Ordering};

mod runner;
#[cfg(test)]
mod t;

// silent: a private counter only ever advanced
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

// silent: immutable data
static NAME: &str = "flui";

// silent: test-only
#[cfg(test)]
static TEST_ONLY: std::sync::Mutex<u8> = std::sync::Mutex::new(0);

// planted: an interior-mutable static
static LOCKED: std::sync::Mutex<u8> = std::sync::Mutex::new(0);

// planted: a `pub` counter, which another crate could store
pub static SHARED_ID: AtomicU64 = AtomicU64::new(1);
pub fn shared_id() -> u64 {
    SHARED_ID.fetch_add(1, Ordering::Relaxed)
}

// planted: an atomic that is stored
static STORED: AtomicU64 = AtomicU64::new(0);
pub fn reset() {
    STORED.store(0, Ordering::Relaxed);
}

// planted: a counter that escapes by reference, with no entry
static BORROWED: AtomicU64 = AtomicU64::new(1);
pub fn borrowed() -> u64 {
    bump(&BORROWED)
}
fn bump(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed)
}

// planted: `test` is false, but the feature may be on
#[cfg(any(test, feature = "x"))]
static FEATURED: std::sync::Mutex<u8> = std::sync::Mutex::new(0);

// planted: a static a macro emits into its callers
macro_rules! emit {
    () => {
        static EMITTED: u8 = 0;
    };
}

// planted: a static inside an unknown macro
some_macro! {
    static HIDDEN: std::cell::Cell<u8> = std::cell::Cell::new(0);
}

// silent: `'static` is a lifetime
other_macro! {
    fn name() -> &'static str { "flui" }
}
"#;

const HOST_RUNNER: &str = r"
thread_local! {
    // listed: the host's trampoline
    static APP_RUNTIME: std::cell::RefCell<Option<u8>> = const { std::cell::RefCell::new(None) };
    // planted: a second host trampoline
    static SECOND_HOST: std::cell::RefCell<u8> = const { std::cell::RefCell::new(0) };
    // planted: a thread-local with no entry
    static QUEUE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
";

/// Declared under `#[cfg(test)]`: never read.
const HOST_TEST_MODULE: &str = r"
static IN_TESTS: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
";

const PLATFORM_LIB: &str = r"
mod platforms;
mod shared;
";

const PLATFORM_BACKEND: &str = r"
thread_local! {
    // listed: this backend's trampoline
    static ACTIVE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
";

const PLATFORM_SHARED: &str = r"
thread_local! {
    // planted: a trampoline outside a backend directory
    static SHARED_CELL: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
";

const THIRD_LIB: &str = r"
thread_local! {
    // planted: a trampoline in a crate that may not have one
    static CLAIMED: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
";

/// The finding each planted violation must produce, and no other.
pub(super) const EXPECTED: [(&str, &str, &str); 13] = [
    (HOST, "LOCKED", super::NEW),
    (HOST, "SHARED_ID", super::NEW),
    (HOST, "STORED", super::NEW),
    (HOST, "BORROWED", super::NEW),
    (HOST, "FEATURED", super::NEW),
    (HOST, "emit!::EMITTED", super::NEW),
    (HOST, "some_macro!::HIDDEN", super::NEW),
    (HOST, "runner::QUEUE", super::NEW),
    (HOST, "runner::SECOND_HOST", super::SECOND_TRAMPOLINE),
    (HOST, "runner::GONE", super::STALE),
    (HOST, "NEXT_ID", super::STALE),
    (PLATFORM, "shared::SHARED_CELL", super::PLACEMENT),
    ("flui-widgets", "CLAIMED", super::PLACEMENT),
];

/// The fixture files and crates.
pub(super) fn crates() -> (Memory, Vec<Krate>) {
    let files = [
        ("crates/flui-app/src/lib.rs", HOST_LIB),
        ("crates/flui-app/src/runner.rs", HOST_RUNNER),
        ("crates/flui-app/src/t.rs", HOST_TEST_MODULE),
        ("crates/flui-platform/src/lib.rs", PLATFORM_LIB),
        ("crates/flui-platform/src/platforms/mod.rs", "mod winit;"),
        (
            "crates/flui-platform/src/platforms/winit/mod.rs",
            PLATFORM_BACKEND,
        ),
        ("crates/flui-platform/src/shared.rs", PLATFORM_SHARED),
        ("crates/flui-widgets/src/lib.rs", THIRD_LIB),
    ]
    .into_iter()
    .map(|(rel, text)| (rel.to_owned(), text.to_owned()))
    .collect();
    let trampoline = |item: &str| json!({ "item": item, "grant": "ADR-0097", "class": "trampoline", "reason": "self-test" });
    let krate = |name: &str, globals: serde_json::Value| Krate {
        name: name.to_owned(),
        manifest: format!("crates/{name}/Cargo.toml"),
        flui: json!({ "globals": globals }),
        targets: vec![Target {
            src: format!("crates/{name}/src/lib.rs"),
            bin: None,
        }],
    };
    let crates = vec![
        krate(
            HOST,
            json!([
                trampoline("runner::APP_RUNTIME"),
                trampoline("runner::SECOND_HOST"),
                { "item": "runner::GONE", "exit": "ADR-0094", "reason": "self-test" },
                { "item": "NEXT_ID", "exit": "ADR-0097", "reason": "self-test" },
            ]),
        ),
        krate(
            PLATFORM,
            json!([
                trampoline("platforms::winit::ACTIVE"),
                trampoline("shared::SHARED_CELL"),
            ]),
        ),
        krate("flui-widgets", json!([trampoline("CLAIMED")])),
    ];
    (Memory(files), crates)
}
