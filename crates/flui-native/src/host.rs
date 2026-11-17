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

#[cfg(any(wayland_host, x11_host, windows))]
pub(crate) use clipboard::MAX_TEXT;
#[cfg(any(target_arch = "wasm32", wayland_host, x11_host, test))]
pub(crate) use clipboard::owned_text;
#[cfg(windows)]
pub(crate) use clipboard::text_fits;
pub use clipboard::{MemoryClipboard, NativeClipboard, NativePrimarySelection};
pub(crate) use clipboard::{SharedClipboard, SyncClipboard};
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    wayland_host,
    x11_host,
))]
pub(crate) use clipboard::{admit_text, host_text};

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{Bounds, Size};
use flui_platform::data_transfer::{DragTransport, NullDragTransport};
use flui_platform::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform::{
    AppDataDir, DeviceId, DisplaySnapshot, HapticFeedback, LaunchKey, PlatformHaptics,
    PlatformSystemChrome, SystemPreferences, SystemUiOverlayStyle, Unsupported, UnsupportedReason,
    WindowCapabilities, WindowHandler, WindowOptions,
};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};

use crate::error::PlatformError;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::platform::{AppHandler, PathPromptOptions, Platform};
use crate::storage::DataStore;
use crate::task::Task;
use crate::text::TextInput;

/// `key`'s launch value where a host reads the environment (ADR-0288 §7): the variable
/// [`LaunchKey::environment_variable`] spells, `None` when it is unset, empty or not Unicode.
pub(crate) fn environment_launch_value(key: LaunchKey) -> Option<String> {
    std::env::var(key.environment_variable())
        .ok()
        .filter(|value| !value.is_empty())
}

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
pub trait Loop: Sized + 'static {
    /// Runs the loop on the calling thread, handing `handler` the owner
    /// capability over `platform` once windows can open; see
    /// [`Platform::run`].
    ///
    /// # Errors
    /// What [`Platform::run`] reports.
    fn run(
        self: Rc<Self>,
        platform: Rc<dyn Platform>,
        handler: Rc<dyn AppHandler>,
    ) -> Result<(), PlatformError>;

    /// [`Platform::quit`].
    fn quit(&self);

    /// [`Platform::request_exit_policy_reevaluation`].
    fn request_exit_policy_reevaluation(&self);
}

/// What the host does for the application outside the closed list: the
/// application's path, opening URLs and paths, file
/// prompts and the compositor's name. They leave [`Platform`] for the system
/// capability crate (ADR-0151 §9); until then the loop carries them, and each
/// host writes its answer, inert ones included.
pub trait Services {
    /// [`Platform::app_path`].
    ///
    /// # Errors
    /// [`PlatformError::AppPath`] when the lookup fails.
    fn app_path(&self) -> Result<PathBuf, PlatformError>;

    /// [`Platform::launch_value`].
    fn launch_value(&self, key: LaunchKey) -> Option<String>;

    /// [`Platform::page_origin`]: `None` but on the web.
    fn page_origin(&self) -> Option<String>;

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
}

/// The window position: opening windows, and what is known of them before
/// one exists.
pub trait Window<O> {
    /// What the windows of one platform share beside the loop `O`: what a
    /// window opens with and what the windows keep in common (the Win32
    /// handle map and configuration, the macOS configuration, the UIKit window
    /// factory, the headless window model), `()` where the loop keeps it all.
    /// The host's constructor builds one, hands it to the loop as the hook the
    /// loop declares and to the platform, which passes it to every call of
    /// this position: one
    /// instance for both (`HostPlatform::from_parts`). It holds the
    /// [`WindowPositions`] the platform made from the table.
    type Shared: 'static;

    /// The platform's own reference to one of its windows, which the
    /// window-level positions are made for: the Win32 handle with its
    /// identity, the content view, what the Activity's
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

    /// [`Platform::window_capabilities`].
    fn capabilities(owner: &O, shared: &Self::Shared) -> WindowCapabilities;

    /// [`Platform::window_frame_inset`].
    fn frame_inset(
        owner: &O,
        shared: &Self::Shared,
        scale_factor: flui_foundation::geometry::DevicePixelRatio,
    ) -> Size<f64>;

    /// [`Platform::display_connection`].
    fn display_connection(
        owner: &O,
        shared: &Self::Shared,
    ) -> Arc<dyn flui_platform::DisplayConnection>;
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

    /// The window's rectangles on the screen changed, on its owner thread:
    /// an assistive technology places the window's nodes from them. A bridge
    /// whose adapter asks the OS where the window is (through the window or
    /// view it holds) has nothing to do.
    fn bounds_changed(&self, bounds: ScreenBounds);
}

/// Where a window lies on the screen, in device pixels: its outer rectangle,
/// with the frame the window manager or the window draws around it, and its
/// content's, which the tree's root covers. A platform that does not tell a
/// window where it is (Wayland) gives the outer rectangle at the origin, so
/// the content's is relative to the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenBounds {
    outer: Bounds<i32>,
    content: Bounds<i32>,
}

impl ScreenBounds {
    /// The window with its frame at `outer` and its content at `content`.
    ///
    /// # Errors
    /// [`InvalidScreenBounds::NegativeSize`] where either rectangle has a
    /// negative width or height, and [`InvalidScreenBounds::ContentOutside`]
    /// where the content does not lie within the outer rectangle.
    pub fn try_new(outer: Bounds<i32>, content: Bounds<i32>) -> Result<Self, InvalidScreenBounds> {
        let sized = |bounds: Bounds<i32>| bounds.size.width >= 0 && bounds.size.height >= 0;
        if !(sized(outer) && sized(content)) {
            return Err(InvalidScreenBounds::NegativeSize);
        }
        // The far edges in `i64`, where no sum of two `i32` overflows.
        let far = |origin: i32, extent: i32| i64::from(origin) + i64::from(extent);
        let within = |near: i32, extent: i32, outer_near: i32, outer_extent: i32| {
            near >= outer_near && far(near, extent) <= far(outer_near, outer_extent)
        };
        if !(within(
            content.origin.x,
            content.size.width,
            outer.origin.x,
            outer.size.width,
        ) && within(
            content.origin.y,
            content.size.height,
            outer.origin.y,
            outer.size.height,
        )) {
            return Err(InvalidScreenBounds::ContentOutside);
        }
        Ok(Self { outer, content })
    }

    /// The window with its frame.
    #[must_use]
    pub const fn outer(&self) -> Bounds<i32> {
        self.outer
    }

    /// The window's content, which the tree's root covers.
    #[must_use]
    pub const fn content(&self) -> Bounds<i32> {
        self.content
    }
}

/// Why two rectangles are not where a window lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidScreenBounds {
    /// A rectangle has a negative width or height.
    #[error("a window rectangle has a negative size")]
    NegativeSize,
    /// The content does not lie within the window's outer rectangle.
    #[error("the content lies outside the window's outer rectangle")]
    ContentOutside,
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

pub(crate) use input::Input;

/// The input position, sealed: public in a crate-private module, so a table
/// outside the crate can name [`Absent`] for it but nothing outside the crate
/// can call it (ADR-0151 §9).
mod input {
    /// The input position above a window: the keyboard layout. Pointer, key
    /// and gesture input reach a window through its own callbacks.
    ///
    /// Nothing above the host reads the layout yet, so it stays in the core
    /// host (ADR-0151 §9): the menu feature, which labels shortcuts,
    /// publishes it with that consumer. The host's integration tests reach it
    /// through `testing::keyboard_layout`.
    pub trait Input<O> {
        /// The current keyboard layout's name as the operating system gives
        /// it; empty where the host does not read the layout.
        fn keyboard_layout(owner: &O) -> String;

        /// Call `callback` on each keyboard layout change. A registration
        /// replaces the callback before it, which is dropped then; a host
        /// that does not read the layout never calls it. The callback is
        /// called with no lock of the platform held, and its panic is the
        /// failure of the turn that heard the change, as a window handler's
        /// is.
        fn on_keyboard_layout_change(owner: &O, callback: Box<dyn FnMut()>);
    }
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
    fn data_transfer(owner: &O) -> Arc<dyn DragTransport>;
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
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Window {}
    /// The displays position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Displays {}
    /// The input position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Input {}
    /// The text position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Text {}
    /// The accessibility position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Accessibility {}
    /// The clipboard position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Clipboard {}
    /// The primary selection position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum PrimarySelection {}
    /// The drag-and-drop position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Drag {}
    /// The haptics position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Haptics {}
    /// The system preferences position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Preferences {}
    /// The lifecycle position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Lifecycle {}
    /// The system chrome position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
    pub enum Chrome {}
    /// The storage position.
    #[derive(Debug)]
    #[expect(clippy::exhaustive_enums, reason = "a position marker has no values")]
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

    fn capabilities(_: &O, (): &()) -> WindowCapabilities {
        WindowCapabilities::NONE
    }

    fn frame_inset(_: &O, (): &(), _: flui_foundation::geometry::DevicePixelRatio) -> Size<f64> {
        Size::new(0.0, 0.0)
    }

    /// No window, so no display either.
    fn display_connection(_: &O, (): &()) -> Arc<dyn flui_platform::DisplayConnection> {
        Arc::new(crate::display_connection::NoDisplay)
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

    fn on_keyboard_layout_change(_: &O, callback: Box<dyn FnMut()>) {
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

    fn bounds_changed(&self, _: ScreenBounds) {}
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
/// [`ClipboardError::Unavailable`](flui_platform::ClipboardError), and a
/// write is refused with it.
impl SyncClipboard for Absent<position::Clipboard> {
    fn read_now(&self) -> flui_platform::ClipboardRead {
        Err(flui_platform::ClipboardError::Unavailable)
    }

    fn write_now(&self, write: flui_platform::ClipboardWrite) -> flui_platform::ClipboardWritten {
        drop(write);
        Err(flui_platform::ClipboardError::Unavailable)
    }

    /// A refused write is kept nowhere, so nothing can keep a history of it.
    fn concealment(&self) -> flui_platform::Concealment {
        flui_platform::Concealment::Honored
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
    fn data_transfer(_: &O) -> Arc<dyn DragTransport> {
        Arc::new(NullDragTransport)
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

#[cfg(test)]
mod tests {
    use flui_foundation::geometry::Point;

    use super::*;

    /// Rectangles that place no window are refused: a negative extent, and
    /// content that leaves the outer rectangle on any side, the far edges
    /// compared without overflow at the ends of the coordinate range.
    #[test]
    fn screen_bounds_hold_their_content_within_the_window() {
        let bounds = |x, y, width, height| Bounds::new(Point::new(x, y), Size::new(width, height));
        let outer = bounds(100, 50, 400, 300);
        for (case, outer, content, expected) in [
            ("framed", outer, bounds(105, 80, 390, 265), Ok(())),
            ("frameless", outer, outer, Ok(())),
            (
                "at the end of the range",
                bounds(i32::MAX, i32::MIN, i32::MAX, i32::MAX),
                bounds(i32::MAX, i32::MIN, i32::MAX, i32::MAX),
                Ok(()),
            ),
            (
                "a negative width",
                outer,
                bounds(105, 80, -1, 265),
                Err(InvalidScreenBounds::NegativeSize),
            ),
            (
                "a negative outer height",
                bounds(100, 50, 400, -300),
                bounds(100, 50, 0, 0),
                Err(InvalidScreenBounds::NegativeSize),
            ),
            (
                "left of the window",
                outer,
                bounds(99, 80, 10, 10),
                Err(InvalidScreenBounds::ContentOutside),
            ),
            (
                "past the window's bottom",
                outer,
                bounds(105, 80, 390, 271),
                Err(InvalidScreenBounds::ContentOutside),
            ),
            (
                "past the end of the range",
                bounds(0, 0, i32::MAX, 10),
                bounds(1, 0, i32::MAX, 10),
                Err(InvalidScreenBounds::ContentOutside),
            ),
        ] {
            assert_eq!(
                ScreenBounds::try_new(outer, content).map(|_| ()),
                expected,
                "{case}"
            );
        }
    }
}
