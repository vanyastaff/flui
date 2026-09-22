use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use crate::ui;
use console::style;
use serde_json::json;

/// Execute the analyze command.
///
/// # Arguments
///
/// * `fix` - Automatically fix issues where possible
/// * `pedantic` - Enable pedantic lints
///
/// # Errors
///
/// Returns `CliError::AnalysisFailed` if clippy finds issues.
pub(crate) fn execute(fix: bool, pedantic: bool) -> CliResult<()> {
    ui::intro(style(" flui analyze ").on_blue().black())?;

    let mut cmd = CargoCommand::clippy()
        .workspace()
        .all_targets()
        .deny_warnings();

    if pedantic {
        cmd = cmd.pedantic();
        ui::info(format!("Pedantic mode: {}", style("enabled").cyan()))?;
    }

    if fix {
        cmd = cmd.fix();
        ui::info("Auto-fixing issues...")?;
        // `cargo clippy --fix` refuses to run on a dirty working tree (and we
        // never pass `--allow-dirty` silently — that would hide fixes from
        // review). Say so up front instead of letting cargo's own refusal be
        // the first the user hears of it.
        ui::note(
            "Before it runs",
            "cargo clippy --fix refuses a dirty working tree.\n\
             Commit or `git stash` first if the command below fails.",
        )?;
    }

    // Clippy's diagnostics go to stderr, but capture under `--json` anyway
    // for the same reason as `test`/`format`: stdout must stay pure NDJSON
    // regardless of what a given cargo subcommand happens to do today.
    let output_style = if ui::is_json() {
        OutputStyle::Captured
    } else {
        OutputStyle::Streaming
    };

    match cmd.output_style(output_style).run() {
        Ok(_) => {
            ui::emit("analyze.done", &json!({ "ok": true }));
            ui::outro(style("Analysis complete - no issues found").green())?;
            Ok(())
        }
        Err(err) => {
            ui::emit("analyze.done", &json!({ "ok": false }));
            ui::outro_cancel("Analysis found issues")?;
            Err(err)
        }
    }
}
