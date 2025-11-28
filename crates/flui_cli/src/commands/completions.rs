use crate::error::CliResult;
use clap::CommandFactory;
use clap_complete::{generate, Shell};
use console::style;
use std::io;

pub fn execute(shell: Option<Shell>) -> CliResult<()> {
    let shell = if let Some(s) = shell {
        s
    } else {
        // Detect shell from environment
        detect_shell()?
    };

    println!(
        "{}",
        style(format!("Generating completions for {:?}...", shell))
            .green()
            .bold()
    );
    println!();

    let mut cmd = crate::Cli::command();
    let bin_name = "flui";

    generate(shell, &mut cmd, bin_name, &mut io::stdout());

    println!();
    println!("{}", style("✓ Shell completions generated").green().bold());
    println!();
    println!("To install completions:");

    match shell {
        Shell::Bash => {
            println!("  {} Add to ~/.bashrc:", style("1.").cyan());
            println!("     eval \"$(flui completions bash)\"");
            println!();
            println!("  {} Or save to file:", style("2.").cyan());
            println!("     flui completions bash > /etc/bash_completion.d/flui");
        }
        Shell::Zsh => {
            println!("  {} Add to ~/.zshrc:", style("1.").cyan());
            println!("     eval \"$(flui completions zsh)\"");
            println!();
            println!("  {} Or save to file:", style("2.").cyan());
            println!("     flui completions zsh > ~/.zfunc/_flui");
            println!("     # Add to ~/.zshrc: fpath+=~/.zfunc");
        }
        Shell::Fish => {
            println!("  {} Save to file:", style("→").cyan());
            println!("     flui completions fish > ~/.config/fish/completions/flui.fish");
        }
        Shell::PowerShell => {
            println!("  {} Add to profile:", style("1.").cyan());
            println!("     flui completions powershell >> $PROFILE");
            println!();
            println!("  {} Or save to file:", style("2.").cyan());
            println!("     flui completions powershell > flui.ps1");
        }
        Shell::Elvish => {
            println!("  {} Save to file:", style("→").cyan());
            println!("     flui completions elvish > ~/.elvish/lib/flui.elv");
        }
        _ => {
            println!("  Please refer to your shell's documentation for installing completions.");
        }
    }

    Ok(())
}

fn detect_shell() -> CliResult<Shell> {
    // Try to detect from SHELL environment variable
    if let Ok(shell_path) = std::env::var("SHELL") {
        if shell_path.contains("bash") {
            return Ok(Shell::Bash);
        } else if shell_path.contains("zsh") {
            return Ok(Shell::Zsh);
        } else if shell_path.contains("fish") {
            return Ok(Shell::Fish);
        }
    }

    // Try to detect from ComSpec (Windows)
    if let Ok(comspec) = std::env::var("ComSpec") {
        if comspec.contains("powershell") || comspec.contains("pwsh") {
            return Ok(Shell::PowerShell);
        }
    }

    // Default to bash on Unix-like systems, PowerShell on Windows
    #[cfg(unix)]
    return Ok(Shell::Bash);

    #[cfg(windows)]
    return Ok(Shell::PowerShell);

    #[cfg(not(any(unix, windows)))]
    Err(CliError::ShellDetection)
}
