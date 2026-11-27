//! [`TestWindow`]: the window a [`Tester`](super::Tester) mounts its view in.

use flui_runtime::owner_turn::CloseRequestHandler;

use crate::host::AppRegistrations;
use crate::storage::MemoryStorage;

/// platform whose conventions the application follows, the storage its
/// documents are kept in, and the capability providers the application
/// registers.
#[derive(Clone)]
pub struct TestWindow {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) platform: Option<flui_platform::TargetPlatform>,
    /// Whether the window plays a browser's page ([`TestWindow::in_browser`]).
    pub(crate) in_browser: bool,
    pub(crate) storage: WindowStorage,
    pub(crate) registrations: AppRegistrations,
    pub(crate) close_request_handler: Option<CloseRequestHandler>,
}

impl std::fmt::Debug for TestWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (overrides, plugins) = self.registrations.counts();
        f.debug_struct("TestWindow")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("platform", &self.platform)
            .field("in_browser", &self.in_browser)
            .field("storage", &self.storage)
            .field("overrides", &overrides)
            .field("plugins", &plugins)
            .field("close_request_handler", &self.close_request_handler)
            .finish()
    }
}

/// Where a [`TestWindow`]'s application keeps its documents.
#[derive(Debug, Clone)]
pub(crate) enum WindowStorage {
    /// A fresh, empty [`MemoryStorage`] for each mount.
    Fresh,
    /// This storage, as left by an earlier run.
    Given(MemoryStorage),
    /// None at all, as on a platform without files.
    Absent,
}

impl TestWindow {
    /// A window of `width` by `height` logical pixels, its application
    /// keeping its documents in a fresh, empty [`MemoryStorage`].
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            platform: None,
            in_browser: false,
            storage: WindowStorage::Fresh,
            registrations: AppRegistrations::new(),
            close_request_handler: None,
        }
    }

    /// Answer the window's close requests with `handler`, as
    /// `AppConfig::close_request_handler` does: asked after the window's close
    /// guard for the user's close and a quit, and first at the session's end,
    /// on the platforms whose window system asks.
    #[must_use]
    pub fn close_request_handler(mut self, handler: CloseRequestHandler) -> Self {
        self.close_request_handler = Some(handler);
        self
    }

    /// Register `plugin`'s providers, as `Application::plugin` does: a widget
    /// acquires what it provides and nothing else does. A
    /// relaunch over the same window registers it again.
    ///
    /// The mount panics when two registrations conflict, as a run fails
    /// before its platform starts.
    #[must_use]
    pub fn plugin(mut self, plugin: impl flui_platform::capability::Plugin) -> Self {
        self.registrations.plugin(plugin);
        self
    }

    /// Provide the capability `C` with `provider`, in place of the built-in
    /// one and of any plugin's, as `Application::capability` does.
    ///
    /// The mount panics when two registrations conflict.
    #[must_use]
    pub fn capability<C: flui_platform::capability::Capability>(
        mut self,
        provider: impl flui_platform::capability::CapabilityProvider<C>,
    ) -> Self {
        self.registrations.capability::<C, _>(provider);
        self
    }

    /// Follow `platform`'s conventions (chords, handles, menus, transitions)
    /// instead of the platform-neutral ones a headless window follows.
    #[must_use]
    pub fn platform(mut self, platform: flui_platform::TargetPlatform) -> Self {
        self.platform = Some(platform);
        self
    }

    /// Play a browser's page, on whatever target the test runs
    /// ([`HeadlessWindow::in_browser`](crate::host::HeadlessWindow::in_browser)): no close is put to the window's
    /// guard or handler, and the session's end delivers its notice alone.
    #[must_use]
    pub fn in_browser(mut self) -> Self {
        self.in_browser = true;
        self
    }

    /// Keep the application's documents in `storage`: a launch over what an
    /// earlier run left there, or over contents the test put there.
    #[must_use]
    pub fn storage(mut self, storage: MemoryStorage) -> Self {
        self.storage = WindowStorage::Given(storage);
        self
    }

    /// Give the application no storage at all, as a platform without files
    /// runs it: a document it opens is read-only and starts from its initial
    /// value.
    #[must_use]
    pub fn without_storage(mut self) -> Self {
        self.storage = WindowStorage::Absent;
        self
    }
}

impl Default for TestWindow {
    /// 800 by 600 logical pixels, as `flutter_test` sizes its surface.
    fn default() -> Self {
        Self::new(800, 600)
    }
}
