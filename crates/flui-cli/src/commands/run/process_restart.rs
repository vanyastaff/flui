//! The process restart: `flui run` of a desktop application rebuilds it beside the running
//! build on each change and replaces that build only with one that succeeded and changed,
//! showing each rebuild over the app's windows (`build.report`, ADR-0395).
//!
//! The run starts the executable its build wrote, as `cargo run` would after the same build:
//! `cargo run` checks the sources once more as it starts, so an edit saved while the app starts
//! would run a build nobody reported. A build cargo did not redo leaves the app running.

use std::cell::OnceCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use flui_protocol::build::{BuildState, Diagnostics};
use flui_protocol::reload::{ReloadTier, Reloaded};

use super::{
    AppChild, DesktopBuild, ReloadStrategy, announce_started, env, reap_exited, spawn_child,
    stop_and_report, timed_build, with_asset_root, with_build, with_registration,
};
use crate::agent::Session;
use crate::agent::build_report::BuildReporter;
use crate::build::util::cargo_messages::{self, Executable};
use crate::error::{CliError, CliResult, ResultExt as _};
use crate::ui;

/// Rebuild the binary and restart the process on every change that rebuilt it.
pub(super) struct ProcessRestart {
    /// The binary and features each build names.
    pub(super) build: DesktopBuild,
    pub(super) profile: Option<String>,
    pub(super) verbose: bool,
    /// The session the app is started with (its endpoint and token): `--agent`'s, built with the
    /// `driver` feature for a profile without debug assertions, or the unlisted one the run opens
    /// only to report its builds.
    pub(super) agent: Option<Session>,
    /// Shows each rebuild over the running app's windows: the session's
    /// agent, as an observer. `None` with `--no-build-overlay`, or when it could not start.
    pub(super) build_report: Option<BuildReporter>,
    /// `--register-associations`: the app registers its URL schemes and file
    /// types with Windows.
    pub(super) register_associations: bool,
    /// The build the running child was started from.
    pub(super) started: Option<Stamp>,
    /// Where the toolchain keeps the target's own dynamic libraries, read once.
    pub(super) toolchain_libraries: OnceCell<Option<PathBuf>>,
}

/// Which build of the executable a child was started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp {
    modified: SystemTime,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok()?,
            len: metadata.len(),
        })
    }
}

/// A build that succeeded: the executable it wrote and the directories it links from.
struct Built {
    executable: Executable,
    linked_paths: Vec<PathBuf>,
}

impl ReloadStrategy for ProcessRestart {
    fn watch_paths(&self) -> Vec<(PathBuf, bool)> {
        vec![
            (PathBuf::from("src"), true),
            (PathBuf::from("Cargo.toml"), false),
        ]
    }

    fn build_and_spawn(&mut self, child: &mut Option<AppChild>) -> CliResult<Reloaded> {
        match self.build() {
            Ok(built) => {
                self.spawn(&built, child)?;
                Ok(Reloaded::applied(ReloadTier::ProcessRestart))
            }
            Err(errors) => Ok(Reloaded::Failed(errors)),
        }
    }

    /// Rebuild while the app keeps running, and replace it only with a build
    /// that succeeded: a compile error leaves the window up with the
    /// previous build while the author fixes it, and a good build replaces
    /// it without the app being gone for the length of the build. A build
    /// cargo did not redo, of the executable the app was started from, leaves
    /// the app as it runs: a save and a reload of one edit restart it once.
    /// Windows refuses to overwrite a running executable, so there the app
    /// stops before the build, and a failed build leaves no app running.
    ///
    /// The app that keeps running shows the build over its windows: that it
    /// runs, then where it failed. A build that succeeded is reported too
    /// before the app is replaced, so a report still on its way lands on a
    /// state that shows nothing, whichever app it reaches.
    fn on_change(
        &mut self,
        _paths: &[PathBuf],
        child: &mut Option<AppChild>,
    ) -> CliResult<Reloaded> {
        if cfg!(windows) {
            stop_and_report(child);
        }
        ui::step("Rebuilding...")?;
        self.show(BuildState::Building);
        let built = self.build();
        // The app was not polled while the build ran.
        reap_exited(child);
        let errors = match built {
            Ok(built) => {
                self.show(BuildState::Succeeded);
                if self.runs(&built, child.as_ref()) {
                    ui::info("The application already runs this build")?;
                    return Ok(Reloaded::Unchanged);
                }
                stop_and_report(child);
                self.spawn(&built, child)?;
                return Ok(Reloaded::applied(ReloadTier::ProcessRestart));
            }
            Err(errors) => errors,
        };
        self.show(BuildState::Failed(errors.clone()));
        if self.is_running(child.as_ref()) {
            ui::warning(
                "Build failed. The application keeps running its previous build... \
                 (fix errors and save to retry)",
            )?;
        } else {
            ui::warning("Build failed. Watching for changes... (fix errors and save to retry)")?;
        }
        Ok(Reloaded::Failed(errors))
    }

    fn reload_label(&self) -> &'static str {
        "Rebuild, and restart if it changed"
    }
}

impl ProcessRestart {
    /// Whether `child` runs the executable `built` names, as that build left it: cargo did not
    /// build it again, and it is the file the child was started from, so a build of another
    /// terminal's that replaced it is a change. Both are read: a file system that keeps
    /// modification times to the second can give a build in the same second the same stamp.
    fn runs(&self, built: &Built, child: Option<&AppChild>) -> bool {
        child.is_some()
            && built.executable.fresh
            && self
                .started
                .is_some_and(|started| Stamp::of(&built.executable.path) == Some(started))
    }

    /// `cargo build` of the application, reported as `run.build.*`; a build
    /// that failed answers its compiler errors.
    fn build(&self) -> Result<Built, Diagnostics> {
        let mut outcome = None;
        timed_build(|| {
            let built = run_cargo_build(
                &self.build,
                self.profile.as_deref(),
                self.verbose,
                self.agent.as_ref(),
            );
            let ok = built.is_ok();
            outcome = Some(built);
            ok
        });
        outcome.unwrap_or_else(|| Err(Diagnostics::new()))
    }

    /// Show `state` over the running app's windows, unless `--no-build-overlay`.
    fn show(&self, state: BuildState) {
        if let Some(reporter) = &self.build_report {
            reporter.report(state);
        }
    }

    /// Start the executable `built` wrote into `child`.
    fn spawn(&mut self, built: &Built, child: &mut Option<AppChild>) -> CliResult<()> {
        if let Some(session) = &self.agent {
            session.clear_endpoint();
        }
        let root = std::env::current_dir().context("failed to resolve application directory")?;
        let mut cmd = Command::new(&built.executable.path);
        cmd.envs(crate::config::project_floors(&root)?.host_cargo_env());
        if let Some(session) = &self.agent {
            session.apply(&mut cmd);
        }
        with_registration(&mut cmd, self.register_associations);
        with_asset_root(&mut cmd, &root);
        cmd.env(env::HOT_RELOAD, "1");
        let toolchain = self
            .toolchain_libraries
            .get_or_init(toolchain_libraries)
            .as_deref();
        let output = profile_output(&built.executable.path);
        let (variable, value) = library_path(
            &built.linked_paths,
            output,
            toolchain,
            std::env::var_os(LIBRARY_PATH_VARIABLE),
        );
        cmd.env(variable, value);
        let started = Stamp::of(&built.executable.path);
        let spawned = spawn_child(cmd, "Failed to start the application")?;
        announce_started(&spawned, "Application started");
        *child = Some(spawned);
        self.started = started;
        Ok(())
    }
}

/// The variable the platform's loader searches for dynamic libraries, which `cargo run` sets.
const LIBRARY_PATH_VARIABLE: &str = if cfg!(windows) {
    "PATH"
} else if cfg!(target_os = "macos") {
    "DYLD_FALLBACK_LIBRARY_PATH"
} else {
    "LD_LIBRARY_PATH"
};

/// The profile's output directory (`target/debug`) of `executable`: its directory, or that
/// directory's parent for an example, which cargo writes to `target/debug/examples`.
fn profile_output(executable: &Path) -> Option<&Path> {
    let directory = executable.parent()?;
    if directory.file_name() == Some(std::ffi::OsStr::new("examples")) {
        directory.parent()
    } else {
        Some(directory)
    }
}

/// The value of [`LIBRARY_PATH_VARIABLE`] the app starts with, as `cargo run` builds it: the
/// directories the build scripts link from that lie inside the profile's `output`, its `deps`
/// and itself, the toolchain's libraries for the target (a `-C prefer-dynamic` build links its
/// standard library from there), then what the variable held. On macOS an unset variable keeps
/// the loader's own fallback (`~/lib`, `/usr/local/lib`, `/usr/lib`).
fn library_path(
    linked_paths: &[PathBuf],
    output: Option<&Path>,
    toolchain: Option<&Path>,
    inherited: Option<OsString>,
) -> (&'static str, OsString) {
    let mut directories = Vec::new();
    if let Some(output) = output {
        directories.extend(
            linked_paths
                .iter()
                .map(|path| strip_link_kind(path))
                .filter(|path| path.starts_with(output)),
        );
        directories.push(output.join("deps"));
        directories.push(output.to_path_buf());
    }
    directories.extend(toolchain.map(Path::to_path_buf));
    match inherited {
        Some(inherited) => directories.extend(std::env::split_paths(&inherited)),
        None if cfg!(target_os = "macos") => {
            if let Some(home) = std::env::var_os("HOME") {
                directories.push(PathBuf::from(home).join("lib"));
            }
            directories.push(PathBuf::from("/usr/local/lib"));
            directories.push(PathBuf::from("/usr/lib"));
        }
        None => {}
    }
    let value = std::env::join_paths(&directories).unwrap_or_else(|_| {
        // A directory whose name holds the separator cannot be listed: the variable is left
        // as the run found it.
        std::env::var_os(LIBRARY_PATH_VARIABLE).unwrap_or_default()
    });
    (LIBRARY_PATH_VARIABLE, value)
}

/// `path` without the kind a build script names it with (`native=`, `dependency=`, `crate=`,
/// `framework=`, `all=`), as cargo reads `cargo::rustc-link-search`.
fn strip_link_kind(path: &Path) -> PathBuf {
    let stripped = path.to_str().and_then(|text| {
        let (kind, rest) = text.split_once('=')?;
        matches!(
            kind,
            "native" | "dependency" | "crate" | "framework" | "all"
        )
        .then_some(rest)
    });
    stripped.map_or_else(|| path.to_path_buf(), PathBuf::from)
}

/// The toolchain's library directory for the host target (`rustc --print target-libdir`), from
/// the `rustc` cargo runs; `None` when it cannot be read.
fn toolchain_libraries() -> Option<PathBuf> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .args(["--print", "target-libdir"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let directory = text.trim();
    (!directory.is_empty()).then(|| PathBuf::from(directory))
}

/// Run `cargo build`; a build that failed answers its compiler errors.
///
/// Cargo writes its messages as JSON, which the run reads for the errors it
/// shows in the app and for the executable it starts, and each message's text
/// is written to stderr as cargo would have written it: in colour on a
/// terminal that takes it.
fn run_cargo_build(
    build: &DesktopBuild,
    profile: Option<&str>,
    verbose: bool,
    agent: Option<&Session>,
) -> Result<Built, Diagnostics> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    // The project's platform floors, which the build compiles against: a
    // refused `flui.toml` is reported as the app starts.
    if let Ok(floors) = std::env::current_dir()
        .map_err(CliError::from)
        .and_then(|root| crate::config::project_floors(&root))
    {
        cmd.envs(floors.host_cargo_env());
    }
    with_build(&mut cmd, build, agent);
    // Cargo's own colour switch decides, then the terminal and `NO_COLOR`.
    let colour = match std::env::var("CARGO_TERM_COLOR").as_deref() {
        Ok("always") => true,
        Ok("never") => false,
        _ => {
            std::io::IsTerminal::is_terminal(&std::io::stderr())
                && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
        }
    };
    cmd.arg(if colour {
        "--message-format=json-diagnostic-rendered-ansi"
    } else {
        "--message-format=json"
    });
    cmd.stdout(Stdio::piped());

    let failed = |why: String| {
        let _ = crate::ui::error(why);
        Err(Diagnostics::new())
    };
    let mut cargo = match cmd.spawn() {
        Ok(cargo) => cargo,
        Err(e) => return failed(format!("failed to run cargo build: {e}")),
    };
    let stdout = cargo
        .stdout
        .take()
        .expect("BUG: cargo's stdout is piped above");
    let messages = cargo_messages::read(
        std::io::BufReader::new(stdout),
        // Locked per message, not for the whole build: the reporter and the watcher log too.
        &mut std::io::stderr(),
    );
    // A read that stopped early leaves cargo blocked on a full pipe: it is
    // stopped, and reaped whatever the read came to.
    if messages.is_err() {
        let _ = cargo.kill();
    }
    let status = cargo.wait();
    match (messages, status) {
        (Ok(messages), Ok(status)) if status.success() => match messages.executable {
            Some(executable) => Ok(Built {
                executable,
                linked_paths: messages.linked_paths,
            }),
            None => failed("cargo build reported no executable to start".to_owned()),
        },
        (Ok(messages), Ok(_)) => Err(messages.errors),
        (Err(e), _) => failed(format!("failed to read cargo build's messages: {e}")),
        (_, Err(e)) => failed(format!("failed to wait for cargo build: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{LIBRARY_PATH_VARIABLE, library_path, profile_output};

    /// The app's library path is `cargo run`'s: the link directories inside the profile's output
    /// first, its `deps` and itself, the toolchain's, then what the run inherited; a directory
    /// outside the output is not searched.
    #[test]
    fn the_app_searches_the_libraries_cargo_run_would() {
        let output = Path::new("/app/target/debug");
        let linked = [
            PathBuf::from("native=/app/target/debug/build/sys-1/out"),
            PathBuf::from("native=/opt/vendor/lib"),
            PathBuf::from("/app/target/debug/build/bare-2/out"),
        ];
        let inherited = std::env::join_paths([Path::new("/inherited")]).expect("a path list");
        let (variable, value) = library_path(
            &linked,
            Some(output),
            Some(Path::new("/toolchain/lib")),
            Some(inherited),
        );
        assert_eq!(variable, LIBRARY_PATH_VARIABLE);
        let searched = std::env::split_paths(&value).collect::<Vec<PathBuf>>();
        assert_eq!(
            searched,
            [
                PathBuf::from("/app/target/debug/build/sys-1/out"),
                PathBuf::from("/app/target/debug/build/bare-2/out"),
                PathBuf::from("/app/target/debug/deps"),
                PathBuf::from("/app/target/debug"),
                PathBuf::from("/toolchain/lib"),
                PathBuf::from("/inherited"),
            ]
        );
    }

    /// An example's executable sits one directory below its profile's output.
    #[test]
    fn an_example_runs_from_below_its_profile_output() {
        assert_eq!(
            profile_output(Path::new("/app/target/debug/app")),
            Some(Path::new("/app/target/debug"))
        );
        assert_eq!(
            profile_output(Path::new("/app/target/debug/examples/demo")),
            Some(Path::new("/app/target/debug"))
        );
    }
}
