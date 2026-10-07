//! Compiler diagnostics for platform owner authority and public paths.

mod trybuild_ui {
    #[test]
    fn ui_tests() {
        let cases = trybuild::TestCases::new();
        cases.pass("tests/ui_pass/owner_and_shared_authority.rs");
        cases.compile_fail("tests/ui/owner_platform_is_not_send.rs");
        cases.compile_fail("tests/ui/owner_thread_token_is_private.rs");
        cases.compile_fail("tests/ui/owner_thread_token_has_no_default.rs");
        cases.compile_fail("tests/ui/basic_velocity_tracker_is_retired.rs");
        cases.compile_fail("tests/ui/platform_embedder_is_retired.rs");
        cases.compile_fail("tests/ui/system_timestamp_is_retired.rs");
        cases.compile_fail("tests/ui/timestamp_provider_is_retired.rs");
    }
}
