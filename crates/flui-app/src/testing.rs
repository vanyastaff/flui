//! The runner's test kit (ADR-0243): a whole application run on the headless
//! platform, driven from the test thread the way an operating system drives a
//! desktop one.
//!
//! [`HeadlessRun::start`] runs an [`Application`] as
//! [`Application::run`] does, through the same runner, on a
//! [`HeadlessPlatform`] instead of the current platform. The headless
//! platform's `run` returns once the application is up, so the test holds a
//! [`HeadlessRunHandle`] while the loop lives: it turns the owner loop, closes
//! a window as the user would and ends the session as the operating
//! system would. [`HeadlessRunHandle::join`] tears the loop down as the end
//! of [`Application::run`] does and reports how it ended.
//!
//! One run at a time per thread: the runner keeps its loop in thread-local
//! state, as it does on a desktop.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use flui_native::{
    HeadlessHostEvents, HeadlessOpenWindows, HeadlessOwnerTurns, HeadlessPlatform, MockWindow,
};
use flui_platform::{SessionEndAnswer, SessionEndPhase};
use flui_view::View;

use crate::app::runner::{StartedLoop, start_loop};
use crate::app::{AppHandle, AppRunError, Application};

pub use crate::app::runner::TeardownStep;

/// The process exit code of a run that ended with a panic: the code Rust's
/// runtime gives a process whose main thread panicked.
pub const EXIT_CODE_PANIC: i32 = 101;

/// Starts application runs on the headless platform.
#[derive(Debug)]
#[non_exhaustive]
pub struct HeadlessRun;

impl HeadlessRun {
    /// Run `application` on a fresh [`HeadlessPlatform`] until it is up.
    #[must_use]
    pub fn start<V, F>(application: Application<V, F>) -> HeadlessRunHandle
    where
        V: View + Clone + 'static,
        F: FnMut(&AppHandle) -> V + 'static,
    {
        Self::start_on(application, HeadlessPlatform::new())
    }

    /// Run `application` on `platform`, built with the controls the test
    /// needs, until it is up.
    #[must_use]
    pub fn start_on<V, F>(
        application: Application<V, F>,
        platform: HeadlessPlatform,
    ) -> HeadlessRunHandle
    where
        V: View + Clone + 'static,
        F: FnMut(&AppHandle) -> V + 'static,
    {
        let turns = platform.owner_turns();
        let host_events = platform.host_events();
        let windows = platform.open_windows();
        let teardown = Rc::new(RefCell::new(Vec::new()));
        let started = catch_unwind(AssertUnwindSafe(|| {
            start_loop(
                application,
                || Ok(Box::new(platform) as Box<dyn flui_native::Platform>),
                Rc::clone(&teardown),
            )
        }));
        let state = match started {
            Ok(Ok(started)) => Started::Running(started),
            Ok(Err(error)) => Started::Failed(error),
            Err(payload) => Started::Panicked(panic_message(payload.as_ref())),
        };
        HeadlessRunHandle {
            state,
            turns,
            host_events,
            windows,
            teardown,
        }
    }
}

enum Started {
    Running(StartedLoop),
    /// The run failed before its loop started.
    Failed(AppRunError),
    /// A panic unwound out of the platform's `run`; the loop's teardown ran
    /// during the unwind.
    Panicked(String),
}

/// A headless application run that is up: what the test drives it with.
/// Dropped without [`join`](Self::join), the loop is torn down all the same.
pub struct HeadlessRunHandle {
    state: Started,
    turns: HeadlessOwnerTurns,
    host_events: HeadlessHostEvents,
    windows: HeadlessOpenWindows,
    teardown: Rc<RefCell<Vec<TeardownStep>>>,
}

impl std::fmt::Debug for HeadlessRunHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessRunHandle")
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl HeadlessRunHandle {
    /// Run one turn of the owner loop: the work posted to it since the last
    /// turn, a quit included.
    pub fn run_owner_turn(&self) {
        self.turns.drive();
    }

    /// Whether the platform still runs: `false` once the application quit.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.turns.is_running()
    }

    /// Every window the application holds open, in the order it opened: a
    /// test closes one as the user does with
    /// [`MockWindow::simulate_close`].
    #[must_use]
    pub fn open_windows(&self) -> Vec<MockWindow> {
        self.windows.all()
    }

    /// Report one phase of the user's session ending, as the operating system
    /// does, and return the application's answer.
    pub fn end_session(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.host_events.session_end(phase)
    }

    /// Tear the loop down as the end of [`Application::run`] does, and report
    /// how the run ended: `Err` with what `Application::run` would return,
    /// or the exit and the teardown steps that ran.
    ///
    /// # Errors
    ///
    /// What [`Application::run`] returns for the same run.
    pub fn join(self) -> Result<ExitReport, AppRunError> {
        let exit = match self.state {
            Started::Running(started) => {
                match catch_unwind(AssertUnwindSafe(|| started.finish())) {
                    Ok(Ok(())) => RunExit::Returned,
                    Ok(Err(error)) => return Err(error),
                    Err(payload) => RunExit::Panicked {
                        message: panic_message(payload.as_ref()),
                    },
                }
            }
            Started::Failed(error) => return Err(error),
            Started::Panicked(message) => RunExit::Panicked { message },
        };
        Ok(ExitReport {
            exit,
            teardown: self.teardown.take(),
        })
    }
}

/// How a joined run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExitReport {
    /// Whether the run returned or a panic ended it.
    pub exit: RunExit,
    /// The teardown steps that ended, in the order they ran.
    pub teardown: Vec<TeardownStep>,
}

impl ExitReport {
    /// The code the process would exit with: `0` when the run returned,
    /// [`EXIT_CODE_PANIC`] when a panic ended it.
    #[must_use]
    pub fn code(&self) -> i32 {
        match self.exit {
            RunExit::Returned => 0,
            RunExit::Panicked { .. } => EXIT_CODE_PANIC,
        }
    }
}

/// How a run's loop ended.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RunExit {
    /// The run returned.
    Returned,
    /// A panic unwound out of the run, after its teardown.
    Panicked {
        /// The panic's message.
        message: String,
    },
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_owned())
}
