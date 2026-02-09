//! Run command for executing FLUI applications.
//!
//! Wraps `cargo run` with hot reload support and device selection.
//! Hot reload uses `notify-debouncer-mini` to watch `src/` and `Cargo.toml`
//! for changes, then kills and restarts the application.

use crate::error::{CliError, CliResult, ResultExt};
use crate::runner::{CargoCommand, OutputStyle};
use console::style;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Execute the run command.
///
/// When `hot_reload` is true (and not in release mode), watches `src/` and
/// `Cargo.toml` for changes and rebuilds/restarts the app automatically.
pub fn execute(
    device: Option<String>,
    release: bool,
    hot_reload: bool,
    profile: Option<String>,
    verbose: bool,
) -> CliResult<()> {
    let mode = if release { "release" } else { "debug" };
    cliclack::intro(style(" flui run ").on_green().black())?;
    cliclack::log::info(format!("Mode: {}", style(mode).cyan()))?;

    // Check if in FLUI project
    ensure_flui_project()?;

    // Select device
    let target_device = device.map_or_else(select_default_device, Ok)?;
    cliclack::log::info(format!("Target device: {}", style(&target_device).cyan()))?;

    if hot_reload && !release {
        cliclack::log::success("Hot reload enabled")?;
        watch_and_rebuild(profile.as_deref(), verbose)?;
    } else {
        run_once(release, profile, verbose)?;
    }

    cliclack::outro(style("Application finished").green())?;
    Ok(())
}

/// Run the app once without hot reload.
fn run_once(release: bool, profile: Option<String>, verbose: bool) -> CliResult<()> {
    let mut cmd = CargoCommand::run_app();

    if release {
        cmd = cmd.release();
    } else if let Some(prof) = profile {
        cmd = cmd.profile(prof);
    }

    if verbose {
        cmd = cmd.verbose();
    }

    cliclack::log::step("Building and running...")?;
    let _ = cmd.output_style(OutputStyle::Streaming).run()?;
    Ok(())
}

/// Watch for file changes and rebuild/restart the application.
///
/// Uses `notify-debouncer-mini` with 500ms debounce. On change:
/// 1. Kill current child process
/// 2. Run `cargo build`
/// 3. Spawn new `cargo run` child
fn watch_and_rebuild(profile: Option<&str>, verbose: bool) -> CliResult<()> {
    use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};

    // Initial build.
    cliclack::log::step("Building project...")?;
    let build_ok = run_cargo_build(profile, verbose);
    if !build_ok {
        return Err(CliError::BuildFailed {
            platform: "desktop".to_string(),
            details: "Initial build failed".to_string(),
        });
    }

    // Spawn the app.
    let mut child = spawn_app(profile, verbose)?;
    cliclack::log::success(format!("Application started (PID {})", child.id()))?;
    cliclack::log::info("Watching src/ for changes...")?;

    // Set up file watcher. If this fails, kill the child before propagating.
    let (tx, rx) = mpsc::channel();

    let debouncer_result = new_debouncer(Duration::from_millis(500), tx);
    let mut debouncer = match debouncer_result {
        Ok(d) => d,
        Err(e) => {
            let _ = child.kill();
            wait_with_timeout(&mut child, Duration::from_secs(5));
            return Err(CliError::context(e, "Failed to create file watcher"));
        }
    };

    let watcher = debouncer.watcher();
    if let Err(e) = watcher.watch(
        Path::new("src"),
        notify_debouncer_mini::notify::RecursiveMode::Recursive,
    ) {
        let _ = child.kill();
        wait_with_timeout(&mut child, Duration::from_secs(5));
        return Err(CliError::context(e, "Failed to watch src/"));
    }
    if let Err(e) = watcher.watch(
        Path::new("Cargo.toml"),
        notify_debouncer_mini::notify::RecursiveMode::NonRecursive,
    ) {
        tracing::debug!("Could not watch Cargo.toml: {e}");
    }

    // Watch loop.
    loop {
        // Check if the child exited on its own.
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!("Application exited with: {:?}", status);
                cliclack::log::info("Application exited. Watching for changes to restart...")?;

                // Wait for next file change to restart.
                match rx.recv() {
                    Ok(Ok(events)) => {
                        if events.iter().any(|e| e.kind == DebouncedEventKind::Any) {
                            log_changed_files(&events)?;
                        }
                    }
                    Ok(Err(errors)) => {
                        tracing::warn!("Watch errors: {:?}", errors);
                        continue;
                    }
                    Err(_) => break, // Channel closed
                }

                // Rebuild and respawn.
                if run_cargo_build(profile, verbose) {
                    child = spawn_app(profile, verbose)?;
                    cliclack::log::success(format!("Application restarted (PID {})", child.id()))?;
                } else {
                    cliclack::log::warning(
                        "Build failed. Watching for changes... (fix errors and save to retry)",
                    )?;
                }
                continue;
            }
            Ok(None) => { /* Still running */ }
            Err(e) => {
                tracing::warn!("Error checking child status: {}", e);
            }
        }

        // Wait for file change events with a short timeout so we can poll child status.
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(events)) => {
                if !events.iter().any(|e| e.kind == DebouncedEventKind::Any) {
                    continue;
                }

                log_changed_files(&events)?;

                // Kill current process.
                if let Err(e) = child.kill() {
                    tracing::debug!("Could not kill child process: {e}");
                }
                wait_with_timeout(&mut child, Duration::from_secs(5));

                // Rebuild.
                cliclack::log::step("Rebuilding...")?;
                if run_cargo_build(profile, verbose) {
                    child = spawn_app(profile, verbose)?;
                    cliclack::log::success(format!("Application restarted (PID {})", child.id()))?;
                } else {
                    cliclack::log::warning(
                        "Build failed. Watching for changes... (fix errors and save to retry)",
                    )?;
                    // Wait for next change before trying again — spawn a placeholder.
                    // We'll detect the missing child on next loop iteration.
                    // Use a dummy child that immediately exits.
                    child = spawn_wait_dummy()?;
                }
            }
            Ok(Err(errors)) => {
                tracing::warn!("Watch errors: {:?}", errors);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Normal — just loop back to check child status.
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Cleanup: kill the child if still running.
    let _ = child.kill();
    wait_with_timeout(&mut child, Duration::from_secs(5));

    Ok(())
}

/// Run `cargo build` and return whether it succeeded.
fn run_cargo_build(profile: Option<&str>, verbose: bool) -> bool {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    match cmd.status() {
        Ok(status) => status.success(),
        Err(e) => {
            tracing::error!("Failed to run cargo build: {}", e);
            false
        }
    }
}

/// Spawn the application as a child process.
fn spawn_app(profile: Option<&str>, verbose: bool) -> CliResult<Child> {
    let mut cmd = Command::new("cargo");
    cmd.arg("run");

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    cmd.env("FLUI_HOT_RELOAD", "1");
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    cmd.spawn().context("Failed to spawn application")
}

/// Wait for a child process to exit, with a timeout to prevent infinite blocking.
///
/// If the child does not exit within `timeout`, it is left running. The caller
/// should have already called `child.kill()` before invoking this.
fn wait_with_timeout(child: &mut Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if Instant::now() >= deadline {
                    tracing::warn!(
                        "Child process (PID {}) did not exit within {:?}",
                        child.id(),
                        timeout
                    );
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                tracing::debug!("Error waiting for child: {e}");
                return;
            }
        }
    }
}

/// Spawn a dummy process that exits immediately (used as placeholder after build failure).
fn spawn_wait_dummy() -> CliResult<Child> {
    // On Windows, use `cmd /c exit 0`; on Unix, use `true`.
    #[cfg(windows)]
    let mut cmd = Command::new("cmd");
    #[cfg(windows)]
    cmd.args(["/c", "exit", "0"]);

    #[cfg(not(windows))]
    let mut cmd = Command::new("true");

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn dummy process")
}

/// Log which files changed.
#[expect(
    clippy::unnecessary_wraps,
    reason = "consistent error handling interface"
)]
fn log_changed_files(events: &[notify_debouncer_mini::DebouncedEvent]) -> CliResult<()> {
    for event in events {
        if event.kind == notify_debouncer_mini::DebouncedEventKind::Any {
            let _ = cliclack::log::info(format!(
                "Change detected: {}",
                style(event.path.display()).dim()
            ));
        }
    }
    Ok(())
}

/// Ensure we're in a FLUI project directory.
fn ensure_flui_project() -> CliResult<()> {
    let cargo_toml = Path::new("Cargo.toml");

    if !cargo_toml.exists() {
        return Err(CliError::NotFluiProject {
            reason: "Cargo.toml not found".to_string(),
        });
    }

    // Check for FLUI dependency
    let content = std::fs::read_to_string(cargo_toml)?;
    if !content.contains("flui_app") && !content.contains("flui_widgets") {
        return Err(CliError::NotFluiProject {
            reason: "flui_app or flui_widgets dependency not found in Cargo.toml".to_string(),
        });
    }

    Ok(())
}

/// Select the default device based on host OS.
#[expect(
    clippy::unnecessary_wraps,
    reason = "consistent error handling interface"
)]
fn select_default_device() -> CliResult<String> {
    #[cfg(target_os = "windows")]
    return Ok("Windows Desktop".to_string());

    #[cfg(target_os = "linux")]
    return Ok("Linux Desktop".to_string());

    #[cfg(target_os = "macos")]
    return Ok("macOS Desktop".to_string());

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(CliError::NoDefaultDevice)
}
