//! trybuild driver for the `tests/ui/` compile-fail corpus.
//!
//! Phase 3 §U34 (SC-014): locks the FR-034 friendly diagnostic at
//! `column!` arity > 16. trybuild matches the captured `.stderr`
//! against the rustc output for each `compile_fail` entry — the
//! match is substring-based, so surrounding framing drift (line
//! numbers, file paths) does not regress the test as long as the
//! FR-034 friendly-error substring stays intact.
//!
//! Adding a new ui-test: drop a `.rs` + matching `.stderr` under
//! `tests/ui/` and add the path here. Regenerating `.stderr` after
//! an intentional diagnostic change: set the `TRYBUILD=overwrite`
//! environment variable before running this test (`TRYBUILD=overwrite
//! cargo test -p flui-view --test trybuild_ui`).

#[test]
fn ui_tests() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/column_17_compile_error.rs");
}
