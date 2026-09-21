//! Shell completions generator.
//!
//! Generates shell completion scripts for various shells.

use crate::error::CliResult;
use crate::ui;
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use console::style;
use std::io;
use std::path::Path;

/// Execute the completions command.
///
/// # Arguments
///
/// * `shell` - Target shell (auto-detected if not specified)
///
/// # Errors
///
/// Returns `CliError::ShellDetectionFailed` if shell cannot be detected.
pub fn execute(shell: Option<Shell>) -> CliResult<()> {
    let shell = shell.unwrap_or_else(detect_shell);

    // The completion script is the only thing that may ever reach stdout —
    // everything else (banner, install instructions, closing line) goes to
    // stderr through `ui::`, so `flui completions zsh > _flui` stays a
    // clean script and `--quiet` suppresses all of it.
    ui::intro(style(" flui completions ").on_yellow().black())?;

    let mut cmd = crate::Cli::command();
    if ui::is_json() {
        // Under `--json` stdout is an event stream, so the script travels
        // inside an event instead of being written raw.
        let mut script = Vec::new();
        generate(shell, &mut cmd, "flui", &mut script);
        ui::emit(
            "completions",
            &serde_json::json!({
                "shell": format!("{shell:?}").to_ascii_lowercase(),
                "script": String::from_utf8_lossy(&script),
            }),
        );
        return Ok(());
    }
    generate(shell, &mut cmd, "flui", &mut io::stdout());

    let instructions = installation_instructions(shell);
    ui::note("Installation Instructions", instructions)?;

    ui::outro(format!(
        "Completions generated for {}",
        style(format!("{shell:?}")).cyan()
    ))?;

    Ok(())
}

/// Detect the user's shell from `$SHELL`'s basename.
///
/// Matching the *basename* rather than checking `contains` matters: a
/// wrapper path like `/opt/zsh-bash/bin/fish` contains both "zsh" and
/// "bash" as substrings of directory names that have nothing to do with
/// the shell actually being run.
fn detect_shell() -> Shell {
    if let Ok(shell_path) = std::env::var("SHELL") {
        let basename = Path::new(&shell_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(shell_path.as_str());
        match basename {
            "bash" => return Shell::Bash,
            "zsh" => return Shell::Zsh,
            "fish" => return Shell::Fish,
            "pwsh" | "powershell" => return Shell::PowerShell,
            _ => {}
        }
    }

    if let Ok(comspec) = std::env::var("ComSpec") {
        let basename = Path::new(&comspec)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(comspec.as_str());
        if basename.eq_ignore_ascii_case("powershell") || basename.eq_ignore_ascii_case("pwsh") {
            return Shell::PowerShell;
        }
    }

    #[cfg(unix)]
    return Shell::Bash;

    #[cfg(windows)]
    return Shell::PowerShell;

    #[cfg(not(any(unix, windows)))]
    Shell::Bash
}

/// Installation instructions for the detected shell.
fn installation_instructions(shell: Shell) -> String {
    match shell {
        Shell::Bash => format!(
            "{}\n  {}\n\n{}\n  {}",
            style("Add to ~/.bashrc:").bold(),
            style("eval \"$(flui completions bash)\"").dim(),
            style("Or save to file:").bold(),
            style("flui completions bash > /etc/bash_completion.d/flui").dim(),
        ),
        Shell::Zsh => format!(
            "{}\n  {}\n\n{}\n  {}\n  {}",
            style("Add to ~/.zshrc:").bold(),
            style("eval \"$(flui completions zsh)\"").dim(),
            style("Or save to file:").bold(),
            style("flui completions zsh > ~/.zfunc/_flui").dim(),
            style("# Add to ~/.zshrc: fpath+=~/.zfunc").dim(),
        ),
        Shell::Fish => format!(
            "{}\n  {}",
            style("Save to file:").bold(),
            style("flui completions fish > ~/.config/fish/completions/flui.fish").dim(),
        ),
        Shell::PowerShell => format!(
            "{}\n  {}\n\n{}\n  {}",
            style("Add to profile:").bold(),
            style("flui completions powershell >> $PROFILE").dim(),
            style("Or save to file:").bold(),
            style("flui completions powershell > flui.ps1").dim(),
        ),
        Shell::Elvish => format!(
            "{}\n  {}",
            style("Save to file:").bold(),
            style("flui completions elvish > ~/.elvish/lib/flui.elv").dim(),
        ),
        _ => "Please refer to your shell's documentation.".to_string(),
    }
}
