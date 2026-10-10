//! Compiler diagnostics for the package-author boundary.

mod trybuild_ui {
    #[test]
    fn ui_tests() {
        let cases = trybuild::TestCases::new();
        cases.pass("tests/ui/composition_root_runtime.rs");
        cases.pass("tests/ui/nested_motion.rs");
        cases.compile_fail("tests/ui/sdk_runtime_is_private.rs");
        cases.compile_fail("tests/ui/empty_motion.rs");
        cases.compile_fail("tests/ui/non_motion_field.rs");
        cases.compile_fail("tests/ui/generic_motion_width.rs");
    }
}
