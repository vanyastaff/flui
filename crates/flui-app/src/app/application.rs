//! Desktop application lifetime, independent of any particular widget tree.
use super::{
    AppConfig,
    application_control::{AppHandle, AppWindowError},
};
use flui_view::View;
use std::marker::PhantomData;

/// Whether startup requests the designated main window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StartupWindow {
    /// Reserve and open the initial main window after `on_ready`.
    #[default]
    Open,
    /// Start without a window. The configured exit policy still applies.
    None,
}

/// Failure of application startup or the native event loop.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppRunError {
    /// Native platform initialization or event-loop failure.
    #[error("platform startup or event loop failed: {source}")]
    Platform {
        /// Original native failure.
        #[source]
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    },
    /// The reserved startup window could not be installed.
    #[error("initial main window failed: {0}")]
    InitialWindow(#[source] AppWindowError),
    /// The startup observer unwound before initial creation.
    #[error("application on_ready callback panicked")]
    OnReadyPanicked,
    /// A configured application service failed to start.
    #[error("application service startup failed: {source}")]
    Service {
        /// Original service startup failure.
        #[source]
        source: std::sync::Arc<dyn std::error::Error + Send + Sync>,
    },
}

pub(crate) type ReadyObserver = Box<dyn FnOnce(&AppHandle)>;
pub(crate) type WindowErrorObserver = Box<dyn FnMut(&AppWindowError)>;

/// A desktop application's loop-owned factory and configuration.
///
/// Every new main window gets a fresh view tree. Persistent application data
/// belongs in factory captures; those captures may use `Rc` and need not be Send.
/// The factory and observers run on the application owner thread.
pub struct Application<V, F> {
    pub(crate) factory: F,
    pub(crate) config: AppConfig,
    pub(crate) startup: StartupWindow,
    pub(crate) ready: Option<ReadyObserver>,
    pub(crate) window_error: Option<WindowErrorObserver>,
    marker: PhantomData<fn() -> V>,
}
impl<V, F> Application<V, F>
where
    V: View + Clone + 'static,
    F: FnMut(&AppHandle) -> V + 'static,
{
    /// Create an application with the default last-window exit policy.
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            config: AppConfig::default(),
            startup: StartupWindow::Open,
            ready: None,
            window_error: None,
            marker: PhantomData,
        }
    }
    /// Set window and loop service configuration.
    pub fn with_config(mut self, config: AppConfig) -> Self {
        self.config = config;
        self
    }
    /// Select whether startup reserves an initial main window.
    pub fn with_startup_window(mut self, startup: StartupWindow) -> Self {
        self.startup = startup;
        self
    }
    /// Called once before any root factory invocation. Quit suppresses startup creation.
    pub fn on_ready(mut self, callback: impl FnOnce(&AppHandle) + 'static) -> Self {
        self.ready = Some(Box::new(callback));
        self
    }
    /// Observe later window failures after rollback and reply settlement.
    /// A panicking observer is retired; it does not poison later show requests.
    pub fn on_window_error(mut self, callback: impl FnMut(&AppWindowError) + 'static) -> Self {
        self.window_error = Some(Box::new(callback));
        self
    }
    /// Run the native loop and release its services before returning.
    pub fn run(self) -> Result<(), AppRunError> {
        super::runner::run_application(self)
    }
}

impl<V, F> std::fmt::Debug for Application<V, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Application")
            .field("config", &self.config)
            .field("startup", &self.startup)
            .finish_non_exhaustive()
    }
}
