//! Run command for executing FLUI applications.
//!
//! Two hot-reload modes:
//!
//! - **Process restart** (default): watch `src/`, kill + `cargo run` on change.
//! - **Worker host** (`flui.toml` `[hot_reload]`): watch worker UI sources,
//!   rebuild `cdylib` only; host applies `HotReloadTier::HotReload` in-process.

use crate::config::{FluiConfig, HotReloadConfig};
use crate::error::{CliError, CliResult, ResultExt};
use crate::runner::{CargoCommand, OutputStyle};
use console::style;
use flui_hot_reload::{
    dev::SourceWatcher,
    engine::env as worker_env,
    strategy::{env, timing},
};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::RecvTimeoutError;
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
        if let Some(project) = find_worker_hot_reload_project()? {
            cliclack::log::success(format!(
                "Worker hot reload: {} → {}",
                style(&project.config.worker_package).cyan(),
                style(&project.config.host_package).cyan()
            ))?;
            watch_worker_hot_reload(&project, profile.as_deref(), verbose)?;
        } else {
            cliclack::log::success("Hot reload enabled (process restart)")?;
            watch_and_rebuild(profile.as_deref(), verbose)?;
        }
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
fn watch_and_rebuild(profile: Option<&str>, verbose: bool) -> CliResult<()> {
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

    let mut watcher = SourceWatcher::new().map_err(|e| CliError::context(e, "Failed to create file watcher"))?;

    if let Err(e) = watcher.watch(Path::new("src"), true) {
        let _ = child.kill();
        wait_with_timeout(&mut child, Duration::from_secs(5));
        return Err(CliError::context(e, "Failed to watch src/"));
    }
    if let Err(e) = watcher.watch(Path::new("Cargo.toml"), false) {
        tracing::debug!("Could not watch Cargo.toml: {e}");
    }

    // Watch loop.
    loop {
        // Check if the child exited on its own.
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!("Application exited with: {:?}", status);
                cliclack::log::info("Application exited. Watching for changes to restart...")?;

                if let Some(paths) = watcher.recv() {
                    log_changed_paths(&paths)?;
                }

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

        match watcher.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(paths)) => {
                log_changed_paths(&paths)?;

                if let Err(e) = child.kill() {
                    tracing::debug!("Could not kill child process: {e}");
                }
                wait_with_timeout(&mut child, Duration::from_secs(5));

                cliclack::log::step("Rebuilding...")?;
                if run_cargo_build(profile, verbose) {
                    child = spawn_app(profile, verbose)?;
                    cliclack::log::success(format!("Application restarted (PID {})", child.id()))?;
                } else {
                    cliclack::log::warning(
                        "Build failed. Watching for changes... (fix errors and save to retry)",
                    )?;
                    child = spawn_wait_dummy()?;
                }
            }
            Ok(None) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

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

    cmd.env(env::HOT_RELOAD, "1");
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

fn log_changed_paths(paths: &[PathBuf]) -> CliResult<()> {
    for path in paths {
        let _ = cliclack::log::info(format!(
            "Change detected: {}",
            style(path.display()).dim()
        ));
    }
    Ok(())
}

/// Resolved host/worker project paths for `flui run`.
struct WorkerHotReloadProject {
    config: HotReloadConfig,
    /// Directory containing `flui.toml`.
    config_dir: PathBuf,
    /// Cargo workspace root (`target/` lives here).
    workspace_root: PathBuf,
}

/// Flutter-parity hot reload: keep host alive, rebuild worker `cdylib` on save.
fn watch_worker_hot_reload(
    project: &WorkerHotReloadProject,
    profile: Option<&str>,
    verbose: bool,
) -> CliResult<()> {
    let worker_path = worker_dylib_path(
        &project.workspace_root,
        &project.config.worker_lib,
        profile,
    );

    cliclack::log::step("Building worker and host...")?;
    if !run_cargo_build_package(&project.config.worker_package, profile, verbose) {
        return Err(CliError::BuildFailed {
            platform: "desktop".to_string(),
            details: format!(
                "Initial build failed for {}",
                project.config.worker_package
            ),
        });
    }
    if !run_cargo_build_package(&project.config.host_package, profile, verbose) {
        return Err(CliError::BuildFailed {
            platform: "desktop".to_string(),
            details: format!("Initial build failed for {}", project.config.host_package),
        });
    }

    let mut child = spawn_host_package(
        &project.config.host_package,
        &worker_path,
        profile,
        verbose,
    )?;
    cliclack::log::success(format!(
        "Host started (PID {}) — worker at {}",
        child.id(),
        style(worker_path.display()).dim()
    ))?;

    let logic_src = project.config_dir.join(&project.config.logic_watch);
    let types_src = project
        .config
        .types_watch
        .as_ref()
        .map(|p| project.config_dir.join(p));

    let mut watcher = SourceWatcher::new().map_err(|e| CliError::context(e, "Failed to create file watcher"))?;

    watcher
        .watch(&logic_src, true)
        .map_err(|e| CliError::context(e, format!("Failed to watch {}", logic_src.display())))?;
    if let Some(ref types) = types_src {
        if types.exists() {
            watcher.watch(types, true).map_err(|e| {
                CliError::context(e, format!("Failed to watch {}", types.display()))
            })?;
        }
    }

    cliclack::log::info(format!(
        "Watching {} — edit UI code and save (host stays running)",
        style(logic_src.display()).dim()
    ))?;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                tracing::debug!("Host exited with: {:?}", status);
                cliclack::log::warning("Host exited. Rebuild and restart on next save...")?;
                if let Some(paths) = watcher.recv() {
                    handle_worker_watch_event(project, profile, verbose, &paths, &mut child)?;
                }
                continue;
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("Error checking host status: {e}"),
        }

        match watcher.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(paths)) => {
                handle_worker_watch_event(project, profile, verbose, &paths, &mut child)?;
            }
            Ok(None) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.kill();
    wait_with_timeout(&mut child, Duration::from_secs(5));
    Ok(())
}

fn handle_worker_watch_event(
    project: &WorkerHotReloadProject,
    profile: Option<&str>,
    verbose: bool,
    paths: &[PathBuf],
    child: &mut Child,
) -> CliResult<()> {
    log_changed_paths(paths)?;

    let types_src = project
        .config
        .types_watch
        .as_ref()
        .map(|p| project.config_dir.join(p));
    let types_changed = types_src.is_some_and(|types| {
        paths.iter().any(|p| p.starts_with(&types))
    });

    if types_changed {
        cliclack::log::step("Types changed — rebuilding host (hot restart)...")?;
        match child.try_wait() {
            Ok(None) => {
                let _ = child.kill();
                wait_with_timeout(child, Duration::from_secs(5));
            }
            Ok(Some(_)) => {}
            Err(e) => tracing::warn!("Error checking host status: {e}"),
        }
        let ok = run_cargo_build_package(&project.config.worker_package, profile, verbose)
            && run_cargo_build_package(&project.config.host_package, profile, verbose);
        if ok {
            let worker_path = worker_dylib_path(
                &project.workspace_root,
                &project.config.worker_lib,
                profile,
            );
            *child = spawn_host_package(
                &project.config.host_package,
                &worker_path,
                profile,
                verbose,
            )?;
            cliclack::log::success(format!("Host restarted (PID {})", child.id()))?;
        } else {
            cliclack::log::warning("Build failed — fix errors and save to retry")?;
        }
        return Ok(());
    }

    cliclack::log::step("Rebuilding worker (state preserved in host)...")?;
    if run_cargo_build_package(&project.config.worker_package, profile, verbose) {
        cliclack::log::success(
            "Worker rebuilt — host will hot-reload on next frame (~500ms)",
        )?;
        match child.try_wait() {
            Ok(None) => {
                // Host still running — WorkerReloadDriver picks up the new dylib.
            }
            Ok(Some(_)) | Err(_) => {
                let worker_path = worker_dylib_path(
                    &project.workspace_root,
                    &project.config.worker_lib,
                    profile,
                );
                *child = spawn_host_package(
                    &project.config.host_package,
                    &worker_path,
                    profile,
                    verbose,
                )?;
                cliclack::log::success(format!("Host restarted (PID {})", child.id()))?;
            }
        }
    } else {
        cliclack::log::warning("Worker build failed — fix errors and save to retry")?;
    }
    Ok(())
}

fn spawn_host_package(
    package: &str,
    worker_plugin: &Path,
    profile: Option<&str>,
    verbose: bool,
) -> CliResult<Child> {
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "-p", package]);

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    cmd.env(worker_env::WORKER_PLUGIN, worker_plugin);
    cmd.env(env::HOT_RELOAD, "1");
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    cmd.spawn().context("Failed to spawn host application")
}

fn run_cargo_build_package(package: &str, profile: Option<&str>, verbose: bool) -> bool {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "-p", package]);

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    match cmd.status() {
        Ok(status) => status.success(),
        Err(e) => {
            tracing::error!("Failed to run cargo build -p {package}: {e}");
            false
        }
    }
}

fn worker_dylib_path(workspace_root: &Path, worker_lib: &str, profile: Option<&str>) -> PathBuf {
    let profile_dir = profile.unwrap_or("debug");
    let mut path = workspace_root.join("target").join(profile_dir);
    #[cfg(windows)]
    {
        path.push(format!("{worker_lib}.dll"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        path.push(format!("lib{worker_lib}.so"));
    }
    #[cfg(target_os = "macos")]
    {
        path.push(format!("lib{worker_lib}.dylib"));
    }
    path
}

fn find_worker_hot_reload_project() -> CliResult<Option<WorkerHotReloadProject>> {
    let Some((config_dir, config)) = find_flui_config()? else {
        return Ok(None);
    };
    let Some(hot_reload) = config.hot_reload else {
        return Ok(None);
    };
    let workspace_root = find_workspace_root(&config_dir).ok_or_else(|| CliError::NotFluiProject {
        reason: "Could not find Cargo workspace root for worker hot reload".to_string(),
    })?;
    Ok(Some(WorkerHotReloadProject {
        config: hot_reload,
        config_dir,
        workspace_root,
    }))
}

fn find_flui_config() -> CliResult<Option<(PathBuf, FluiConfig)>> {
    let mut dir = std::env::current_dir().context("Could not read current directory")?;
    loop {
        let path = dir.join("flui.toml");
        if path.exists() {
            let config = FluiConfig::load_from(&path)?;
            return Ok(Some((dir, config)));
        }
        if !dir.pop() {
            break;
        }
    }
    Ok(None)
}

fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.exists() {
            if let Ok(content) = std::fs::read_to_string(&cargo) {
                if content.contains("[workspace]") {
                    return Some(dir);
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Ensure we're in a FLUI project directory.
fn ensure_flui_project() -> CliResult<()> {
    if find_worker_hot_reload_project()?.is_some() {
        return Ok(());
    }

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

/// Execute scene-only hot-reload mode for Android.
///
/// Watches the scene crate's `src/` directory for changes and rebuilds/pushes
/// the scene plugin `.so` to the device without restarting the app.
/// The host app detects the new `.so` via mtime polling and reloads automatically.
pub fn execute_scene(
    scene_crate: &str,
    package: &str,
    target: &str,
    release: bool,
    _verbose: bool,
) -> CliResult<()> {
    use flui_build::android::AndroidBuilder;

    let mode = if release { "release" } else { "debug" };
    cliclack::intro(style(" flui run --scene ").on_magenta().black())?;
    cliclack::log::info(format!(
        "Scene hot-reload: {} ({}, {})",
        style(scene_crate).cyan(),
        style(target).cyan(),
        style(mode).cyan()
    ))?;

    let workspace_root = std::env::current_dir()?;
    let builder = AndroidBuilder::new(&workspace_root).map_err(|e| CliError::BuildFailed {
        platform: "android".to_string(),
        details: e.to_string(),
    })?;

    let lib_name = "libflui_scene.so";

    // Initial build + push
    cliclack::log::step("Building scene plugin...")?;
    let start = Instant::now();

    let rt = tokio::runtime::Runtime::new()?;
    let so_path = rt
        .block_on(builder.build_scene_plugin(target, scene_crate, release))
        .map_err(|e| CliError::BuildFailed {
            platform: "android".to_string(),
            details: e.to_string(),
        })?;

    cliclack::log::success(format!(
        "Built in {:.2}s: {}",
        start.elapsed().as_secs_f64(),
        style(so_path.display()).dim()
    ))?;

    cliclack::log::step("Pushing to device...")?;
    rt.block_on(builder.push_scene_plugin(&so_path, package, lib_name))
        .map_err(|e| CliError::BuildFailed {
            platform: "android".to_string(),
            details: e.to_string(),
        })?;
    cliclack::log::success("Plugin pushed to device")?;

    // Watch scene crate src/ for changes
    let scene_src = workspace_root
        .join("examples")
        .join(scene_crate.replace("flui-", ""))
        .join("src");

    if !scene_src.exists() {
        // Try alternative path pattern
        let alt = workspace_root
            .join("examples")
            .join(scene_crate)
            .join("src");
        if !alt.exists() {
            return Err(CliError::BuildFailed {
                platform: "android".to_string(),
                details: format!(
                    "Scene crate src/ not found at {} or {}",
                    scene_src.display(),
                    alt.display()
                ),
            });
        }
    }

    cliclack::log::info(format!(
        "Watching {} for changes...",
        style(scene_src.display()).dim()
    ))?;

    let mut watcher = SourceWatcher::with_debounce(timing::ANDROID_SCENE_DEBOUNCE)
        .map_err(|e| CliError::context(e, "Failed to create file watcher"))?;

    watcher
        .watch(&scene_src, true)
        .map_err(|e| CliError::context(e, "Failed to watch scene crate"))?;

    loop {
        let Some(paths) = watcher.recv() else {
            break;
        };

        log_changed_paths(&paths)?;

        let start = Instant::now();
        cliclack::log::step("Rebuilding scene plugin...")?;

        match rt.block_on(builder.build_scene_plugin(target, scene_crate, release)) {
            Ok(so) => {
                let build_time = start.elapsed();
                cliclack::log::step("Pushing to device...")?;
                match rt.block_on(builder.push_scene_plugin(&so, package, lib_name)) {
                    Ok(()) => {
                        cliclack::log::success(format!(
                            "Updated in {:.2}s",
                            build_time.as_secs_f64()
                        ))?;
                    }
                    Err(e) => {
                        cliclack::log::warning(format!("Push failed: {e}"))?;
                    }
                }
            }
            Err(e) => {
                cliclack::log::warning(format!("Build failed: {e}"))?;
                cliclack::log::info("Fix errors and save to retry...")?;
            }
        }
    }

    cliclack::outro(style("Scene hot-reload stopped").green())?;
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
