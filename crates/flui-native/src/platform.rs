//! [`HostPlatform`]: the one implementation of [`Platform`], over a host's
//! table (ADR-0151 §10).

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::{
    ClipboardItem, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, StorageName, SystemNotice,
    SystemPreferences, WindowCapabilities, WindowEvent, WindowId, WindowOptions,
};

use crate::error::PlatformError;
use crate::host::{
    Clipboard as _, Displays as _, Drag as _, Host, Input as _, Lifecycle as _, Loop as _,
    Preferences as _, Services as _, Storage as _, Window as _,
};
use crate::storage::DataDirs;
use crate::task::Task;
use crate::traits::{
    HostWindow, OpenWindowError, PathPromptOptions, Platform, PlatformExecutor,
    PlatformReadyCallback,
};

/// The platform of host `H`: every [`Platform`] method answered by the
/// position of `H`'s table that holds it.
///
/// Built by the host's constructor in `hosts/`; the public names
/// (`WindowsPlatform`, `HeadlessPlatform`, …) are this type over each table.
pub struct HostPlatform<H: Host> {
    owner: Arc<H::Loop>,
    host: PhantomData<fn() -> H>,
}

impl<H: Host> HostPlatform<H> {
    /// The platform over `owner`, the host's loop: what each host's
    /// constructor returns.
    pub(crate) fn from_loop(owner: H::Loop) -> Self {
        Self {
            owner: Arc::new(owner),
            host: PhantomData,
        }
    }

    /// The host's loop, for what a host offers beyond [`Platform`] (test
    /// controls, a scene registration).
    pub(crate) fn owner(&self) -> &H::Loop {
        &self.owner
    }

    /// A handle on the host's loop, for a host whose loop runs as an `Arc`.
    pub(crate) fn shared_owner(&self) -> Arc<H::Loop> {
        Arc::clone(&self.owner)
    }
}

impl<H: Host> std::fmt::Debug for HostPlatform<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostPlatform")
            .field("host", &H::NAME)
            .finish_non_exhaustive()
    }
}

impl<H: Host> Platform for HostPlatform<H> {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.owner.background_executor()
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        let owner = self.shared_owner();
        let platform: Arc<dyn Platform> = Arc::<Self>::from(self);
        owner.run(platform, on_ready)
    }

    fn quit(&self) {
        self.owner.quit();
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        self.owner.set_exit_policy_hook(hook);
    }

    fn request_exit_policy_reevaluation(&self) {
        self.owner.request_exit_policy_reevaluation();
    }

    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        self.owner.set_wake_deadline_hook(hook);
    }

    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        H::Window::open(&self.owner, options)
    }

    fn active_window(&self) -> Option<WindowId> {
        H::Window::active(&self.owner)
    }

    fn preferences(&self) -> Result<SystemPreferences, PlatformError> {
        H::Preferences::sample(&self.owner)
    }

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        H::Displays::snapshot(&self.owner)
    }

    fn clipboard(&self) -> Arc<dyn flui_platform::Clipboard> {
        H::Clipboard::clipboard(&self.owner)
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        H::Drag::data_transfer(&self.owner)
    }

    fn activate(&self, ignoring_other_apps: bool) {
        H::Lifecycle::activate(&self.owner, ignoring_other_apps);
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        H::Clipboard::write(&self.owner, item);
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        H::Clipboard::read(&self.owner)
    }

    fn open_url(&self, url: &str) {
        self.owner.open_url(url);
    }

    fn reveal_path(&self, path: &Path) {
        self.owner.reveal_path(path);
    }

    fn open_path(&self, path: &Path) {
        self.owner.open_path(path);
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        self.owner.prompt_for_paths(options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        self.owner.prompt_for_new_path(directory, suggested_name)
    }

    fn keyboard_layout(&self) -> String {
        H::Input::keyboard_layout(&self.owner)
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        H::Input::on_keyboard_layout_change(&self.owner, callback);
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.owner.on_quit(callback);
    }

    fn on_session_end(&self, callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>) {
        H::Lifecycle::on_session_end(&self.owner, callback);
    }

    fn on_host_notice(&self, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        H::Lifecycle::on_host_notice(&self.owner, callback);
    }

    fn release_session_end(&self) {
        H::Lifecycle::release_session_end(&self.owner);
    }

    fn window_capabilities(&self) -> WindowCapabilities {
        H::Window::capabilities(&self.owner)
    }

    fn window_frame_inset(&self, scale_factor: f64) -> Size<f64> {
        H::Window::frame_inset(&self.owner, scale_factor)
    }

    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        H::Lifecycle::on_reopen(&self.owner, callback);
    }

    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        H::Window::on_event(&self.owner, callback);
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>) {
        // No host produces opened URLs yet: the activation feature brings the
        // producer and replaces this registration with its handler.
        drop(callback);
    }

    fn name(&self) -> &'static str {
        H::NAME
    }

    fn compositor_name(&self) -> &'static str {
        self.owner.compositor_name()
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        self.owner.app_path()
    }

    fn data_dirs(&self, app: &StorageName) -> Option<DataDirs> {
        H::Storage::data_dirs(&self.owner, app)
    }
}
