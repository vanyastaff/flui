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
//! needs of it through a hook its own module declares.
//!
//! The window-level positions (text, accessibility, haptics, system chrome)
//! are what every window of the host hands out. Each is made for one window,
//! from the platform's own reference to it ([`Window::Ref`]), by the type the
//! table names ([`Text`], [`Accessibility`], [`Haptics`], [`Chrome`]); the
//! platform turns the table into [`WindowPositions`] once and hands them to
//! what its windows share, so a window holds exactly what its table names and
//! names no feature's platform file. A table that leaves one of them
//! [`Absent`] gets windows that hand out [`Absent`].

mod clipboard;

#[cfg(any(windows, wayland_host, x11_host))]
pub(crate) use clipboard::MAX_TEXT;
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    wayland_host,
    x11_host,
    feature = "winit-backend"
))]
pub(crate) use clipboard::host_text;
#[cfg(any(target_arch = "wasm32", wayland_host, x11_host))]
pub(crate) use clipboard::owned_text;
pub use clipboard::{MemoryClipboard, NativeClipboard, NativePrimarySelection};
pub(crate) use clipboard::{SharedClipboard, SyncClipboard};

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_platform::data_transfer::{DataTransferSource, NullDataTransferSource};
use flui_platform::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform::{
    AppDataDir, DeviceId, DisplaySnapshot, HapticFeedback, PlatformHaptics, PlatformSystemChrome,
    SystemPreferences, SystemUiOverlayStyle, Unsupported, UnsupportedReason, WindowCapabilities,
    WindowHandler, WindowId, WindowOptions,
};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

use crate::error::PlatformError;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::platform::{AppHandler, PathPromptOptions, Platform, PlatformExecutor};
use crate::storage::DataStore;
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
    /// Text input a window hands out: the IME requests or the field's store,
    /// and the system text size.
    type Text: Text<WindowRefOf<Self>>;
    /// The accessibility tree a window hands out.
    type Accessibility: Accessibility<WindowRefOf<Self>>;
    /// The clipboard.
    type Clipboard: Clipboard<Self::Loop>;
    /// The primary selection, where the platform has one.
    type PrimarySelection: PrimarySelection<Self::Loop>;
    /// The drag-and-drop transport.
    type Drag: Drag<Self::Loop>;
    /// Haptic feedback a window hands out.
    type Haptics: Haptics<WindowRefOf<Self>>;
    /// System preferences.
    type Preferences: Preferences<Self::Loop>;
    /// Session end, host notices, reopen and activation.
    type Lifecycle: Lifecycle<Self::Loop>;
    /// The system bars a window styles.
    type Chrome: Chrome<WindowRefOf<Self>>;
    /// Where an application keeps its data.
    type Storage: Storage<Self::Loop>;
}

/// What host `H`'s window-level positions are made for: its platform's
/// reference to one window ([`Window::Ref`]).
pub type WindowRefOf<H> = <<H as Host>::Window as Window<<H as Host>::Loop>>::Ref;

/// The owner thread's loop. It keeps the application's handler for the run in
/// the owner's delivery (`owner::app_delivery`), which its sources report to.
pub trait Loop: Send + Sync + Sized + 'static {
    /// Runs the loop on the calling thread, handing `handler` the owner
    /// capability over `platform` once windows can open; see
    /// [`Platform::run`].
    ///
    /// # Errors
    /// What [`Platform::run`] reports.
    fn run(
        self: Arc<Self>,
        platform: Arc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError>;

    /// [`Platform::quit`].
    fn quit(&self);

    /// [`Platform::request_exit_policy_reevaluation`].
    fn request_exit_policy_reevaluation(&self);
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
    /// instance for both (`HostPlatform::from_parts`). It holds the
    /// [`WindowPositions`] the platform made from the table.
    type Shared: Send + Sync + 'static;

    /// The platform's own reference to one of its windows, which the
    /// window-level positions are made for: the Win32 handle with its
    /// identity, the content view, the winit window, what the Activity's
    /// window is made of.
    type Ref: ?Sized + 'static;

    /// [`Platform::open_window`]: the window is built with `handler`, which
    /// it delivers to from its first event on. The capability snapshot is
    /// the platform's to deliver once this returns.
    ///
    /// # Errors
    /// What [`Platform::open_window`] reports.
    fn open(
        owner: &O,
        shared: &Arc<Self::Shared>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError>;

    /// [`Platform::active_window`].
    fn active(owner: &O, shared: &Self::Shared) -> Option<WindowId>;

    /// [`Platform::window_capabilities`].
    fn capabilities(owner: &O, shared: &Self::Shared) -> WindowCapabilities;

    /// [`Platform::window_frame_inset`].
    fn frame_inset(owner: &O, shared: &Self::Shared, scale_factor: f64) -> Size<f64>;
}

/// The text position: what one window, `W`, holds of the text feature.
///
/// Made once as the window opens, on its owner thread; the window hands out
/// what it answers and tells it when the window goes.
pub trait Text<W: ?Sized> {
    /// The text input `window` holds.
    fn for_window(window: &W) -> Arc<dyn TextInput>;
}

/// The accessibility position: the bridge one window, `W`, holds.
pub trait Accessibility<W: ?Sized> {
    /// The bridge of `window`, made as it opens, on its owner thread.
    fn for_window(window: &W) -> Arc<dyn WindowAccessibility>;
}

/// An accessibility bridge as the window that holds it reaches it: what it
/// hands out, and what it lets go of before the window is destroyed.
pub trait WindowAccessibility: PlatformAccessibility {
    /// The window is about to be destroyed, on its owner thread: undo what
    /// the bridge put on it (a subclass of its view or its procedure) while
    /// the window still exists. A later call is a no-op.
    fn detach(&self);

    /// The window became the key window or stopped being it, on its owner
    /// thread. A bridge whose adapter follows focus itself (through the
    /// window procedure it subclasses) has nothing to do.
    fn focus_changed(&self, focused: bool);
}

/// The haptics position: the actuator one window, `W`, hands out.
pub trait Haptics<W: ?Sized> {
    /// The actuator of `window`, made for it on its owner thread.
    fn for_window(window: &W) -> Arc<dyn PlatformHaptics>;
}

/// The system chrome position: the system bars one window, `W`, styles.
pub trait Chrome<W: ?Sized> {
    /// The system chrome of `window`, made for it on its owner thread.
    fn for_window(window: &W) -> Arc<dyn PlatformSystemChrome>;
}

/// What the windows of one platform hand out of the window-level positions,
/// made from its host's table: each window asks for its own with its
/// platform's reference to it, `W`.
///
/// Only the platform makes them, from the table it implements
/// ([`HostPlatform::from_parts`](crate::HostPlatform) hands them to what the
/// windows share), so a window holds exactly the types its table names and
/// cannot build others.
pub struct WindowPositions<W: ?Sized + 'static> {
    text: fn(&W) -> Arc<dyn TextInput>,
    accessibility: fn(&W) -> Arc<dyn WindowAccessibility>,
    haptics: fn(&W) -> Arc<dyn PlatformHaptics>,
    chrome: fn(&W) -> Arc<dyn PlatformSystemChrome>,
}

impl<W: ?Sized + 'static> WindowPositions<W> {
    /// The positions host `H`'s table names, for the windows of its platform.
    pub(crate) fn of<H>() -> Self
    where
        H: Host,
        H::Window: Window<H::Loop, Ref = W>,
    {
        Self {
            text: <H::Text as Text<W>>::for_window,
            accessibility: <H::Accessibility as Accessibility<W>>::for_window,
            haptics: <H::Haptics as Haptics<W>>::for_window,
            chrome: <H::Chrome as Chrome<W>>::for_window,
        }
    }

    /// What `window` holds of the positions, made for it together as it
    /// opens.
    pub(crate) fn for_window(&self, window: &W) -> WindowFacets {
        WindowFacets {
            text: (self.text)(window),
            accessibility: (self.accessibility)(window),
            haptics: (self.haptics)(window),
            chrome: (self.chrome)(window),
        }
    }
}

/// What one window holds of the window-level positions, made together as it
/// opens ([`WindowPositions::for_window`]) and handed out by the window as
/// its own.
#[derive(Clone)]
pub(crate) struct WindowFacets {
    /// Its text input.
    pub(crate) text: Arc<dyn TextInput>,
    /// Its accessibility bridge.
    pub(crate) accessibility: Arc<dyn WindowAccessibility>,
    /// Its haptic actuator.
    pub(crate) haptics: Arc<dyn PlatformHaptics>,
    /// The system chrome it styles.
    pub(crate) chrome: Arc<dyn PlatformSystemChrome>,
}

impl std::fmt::Debug for WindowFacets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowFacets").finish_non_exhaustive()
    }
}

impl<W: ?Sized + 'static> Clone for WindowPositions<W> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<W: ?Sized + 'static> Copy for WindowPositions<W> {}

impl<W: ?Sized + 'static> std::fmt::Debug for WindowPositions<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowPositions").finish_non_exhaustive()
    }
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
    fn clipboard(owner: &O) -> Rc<dyn NativeClipboard>;
}

/// The primary selection position.
pub trait PrimarySelection<O> {
    /// [`Platform::primary_selection`].
    fn primary_selection(owner: &O) -> Option<Rc<dyn NativePrimarySelection>>;
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
    /// [`Platform::release_session_end`].
    fn release_session_end(owner: &O);

    /// [`Platform::session_end_support`].
    fn session_end_support(owner: &O) -> flui_platform::SessionEndSupport;

    /// [`Platform::activate`].
    fn activate(owner: &O, ignoring_other_apps: bool);
}

/// The storage position.
pub trait Storage<O> {
    /// [`Platform::data_store`].
    fn data_store(owner: &O, app: &AppDataDir) -> Option<DataStore>;
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
    /// The primary selection position.
    #[derive(Debug)]
    pub enum PrimarySelection {}
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

/// What [`Absent`] answers wherever its position can say "not here".
const NOT_ON_THIS_PLATFORM: Unsupported = Unsupported::new(UnsupportedReason::NotOnThisPlatform);

/// Position `F` is not on this platform.
///
/// The one type a table names for a missing position. Wherever its position
/// can say "not here" it answers [`Unsupported`] with
/// [`UnsupportedReason::NotOnThisPlatform`]: no window opens, no display is
/// read, a haptic plays nothing. Where the position has no such answer, it
/// answers as having nothing: a drag offers nothing, the preferences are
/// unknown, the data has nowhere to go, and every registration is dropped
/// unused because nothing will call it.
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
    /// And none is referred to.
    type Ref = ();

    fn open(
        _: &O,
        _: &Arc<()>,
        _: WindowOptions,
        _: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        Err(NOT_ON_THIS_PLATFORM.into())
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
}

impl<O> Displays<O> for Absent<position::Displays> {
    fn snapshot(_: &O) -> Result<DisplaySnapshot, PlatformError> {
        Err(NOT_ON_THIS_PLATFORM.into())
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

    /// No adapter, no cursor to report.
    fn assistive_focus_support(&self) -> flui_semantics::AssistiveFocusSupport {
        flui_semantics::AssistiveFocusSupport::Unreported
    }
}

/// A bridge that put nothing on its window has nothing to let go of.
impl WindowAccessibility for Absent<position::Accessibility> {
    fn detach(&self) {}

    fn focus_changed(&self, _: bool) {}
}

/// Every window hands out a bridge that is never active.
impl<W: ?Sized> Accessibility<W> for Absent<position::Accessibility> {
    fn for_window(_: &W) -> Arc<dyn WindowAccessibility> {
        Arc::new(Self::NEW)
    }
}

/// Every window holds a text input that hands out none.
impl<W: ?Sized> Text<W> for Absent<position::Text> {
    fn for_window(_: &W) -> Arc<dyn TextInput> {
        Arc::new(Self::NEW)
    }
}

/// A host without a clipboard reads none: a read answers
/// [`ClipboardError::Unavailable`](flui_platform::ClipboardError), a write
/// is dropped.
impl SyncClipboard for Absent<position::Clipboard> {
    fn read_now(&self) -> flui_platform::ClipboardRead {
        Err(flui_platform::ClipboardError::Unavailable)
    }

    fn write_now(&self, text: String) {
        drop(text);
    }
}

impl<O> Clipboard<O> for Absent<position::Clipboard> {
    fn clipboard(_: &O) -> Rc<dyn NativeClipboard> {
        Rc::new(Self::NEW)
    }
}

/// A platform without a primary selection hands out none.
impl<O> PrimarySelection<O> for Absent<position::PrimarySelection> {
    fn primary_selection(_: &O) -> Option<Rc<dyn NativePrimarySelection>> {
        None
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
        NOT_ON_THIS_PLATFORM.into()
    }

    fn play(&self, _: &HapticPattern, _: HapticFeedback, _: &HapticRequest) -> HapticOutcome {
        NOT_ON_THIS_PLATFORM.into()
    }

    fn capabilities(&self, _: Option<DeviceId>) -> HapticCapabilities {
        HapticCapabilities::NONE
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Every window hands out an actuator that plays nothing, for want of the
/// platform.
impl<W: ?Sized> Haptics<W> for Absent<position::Haptics> {
    fn for_window(_: &W) -> Arc<dyn PlatformHaptics> {
        Arc::new(Self::NEW)
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
    fn release_session_end(_: &O) {}

    fn session_end_support(_: &O) -> flui_platform::SessionEndSupport {
        flui_platform::SessionEndSupport::NotifyOnly
    }

    fn activate(_: &O, _: bool) {}
}

impl PlatformSystemChrome for Absent<position::Chrome> {
    fn set_overlay_style(&self, _: SystemUiOverlayStyle) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Every window styles system bars that are not there.
impl<W: ?Sized> Chrome<W> for Absent<position::Chrome> {
    fn for_window(_: &W) -> Arc<dyn PlatformSystemChrome> {
        Arc::new(Self::NEW)
    }
}

impl<O> Storage<O> for Absent<position::Storage> {
    fn data_store(_: &O, _: &AppDataDir) -> Option<DataStore> {
        None
    }
}
