//! What a desktop [`Application`] adds: the observers of its lifetime and a
//! run that starts without a window.
use super::{
    Application, LaunchError,
    application_control::{AppHandle, AppWindowError},
};
use flui_view::View;

/// Whether startup requests the designated main window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum StartupWindow {
    /// Reserve and open the initial main window after `on_ready`.
    #[default]
    Open,
    /// Start without a window. The configured exit policy still applies.
    None,
}

pub(crate) type ReadyObserver = Box<dyn FnOnce(&AppHandle)>;
pub(crate) type WindowErrorObserver = Box<dyn FnMut(&AppWindowError)>;

impl Application {
    /// Called once on the owner thread before the first main window is
    /// built, with the [`AppHandle`] that shows the main window or quits from
    /// any thread. A quit requested here suppresses the startup window.
    /// Desktop only.
    pub fn on_ready(mut self, callback: impl FnOnce(&AppHandle) + 'static) -> Self {
        self.ready = Some(Box::new(callback));
        self
    }

    /// Observe later window failures after rollback and reply settlement.
    /// A panicking observer is retired; it does not poison later show
    /// requests. Desktop only.
    pub fn on_window_error(mut self, callback: impl FnMut(&AppWindowError) + 'static) -> Self {
        self.window_error = Some(Box::new(callback));
        self
    }

    /// Run a resident application: one that starts without a window and
    /// lives until it quits, by its exit policy or an
    /// [`AppHandle::request_quit`]. Each main window, opened by
    /// [`AppHandle::request_show_main_window`], mounts a fresh tree that
    /// `factory` builds on the owner thread; state that outlives a window
    /// belongs in the factory's captures, which may be `Rc`. Desktop only.
    ///
    /// # Errors
    ///
    /// As [`run`](Self::run), but with no startup window to fail.
    ///
    /// # Panics
    ///
    /// As [`run`](Self::run).
    pub fn run_resident<V, F>(self, factory: F) -> Result<(), LaunchError>
    where
        V: View + Clone + 'static,
        F: FnMut(&AppHandle) -> V + 'static,
    {
        super::runner::run_application(self.into_run(factory, StartupWindow::None))
    }

    /// The run of `self` with `factory` building its main windows.
    pub(crate) fn into_run<V, F>(self, factory: F, startup: StartupWindow) -> DesktopRun<V, F> {
        let Self {
            config,
            capabilities,
            ready,
            window_error,
        } = self;
        DesktopRun {
            factory,
            config,
            startup,
            ready,
            window_error,
            capabilities,
            marker: std::marker::PhantomData,
        }
    }
}

/// A desktop run: an [`Application`] with the factory of its main windows
/// and whether startup opens one.
pub(crate) struct DesktopRun<V, F> {
    pub(crate) factory: F,
    pub(crate) config: super::AppConfig,
    pub(crate) startup: StartupWindow,
    pub(crate) ready: Option<ReadyObserver>,
    pub(crate) window_error: Option<WindowErrorObserver>,
    pub(crate) capabilities: super::capability::AppCapabilities,
    marker: std::marker::PhantomData<fn() -> V>,
}

/// The builder the runner's own tests start a run with, in the order they
/// read: the factory first, then what it runs with.
#[cfg(test)]
impl<V, F> DesktopRun<V, F>
where
    F: FnMut(&AppHandle) -> V,
{
    pub(crate) fn new(factory: F) -> Self {
        Application::new().into_run(factory, StartupWindow::Open)
    }

    pub(crate) fn config(mut self, config: super::AppConfig) -> Self {
        self.config = config;
        self
    }

    pub(crate) fn startup_window(mut self, startup: StartupWindow) -> Self {
        self.startup = startup;
        self
    }

    pub(crate) fn on_ready(mut self, callback: impl FnOnce(&AppHandle) + 'static) -> Self {
        self.ready = Some(Box::new(callback));
        self
    }
}
