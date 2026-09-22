//! Run command for executing FLUI applications.
//!
//! Two hot-reload modes share one development loop ([`dev_loop`]):
//!
//! - **Process restart** (default): watch `src/`, kill + `cargo run` on change.
//! - **Worker host** (`flui.toml` `[hot_reload]`): watch worker UI sources,
//!   rebuild `cdylib` only; host applies `HotReloadTier::HotReload` in-process.
//!
//! The loop multiplexes four event sources — file changes, hot-keys from the
//! terminal (`r` reload, `R` restart, `c` clear, `h` help, `q` quit), Ctrl-C,
//! and the child exiting — and guarantees the child is stopped before the
//! command returns, whichever of them ends the session. In `--json` mode the
//! same loop narrates itself as `run.*` events and forwards the app's stdout
//! line by line as `run.app.log` so a tool driving `flui run` sees everything
//! on one machine-readable stream.

use crate::config::{FluiConfig, HotReloadConfig};
use crate::error::{CliError, CliResult, ResultExt};
use crate::ui;
use crate::watch::{SourceWatcher, timing};
use console::style;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

/// Execute the run command.
///
/// When `hot_reload` is true (and not in release mode), watches the project's
/// sources and rebuilds/restarts (or hot-reloads the worker) on change.
pub(crate) fn execute(
    device: Option<String>,
    release: bool,
    hot_reload: bool,
    profile: Option<String>,
    verbose: bool,
) -> CliResult<()> {
    let mode = if release { "release" } else { "debug" };
    ui::intro(style(" flui run ").on_green().black())?;
    ui::info(format!("Mode: {}", style(mode).cyan()))?;

    let project = ensure_flui_project()?;

    let target = resolve_target(device.as_deref())?;
    ui::info(format!("Target device: {}", style(target.label()).cyan()))?;
    ui::emit(
        "run.start",
        &serde_json::json!({ "device": target.id(), "mode": mode, "hot_reload": hot_reload && !release }),
    );

    if let Target::IosSimulator(udid) = &target {
        return super::ios::run(udid, release, profile.as_deref());
    }

    let result = run_session(project, hot_reload && !release, release, profile, verbose);
    // `run.stop` closes `run.start` on every outcome; the `error` event that
    // follows a failure carries the exit code.
    ui::emit(
        "run.stop",
        &serde_json::json!({ "interrupted": matches!(result, Err(CliError::Interrupted)) }),
    );
    result?;
    ui::outro(style("Application finished").green())?;
    Ok(())
}

/// The part of `execute` whose outcome `run.stop` reports.
fn run_session(
    project: Project,
    hot_reload: bool,
    release: bool,
    profile: Option<String>,
    verbose: bool,
) -> CliResult<()> {
    if hot_reload {
        match project {
            Project::Worker(project) => {
                ui::success(format!(
                    "Worker hot reload: {} → {}",
                    style(&project.config.worker_package).cyan(),
                    style(&project.config.host_package).cyan()
                ))?;
                let mut strategy = WorkerHost {
                    project,
                    profile: profile.clone(),
                    verbose,
                };
                dev_loop(&mut strategy)?;
            }
            Project::Application => {
                ui::success("Hot reload enabled (process restart)")?;
                let mut strategy = ProcessRestart {
                    profile: profile.clone(),
                    verbose,
                };
                dev_loop(&mut strategy)?;
            }
        }
    } else {
        run_once(release, profile, verbose)?;
    }
    Ok(())
}

// ============================================================================
// Target device resolution
// ============================================================================

/// Where `flui run` launches the application.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// This machine.
    Host,
    /// An iOS simulator, by exact UDID.
    IosSimulator(String),
}

impl Target {
    fn id(&self) -> String {
        match self {
            Self::Host => host_device_id(),
            Self::IosSimulator(udid) => udid.clone(),
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Host => format!("{} (this machine)", host_device_id()),
            Self::IosSimulator(udid) => format!("iOS simulator {udid}"),
        }
    }
}

/// The host's own device id, as `flui devices` lists it.
fn host_device_id() -> String {
    std::env::consts::OS.to_string()
}

/// Device names that mean "this machine".
fn is_host_device(device: &str) -> bool {
    let lower = device.trim().to_ascii_lowercase();
    matches!(lower.as_str(), "desktop" | "host" | "this") || lower == std::env::consts::OS
}

/// Resolve `--device` against the same discovery `flui devices` performs.
///
/// Exact id, then exact name (case-insensitive), then a unique prefix of
/// either. A device that exists but cannot be driven by `flui run` yet
/// (Android, a browser) is an `Unsupported` error naming the alternative —
/// never a silent fallback to another platform.
fn resolve_target(device: Option<&str>) -> CliResult<Target> {
    let Some(query) = device.map(str::trim).filter(|q| !q.is_empty()) else {
        return Ok(Target::Host);
    };
    if is_host_device(query) {
        return Ok(Target::Host);
    }

    let discovery = super::devices::discover(None, false);
    let matched = select_device(&discovery.devices, query)?;
    match_target(matched)
}

/// Pick one device for `query` out of `devices` (see [`resolve_target`]).
fn select_device<'d>(
    devices: &'d [super::devices::Device],
    query: &str,
) -> CliResult<&'d super::devices::Device> {
    let lower = query.to_ascii_lowercase();
    if let Some(device) = devices.iter().find(|d| d.id == query) {
        return Ok(device);
    }
    if let Some(device) = devices
        .iter()
        .find(|d| d.name.to_ascii_lowercase() == lower)
    {
        return Ok(device);
    }
    let candidates: Vec<&super::devices::Device> = devices
        .iter()
        .filter(|d| {
            d.id.to_ascii_lowercase().starts_with(&lower)
                || d.name.to_ascii_lowercase().starts_with(&lower)
        })
        .collect();
    match candidates.as_slice() {
        [device] => Ok(device),
        [] => Err(CliError::DeviceNotFound {
            name: query.to_string(),
            hint: "run `flui devices` to see ids and names".into(),
        }),
        many => Err(CliError::DeviceNotFound {
            name: query.to_string(),
            hint: format!(
                "ambiguous; candidates: {}",
                many.iter()
                    .map(|d| format!("{} ({})", d.name, d.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

fn match_target(device: &super::devices::Device) -> CliResult<Target> {
    use crate::DevicePlatform;
    match device.platform {
        DevicePlatform::Desktop => Ok(Target::Host),
        DevicePlatform::Ios => Ok(Target::IosSimulator(device.id.clone())),
        DevicePlatform::Android => Err(CliError::Unsupported {
            what: format!("running on Android device {}", device.id),
            reason: "`flui run` cannot install to Android yet; build with `flui build android` and install the APK with adb, or use `flui run --scene` for scene hot reload".into(),
        }),
        DevicePlatform::Web => Err(CliError::Unsupported {
            what: format!("running in {}", device.name),
            reason: "`flui run` has no browser target yet; build with `flui build web` and serve the output".into(),
        }),
    }
}

// ============================================================================
// Run once
// ============================================================================

/// Run the app once without hot reload, and wait for it.
///
/// Goes through the same child policy as the dev loop, so `--json` gets the
/// `run.app.*` events and Ctrl-C stops the app before the CLI returns.
fn run_once(release: bool, profile: Option<String>, verbose: bool) -> CliResult<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("run");
    if release {
        cmd.arg("--release");
    } else if let Some(prof) = profile {
        cmd.args(["--profile", &prof]);
    }
    if verbose {
        cmd.arg("--verbose");
    }

    ui::step("Building and running...")?;
    let interrupted = install_interrupt_flag();
    let mut child = spawn_child(cmd, "Failed to spawn application")?;
    announce_started(&child, "Application started");
    loop {
        // The flag first: on a terminal Ctrl-C reaches the whole foreground
        // group, so the app usually dies by signal *before* this loop sees
        // the flag — that is an interruption, not an application failure.
        if interrupted.swap(false, Ordering::SeqCst) {
            if stop_child(Some(&mut child)) {
                ui::emit("run.app.stop", &serde_json::json!({}));
            }
            return Err(CliError::Interrupted);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                ui::emit(
                    "run.app.exit",
                    &serde_json::json!({ "code": status.code() }),
                );
                if status.success() {
                    return Ok(());
                }
                // A signal death right after Ctrl-C is the interruption.
                std::thread::sleep(Duration::from_millis(50));
                if status.code().is_none() && interrupted.swap(false, Ordering::SeqCst) {
                    return Err(CliError::Interrupted);
                }
                return Err(CliError::RunFailed {
                    details: describe_exit(status),
                });
            }
            Ok(None) => {}
            Err(e) => return Err(CliError::context(e, "Could not wait for the application")),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Environment variables the dev loop sets for the app it launches. They are
/// the contract with the runtime half of hot reload (`flui_hot_reload::engine::env`
/// and `strategy::env`, read inside the app); the names are duplicated here
/// so the CLI does not link the framework, and `env_names_match_the_runtime`
/// below keeps the two in step.
mod env {
    /// Set to `1` when the app is launched by `flui run` with hot reload on.
    pub(crate) const HOT_RELOAD: &str = "FLUI_HOT_RELOAD";
    /// Path of the worker `cdylib` a `--hot-reload` host should load.
    pub(crate) const WORKER_PLUGIN: &str = "FLUI_WORKER_PLUGIN";
}

// ============================================================================
// Development loop
// ============================================================================

/// What the loop can be told to do by a key press.
///
/// Only [`KeyReader`] constructs these, and it reads keys on Unix alone;
/// the loop still matches every variant so there is one dev loop, not two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
enum HotKey {
    /// `r`: apply the cheapest reload the strategy offers.
    Reload,
    /// `R`: stop the app, rebuild everything, start again.
    Restart,
    /// `c`: clear the terminal.
    Clear,
    /// `h` / `?`: print the key legend.
    Help,
    /// `q`: stop the app and return.
    Quit,
}

impl HotKey {
    /// The key bound to each action; anything else is ignored.
    #[cfg(unix)]
    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            b'r' => Some(Self::Reload),
            b'R' => Some(Self::Restart),
            b'c' => Some(Self::Clear),
            b'h' | b'?' => Some(Self::Help),
            b'q' => Some(Self::Quit),
            _ => None,
        }
    }
}

/// One thing the loop reacts to.
#[derive(Debug)]
enum LoopEvent {
    Changed(Vec<PathBuf>),
    Key(HotKey),
    Interrupted,
    ChildExited(ExitStatus),
    /// The watcher channel closed; nothing more will arrive.
    WatcherClosed,
}

/// A hot-reload mode: how to build, spawn, and react to a change.
///
/// Strategies borrow the running app through `&mut Option<Child>` and never
/// take it by value: the loop keeps ownership, so an error anywhere inside a
/// strategy still leaves the child where the loop's cleanup can stop it.
trait ReloadStrategy {
    /// Paths to watch (`recursive` flag per path).
    fn watch_paths(&self) -> Vec<(PathBuf, bool)>;
    /// Build everything and start the app into `child`. Leaves `None` when
    /// the build failed (the loop keeps watching so the user can fix and save).
    fn build_and_spawn(&mut self, child: &mut Option<Child>) -> CliResult<()>;
    /// React to changed sources with the app possibly still running.
    fn on_change(&mut self, paths: &[PathBuf], child: &mut Option<Child>) -> CliResult<()>;
    /// The cheapest reload (`r`). Defaults to a change with no paths.
    fn reload(&mut self, child: &mut Option<Child>) -> CliResult<()> {
        self.on_change(&[], child)
    }
    /// What `r` does, for the legend.
    fn reload_label(&self) -> &'static str;
}

/// Drive `strategy` until `q`, Ctrl-C, or the watcher going away.
///
/// The child is always stopped on the way out — including when a step
/// returns an error — so a failed `flui run` never leaves an orphaned app.
fn dev_loop(strategy: &mut dyn ReloadStrategy) -> CliResult<()> {
    let interrupted = install_interrupt_flag();
    let keys = KeyReader::start();

    ui::step("Building project...")?;
    let mut child = None;
    let result = strategy
        .build_and_spawn(&mut child)
        .and_then(|()| {
            if child.is_none() {
                ui::warning(
                    "Build failed. Watching for changes... (fix errors and save to retry)",
                )?;
            }
            start_watcher(strategy.watch_paths())
        })
        .and_then(|watcher| {
            if keys.is_some() {
                print_key_legend(strategy)?;
            }
            drive(strategy, interrupted, keys.as_ref(), &watcher, &mut child)
        });

    // Cleanup runs for every outcome above, then the terminal is restored
    // (the key reader drops here) before anything else is printed.
    stop_and_report(&mut child);
    drop(keys);
    result
}

fn drive(
    strategy: &mut dyn ReloadStrategy,
    interrupted: &AtomicBool,
    keys: Option<&KeyReader>,
    watcher: &SourceWatcher,
    child: &mut Option<Child>,
) -> CliResult<()> {
    loop {
        match next_event(interrupted, keys, watcher, child.as_mut()) {
            LoopEvent::Interrupted => {
                ui::info("Interrupted, stopping...")?;
                return Err(CliError::Interrupted);
            }
            LoopEvent::Key(HotKey::Quit) => {
                ui::info("Stopping...")?;
                return Ok(());
            }
            LoopEvent::WatcherClosed => return Ok(()),
            LoopEvent::ChildExited(status) => {
                ui::emit(
                    "run.app.exit",
                    &serde_json::json!({ "code": status.code() }),
                );
                ui::info(format!(
                    "Application exited ({}). Watching for changes to restart...",
                    describe_exit(status)
                ))?;
                *child = None;
            }
            LoopEvent::Changed(paths) => {
                log_changed_paths(&paths);
                ui::emit("run.change", &serde_json::json!({ "paths": paths }));
                strategy.on_change(&paths, child)?;
            }
            LoopEvent::Key(HotKey::Reload) => {
                ui::step(format!("{} (r)", strategy.reload_label()))?;
                strategy.reload(child)?;
            }
            LoopEvent::Key(HotKey::Restart) => {
                ui::step("Restarting (R)...")?;
                stop_and_report(child);
                strategy.build_and_spawn(child)?;
                ui::emit(
                    "run.reload",
                    &serde_json::json!({ "kind": "restart", "ok": child.is_some() }),
                );
                if child.is_none() {
                    ui::warning("Build failed — fix errors and save to retry")?;
                }
            }
            LoopEvent::Key(HotKey::Clear) => {
                let _ = console::Term::stderr().clear_screen();
            }
            LoopEvent::Key(HotKey::Help) => print_key_legend(strategy)?,
        }
    }
}

/// Poll every source once, in priority order, waiting on the watcher for at
/// most 100 ms so key presses and Ctrl-C are picked up promptly.
fn next_event(
    interrupted: &AtomicBool,
    keys: Option<&KeyReader>,
    watcher: &SourceWatcher,
    mut child: Option<&mut Child>,
) -> LoopEvent {
    loop {
        if interrupted.swap(false, Ordering::SeqCst) {
            return LoopEvent::Interrupted;
        }
        if let Some(key) = keys.and_then(KeyReader::try_recv) {
            return LoopEvent::Key(key);
        }
        if let Some(child) = child.as_deref_mut() {
            match child.try_wait() {
                Ok(Some(status)) => return LoopEvent::ChildExited(status),
                Ok(None) => {}
                Err(error) => {
                    let _ = ui::warning(format!("could not poll the application: {error}"));
                }
            }
        }
        match watcher.recv_timeout(Duration::from_millis(100)) {
            Ok(Some(paths)) => return LoopEvent::Changed(paths),
            Ok(None) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return LoopEvent::WatcherClosed,
        }
    }
}

fn start_watcher(paths: Vec<(PathBuf, bool)>) -> CliResult<SourceWatcher> {
    let mut watcher =
        SourceWatcher::new().map_err(|e| CliError::context(e, "Failed to create file watcher"))?;
    for (path, recursive) in paths {
        if !path.exists() {
            ui::debug(format!(
                "not watching {}: it does not exist",
                path.display()
            ));
            continue;
        }
        watcher
            .watch(&path, recursive)
            .map_err(|e| CliError::context(e, format!("failed to watch {}", path.display())))?;
        ui::info(format!("Watching {}", style(path.display()).dim()))?;
    }
    Ok(watcher)
}

fn print_key_legend(strategy: &dyn ReloadStrategy) -> CliResult<()> {
    ui::note(
        "Keys",
        format!(
            "{}  {}\n{}  restart (stop, rebuild all, start)\n{}  clear screen\n{}  this help\n{}  quit (Ctrl-C also stops the app)",
            style("r").bold(),
            strategy.reload_label().to_ascii_lowercase(),
            style("R").bold(),
            style("c").bold(),
            style("h").bold(),
            style("q").bold(),
        ),
    )?;
    Ok(())
}

/// The process-wide "Ctrl-C was pressed" flag.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Ctrl-C sets [`INTERRUPTED`] instead of killing the CLI outright: the app
/// must be stopped and reported first. The listener is installed once per
/// process; later loops (there is only ever one) reuse it.
fn install_interrupt_flag() -> &'static AtomicBool {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // A tiny current-thread runtime on its own thread hosts the async
        // signal listener; the loop itself stays synchronous.
        std::thread::Builder::new()
            .name("flui-ctrl-c".into())
            .spawn(|| {
                // `enable_all`, not `enable_io`: on Unix the signal driver
                // rides on the I/O driver, but on Windows `ctrl_c` uses the
                // console handler and tokio's `signal` feature does not even
                // expose `enable_io` there.
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    let _ = crate::ui::warning(
                        "could not start the Ctrl-C listener; default handling applies".to_string(),
                    );
                    return;
                };
                runtime.block_on(async {
                    while tokio::signal::ctrl_c().await.is_ok() {
                        INTERRUPTED.store(true, Ordering::SeqCst);
                    }
                });
            })
            .ok();
    });
    &INTERRUPTED
}

// ============================================================================
// Hot-keys
// ============================================================================

/// Single-key input for the dev loop.
///
/// Owns the terminal mode for the whole session: canonical mode and echo are
/// switched off once at start and restored exactly once when the reader is
/// dropped, on every exit path. Signals (`ISIG`) stay enabled, so Ctrl-C is
/// still a real SIGINT — delivered to the CLI *and* the app in the same
/// process group — and the interrupt flag, not a key, reports it. Reads use
/// a 100 ms timeout (`VMIN=0`, `VTIME=1`) so the reader thread notices the
/// stop flag and can be joined before the mode is restored; a reader stuck
/// in a blocking read was how a previous version left the shell in raw mode.
///
/// Only Unix terminals are supported; elsewhere hot-keys are simply absent
/// and the legend is not printed.
struct KeyReader {
    rx: mpsc::Receiver<HotKey>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// Held for its `Drop`: restores the terminal after the thread is joined.
    #[cfg(unix)]
    _mode: raw_mode::Guard,
}

impl KeyReader {
    /// Start reading keys, or `None` when the session is not interactive
    /// (CI, pipes, `--json`, `--non-interactive`) or the platform has no
    /// raw-mode support.
    fn start() -> Option<Self> {
        if !ui::is_interactive() {
            return None;
        }
        #[cfg(unix)]
        {
            let mode = raw_mode::Guard::enter().ok()?;
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, rx) = mpsc::channel();
            let thread_stop = Arc::clone(&stop);
            let thread = std::thread::Builder::new()
                .name("flui-keys".into())
                .spawn(move || {
                    // A `None` is the read timeout: loop around and re-check
                    // the stop flag.
                    while !thread_stop.load(Ordering::SeqCst) {
                        if let Some(key) = raw_mode::read_byte().and_then(HotKey::from_byte)
                            && tx.send(key).is_err()
                        {
                            return;
                        }
                    }
                })
                .ok()?;
            Some(Self {
                rx,
                stop,
                thread: Some(thread),
                _mode: mode,
            })
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    fn try_recv(&self) -> Option<HotKey> {
        self.rx.try_recv().ok()
    }
}

impl Drop for KeyReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // `self._mode` drops after this and restores the terminal.
    }
}

#[cfg(unix)]
mod raw_mode {
    //! Minimal terminal-mode handling for the hot-key reader (see
    //! [`super::KeyReader`]), through `rustix`'s safe termios wrappers.

    use rustix::termios::{LocalModes, OptionalActions, SpecialCodeIndex, Termios};
    use std::io;

    /// Restores the original terminal attributes on drop.
    pub(super) struct Guard {
        original: Termios,
    }

    impl Guard {
        /// Switch stdin to non-canonical, no-echo mode with 100 ms reads.
        pub(super) fn enter() -> io::Result<Self> {
            let stdin = io::stdin();
            let original = rustix::termios::tcgetattr(&stdin)?;
            let mut raw = original.clone();
            raw.local_modes &= !(LocalModes::ICANON | LocalModes::ECHO);
            raw.special_codes[SpecialCodeIndex::VMIN] = 0;
            raw.special_codes[SpecialCodeIndex::VTIME] = 1;
            rustix::termios::tcsetattr(&stdin, OptionalActions::Now, &raw)?;
            Ok(Self { original })
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = rustix::termios::tcsetattr(io::stdin(), OptionalActions::Now, &self.original);
        }
    }

    /// Read one byte from stdin, or `None` after the `VTIME` timeout.
    pub(super) fn read_byte() -> Option<u8> {
        let mut byte = [0u8; 1];
        match rustix::io::read(io::stdin(), &mut byte) {
            Ok(1) => Some(byte[0]),
            _ => None,
        }
    }
}

// ============================================================================
// Child processes
// ============================================================================

/// Stop the app if it is running and report it (`run.app.stop`), so every
/// `run.app.start` a consumer saw is closed by exactly one stop or exit.
fn stop_and_report(child: &mut Option<Child>) {
    if stop_child(child.as_mut()) {
        ui::emit("run.app.stop", &serde_json::json!({}));
    }
    *child = None;
}

/// Kill and reap the app if it is still running (bounded wait).
/// Returns whether a running app was actually stopped.
fn stop_child(child: Option<&mut Child>) -> bool {
    let Some(child) = child else { return false };
    if let Ok(Some(_)) = child.try_wait() {
        return false;
    }
    if let Err(e) = child.kill() {
        crate::ui::debug(format!("could not kill child process: {e}"));
    }
    wait_with_timeout(child, Duration::from_secs(5));
    true
}

fn describe_exit(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated by signal".to_string(),
        |code| format!("exit code {code}"),
    )
}

/// Wrap a build in `run.build.start` / `run.build.done` events and a timing line.
fn timed_build(build: impl FnOnce() -> bool) -> bool {
    ui::emit("run.build.start", &serde_json::json!({}));
    let started = Instant::now();
    let ok = build();
    let seconds = started.elapsed().as_secs_f64();
    ui::emit(
        "run.build.done",
        &serde_json::json!({ "ok": ok, "seconds": seconds }),
    );
    if ok {
        let _ = ui::success(format!("Built in {seconds:.1}s"));
    }
    ok
}

fn announce_started(child: &Child, what: &str) {
    ui::emit("run.app.start", &serde_json::json!({ "pid": child.id() }));
    let _ = ui::success(format!("{what} (PID {})", child.id()));
}

/// Spawn with the stdio policy the session needs.
///
/// - The app never shares the CLI's stdin: the key reader owns the terminal
///   (and in CI there is nothing to read).
/// - In `--json` mode both of the app's output streams are piped and
///   forwarded line by line as `run.app.log {stream, line}`, so the machine
///   stream on stdout stays one object per line and nothing the app says is
///   lost to a human-only channel. In human mode both are inherited.
fn spawn_child(mut cmd: Command, context: &str) -> CliResult<Child> {
    cmd.stdin(Stdio::null());
    if ui::is_json() {
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
    } else {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }
    let mut child = cmd.spawn().context(context)?;
    if let Some(stdout) = child.stdout.take() {
        forward_app_stream("stdout", stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        forward_app_stream("stderr", stderr);
    }
    Ok(child)
}

/// Forward one of the app's output streams as `run.app.log` events.
fn forward_app_stream<R: std::io::Read + Send + 'static>(stream: &'static str, reader: R) {
    std::thread::Builder::new()
        .name(format!("flui-app-{stream}"))
        .spawn(move || {
            for line in std::io::BufReader::new(reader).lines() {
                match line {
                    Ok(line) => ui::emit(
                        "run.app.log",
                        &serde_json::json!({ "stream": stream, "line": line }),
                    ),
                    Err(_) => break,
                }
            }
        })
        .ok();
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
                    let _ = crate::ui::warning(format!(
                        "Child process (PID {}) did not exit within {:?}",
                        child.id(),
                        timeout
                    ));
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                crate::ui::debug(format!("Error waiting for child: {e}"));
                return;
            }
        }
    }
}

fn log_changed_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = ui::info(format!("Change detected: {}", style(path.display()).dim()));
    }
}

// ============================================================================
// Strategy: process restart
// ============================================================================

/// Rebuild the binary and restart the process on every change.
struct ProcessRestart {
    profile: Option<String>,
    verbose: bool,
}

impl ReloadStrategy for ProcessRestart {
    fn watch_paths(&self) -> Vec<(PathBuf, bool)> {
        vec![
            (PathBuf::from("src"), true),
            (PathBuf::from("Cargo.toml"), false),
        ]
    }

    fn build_and_spawn(&mut self, child: &mut Option<Child>) -> CliResult<()> {
        if !timed_build(|| run_cargo_build(self.profile.as_deref(), self.verbose)) {
            return Ok(());
        }
        let spawned = spawn_app(self.profile.as_deref(), self.verbose)?;
        announce_started(&spawned, "Application started");
        *child = Some(spawned);
        Ok(())
    }

    fn on_change(&mut self, _paths: &[PathBuf], child: &mut Option<Child>) -> CliResult<()> {
        stop_and_report(child);
        ui::step("Rebuilding...")?;
        self.build_and_spawn(child)?;
        ui::emit(
            "run.reload",
            &serde_json::json!({ "kind": "restart", "ok": child.is_some() }),
        );
        if child.is_none() {
            ui::warning("Build failed. Watching for changes... (fix errors and save to retry)")?;
        }
        Ok(())
    }

    fn reload_label(&self) -> &'static str {
        "Rebuild and restart"
    }
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
            let _ = crate::ui::error(format!("failed to run cargo build: {e}"));
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
    spawn_child(cmd, "Failed to spawn application")
}

// ============================================================================
// Strategy: worker host (Flutter-parity hot reload)
// ============================================================================

/// Resolved host/worker project paths for `flui run`.
struct WorkerHotReloadProject {
    config: HotReloadConfig,
    /// Directory containing `flui.toml`.
    config_dir: PathBuf,
    /// Cargo workspace root (`target/` lives here).
    workspace_root: PathBuf,
}

/// Keep the host alive, rebuild the worker `cdylib` on save.
struct WorkerHost {
    project: WorkerHotReloadProject,
    profile: Option<String>,
    verbose: bool,
}

impl WorkerHost {
    fn worker_path(&self) -> PathBuf {
        worker_dylib_path(
            &self.project.workspace_root,
            &self.project.config.worker_lib,
            self.profile.as_deref(),
        )
    }

    fn types_src(&self) -> Option<PathBuf> {
        self.project
            .config
            .types_watch
            .as_ref()
            .map(|p| self.project.config_dir.join(p))
    }

    /// Build worker + host in ONE invocation — see `run_cargo_build_packages`.
    fn build_both(&self) -> bool {
        timed_build(|| {
            run_cargo_build_packages(
                &[
                    &self.project.config.worker_package,
                    &self.project.config.host_package,
                ],
                self.profile.as_deref(),
                self.verbose,
                None,
            )
        })
    }

    /// Stage the freshly built worker and point the sidecar at it.
    fn publish(&self) -> CliResult<PathBuf> {
        let canonical = self.worker_path();
        let staged = stage_worker_artifact(&canonical, &canonical, true)?;
        publish_worker_plugin(&canonical, &staged)?;
        Ok(staged)
    }

    fn spawn_host(&self, staged: &Path) -> CliResult<Child> {
        spawn_host_package(
            &self.project.config.host_package,
            staged,
            self.profile.as_deref(),
            self.verbose,
        )
    }
}

impl ReloadStrategy for WorkerHost {
    fn watch_paths(&self) -> Vec<(PathBuf, bool)> {
        let mut paths = vec![(
            self.project
                .config_dir
                .join(&self.project.config.logic_watch),
            true,
        )];
        if let Some(types) = self.types_src() {
            paths.push((types, true));
        }
        paths
    }

    fn build_and_spawn(&mut self, child: &mut Option<Child>) -> CliResult<()> {
        if !self.build_both() {
            return Ok(());
        }
        let staged = self.publish()?;
        let host = self.spawn_host(&staged)?;
        ui::emit(
            "run.app.start",
            &serde_json::json!({ "pid": host.id(), "worker": staged }),
        );
        ui::success(format!(
            "Host started (PID {}) — worker at {}",
            host.id(),
            style(staged.display()).dim()
        ))?;
        *child = Some(host);
        Ok(())
    }

    fn on_change(&mut self, paths: &[PathBuf], child: &mut Option<Child>) -> CliResult<()> {
        let types_changed = self
            .types_src()
            .is_some_and(|types| paths.iter().any(|p| p.starts_with(&types)));

        if types_changed {
            ui::step("Types changed — rebuilding host (hot restart)...")?;
            stop_and_report(child);
            self.build_and_spawn(child)?;
            ui::emit(
                "run.reload",
                &serde_json::json!({ "kind": "restart", "ok": child.is_some() }),
            );
            if child.is_none() {
                ui::warning("Build failed — fix errors and save to retry")?;
            }
            return Ok(());
        }

        ui::step("Rebuilding worker (state preserved in host)...")?;
        // Rebuild the worker TOGETHER with the host, in the normal target dir:
        // building the worker alone gave it a different compiled instance of
        // the shared framework crates and its `TypeId`s stopped matching the
        // host's. The host itself is unchanged and therefore not relinked.
        if !self.build_both() {
            ui::warning("Worker build failed — fix errors and save to retry")?;
            ui::emit(
                "run.reload",
                &serde_json::json!({ "kind": "hot", "ok": false }),
            );
            return Ok(());
        }
        let staged = self.publish()?;
        let host_alive = matches!(child.as_mut().map(Child::try_wait), Some(Ok(None)));
        if host_alive {
            ui::success("Worker rebuilt — host will hot-reload on next frame (~500ms)")?;
            ui::emit(
                "run.reload",
                &serde_json::json!({ "kind": "hot", "ok": true, "worker": staged }),
            );
        } else {
            let host = self.spawn_host(&staged)?;
            announce_started(&host, "Host restarted");
            *child = Some(host);
            ui::emit(
                "run.reload",
                &serde_json::json!({ "kind": "restart", "ok": true }),
            );
        }
        Ok(())
    }

    fn reload_label(&self) -> &'static str {
        "Rebuild worker (hot reload)"
    }
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
    std::fs::create_dir_all(parent).context("failed to create worker staging directory")?;

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
    let bytes = std::fs::read(built).with_context(|| {
        format!(
            "failed to read the freshly built worker at {}",
            built.display()
        )
    })?;
    let dest = parent.join(format!("{stem}{STAGING_INFIX}{:08x}{ext}", fnv1a(&bytes)));

    // An existing file with this hash already holds these exact bytes. Leave it
    // in place — rewriting a shared library the host may have mapped is what the
    // deferred-unmap hazard turns into a stale image.
    if dest.exists() {
        return Ok(dest);
    }

    std::fs::write(&dest, &bytes)
        .with_context(|| format!("failed to stage worker to {}", dest.display()))?;

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
            ui::debug(format!(
                "ad-hoc signed the staged worker {}",
                path.display()
            ));
        }
        Ok(output) => {
            let _ = ui::warning(format!(
                "codesign of the staged worker {} failed; continuing (load may still succeed): {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Err(error) => {
            let _ = ui::warning(format!(
                "could not run codesign for {}; continuing (load may still succeed): {error}",
                path.display()
            ));
        }
    }
}

/// Write `.flui_worker_plugin` next to the canonical dylib so the host loads
/// the staged artifact without overwriting a locked file.
fn publish_worker_plugin(canonical: &Path, load_path: &Path) -> CliResult<()> {
    let manifest = worker_plugin_manifest_path(canonical);
    if let Some(parent) = manifest.parent() {
        std::fs::create_dir_all(parent).context("failed to create worker manifest directory")?;
    }
    std::fs::write(&manifest, load_path.as_os_str().as_encoded_bytes())
        .with_context(|| format!("failed to write {}", manifest.display()))?;
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

    cmd.env(env::WORKER_PLUGIN, worker_plugin);
    cmd.env(env::HOT_RELOAD, "1");
    spawn_child(cmd, "Failed to spawn host application")
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
            let _ = crate::ui::error(format!("failed to run cargo build for {packages:?}: {e}"));
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
    let mut dir = std::env::current_dir().context("could not read current directory")?;
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

#[derive(serde::Deserialize)]
struct ProjectMetadata {
    packages: Vec<ProjectPackage>,
}

#[derive(serde::Deserialize)]
struct ProjectPackage {
    manifest_path: PathBuf,
    dependencies: Vec<ProjectDependency>,
    #[serde(default)]
    targets: Vec<ProjectTarget>,
}

#[derive(serde::Deserialize)]
struct ProjectTarget {
    kind: Vec<String>,
}

/// What `flui run` found in the working directory.
enum Project {
    /// A single application package (`cargo run` is the entry point).
    Application,
    /// A host/worker workspace declared by `flui.toml`'s `[hot_reload]`.
    Worker(WorkerHotReloadProject),
}

#[derive(serde::Deserialize)]
struct ProjectDependency {
    name: String,
    kind: Option<String>,
}

/// Cargo resolves aliases and workspace inheritance; only normal declarations
/// identify an application, not its test or build tooling.
fn has_flui_dependency(dependencies: &[ProjectDependency]) -> bool {
    dependencies.iter().any(|dependency| {
        dependency.kind.is_none()
            && matches!(
                dependency.name.as_str(),
                "flui" | "flui-app" | "flui-widgets"
            )
    })
}

fn metadata_identifies_project(bytes: &[u8], manifest: &Path) -> CliResult<bool> {
    let metadata: ProjectMetadata = serde_json::from_slice(bytes)
        .context("could not decode Cargo metadata; expected --format-version 1 JSON")?;
    let manifest = manifest
        .canonicalize()
        .context("could not resolve project Cargo.toml")?;
    for package in metadata.packages {
        let package_manifest = package
            .manifest_path
            .canonicalize()
            .context("could not resolve a package manifest reported by Cargo")?;
        if package_manifest == manifest {
            if !has_flui_dependency(&package.dependencies) {
                return Ok(false);
            }
            let has_bin = package
                .targets
                .iter()
                .any(|target| target.kind.iter().any(|kind| kind == "bin"));
            if !package.targets.is_empty() && !has_bin {
                return Err(CliError::NotFluiProject {
                    reason: "this package is a library (no binary target); `flui run` needs an application — run its tests with `flui test`, or use it from an app crate".into(),
                });
            }
            return Ok(true);
        }
    }
    Err(CliError::NotFluiProject {
        reason: "Cargo metadata contains no package for this Cargo.toml; run from an application package directory, not a virtual workspace root".into(),
    })
}

/// Ensure we're in a FLUI project directory and say which kind.
fn ensure_flui_project() -> CliResult<Project> {
    if let Some(project) = find_worker_hot_reload_project()? {
        return Ok(Project::Worker(project));
    }

    let cargo_toml = Path::new("Cargo.toml");

    if !cargo_toml.exists() {
        return Err(CliError::NotFluiProject {
            reason: "Cargo.toml not found".to_string(),
        });
    }

    let output = Command::new("cargo")
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(cargo_toml)
        .output()
        .context("could not run cargo metadata; ensure Cargo is installed and on PATH")?;
    if !output.status.success() {
        return Err(CliError::NotFluiProject {
            reason: format!(
                "cargo metadata failed; fix the project manifest: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    if !metadata_identifies_project(&output.stdout, cargo_toml)? {
        return Err(CliError::NotFluiProject {
            reason: "normal dependency on flui (or legacy flui-app/flui-widgets) not found in this package".into(),
        });
    }

    Ok(Project::Application)
}

/// Execute scene-only hot-reload mode for Android.
///
/// Watches the scene crate's `src/` directory for changes and rebuilds/pushes
/// the scene plugin `.so` to the device without restarting the app.
/// The host app detects the new `.so` via mtime polling and reloads automatically.
pub(crate) fn execute_scene(
    scene_crate: &str,
    package: &str,
    target: &str,
    release: bool,
    _verbose: bool,
) -> CliResult<()> {
    use crate::build::android::AndroidBuilder;

    let mode = if release { "release" } else { "debug" };
    ui::intro(style(" flui run --scene ").on_magenta().black())?;
    ui::info(format!(
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
    ui::step("Building scene plugin...")?;
    let start = Instant::now();

    let rt = tokio::runtime::Runtime::new()?;
    let so_path = rt
        .block_on(builder.build_scene_plugin(target, scene_crate, release))
        .map_err(|e| CliError::BuildFailed {
            platform: "android".to_string(),
            details: e.to_string(),
        })?;

    ui::success(format!(
        "Built in {:.2}s: {}",
        start.elapsed().as_secs_f64(),
        style(so_path.display()).dim()
    ))?;

    ui::step("Pushing to device...")?;
    rt.block_on(builder.push_scene_plugin(&so_path, package, lib_name))
        .map_err(|e| CliError::BuildFailed {
            platform: "android".to_string(),
            details: e.to_string(),
        })?;
    ui::success("Plugin pushed to device")?;

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

    ui::info(format!(
        "Watching {} for changes...",
        style(scene_src.display()).dim()
    ))?;

    let mut watcher = SourceWatcher::with_debounce(timing::ANDROID_SCENE_DEBOUNCE)
        .map_err(|e| CliError::context(e, "Failed to create file watcher"))?;

    watcher
        .watch(&scene_src, true)
        .map_err(|e| CliError::context(e, "Failed to watch scene crate"))?;

    let interrupted = install_interrupt_flag();
    loop {
        let paths = match watcher.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(paths)) => paths,
            Ok(None) | Err(RecvTimeoutError::Timeout) => {
                if interrupted.swap(false, Ordering::SeqCst) {
                    return Err(CliError::Interrupted);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        log_changed_paths(&paths);

        let start = Instant::now();
        ui::step("Rebuilding scene plugin...")?;

        match rt.block_on(builder.build_scene_plugin(target, scene_crate, release)) {
            Ok(so) => {
                let build_time = start.elapsed();
                ui::step("Pushing to device...")?;
                match rt.block_on(builder.push_scene_plugin(&so, package, lib_name)) {
                    Ok(()) => {
                        ui::success(format!("Updated in {:.2}s", build_time.as_secs_f64()))?;
                    }
                    Err(e) => {
                        ui::warning(format!("Push failed: {e}"))?;
                    }
                }
            }
            Err(e) => {
                ui::warning(format!("Build failed: {e}"))?;
                ui::info("Fix errors and save to retry...")?;
            }
        }
    }

    ui::outro(style("Scene hot-reload stopped").green())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::HotKey;
    use super::{env, fnv1a, has_flui_dependency, is_host_device, stage_worker_artifact};

    /// The env-var names are duplicated from `flui-hot-reload` so the CLI
    /// does not link the framework; this is the only place that proves they
    /// still agree.
    #[test]
    fn env_names_match_the_runtime() {
        assert_eq!(env::HOT_RELOAD, flui_hot_reload::strategy::env::HOT_RELOAD);
        assert_eq!(
            env::WORKER_PLUGIN,
            flui_hot_reload::engine::env::WORKER_PLUGIN
        );
    }

    #[test]
    fn host_device_aliases() {
        for name in ["desktop", "Desktop", "host", " this ", std::env::consts::OS] {
            assert!(is_host_device(name), "{name}");
        }
        for name in ["emulator-5554", "iPhone 17 Pro", "", "linuxx"] {
            assert!(
                !is_host_device(name) || name == std::env::consts::OS,
                "{name}"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn hot_key_bindings() {
        assert_eq!(HotKey::from_byte(b'r'), Some(HotKey::Reload));
        assert_eq!(HotKey::from_byte(b'R'), Some(HotKey::Restart));
        assert_eq!(HotKey::from_byte(b'c'), Some(HotKey::Clear));
        assert_eq!(HotKey::from_byte(b'h'), Some(HotKey::Help));
        assert_eq!(HotKey::from_byte(b'?'), Some(HotKey::Help));
        assert_eq!(HotKey::from_byte(b'q'), Some(HotKey::Quit));
        assert_eq!(
            HotKey::from_byte(0x03),
            None,
            "Ctrl-C is a signal, not a key"
        );
        assert_eq!(HotKey::from_byte(b'x'), None);
    }

    #[test]
    fn device_selection_prefers_exact_id_then_name_then_unique_prefix() {
        use crate::DevicePlatform;
        use crate::commands::devices::{Device, Kind, Status};
        let device = |id: &str, name: &str| Device {
            id: id.into(),
            name: name.into(),
            platform: DevicePlatform::Ios,
            kind: Kind::Simulator,
            status: Status::Shutdown,
            details: std::collections::BTreeMap::default(),
        };
        let devices = vec![
            device("AAAA-1", "iPhone 17"),
            device("AAAA-2", "iPhone 17 Pro"),
            device("BBBB-1", "iPad Air"),
        ];
        assert_eq!(
            super::select_device(&devices, "AAAA-2").unwrap().name,
            "iPhone 17 Pro"
        );
        assert_eq!(
            super::select_device(&devices, "iphone 17").unwrap().id,
            "AAAA-1"
        );
        assert_eq!(super::select_device(&devices, "ipad").unwrap().id, "BBBB-1");
        let ambiguous = super::select_device(&devices, "AAAA").unwrap_err();
        assert!(ambiguous.to_string().contains("ambiguous"), "{ambiguous}");
        assert_eq!(ambiguous.exit_code(), 5);
        let missing = super::select_device(&devices, "pixel").unwrap_err();
        assert_eq!(missing.exit_code(), 5);
    }

    #[test]
    fn project_identity_requires_exact_normal_framework_dependencies() {
        for name in [
            "flui",
            "flui-app",
            "flui-widgets",
            "flui-extra",
            "flui_widgets",
            "serde",
        ] {
            for kind in [None, Some("dev"), Some("build")] {
                let dependencies = [super::ProjectDependency {
                    name: name.into(),
                    kind: kind.map(str::to_owned),
                }];
                assert_eq!(
                    has_flui_dependency(&dependencies),
                    kind.is_none() && matches!(name, "flui" | "flui-app" | "flui-widgets")
                );
            }
        }
    }

    #[test]
    fn malformed_metadata_has_decode_context() {
        let error =
            super::metadata_identifies_project(b"not JSON", std::path::Path::new("Cargo.toml"))
                .expect_err("invalid metadata");
        assert!(
            error
                .to_string()
                .contains("could not decode Cargo metadata")
        );
    }

    #[cfg(unix)]
    #[test]
    fn metadata_matches_canonical_manifest_identity() {
        let tmp = tempfile::TempDir::new().expect("temporary manifest");
        let manifest = tmp.path().join("Cargo.toml");
        std::fs::write(&manifest, "").expect("manifest file");
        let alias = tmp.path().join("alias.toml");
        std::os::unix::fs::symlink(&manifest, &alias).expect("manifest alias");
        let bytes = serde_json::to_vec(&serde_json::json!({
            "packages": [{"manifest_path": manifest, "dependencies": [{"name": "flui", "kind": null}]}]
        })).expect("metadata JSON");
        assert!(super::metadata_identifies_project(&bytes, &alias).expect("canonical identity"));
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
