//! Compiler diagnostics for the host's runtime authority.

mod trybuild_ui {
    #[test]
    fn ui_tests() {
        let cases = trybuild::TestCases::new();
        cases.pass("tests/ui_pass/host_frame_and_pipeline.rs");
        cases.compile_fail("tests/ui/binding_pipeline_is_not_send.rs");
        cases.compile_fail("tests/ui/render_frame_is_private.rs");
    }
}
