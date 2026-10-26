//! The table of a host: one type for each position of the closed list
//! (ADR-0151 §3, §10).
//!
//! A host is FLUI's integration with one platform. [`Host`] names, for every
//! position, the type that fills it there, so a host that leaves a position
//! out does not compile, and [`Absent`] is the one way to say "not on this
//! platform". `hosts/<os>.rs` holds each table and its constructor;
//! [`HostPlatform`](crate::HostPlatform) is the one implementation of
//! [`Platform`] and reaches every position through the table.
//!
//! The trait is a table, not a generic core: a feature's model is not
//! parameterised by it, and [`Platform`] erases it at the boundary, so a target
//! compiles one `HostPlatform<H>` per host it carries, not one per feature and
//! host.
//!
//! The application-level positions take the host's loop as their owner `O`:
//! the loop holds what the operating system hands out once per process, and a
//! position reaches its own object through it. A position whose object the
//! loop keeps is the loop itself: the owner's host trait makes the object
//! (`WindowsHost::drag`, `WebHost::clipboard`, ...), the loop keeps it in a
//! typed field and answers the position from it, so a table cannot name one
//! type and build another. The window position has an object of its own,
//! what the platform's windows share ([`Window::Shared`]): the platform holds
//! it beside the loop, and the loop, which names no feature, reaches what it
//! needs of it through a hook its own module declares. The window-level
//! positions
//! (text, accessibility, haptics, system chrome) are what every window of the
//! host hands out; the table names their type, and a window that has none
//! answers as the window contract says today (no object), until the absence
//! vocabulary makes those answers total.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_platform::data_transfer::{DataTransferSource, NullDataTransferSource};
use flui_platform::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform::{
    ClipboardItem, DeviceId, DisplaySnapshot, HapticFeedback, PlatformHaptics,
    PlatformSystemChrome, SessionEndAnswer, SessionEndPhase, StorageName, SystemNotice,
    SystemPreferences, SystemUiOverlayStyle, WindowCapabilities, WindowEvent, WindowId,
    WindowOptions,
};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

use crate::error::PlatformError;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::platform::{PathPromptOptions, Platform, PlatformExecutor, PlatformReadyCallback};
use crate::storage::DataDirs;
use crate::task::Task;
use crate::text::TextInput;

/// One type for each position of the closed list (ADR-0151 §3).
///
/// `hosts/<os>.rs` implements it for its platform and builds the
/// [`HostPlatform`](crate::HostPlatform) it describes; nothing else does. A
/// position the platform does not have is [`Absent`] of that position.
pub trait Host: 'static {
    /// The platform's name, for logs and diagnostics.
    const NAME: &'static str;

    /// The owner thread's loop: what the operating system hands out once per
    /// process, and the services no position holds.
    type Loop: Loop + Services;
    /// Opening windows and what is known of them before one exists.
    type Window: Window<Self::Loop>;
    /// The displays and their snapshot.
    type Displays: Displays<Self::Loop>;
    /// What the keyboard layout is and when it changes.
    type Input: Input<Self::Loop>;
    /// Text input a window hands out: the IME requests or the field's store.
    type Text: TextInput;
    /// The accessibility tree a window hands out.
    type Accessibility: PlatformAccessibility;
    /// The clipboard.
    type Clipboard: Clipboard<Self::Loop>;
    /// The drag-and-drop transport.
    type Drag: Drag<Self::Loop>;
    /// Haptic feedback a window hands out.
    type Haptics: PlatformHaptics;
    /// System preferences.
    type Preferences: Preferences<Self::Loop>;
    /// Session end, host notices, reopen and activation.
    type Lifecycle: Lifecycle<Self::Loop>;
    /// The system bars a window styles.
    type Chrome: PlatformSystemChrome;
    /// Where an application keeps its data.
    type Storage: Storage<Self::Loop>;
}

/// The owner thread's loop.
pub trait Loop: Send + Sync + Sized + 'static {
    /// Runs the loop on the calling thread, handing `on_ready` the owner
    /// capability over `platform` once windows can open; see
    /// [`Platform::run`].
    ///
    /// # Errors
    /// What [`Platform::run`] reports.
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        on_ready: PlatformReadyCallback,
    ) -> Result<(), PlatformError>;

    /// [`Platform::quit`].
    fn quit(&self);

    /// [`Platform::on_quit`].
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);

    /// [`Platform::set_exit_policy_hook`].
    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>);

    /// [`Platform::request_exit_policy_reevaluation`].
    fn request_exit_policy_reevaluation(&self);

    /// [`Platform::set_wake_deadline_hook`].
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    );
}

/// What the host does for the application outside the closed list: the
/// background executor, the application's path, opening URLs and paths, file
/// prompts and the compositor's name. They leave [`Platform`] for the system
/// capability crate (ADR-0151 §9); until then the loop carries them, and each
/// host writes its answer, inert ones included.
pub trait Services {
    /// [`Platform::background_executor`].
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;

    /// [`Platform::app_path`].
    ///
    /// # Errors
    /// [`PlatformError::AppPath`] when the lookup fails.
    fn app_path(&self) -> Result<PathBuf, PlatformError>;

    /// [`Platform::open_url`].
    fn open_url(&self, url: &str);

    /// [`Platform::reveal_path`].
    fn reveal_path(&self, path: &Path);

    /// [`Platform::open_path`].
    fn open_path(&self, path: &Path);

    /// [`Platform::prompt_for_paths`].
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>>;

    /// [`Platform::prompt_for_new_path`].
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>>;

    /// [`Platform::compositor_name`].
    fn compositor_name(&self) -> &'static str;
}

/// The window position: opening windows, and what is known of them before
/// one exists.
pub trait Window<O> {
    /// What the windows of one platform share beside the loop `O`: what a
    /// window opens with and what the windows keep in common (the Win32
    /// handle map and configuration, the macOS configuration, the UIKit window
    /// factory, the winit frame inset and window manager's view, the headless
    /// window model), `()` where the loop keeps it all. The host's constructor
    /// builds one, hands it to the loop as the hook the loop declares and to
    /// the platform, which passes it to every call of this position: one
    /// instance for both (`HostPlatform::from_parts`).
    type Shared: Send + Sync + 'static;

    /// [`Platform::open_window`].
    ///
    /// # Errors
    /// What [`Platform::open_window`] reports.
    fn open(
        owner: &O,
        shared: &Arc<Self::Shared>,
        options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError>;

    /// [`Platform::active_window`].
    fn active(owner: &O, shared: &Self::Shared) -> Option<WindowId>;

    /// [`Platform::window_capabilities`].
    fn capabilities(owner: &O, shared: &Self::Shared) -> WindowCapabilities;

    /// [`Platform::window_frame_inset`].
    fn frame_inset(owner: &O, shared: &Self::Shared, scale_factor: f64) -> Size<f64>;

    /// [`Platform::on_window_event`].
    fn on_event(owner: &O, callback: Box<dyn FnMut(WindowEvent) + Send>);
}

/// The displays position.
pub trait Displays<O> {
    /// [`Platform::display_snapshot`].
    ///
    /// # Errors
    /// What [`Platform::display_snapshot`] reports.
    fn snapshot(owner: &O) -> Result<DisplaySnapshot, PlatformError>;
}

/// The input position, as far as the application reaches it: the keyboard
/// layout. Pointer, key and gesture input reach a window through its own
/// callbacks.
pub trait Input<O> {
    /// [`Platform::keyboard_layout`].
    fn keyboard_layout(owner: &O) -> String;

    /// [`Platform::on_keyboard_layout_change`].
    fn on_keyboard_layout_change(owner: &O, callback: Box<dyn FnMut() + Send>);
}

/// The clipboard position.
pub trait Clipboard<O> {
    /// [`Platform::clipboard`].
    fn clipboard(owner: &O) -> Arc<dyn flui_platform::Clipboard>;

    /// [`Platform::write_to_clipboard`]: the item's text, through
    /// [`clipboard`](Self::clipboard), unless the host keeps richer items.
    fn write(owner: &O, item: ClipboardItem) {
        if let Some(text) = item.text_content() {
            Self::clipboard(owner).write_text(text.to_string());
        }
    }

    /// [`Platform::read_from_clipboard`]: the clipboard's text, unless the host
    /// keeps richer items.
    fn read(owner: &O) -> Option<ClipboardItem> {
        Self::clipboard(owner).read_text().map(ClipboardItem::text)
    }
}

/// The drag-and-drop position.
pub trait Drag<O> {
    /// [`Platform::data_transfer`].
    fn data_transfer(owner: &O) -> Arc<dyn DataTransferSource>;
}

/// The system preferences position.
pub trait Preferences<O> {
    /// [`Platform::preferences`].
    ///
    /// # Errors
    /// What [`Platform::preferences`] reports.
    fn sample(owner: &O) -> Result<SystemPreferences, PlatformError>;
}

/// The lifecycle position: session end, host notices, reopen and activation.
pub trait Lifecycle<O> {
    /// [`Platform::on_session_end`].
    fn on_session_end(
        owner: &O,
        callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>,
    );

    /// [`Platform::on_host_notice`].
    fn on_host_notice(owner: &O, callback: Box<dyn FnMut(SystemNotice) + Send>);

    /// [`Platform::release_session_end`].
    fn release_session_end(owner: &O);

    /// [`Platform::on_reopen`].
    fn on_reopen(owner: &O, callback: Box<dyn FnMut() + Send>);

    /// [`Platform::activate`].
    fn activate(owner: &O, ignoring_other_apps: bool);
}

/// The storage position.
pub trait Storage<O> {
    /// [`Platform::data_dirs`].
    fn data_dirs(owner: &O, app: &StorageName) -> Option<DataDirs>;
}

/// The positions [`Absent`] is named with.
pub mod position {
    /// The window position.
    #[derive(Debug)]
    pub enum Window {}
    /// The displays position.
    #[derive(Debug)]
    pub enum Displays {}
    /// The input position.
    #[derive(Debug)]
    pub enum Input {}
    /// The text position.
    #[derive(Debug)]
    pub enum Text {}
    /// The accessibility position.
    #[derive(Debug)]
    pub enum Accessibility {}
    /// The clipboard position.
    #[derive(Debug)]
    pub enum Clipboard {}
    /// The drag-and-drop position.
    #[derive(Debug)]
    pub enum Drag {}
    /// The haptics position.
    #[derive(Debug)]
    pub enum Haptics {}
    /// The system preferences position.
    #[derive(Debug)]
    pub enum Preferences {}
    /// The lifecycle position.
    #[derive(Debug)]
    pub enum Lifecycle {}
    /// The system chrome position.
    #[derive(Debug)]
    pub enum Chrome {}
    /// The storage position.
    #[derive(Debug)]
    pub enum Storage {}
}

/// Position `F` is not on this platform.
///
/// The one type a table names for a missing position. It answers in the
/// position's own words for "not here": no window opens, no display is read,
/// a drag offers nothing, a haptic is [`HapticOutcome::Unavailable`], the
/// preferences are unknown, the data has nowhere to go, and every registration
/// is dropped unused because nothing will call it.
pub struct Absent<F>(PhantomData<fn() -> F>);

impl<F> Absent<F> {
    const NEW: Self = Self(PhantomData);
}

impl<F> std::fmt::Debug for Absent<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Absent<{}>", std::any::type_name::<F>())
    }
}

impl<O> Window<O> for Absent<position::Window> {
    /// No window is ever opened, so nothing is shared.
    type Shared = ();

    fn open(_: &O, _: &Arc<()>, _: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        // Not `Unavailable`, which promises a later call may succeed.
        Err(OpenWindowError::Backend {
            message: "this platform opens no windows".to_owned(),
        })
    }

    fn active(_: &O, (): &()) -> Option<WindowId> {
        None
    }

    fn capabilities(_: &O, (): &()) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn frame_inset(_: &O, (): &(), _: f64) -> Size<f64> {
        Size::new(0.0, 0.0)
    }

    fn on_event(_: &O, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        drop(callback);
    }
}

impl<O> Displays<O> for Absent<position::Displays> {
    fn snapshot(_: &O) -> Result<DisplaySnapshot, PlatformError> {
        Err(PlatformError::Displays {
            message: "this platform has no display source".to_owned(),
        })
    }
}

impl<O> Input<O> for Absent<position::Input> {
    fn keyboard_layout(_: &O) -> String {
        String::new()
    }

    fn on_keyboard_layout_change(_: &O, callback: Box<dyn FnMut() + Send>) {
        drop(callback);
    }
}

impl PlatformAccessibility for Absent<position::Accessibility> {
    fn publish(&self, update: accesskit::TreeUpdate) {
        drop(update);
    }

    fn is_active(&self) -> bool {
        false
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        drop(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        drop(listener);
    }
}

impl flui_platform::Clipboard for Absent<position::Clipboard> {
    fn read_text(&self) -> Option<String> {
        None
    }

    fn write_text(&self, text: String) {
        drop(text);
    }
}

impl<O> Clipboard<O> for Absent<position::Clipboard> {
    fn clipboard(_: &O) -> Arc<dyn flui_platform::Clipboard> {
        Arc::new(Self::NEW)
    }
}

impl<O> Drag<O> for Absent<position::Drag> {
    fn data_transfer(_: &O) -> Arc<dyn DataTransferSource> {
        Arc::new(NullDataTransferSource)
    }
}

impl PlatformHaptics for Absent<position::Haptics> {
    fn perform(&self, _: HapticFeedback) {}

    fn perform_with(&self, _: HapticFeedback, _: &HapticRequest) -> HapticOutcome {
        HapticOutcome::Unavailable
    }

    fn play(&self, _: &HapticPattern, _: HapticFeedback, _: &HapticRequest) -> HapticOutcome {
        HapticOutcome::Unavailable
    }

    fn capabilities(&self, _: Option<DeviceId>) -> HapticCapabilities {
        HapticCapabilities::NONE
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<O> Preferences<O> for Absent<position::Preferences> {
    fn sample(_: &O) -> Result<SystemPreferences, PlatformError> {
        // `Platform::preferences`: a host with no observation reports every
        // field unknown and leaves the fallback to consumers.
        Ok(SystemPreferences::default())
    }
}

impl<O> Lifecycle<O> for Absent<position::Lifecycle> {
    fn on_session_end(_: &O, callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>) {
        drop(callback);
    }

    fn on_host_notice(_: &O, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        drop(callback);
    }

    fn release_session_end(_: &O) {}

    fn on_reopen(_: &O, callback: Box<dyn FnMut() + Send>) {
        drop(callback);
    }

    fn activate(_: &O, _: bool) {}
}

impl PlatformSystemChrome for Absent<position::Chrome> {
    fn set_overlay_style(&self, _: SystemUiOverlayStyle) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<O> Storage<O> for Absent<position::Storage> {
    fn data_dirs(_: &O, _: &StorageName) -> Option<DataDirs> {
        None
    }
}
