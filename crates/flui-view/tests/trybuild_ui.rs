//! trybuild driver for the `tests/ui/` compile-fail corpus.
//!
//! Locks the FR-034 friendly diagnostic at
//! `column!` arity > 16. trybuild compares each `compile_fail`
//! entry's captured rustc output against the sibling `.stderr`
//! file. The comparison is **whole-output**, not a `contains`
//! substring search; trybuild normalizes a handful of fields
//! (line numbers, file paths, hashes) and supports `...` wildcards
//! inside the `.stderr` snapshot for variance-tolerant matches.
//! The contract is therefore: rustc emits an error block
//! whose first line carries the FR-034 message verbatim, and the
//! `.stderr` snapshot captures the surrounding framing.
//!
//! Adding a new ui-test: drop a `.rs` + matching `.stderr` under
//! `tests/ui/` and add a `t.compile_fail(…)` call below. If the
//! captured rustc framing is brittle across rustc versions or
//! local file paths, replace the variant lines in `.stderr` with
//! the trybuild `...` wildcard so the assertion stays focused on
//! the FR-034 substring.
//!
//! Regenerating `.stderr` after an intentional diagnostic change:
//! set the `TRYBUILD=overwrite` environment variable before running
//! this test (`TRYBUILD=overwrite cargo test -p flui-view --test
//! trybuild_ui`).

#[test]
fn ui_tests() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/column_17_compile_error.rs");
}
