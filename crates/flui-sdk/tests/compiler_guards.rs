//! Compiler diagnostics for the package-author boundary.

mod trybuild_ui {
    #[test]
    fn ui_tests() {
        let cases = trybuild::TestCases::new();
        cases.pass("tests/ui/composition_root_runtime.rs");
        cases.compile_fail("tests/ui/sdk_runtime_is_private.rs");
    }
}
