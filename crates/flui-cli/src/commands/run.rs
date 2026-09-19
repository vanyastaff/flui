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

    let mut watcher =
        SourceWatcher::new().map_err(|e| CliError::context(e, "Failed to create file watcher"))?;

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
                    log_changed_paths(&paths);
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
                log_changed_paths(&paths);

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
            Ok(None) | Err(RecvTimeoutError::Timeout) => {}
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

fn log_changed_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = cliclack::log::info(format!("Change detected: {}", style(path.display()).dim()));
    }
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
    let worker_path =
        worker_dylib_path(&project.workspace_root, &project.config.worker_lib, profile);

    cliclack::log::step("Building worker and host...")?;
    // One invocation for both: see `run_cargo_build_packages` — a split build
    // gives the worker a different instance of the shared framework crates and
    // its `TypeId`s stop matching the host's.
    if !run_cargo_build_packages(
        &[&project.config.worker_package, &project.config.host_package],
        profile,
        verbose,
        None,
    ) {
        return Err(CliError::BuildFailed {
            platform: "desktop".to_string(),
            details: format!(
                "Initial build failed for {} / {}",
                project.config.worker_package, project.config.host_package
            ),
        });
    }
    // Load the host from a content-addressed staged copy, never the canonical
    // `target/.../lib<worker>.dylib`. The host maps this file for the whole
    // session, so keeping the canonical output unmapped is what lets the next
    // `cargo build` overwrite it on every platform (the same guarantee the old
    // isolated-target-dir bought, without splitting the crate instances — see
    // `run_cargo_build_packages`).
    let staged = stage_worker_artifact(&worker_path, &worker_path, true)?;
    publish_worker_plugin(&worker_path, &staged)?;

    let mut child = spawn_host_package(&project.config.host_package, &staged, profile, verbose)?;
    cliclack::log::success(format!(
        "Host started (PID {}) — worker at {}",
        child.id(),
        style(staged.display()).dim()
    ))?;

    let logic_src = project.config_dir.join(&project.config.logic_watch);
    let types_src = project
        .config
        .types_watch
        .as_ref()
        .map(|p| project.config_dir.join(p));

    let mut watcher =
        SourceWatcher::new().map_err(|e| CliError::context(e, "Failed to create file watcher"))?;

    watcher
        .watch(&logic_src, true)
        .map_err(|e| CliError::context(e, format!("Failed to watch {}", logic_src.display())))?;
    if let Some(ref types) = types_src
        && types.exists()
    {
        watcher
            .watch(types, true)
            .map_err(|e| CliError::context(e, format!("Failed to watch {}", types.display())))?;
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
            Ok(None) | Err(RecvTimeoutError::Timeout) => {}
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
    log_changed_paths(paths);

    let types_src = project
        .config
        .types_watch
        .as_ref()
        .map(|p| project.config_dir.join(p));
    let types_changed = types_src.is_some_and(|types| paths.iter().any(|p| p.starts_with(&types)));

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
        let ok = run_cargo_build_packages(
            &[&project.config.worker_package, &project.config.host_package],
            profile,
            verbose,
            None,
        );
        if ok {
            let worker_path =
                worker_dylib_path(&project.workspace_root, &project.config.worker_lib, profile);
            let staged = stage_worker_artifact(&worker_path, &worker_path, true)?;
            publish_worker_plugin(&worker_path, &staged)?;
            *child = spawn_host_package(&project.config.host_package, &staged, profile, verbose)?;
            cliclack::log::success(format!("Host restarted (PID {})", child.id()))?;
        } else {
            cliclack::log::warning("Build failed — fix errors and save to retry")?;
        }
        return Ok(());
    }

    cliclack::log::step("Rebuilding worker (state preserved in host)...")?;
    // Rebuild the worker TOGETHER with the host, in the normal target dir.
    //
    // Building the worker alone (or into an isolated `--target-dir`) is what
    // produced a worker whose shared framework crates were a *different*
    // compiled instance than the host's, so its `TypeId`s missed the host's
    // inherited-view maps and the first reloaded frame panicked — see
    // `run_cargo_build_packages`. Passing the host in the same invocation makes
    // cargo unify the dependency graph once; the host itself is unchanged and
    // therefore not relinked, so this does not touch the running binary (and
    // so does not fight the OS lock on it).
    let built_ok = run_cargo_build_packages(
        &[&project.config.worker_package, &project.config.host_package],
        profile,
        verbose,
        None,
    );

    if built_ok {
        let canonical =
            worker_dylib_path(&project.workspace_root, &project.config.worker_lib, profile);
        // Stage under a content-addressed name so the host loads a *new* path
        // (defeats macOS's deferred-unmap dyld) while the canonical output
        // stays free for the next build.
        let staged = stage_worker_artifact(&canonical, &canonical, true)?;
        publish_worker_plugin(&canonical, &staged)?;
        cliclack::log::success("Worker rebuilt — host will hot-reload on next frame (~500ms)")?;
        if let Ok(None) = child.try_wait() {
            // Host still running — WorkerReloadDriver picks up the new dylib.
        } else {
            *child = spawn_host_package(&project.config.host_package, &staged, profile, verbose)?;
            cliclack::log::success(format!("Host restarted (PID {})", child.id()))?;
        }
    } else {
        cliclack::log::warning("Worker build failed — fix errors and save to retry")?;
    }
    Ok(())
}

/// Infix marking a staged (versioned) worker artifact, mirroring
/// `hot-lib-reloader`'s `-hot-` shadow-file convention. It keeps staged names
/// disjoint from the canonical output and from cargo's own `.d`/dep artifacts,
/// so pruning staged files can never remove a file cargo owns.
const STAGING_INFIX: &str = "-hot-";

/// Copy a freshly built worker into a **content-addressed** staging file the
/// host can load, and return that path.
///
/// The staged name embeds a hash of the built bytes (`{stem}-hot-{hash}{ext}`),
/// which is what makes an in-process reload actually reload on macOS: dyld is a
/// deferred-unmap runtime, so re-opening the *same* path can serve the retained,
/// stale image instead of the rebuilt one. A fixed A/B slot pair (the previous
/// scheme) hit exactly that: once both slots existed the path stopped changing
/// and every later rebuild was served from cache. Because the name tracks the
/// content, a rebuild that produces different bytes yields a different path — and
/// a rebuild that produces *identical* bytes reuses the existing file untouched,
/// never rewriting a mapping the host may still have loaded.
///
/// Staging exists independently of macOS: on Windows the canonical output is
/// locked by the running host, so the host loads the copy instead. The `use_staging`
/// flag preserves the direct-load path for callers that do not need it.
fn stage_worker_artifact(built: &Path, canonical: &Path, use_staging: bool) -> CliResult<PathBuf> {
    if !use_staging {
        return Ok(built.to_path_buf());
    }

    let parent = canonical.parent().ok_or_else(|| CliError::BuildFailed {
        platform: "desktop".to_string(),
        details: "worker dylib path has no parent directory".to_string(),
    })?;
    std::fs::create_dir_all(parent).context("Failed to create worker staging directory")?;

    let ext = canonical
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let stem = canonical
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("worker");

    // Read once and hash the exact bytes that are written, so the name can never
    // disagree with the content (a read-then-copy would open a window where the
    // file changes between hashing and copying).
    let bytes = std::fs::read(built)
        .with_context(|| format!("Failed to read freshly built worker at {}", built.display()))?;
    let dest = parent.join(format!("{stem}{STAGING_INFIX}{:08x}{ext}", fnv1a(&bytes)));

    // An existing file with this hash already holds these exact bytes. Leave it
    // in place — rewriting a shared library the host may have mapped is what the
    // deferred-unmap hazard turns into a stale image.
    if dest.exists() {
        return Ok(dest);
    }

    std::fs::write(&dest, &bytes)
        .with_context(|| format!("Failed to stage worker to {}", dest.display()))?;

    // Defensive only: cargo already ad-hoc linker-signs dylibs on macOS
    // (`flags=adhoc,linker-signed`), and the host runs the worker under the same
    // un-hardened `cargo run` image, so an unsigned staged copy loads today. This
    // keeps a future hardened-runtime/library-validation host from refusing it,
    // and never fails the reload (a missing `codesign` is a warning, not an error).
    #[cfg(target_os = "macos")]
    codesign_ad_hoc(&dest);

    prune_stale_staged_workers(parent, stem, &ext, &dest);

    Ok(dest)
}

/// FNV-1a (32-bit) over `bytes` — a tiny, dependency-free content hash whose
/// only job is to give distinct builds distinct staging names. It is not a
/// security boundary and collisions are not a correctness risk here: a
/// collision would merely reuse an existing staged file (the bytes are compared
/// by the host, not by this hash).
fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Best-effort removal of superseded staged workers. Errors are ignored: a file
/// may still be mapped by a running host (Windows locks it; macOS defers the
/// unmap), and a leftover staged file is harmless. `keep` is never removed.
fn prune_stale_staged_workers(parent: &Path, stem: &str, ext: &str, keep: &Path) {
    let prefix = format!("{stem}{STAGING_INFIX}");
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(ext) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Ad-hoc sign `path` (`codesign --sign -`). Best-effort: a missing binary or a
/// failed sign is logged and ignored — see `stage_worker_artifact` for why this
/// is defensive rather than load-bearing today.
#[cfg(target_os = "macos")]
fn codesign_ad_hoc(path: &Path) {
    let output = std::process::Command::new("codesign")
        .args(["--sign", "-", "-v", "--force"])
        .arg(path)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            tracing::debug!(path = %path.display(), "ad-hoc signed staged worker");
        }
        Ok(output) => {
            tracing::warn!(
                path = %path.display(),
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "codesign of the staged worker failed; continuing (load may still succeed)"
            );
        }
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "could not run codesign; continuing (load may still succeed)"
            );
        }
    }
}

/// Write `.flui_worker_plugin` next to the canonical dylib so the host loads
/// the staged artifact without overwriting a locked file.
fn publish_worker_plugin(canonical: &Path, load_path: &Path) -> CliResult<()> {
    let manifest = worker_plugin_manifest_path(canonical);
    if let Some(parent) = manifest.parent() {
        std::fs::create_dir_all(parent).context("Failed to create worker manifest directory")?;
    }
    std::fs::write(&manifest, load_path.as_os_str().as_encoded_bytes())
        .with_context(|| format!("Failed to write {}", manifest.display()))?;
    Ok(())
}

fn worker_plugin_manifest_path(canonical: &Path) -> PathBuf {
    canonical.parent().map_or_else(
        || PathBuf::from(".flui_worker_plugin"),
        |dir| dir.join(".flui_worker_plugin"),
    )
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

/// Build one or more packages in a **single** cargo invocation.
///
/// The worker `cdylib` and the host binary are not independent: both link the
/// shared framework crates (`flui-view`, `flui-widgets`, …), and each of those
/// crates must be the *same compiled instance* on both sides. Cargo unifies
/// dependency features **per invocation**, so building the worker with
/// `cargo build -p worker` and the host with `cargo build -p host` can resolve a
/// shared crate with different feature sets. Two instances of `flui-widgets`
/// mean two `-C metadata` hashes and therefore two different
/// `TypeId::of::<T>()` values for the *same* type — Rust's `TypeId` is stable
/// only within one compiled instance of a crate.
///
/// This is not theoretical: the inherited-view lookup that `GestureDetector`
/// performs in `init_state` (`GestureArenaScope::of`) keys a `HashMap<TypeId,
/// ElementId>` on the host side. With a split instance the worker's `TypeId`
/// misses the host's map and the first frame panics with
/// "gesture consumers must be mounted beneath GestureArenaScope" — the exact
/// failure that made `flui run` unusable whenever host and worker were built
/// separately. Passing every `-p` flag to one `cargo build` makes cargo unify
/// once, so both binaries link one instance and the `TypeId`s agree.
fn run_cargo_build_packages(
    packages: &[&str],
    profile: Option<&str>,
    verbose: bool,
    target_dir: Option<&Path>,
) -> bool {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    for package in packages {
        cmd.args(["-p", package]);
    }

    if let Some(dir) = target_dir {
        let dir = dir.to_string_lossy();
        cmd.args(["--target-dir", &dir]);
    }

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    match cmd.status() {
        Ok(status) => status.success(),
        Err(e) => {
            tracing::error!("Failed to run cargo build for {packages:?}: {e}");
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
    let workspace_root =
        find_workspace_root(&config_dir).ok_or_else(|| CliError::NotFluiProject {
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
        if cargo.exists()
            && let Ok(content) = std::fs::read_to_string(&cargo)
            && content.contains("[workspace]")
        {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Whether a `Cargo.toml` declares a FLUI dependency.
///
/// The package names are hyphenated (`flui-app`) — that is what the templates
/// emit and what Cargo expects. The underscore spelling is accepted too, since
/// a hand-written manifest may rename the dependency to its crate name.
fn has_flui_dependency(cargo_toml: &str) -> bool {
    let normalized = cargo_toml.replace('-', "_");
    normalized.contains("flui_app") || normalized.contains("flui_widgets")
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

    let content = std::fs::read_to_string(cargo_toml)?;
    if !has_flui_dependency(&content) {
        return Err(CliError::NotFluiProject {
            reason: "flui-app or flui-widgets dependency not found in Cargo.toml".to_string(),
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

    while let Some(paths) = watcher.recv() {
        log_changed_paths(&paths);

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

#[cfg(test)]
mod tests {
    use super::{fnv1a, has_flui_dependency, stage_worker_artifact};

    /// `flui create` emits hyphenated dep names; `flui run` must recognise the
    /// project it just generated.
    #[test]
    fn generated_manifest_is_recognised_as_a_flui_project() {
        assert!(has_flui_dependency(
            r#"flui-app = { path = "../../crates/flui-app" }"#
        ));
        assert!(has_flui_dependency(r#"flui_widgets = "0.2.0""#));
        assert!(!has_flui_dependency(r#"serde = "1.0""#));
    }

    /// The staging name must change when the built bytes change — that is what
    /// makes macOS's deferred-unmap dyld load a fresh image instead of serving
    /// the stale retained one. Distinct bytes -> distinct paths.
    #[test]
    fn staging_path_tracks_content() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("libcounter_logic.dylib");

        let built_a = dir.path().join("built-a.bin");
        std::fs::write(&built_a, b"first build").expect("write build a");
        let staged_a = stage_worker_artifact(&built_a, &canonical, true).expect("stage a");
        assert!(staged_a.exists());

        let built_b = dir.path().join("built-b.bin");
        std::fs::write(&built_b, b"second build, one byte more").expect("write build b");
        let staged_b = stage_worker_artifact(&built_b, &canonical, true).expect("stage b");

        assert_ne!(
            staged_a, staged_b,
            "different bytes must stage at different paths so the reload cannot be served stale"
        );
        assert!(
            staged_b.exists(),
            "the newest staged worker is the one the sidecar points at"
        );
    }

    /// Rebuilding identical bytes must reuse the existing staged file untouched.
    /// Rewriting a library the host may still have mapped is exactly the hazard
    /// the content-addressed name exists to avoid.
    #[test]
    fn staging_reuses_an_identical_build_without_rewriting() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("libcounter_logic.dylib");

        let built = dir.path().join("built.bin");
        std::fs::write(&built, b"same bytes").expect("write build");

        let staged_first = stage_worker_artifact(&built, &canonical, true).expect("stage first");
        let rewritten_at = std::fs::metadata(&staged_first)
            .and_then(|m| m.modified())
            .expect("staged mtime");

        std::thread::sleep(std::time::Duration::from_millis(10));
        let staged_again = stage_worker_artifact(&built, &canonical, true).expect("stage again");

        assert_eq!(staged_first, staged_again, "same content -> same path");
        let rewritten_at_again = std::fs::metadata(&staged_again)
            .and_then(|m| m.modified())
            .expect("staged mtime after reuse");
        assert_eq!(
            rewritten_at, rewritten_at_again,
            "an identical rebuild must not rewrite the staged file in place"
        );
    }

    /// Superfluous staged versions are pruned, but the version just staged is
    /// never among them.
    #[test]
    fn staging_prunes_superseded_versions_but_keeps_the_new_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("libcounter_logic.dylib");

        let mut staged = Vec::new();
        for (i, bytes) in [&b"v1"[..], b"v2", b"v3"].into_iter().enumerate() {
            let built = dir.path().join(format!("built-{i}.bin"));
            std::fs::write(&built, bytes).expect("write build");
            staged.push(stage_worker_artifact(&built, &canonical, true).expect("stage"));
        }

        let newest = staged.last().expect("at least one staged");
        assert!(newest.exists(), "the just-staged version must survive");
        for old in &staged[..staged.len() - 1] {
            assert!(
                !old.exists(),
                "a superseded staged worker should be pruned: {}",
                old.display()
            );
        }
    }

    /// `use_staging = false` returns the built path directly (the caller loads it).
    #[test]
    fn staging_disabled_returns_the_built_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let canonical = dir.path().join("libcounter_logic.dylib");
        let built = dir.path().join("built.bin");
        std::fs::write(&built, b"x").expect("write build");

        let staged = stage_worker_artifact(&built, &canonical, false).expect("stage");
        assert_eq!(staged, built);
    }

    /// FNV-1a is only required to be stable and to distinguish obvious
    /// differences; pin both so a future edit cannot silently weaken it.
    #[test]
    fn fnv1a_is_stable_and_distinguishes_content() {
        assert_eq!(fnv1a(b""), 0x811c_9dc5);
        assert_eq!(fnv1a(b"a"), fnv1a(b"a"));
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
    }
}
