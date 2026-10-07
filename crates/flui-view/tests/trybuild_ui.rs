//! Diagnostic snapshots for view compiler contracts.
//!
//! Each negative fixture compares rustc output with its sibling `.stderr`.
//! Inspect the diagnostic before accepting a snapshot change: a missing import
//! or unrelated error is not evidence for the contract the fixture guards.
//! Passing callers under `tests/ui_pass/` exercise valid public paths.

#[test]
fn ui_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/ui_pass/state_and_depth.rs");
    t.compile_fail("tests/ui/writer_source_minting_is_private.rs");
    t.compile_fail("tests/ui/state_cell_is_not_send.rs");
    t.compile_fail("tests/ui/state_cell_is_not_sync.rs");
    t.compile_fail("tests/ui/state_handle_is_not_send.rs");
    t.compile_fail("tests/ui/state_handle_is_not_sync.rs");
    t.compile_fail("tests/ui/element_depth_minting_is_private.rs");
    t.compile_fail("tests/ui/element_depth_rejects_raw_stamping.rs");
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
    // ADR-0086: the typed signal write. The snapshots are the record of what
    // a user sees for each mistake; the pilot record in ADR-0086 §9 grades
    // them.
    t.compile_fail("tests/ui/signal_write_through_build_context.rs");
    t.compile_fail("tests/ui/signal_write_without_a_writer.rs");
    t.compile_fail("tests/ui/unit_closure_where_event_cx_expected.rs");
    t.compile_fail("tests/ui/let_bound_event_closure_without_helper.rs");
    t.compile_fail("tests/ui/writer_escapes_the_dispatch.rs");
}
