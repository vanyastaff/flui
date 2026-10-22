//! Compiler diagnostics for platform owner authority and public paths.

mod trybuild_ui {
    #[test]
    fn ui_tests() {
        let cases = trybuild::TestCases::new();
        cases.pass("tests/ui_pass/owner_and_shared_authority.rs");
        cases.pass("tests/ui_pass/input_callback_captures_rc.rs");
        cases.compile_fail("tests/ui/host_table_names_every_position.rs");
        cases.compile_fail("tests/ui/platform_and_host_window_are_sealed.rs");
        cases.compile_fail("tests/ui/owner_platform_is_not_send.rs");
        cases.compile_fail("tests/ui/owner_thread_token_is_private.rs");
        // rustc spells the trait `Default` where its name is unique among the
        // linked crates and `std::default::Default` where it is not, as on
        // Windows: one snapshot per spelling.
        #[cfg(windows)]
        cases.compile_fail("tests/ui/windows/owner_thread_token_has_no_default.rs");
        #[cfg(not(windows))]
        cases.compile_fail("tests/ui/owner_thread_token_has_no_default.rs");
        cases.compile_fail("tests/ui/basic_velocity_tracker_is_retired.rs");
        cases.compile_fail("tests/ui/platform_embedder_is_retired.rs");
        cases.compile_fail("tests/ui/system_timestamp_is_retired.rs");
        cases.compile_fail("tests/ui/timestamp_provider_is_retired.rs");
    }
}
