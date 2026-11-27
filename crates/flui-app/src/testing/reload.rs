//! A development reload hook a test scripts (ADR-0393 §3, §4): it reports the
//! versions the test loads, runs the entry the test gives it when the loop
//! restarts, and keeps what the loop reported back.

use std::sync::Arc;

use flui_view::dev_reload::{
    CodeVersion, DevReloadHook, ReloadEvent, ReloadOutcome, ReloadWake, RestartFailure, Unapplied,
};
use parking_lot::Mutex;

/// A [`DevReloadHook`] a test drives: installed with
/// [`AppConfig::dev_reload`](crate::AppConfig::dev_reload) on a
/// [`HeadlessRun`](super::HeadlessRun), it reports the version
/// [`load`](Self::load) gave last at the next poll, runs the
/// [`entry`](Self::entry) when the loop restarts, and keeps every outcome
/// for [`take_outcomes`](Self::take_outcomes). Clones share one script.
#[derive(Clone, Default)]
pub struct ScriptedReload(Arc<Mutex<Script>>);

#[derive(Default)]
struct Script {
    /// The version loaded since the last poll: a later load replaces it, as
    /// a hook reports only the version loaded now.
    loaded: Option<CodeVersion>,
    wake: Option<ReloadWake>,
    entry: Option<Box<dyn FnMut() + Send>>,
    outcomes: Vec<ScriptedOutcome>,
    attached: bool,
    entries_run: usize,
}

impl std::fmt::Debug for ScriptedReload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let script = self.0.lock();
        f.debug_struct("ScriptedReload")
            .field("loaded", &script.loaded)
            .field("attached", &script.attached)
            .finish_non_exhaustive()
    }
}

impl ScriptedReload {
    /// A hook with nothing loaded and no entry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The next poll reports `version` loaded, in place of a version loaded
    /// before it that no poll reported; the loop is woken to poll it,
    /// as a tool that finished a build wakes it.
    pub fn load(&self, version: CodeVersion) {
        let wake = {
            let mut script = self.0.lock();
            script.loaded = Some(version);
            script.wake.clone()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }

    /// What a restart runs as the application's entry: the code that calls
    /// [`Application::run`](crate::Application::run). Without one, a restart
    /// is handed no application.
    pub fn entry(&self, entry: impl FnMut() + Send + 'static) {
        self.0.lock().entry = Some(Box::new(entry));
    }

    /// What the loop reported since the last call, in order.
    #[must_use]
    pub fn take_outcomes(&self) -> Vec<ScriptedOutcome> {
        std::mem::take(&mut self.0.lock().outcomes)
    }

    /// How many times a restart ran the entry.
    #[must_use]
    pub fn entries_run(&self) -> usize {
        self.0.lock().entries_run
    }

    /// Whether a loop has the hook attached.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.0.lock().attached
    }
}

impl DevReloadHook for ScriptedReload {
    fn attach(&mut self, wake: ReloadWake) {
        let mut script = self.0.lock();
        script.attached = true;
        script.wake = Some(wake);
    }

    fn detach(&mut self) {
        let wake = {
            let mut script = self.0.lock();
            script.attached = false;
            script.wake.take()
        };
        drop(wake);
    }

    fn poll(&mut self) -> ReloadEvent {
        self.0
            .lock()
            .loaded
            .take()
            .map_or(ReloadEvent::Unchanged, ReloadEvent::Loaded)
    }

    fn run_entry(&mut self) {
        // Taken out, so the entry runs with no lock held: it may call back
        // into this hook's handle.
        let entry = {
            let mut script = self.0.lock();
            script.entries_run += 1;
            script.entry.take()
        };
        let Some(entry) = entry else {
            return;
        };
        let mut restore = Restore {
            script: &self.0,
            entry: Some(entry),
        };
        (restore
            .entry
            .as_mut()
            .expect("BUG: the guard holds the entry while it runs"))();
    }

    fn applied(&mut self, outcome: &ReloadOutcome<'_>) {
        let report = match outcome {
            ReloadOutcome::Reassembled { version, .. } => ScriptedOutcome::Reassembled(*version),
            ReloadOutcome::Restarted {
                version, unapplied, ..
            } => ScriptedOutcome::Restarted(*version, *unapplied),
            ReloadOutcome::RestartFailed {
                version, reason, ..
            } => ScriptedOutcome::RestartFailed(*version, RestartReason::of(reason)),
            _ => ScriptedOutcome::Other,
        };
        self.0.lock().outcomes.push(report);
    }
}

/// Puts the entry back after it ran, an entry that panicked included, so the
/// next restart runs it again.
struct Restore<'a> {
    script: &'a Mutex<Script>,
    entry: Option<Box<dyn FnMut() + Send>>,
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        let mut script = self.script.lock();
        if script.entry.is_none() {
            script.entry = self.entry.take();
        }
    }
}

/// One outcome the loop reported to a [`ScriptedReload`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScriptedOutcome {
    /// A UI runtime reassembled to the version.
    Reassembled(CodeVersion),
    /// The application was composed again from the version, leaving what
    /// the restart did not apply.
    Restarted(CodeVersion, Unapplied),
    /// The restart to the version composed no application.
    RestartFailed(CodeVersion, RestartReason),
    /// An outcome this kit does not name yet.
    Other,
}

/// Why a restart reported to a [`ScriptedReload`] composed no application.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RestartReason {
    /// The entry panicked.
    EntryPanicked,
    /// The entry handed no application over.
    NoApplication,
    /// The host refused the application, or composing it failed: the error's
    /// text.
    Launch(String),
    /// Code the composition ran panicked.
    ComposePanicked,
    /// A reason this kit does not name yet.
    Other,
}

impl RestartReason {
    fn of(reason: &RestartFailure<'_>) -> Self {
        match reason {
            RestartFailure::EntryPanicked => Self::EntryPanicked,
            RestartFailure::NoApplication => Self::NoApplication,
            RestartFailure::Launch(error) => Self::Launch(error.to_string()),
            RestartFailure::ComposePanicked => Self::ComposePanicked,
            _ => Self::Other,
        }
    }
}
