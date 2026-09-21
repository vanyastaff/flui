use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use crate::ui;
use console::style;
use serde_json::json;

/// Execute the format command.
///
/// # Arguments
///
/// * `check` - If true, only check formatting without modifying files
///
/// # Errors
///
/// Returns `CliError::FormattingCheckFailed` if check mode finds unformatted code.
/// Returns `CliError::FormattingFailed` if formatting fails.
pub fn execute(check: bool) -> CliResult<()> {
    ui::intro(style(" flui format ").on_magenta().black())?;

    let mode = if check { "check" } else { "format" };
    ui::info(format!("Mode: {}", style(mode).cyan()))?;

    let mut cmd = CargoCommand::fmt().all();

    if check {
        cmd = cmd.check();
    }

    // Same reasoning as `test`: `cargo fmt --check` lists unformatted files
    // on stdout, which would pollute the NDJSON stream under `--json`.
    let output_style = if ui::is_json() {
        OutputStyle::Captured
    } else {
        OutputStyle::Streaming
    };

    match cmd.output_style(output_style).run() {
        Ok(_) => {
            ui::emit("format.done", &json!({ "ok": true, "checked": check }));
            if check {
                ui::outro(style("Code is properly formatted").green())?;
            } else {
                ui::outro(style("Code formatted successfully").green())?;
            }
            Ok(())
        }
        Err(err) => {
            // The error's own message ("Code is not formatted. Run 'flui
            // format' to fix." for the check path) reaches stderr through
            // main's generic error printer; nothing here needs to repeat it.
            ui::emit("format.done", &json!({ "ok": false, "checked": check }));
            ui::outro_cancel(if check {
                "Code is not formatted"
            } else {
                "Formatting failed"
            })?;
            Err(err)
        }
    }
}
