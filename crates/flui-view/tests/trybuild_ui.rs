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
    // `#[diagnostic::on_unimplemented]` on the view traits: the message a
    // framework user sees must name the derive/impl they are missing, not
    // the blanket impl the compiler walked through. The expected stderr also
    // lists the crate's own `View` impls, so adding one to flui-view means
    // regenerating it with `TRYBUILD=overwrite`.
    t.compile_fail("tests/ui/not_a_view.rs");
    // ADR-0074 §5.5: the typed field selector cannot be bypassed.
    t.compile_fail("tests/ui/field_mask_erase_is_private.rs");
    // The sealed-trait snapshot lists every missing `BuildContext` method
    // (E0046), so adding a method to the trait means regenerating it.
    t.compile_fail("tests/ui/build_context_is_sealed.rs");
    // ADR-0085 §2: a read scope yields neither its graph nor its sink.
    t.compile_fail("tests/ui/scope_ref_exposes_no_graph.rs");
    t.compile_fail("tests/ui/depend_on_inherited_fields_needs_token.rs");
    t.compile_fail("tests/ui/inherited_access_mutation_needs_token.rs");
}
