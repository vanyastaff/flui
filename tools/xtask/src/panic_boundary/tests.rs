//! The rule over in-memory crates: the self-test, and the cases the fixture
//! does not plant.

use super::fixture::Memory;
use super::*;

const BOUNDARY: &str = r"
pub(crate) enum AfterPanic<R> {
    Answer(fn() -> R),
    Abort,
}

pub(crate) fn os_entry<R>(
    entry: &'static str,
    after_panic: AfterPanic<R>,
    body: impl FnOnce() -> R,
) -> R {
    let _ = (entry, after_panic);
    body()
}
";

/// A crate of `lib.rs` (`mod owner; mod feature;`), the boundary and
/// `feature.rs` holding `feature`.
fn crate_with(feature: &str) -> Memory {
    Memory(
        [
            ("c/src/lib.rs", "mod feature;\nmod owner;\n"),
            ("c/src/owner.rs", "pub(crate) mod panic_boundary;\n"),
            ("c/src/owner/panic_boundary.rs", BOUNDARY),
            ("c/src/feature.rs", feature),
        ]
        .into_iter()
        .map(|(rel, text)| (rel.to_owned(), text.to_owned()))
        .collect(),
    )
}

fn findings(feature: &str) -> Vec<(String, &'static str)> {
    check(&crate_with(feature), "c/src/lib.rs")
        .expect("the crate resolves and parses")
        .findings
        .into_iter()
        .map(|finding| (finding.what, finding.kind))
        .collect()
}

fn the_self_test_reports_exactly_the_planted_findings() {
    let (missed, extra) = self_test_diff();
    assert!(missed.is_empty(), "missed: {missed:?}");
    assert!(extra.is_empty(), "false positives: {extra:?}");
}

fn the_fixture_counts_every_kind() {
    let report = check(&fixture::source(), fixture::ROOT).expect("the fixture resolves and parses");
    let count = |kind| {
        report
            .entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .count()
    };
    assert_eq!(
        Kind::ALL.map(count),
        [10, 2, 2, 1, 6],
        "extern fns, Objective-C methods, COM methods, handlers, callbacks"
    );
    assert_eq!(
        report.adapters.len(),
        3,
        "com, Store::com_entry and contained"
    );
}

fn a_crate_without_the_boundary_is_refused() {
    let crate_without = Memory(
        [("c/src/lib.rs", "extern \"C\" fn f() {}\n")]
            .into_iter()
            .map(|(rel, text)| (rel.to_owned(), text.to_owned()))
            .collect(),
    );
    let findings: Vec<_> = check(&crate_without, "c/src/lib.rs")
        .expect("the crate parses")
        .findings
        .into_iter()
        .map(|finding| (finding.what, finding.kind))
        .collect();
    assert_eq!(
        findings,
        [
            ("f".to_owned(), UNGUARDED),
            ("mod owner::panic_boundary".to_owned(), IDENTITY)
        ]
    );
}

fn a_boundary_module_without_the_function_is_refused() {
    let memory = Memory(
        [
            ("c/src/lib.rs", "mod owner;\n"),
            ("c/src/owner.rs", "pub(crate) mod panic_boundary;\n"),
            (
                "c/src/owner/panic_boundary.rs",
                "pub(crate) fn contain() {}\n",
            ),
        ]
        .into_iter()
        .map(|(rel, text)| (rel.to_owned(), text.to_owned()))
        .collect(),
    );
    let findings: Vec<_> = check(&memory, "c/src/lib.rs")
        .expect("the crate parses")
        .findings
        .into_iter()
        .map(|finding| (finding.what, finding.kind))
        .collect();
    assert_eq!(findings, [("fn os_entry".to_owned(), IDENTITY)]);
}

fn an_adapter_defined_twice_is_no_adapter() {
    let found = findings(
        r#"
use crate::owner::panic_boundary::{AfterPanic, os_entry};
extern "C" fn entry() -> u32 { wrap(|| 1) }
fn wrap(body: impl FnOnce() -> u32) -> u32 { os_entry("a", AfterPanic::Answer(|| 0), body) }
mod inner {
    fn wrap(body: impl FnOnce() -> u32) -> u32 { body() }
}
"#,
    );
    assert_eq!(found, [("entry".to_owned(), UNGUARDED)]);
}

fn an_adapter_calls_the_boundary_itself() {
    let found = findings(
        r#"
use crate::owner::panic_boundary::{AfterPanic, os_entry};
extern "C" fn entry() -> u32 { outer(|| 1) }
fn outer(body: impl FnOnce() -> u32) -> u32 { inner(body) }
fn inner(body: impl FnOnce() -> u32) -> u32 { os_entry("a", AfterPanic::Answer(|| 0), body) }
"#,
    );
    assert_eq!(found, [("entry".to_owned(), UNGUARDED)]);
}

fn a_nested_extern_fn_is_an_entry() {
    let found = findings(
        r#"
fn enumerate() {
    unsafe extern "system" fn visit(_window: isize, _state: isize) -> i32 { 1 }
    let _ = visit;
}
"#,
    );
    assert_eq!(found, [("visit".to_owned(), UNGUARDED)]);
}

fn a_glob_of_the_boundary_binds_its_name() {
    let found = findings(
        r#"
use crate::owner::panic_boundary::*;
extern "C" fn entry() -> u32 { os_entry("e", AfterPanic::Answer(|| 0), || 1) }
"#,
    );
    assert!(found.is_empty(), "{found:?}");
}

fn a_bare_name_without_its_import_is_not_the_boundary() {
    let found = findings(
        r#"
extern "C" fn entry() -> u32 { os_entry("e", AfterPanic::Answer(|| 0), || 1) }
"#,
    );
    assert_eq!(found, [("entry".to_owned(), UNGUARDED)]);
}

fn an_entry_under_a_platform_cfg_is_read() {
    let found = findings(
        r#"
#[cfg(any(target_os = "windows", test))]
mod windows {
    extern "system" fn procedure() -> isize { 0 }
}
#[cfg(all(test, target_os = "windows"))]
mod tests {
    extern "system" fn stub() -> isize { 0 }
}
"#,
    );
    assert_eq!(found, [("procedure".to_owned(), UNGUARDED)]);
}

fn a_method_call_on_something_else_is_no_adapter() {
    let found = findings(
        r#"
use crate::owner::panic_boundary::{AfterPanic, os_entry};
struct Store;
impl Store {
    fn guard(&self, body: impl FnOnce() -> u32) -> u32 { os_entry("s", AfterPanic::Answer(|| 0), body) }
}
impl IStore_Impl for Store_Impl {
    fn Read(&self) -> u32 { self.store.guard(|| 1) }
}
"#,
    );
    assert_eq!(found, [("IStore_Impl::Read".to_owned(), UNGUARDED)]);
}

#[test]
fn panic_boundary_contract() {
    crate::table_test::run_table(
        "panic_boundary_contract",
        &[
            (
                "the_self_test_reports_exactly_the_planted_findings",
                the_self_test_reports_exactly_the_planted_findings as fn(),
            ),
            (
                "the_fixture_counts_every_kind",
                the_fixture_counts_every_kind,
            ),
            (
                "a_crate_without_the_boundary_is_refused",
                a_crate_without_the_boundary_is_refused,
            ),
            (
                "a_boundary_module_without_the_function_is_refused",
                a_boundary_module_without_the_function_is_refused,
            ),
            (
                "an_adapter_defined_twice_is_no_adapter",
                an_adapter_defined_twice_is_no_adapter,
            ),
            (
                "an_adapter_calls_the_boundary_itself",
                an_adapter_calls_the_boundary_itself,
            ),
            (
                "a_nested_extern_fn_is_an_entry",
                a_nested_extern_fn_is_an_entry,
            ),
            (
                "a_glob_of_the_boundary_binds_its_name",
                a_glob_of_the_boundary_binds_its_name,
            ),
            (
                "a_bare_name_without_its_import_is_not_the_boundary",
                a_bare_name_without_its_import_is_not_the_boundary,
            ),
            (
                "an_entry_under_a_platform_cfg_is_read",
                an_entry_under_a_platform_cfg_is_read,
            ),
            (
                "a_method_call_on_something_else_is_no_adapter",
                a_method_call_on_something_else_is_no_adapter,
            ),
        ],
    );
}
