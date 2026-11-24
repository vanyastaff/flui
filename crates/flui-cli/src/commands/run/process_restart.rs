//! The process restart: `flui run` of a desktop application rebuilds it beside the running
//! build on each change and replaces that build only with one that succeeded, showing each
//! rebuild over the app's windows (`build.report`, ADR-0395).

use std::path::PathBuf;
use std::process::{Command, Stdio};

use flui_protocol::build::{BuildState, Diagnostics};
use flui_protocol::reload::{ReloadTier, Reloaded};

use super::{
    AppChild, DesktopBuild, ReloadStrategy, announce_started, reap_exited, spawn_app,
    stop_and_report, timed_build, with_build,
};
use crate::agent::Session;
use crate::agent::build_report::BuildReporter;
use crate::build::util::compile_errors;
use crate::error::{CliError, CliResult};
use crate::ui;
/// Rebuild the binary and restart the process on every change.
pub(super) struct ProcessRestart {
    /// The binary and features each build and run names.
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
            Ok(()) => {
                self.spawn(child)?;
                Ok(Reloaded::applied(ReloadTier::ProcessRestart))
            }
            Err(errors) => Ok(Reloaded::Failed(errors)),
        }
    }

    /// Rebuild while the app keeps running, and replace it only with a build
    /// that succeeded: a compile error leaves the window up with the
    /// previous build while the author fixes it, and a good build replaces
    /// it without the app being gone for the length of the build. Windows
    /// refuses to overwrite a running executable, so there the app stops
    /// before the build, and a failed build leaves no app running.
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
        let reloaded = match self.build() {
            Ok(()) => {
                self.show(BuildState::Succeeded);
                stop_and_report(child);
                self.spawn(child)?;
                return Ok(Reloaded::applied(ReloadTier::ProcessRestart));
            }
            Err(errors) => {
                self.show(BuildState::Failed(errors.clone()));
                Reloaded::Failed(errors)
            }
        };
        // The app was not polled while the build ran.
        reap_exited(child);
        if self.is_running(child.as_ref()) {
            ui::warning(
                "Build failed. The application keeps running its previous build... \
                 (fix errors and save to retry)",
            )?;
        } else {
            ui::warning("Build failed. Watching for changes... (fix errors and save to retry)")?;
        }
        Ok(reloaded)
    }

    fn reload_label(&self) -> &'static str {
        "Rebuild and restart"
    }
}

impl ProcessRestart {
    /// `cargo build` of the application, reported as `run.build.*`; a build
    /// that failed answers its compiler errors.
    fn build(&self) -> Result<(), Diagnostics> {
        let mut failed = None;
        let built = timed_build(|| {
            run_cargo_build(
                &self.build,
                self.profile.as_deref(),
                self.verbose,
                self.agent.as_ref(),
            )
            .map_err(|errors| failed = Some(errors))
            .is_ok()
        });
        if built {
            Ok(())
        } else {
            Err(failed.unwrap_or_default())
        }
    }

    /// Show `state` over the running app's windows, unless `--no-build-overlay`.
    fn show(&self, state: BuildState) {
        if let Some(reporter) = &self.build_report {
            reporter.report(state);
        }
    }

    /// Start the built application into `child`.
    fn spawn(&self, child: &mut Option<AppChild>) -> CliResult<()> {
        if let Some(session) = &self.agent {
            session.clear_endpoint();
        }
        let spawned = spawn_app(
            &self.build,
            self.profile.as_deref(),
            self.verbose,
            self.agent.as_ref(),
            self.register_associations,
        )?;
        announce_started(&spawned, "Application started");
        *child = Some(spawned);
        Ok(())
    }
}

/// Run `cargo build`; a build that failed answers its compiler errors.
///
/// Cargo writes its messages as JSON, which the run reads for the errors it
/// shows in the app, and each message's text is written to stderr as cargo
/// would have written it: in colour on a terminal that takes it.
fn run_cargo_build(
    build: &DesktopBuild,
    profile: Option<&str>,
    verbose: bool,
    agent: Option<&Session>,
) -> Result<(), Diagnostics> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if let Some(prof) = profile {
        cmd.args(["--profile", prof]);
    }

    if verbose {
        cmd.arg("--verbose");
    }

    // The same floors as the run that follows, or the build script reruns:
    // a refused `flui.toml` is reported by that run.
    if let Ok(floors) = std::env::current_dir()
        .map_err(CliError::from)
        .and_then(|root| crate::config::project_floors(&root))
    {
        cmd.envs(floors.host_cargo_env());
    }
    // The same features as the run that follows, or cargo builds twice.
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
    let errors = compile_errors::read(
        std::io::BufReader::new(stdout),
        // Locked per message, not for the whole build: the reporter and the watcher log too.
        &mut std::io::stderr(),
    );
    // A read that stopped early leaves cargo blocked on a full pipe: it is
    // stopped, and reaped whatever the read came to.
    if errors.is_err() {
        let _ = cargo.kill();
    }
    let status = cargo.wait();
    match (errors, status) {
        (Ok(_), Ok(status)) if status.success() => Ok(()),
        (Ok(errors), Ok(_)) => Err(errors),
        (Err(e), _) => failed(format!("failed to read cargo build's messages: {e}")),
        (_, Err(e)) => failed(format!("failed to wait for cargo build: {e}")),
    }
}
