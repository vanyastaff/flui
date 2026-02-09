use crate::error::{CliError, CliResult};
use std::path::Path;
use std::process::Command;

// Re-export scaffolding from flui-build where the logic belongs.
// Used by platform add/remove commands (Phase 4).
#[expect(unused_imports, reason = "re-exports for Phase 4 platform commands")]
pub use flui_build::scaffold::{
    is_valid_platform, scaffold_platform, valid_platform_names, ScaffoldParams,
};

/// Check if a command exists in PATH.
#[expect(dead_code, reason = "utility functions for future commands")]
pub fn command_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

/// Run a command and capture output.
#[expect(dead_code, reason = "utility functions for future commands")]
pub fn run_command(cmd: &str, args: &[&str], cwd: Option<&Path>) -> CliResult<String> {
    let mut command = Command::new(cmd);
    command.args(args);

    if let Some(dir) = cwd {
        command.current_dir(dir);
    }

    let output = command.output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(CliError::CommandFailed {
            context: format!("Command failed: {stderr}"),
            exit_code: output.status.code(),
        })
    }
}

/// Copy directory recursively.
#[allow(dead_code, reason = "utility functions for future commands")]
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> CliResult<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
