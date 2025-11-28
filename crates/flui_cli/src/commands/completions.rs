//! Shell completions generator.
//!
//! Generates shell completion scripts for various shells.

use crate::error::CliResult;
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use console::style;
use std::io;

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
    let shell = shell.map_or_else(detect_shell, Ok)?;

    cliclack::intro(style(" flui completions ").on_yellow().black())?;

    let mut cmd = crate::Cli::command();
    generate(shell, &mut cmd, "flui", &mut io::stdout());

    let instructions = installation_instructions(shell);
    cliclack::note("Installation Instructions", instructions)?;

    cliclack::outro(format!(
        "Completions generated for {}",
        style(format!("{:?}", shell)).cyan()
    ))?;

    Ok(())
}

/// Detect the user's shell from environment variables.
fn detect_shell() -> CliResult<Shell> {
    // Try SHELL environment variable (Unix)
    if let Ok(shell_path) = std::env::var("SHELL") {
        if shell_path.contains("bash") {
            return Ok(Shell::Bash);
        } else if shell_path.contains("zsh") {
            return Ok(Shell::Zsh);
        } else if shell_path.contains("fish") {
            return Ok(Shell::Fish);
        }
    }

    // Try ComSpec (Windows)
    if let Ok(comspec) = std::env::var("ComSpec") {
        if comspec.contains("powershell") || comspec.contains("pwsh") {
            return Ok(Shell::PowerShell);
        }
    }

    // Platform-specific defaults
    #[cfg(unix)]
    return Ok(Shell::Bash);

    #[cfg(windows)]
    return Ok(Shell::PowerShell);

    #[cfg(not(any(unix, windows)))]
    Err(CliError::ShellDetectionFailed)
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
