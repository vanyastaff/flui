//! A scripted [`LauncherBackend`] for scenarios: the double of the launcher
//! contract (ADR-0154 §10, ADR-0243 D4), on the scenario primitive.

use std::path::PathBuf;

use crate::capability::Resolver;
use crate::capability::scripted::{Answer, Scenario, Script, ScriptHold};
use crate::launch::{ExternalUri, LaunchResult, LauncherBackend};

/// One request a [`ScriptedLauncher`] received.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LaunchRequest {
    /// [`Launcher::open_uri`](crate::launch::Launcher::open_uri).
    OpenUri(ExternalUri),
    /// [`Launcher::open_path`](crate::launch::Launcher::open_path).
    OpenPath(PathBuf),
    /// [`Launcher::reveal_path`](crate::launch::Launcher::reveal_path).
    RevealPath(PathBuf),
    /// [`Launcher::trash_path`](crate::launch::Launcher::trash_path).
    TrashPath(PathBuf),
}

/// A launcher whose system is a [`Script`] of [`LaunchRequest`]s: it records
/// every request and answers it as the script says. A request the script
/// does not answer is the scenario's
/// [`Unexpected`](crate::capability::scripted::ScenarioFailure::Unexpected)
/// failure, and its completion ends
/// [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped): there
/// is no answer nobody wrote.
///
/// Cheap to clone; clones share the script, so a test keeps one and hands
/// another to the provider.
#[derive(Clone, Debug)]
pub struct ScriptedLauncher {
    script: Script<LaunchRequest, LaunchResult>,
}

impl Default for ScriptedLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptedLauncher {
    /// A launcher on a scenario of its own, with nothing scripted.
    #[must_use]
    pub fn new() -> Self {
        Self::on(&Scenario::new())
    }

    /// A launcher whose script is on `scenario`, with nothing scripted.
    #[must_use]
    pub fn on(scenario: &Scenario) -> Self {
        Self {
            script: scenario.script("launcher"),
        }
    }

    /// The script: steps matched by request, standing answers, holds.
    #[must_use]
    pub fn script(&self) -> &Script<LaunchRequest, LaunchResult> {
        &self.script
    }

    /// Answer the next request, whichever it is, with `result`.
    pub fn answer_next(&self, result: LaunchResult) {
        self.script.answer_next(Answer::value(result));
    }

    /// Hold every later request while the returned hold lives
    /// ([`Script::hold`]); [`release`](Self::release) answers a held one.
    #[must_use = "requests are held only while the hold is alive"]
    pub fn hold(&self) -> ScriptHold {
        self.script.hold()
    }

    /// Answer the oldest held request with `result`; `false` when none is
    /// held.
    pub fn release(&self, result: LaunchResult) -> bool {
        self.script.release_oldest(result)
    }

    /// The requests received so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<LaunchRequest> {
        self.script.requests()
    }
}

impl LauncherBackend for ScriptedLauncher {
    fn open_uri(&self, uri: ExternalUri, answer: Resolver<LaunchResult>) {
        self.script.receive(LaunchRequest::OpenUri(uri), answer);
    }

    fn open_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.script.receive(LaunchRequest::OpenPath(path), answer);
    }

    fn reveal_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.script.receive(LaunchRequest::RevealPath(path), answer);
    }

    fn trash_path(&self, path: PathBuf, answer: Resolver<LaunchResult>) {
        self.script.receive(LaunchRequest::TrashPath(path), answer);
    }
}
