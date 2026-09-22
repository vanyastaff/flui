use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use crate::ui;
use console::style;
use serde_json::json;

/// Options of `flui test`.
#[derive(Debug, Clone, Default)]
pub(crate) struct TestOptions {
    /// Optional test name filter.
    pub(crate) filter: Option<String>,
    /// Run only unit tests (`--lib`).
    pub(crate) unit: bool,
    /// Run only integration tests (`--tests`).
    pub(crate) integration: bool,
    /// Build the tests in release mode.
    pub(crate) release: bool,
    /// Arguments forwarded to the test harness after `--`.
    pub(crate) harness_args: Vec<String>,
}

/// Execute the test command.
///
/// # Errors
///
/// Returns `CliError::TestsFailed` if any tests fail.
pub(crate) fn execute(options: TestOptions) -> CliResult<()> {
    let TestOptions {
        filter,
        unit,
        integration,
        release,
        harness_args,
    } = options;
    ui::intro(style(" flui test ").on_yellow().black())?;
    ui::emit("test.start", &json!({}));

    let mut cmd = CargoCommand::test();
    if release {
        cmd = cmd.release();
    }
    cmd = cmd.separator_args(harness_args);

    if let Some(ref f) = filter {
        cmd = cmd.filter(f);
        ui::info(format!("Filter: {}", style(f).cyan()))?;
    }

    if unit {
        cmd = cmd.lib_only();
        ui::info("Running unit tests only")?;
    } else if integration {
        cmd = cmd.integration_only();
        ui::info("Running integration tests only")?;
    }

    // Streaming inherits the child's stdout directly — fine in human mode,
    // but under `--json` our stdout must stay pure NDJSON, and `cargo
    // test`'s own harness prints "running N tests" / "test result: ok"
    // straight to stdout. Capture (and drop) it instead; the `test.done`
    // event already carries everything a machine consumer needs.
    let output_style = if ui::is_json() {
        OutputStyle::Captured
    } else {
        OutputStyle::Streaming
    };

    match cmd.output_style(output_style).run() {
        Ok(_) => {
            ui::emit("test.done", &json!({ "ok": true }));
            ui::outro(style("All tests passed").green())?;
            Ok(())
        }
        Err(err) => {
            ui::emit(
                "test.done",
                &json!({ "ok": false, "exit_code": err.exit_code() }),
            );
            ui::outro_cancel("Tests failed")?;
            Err(err)
        }
    }
}
