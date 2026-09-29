//! Single-binary consolidation of flui-cli's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `cli_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`). All tests here
//! drive the built `flui` binary in subprocesses via assert_cmd, so
//! consolidation does not change any process-level isolation.
//!
//! Convention: tests that WRITE process-global state live in their own
//! [[test]] target instead — process isolation beats opt-in locking.
//! (flui-cli currently has none; see flui-view's error_view_recovery
//! for the reference case.)

#[path = "cli_build.rs"]
mod cli_build;
#[path = "cli_completions.rs"]
mod cli_completions;
#[path = "cli_create.rs"]
mod cli_create;
#[path = "cli_devices.rs"]
mod cli_devices;
#[path = "cli_doctor.rs"]
mod cli_doctor;
#[path = "cli_errors.rs"]
mod cli_errors;
#[path = "cli_platform.rs"]
mod cli_platform;
#[path = "cli_run.rs"]
mod cli_run;

/// One scenario per subcommand contract. Every row runs, and the failure report
/// names each row that failed. Scenarios that build real Cargo projects share
/// this test so the binary is compiled and the target directory warmed once.
#[test]
fn subcommand_contracts() {
    let cases: &[(&str, fn())] = &[
        (
            "cli_build::desktop_build_outside_a_project_in_json_mode_emits_a_pure_error_event",
            cli_build::desktop_build_outside_a_project_in_json_mode_emits_a_pure_error_event,
        ),
        (
            "cli_build::live_desktop_build_produces_an_artifact_on_disk",
            cli_build::live_desktop_build_produces_an_artifact_on_disk,
        ),
        (
            "cli_completions::completions_bash",
            cli_completions::completions_bash,
        ),
        (
            "cli_create::generated_widget_project_test_passes",
            cli_create::generated_widget_project_test_passes,
        ),
        (
            "cli_create::create_project_default_template_is_counter",
            cli_create::create_project_default_template_is_counter,
        ),
        (
            "cli_create::all_templates_depend_on_the_public_facade_only",
            cli_create::all_templates_depend_on_the_public_facade_only,
        ),
        (
            "cli_create::dry_run_writes_nothing_and_lists_key_files",
            cli_create::dry_run_writes_nothing_and_lists_key_files,
        ),
        (
            "cli_create::json_create_stdout_is_pure_ndjson_ending_in_create_done",
            cli_create::json_create_stdout_is_pure_ndjson_ending_in_create_done,
        ),
        (
            "cli_devices::devices_json_reports_at_least_the_host_desktop",
            cli_devices::devices_json_reports_at_least_the_host_desktop,
        ),
        (
            "cli_doctor::doctor_json_stdout_is_pure_ndjson",
            cli_doctor::doctor_json_stdout_is_pure_ndjson,
        ),
        (
            "cli_errors::create_with_unknown_template_exits_with_usage_error",
            cli_errors::create_with_unknown_template_exits_with_usage_error,
        ),
        (
            "cli_errors::create_with_an_invalid_project_name_is_rejected",
            cli_errors::create_with_an_invalid_project_name_is_rejected,
        ),
        (
            "cli_platform::platform_remove_with_yes_skips_the_prompt_and_updates_the_config",
            cli_platform::platform_remove_with_yes_skips_the_prompt_and_updates_the_config,
        ),
        (
            "cli_run::json_mode_streams_the_app_lifecycle_as_ndjson",
            cli_run::json_mode_streams_the_app_lifecycle_as_ndjson,
        ),
    ];
    let mut failures = Vec::new();
    for (name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            failures.push(format!("case `{name}` failed: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}
