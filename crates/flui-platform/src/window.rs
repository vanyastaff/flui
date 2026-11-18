//! Window vocabulary: identity, creation options, lifecycle state and
//! the events a backend reports about its windows.
//!
//! These are the values the per-window contract,
//! [`PlatformWindow`](crate::PlatformWindow), and the host-facing
//! `flui_native::Platform` that opens windows (which stays in
//! `flui-native`, ADR-0082 §2) use in their signatures.

use std::num::NonZeroU64;

use flui_foundation::geometry::Size;
use flui_foundation::{IdExhausted, IdMint};

use crate::os_options::OsWindowOptions;
use crate::{WindowExtent, WindowFrame, WindowGeometry, WindowMode, WindowPosition};

// ==================== Creation ====================

/// When a window opened [`WindowOptions::visible`]` == true` first becomes
/// visible to the viewer.
///
/// The choice is the caller's because the two callers differ in what they
/// can promise. An embedder that drives the frame loop (`flui-app`) reports
/// its first presented frame through
/// [`PlatformWindow::reveal_after_first_frame`](crate::PlatformWindow::reveal_after_first_frame),
/// so it may ask
/// for the reveal to wait for that frame and never show a bare background. A
/// direct consumer of the platform crate — an example, a probe, a test — has
/// no such report to give, and a window whose reveal waits for a call that
/// never comes is a window nobody sees; for those, the reveal happens at
/// open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowReveal {
    /// The window is on screen when `open_window` returns (the default).
    /// Every backend supports this.
    #[default]
    AtOpen,
    /// The window is ordered on screen but stays invisible to the viewer
    /// until [`PlatformWindow::reveal_after_first_frame`](crate::PlatformWindow::reveal_after_first_frame) is
    /// called; the caller commits to calling it (or to a bounded fallback
    /// that does). A backend that cannot defer treats this as
    /// [`Self::AtOpen`].
    AfterFirstFrame,
}

/// How a window trades pointer input latency against batching.
///
/// A touch or pen stroke drawn live (ink, a canvas) wants every sample the
/// hardware produced, as early as the system can deliver it; a list or a form
/// is served as well by the system's per-frame batches. A backend that cannot
/// deliver input earlier than it already does ignores [`Self::Low`], and its
/// [`WindowCapabilities`](crate::WindowCapabilities) are unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputLatency {
    /// The system's usual delivery: pointer samples batched once per frame.
    #[default]
    Balanced,
    /// Pointer samples as soon as the system has them: Android unbuffered
    /// dispatch, the web's `pointerrawupdate`.
    Low,
}

/// Window creation options.
///
/// Adding an option is not a breaking change: the type is `#[non_exhaustive]`,
/// so it is built from [`Default`] with its builder methods, each named after
/// the field it sets ([`title`](Self::title), [`size`](Self::size), …).
///
/// # Geometry
///
/// [`size`](Self::size) is the **content** size: what
/// [`WindowState::content_size`](crate::WindowState::content_size) reports
/// after the window opens. The backend adds the window's frame (title bar and
/// borders) to ask the window system for the outer window, so reopening a
/// window with the size it last reported does not shrink it by its frame.
///
/// [`position`](Self::position) and the position of
/// [`normal_geometry`](Self::normal_geometry) are each a [`WindowPosition`]:
/// a point of one display, in that display's logical pixels from its top-left
/// corner. A backend converts it through that display's own scale and origin,
/// so a desktop of mixed scales needs no single global scale. A position on a
/// display the backend's display snapshot no longer holds is no place the
/// backend can find: the window opens where the window system places it, and
/// its first snapshot says where that is.
///
/// The numbers are checked by their types ([`WindowExtent`],
/// [`WindowPosition`]), so only what a type cannot say is checked when the
/// window opens ([`geometry`](Self::geometry)): a size or limit that encloses
/// no area, a minimum above the maximum.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WindowOptions {
    /// Window title
    pub title: String,
    /// Initial content size, in logical pixels. One that encloses no area
    /// refuses the open ([`geometry`](Self::geometry)).
    pub size: WindowExtent,
    /// Whether window is resizable
    pub resizable: bool,
    /// Whether the window should be visible initially.
    ///
    /// `true` is the INTENDED state: with [`Self::reveal`] at its default
    /// the window is on screen when `open_window` returns; with
    /// [`WindowReveal::AfterFirstFrame`] a backend that can defer keeps it
    /// invisible to the viewer until the embedder reports its first
    /// presented frame, its snapshot's
    /// [`WindowState::visible`](crate::WindowState::visible) `true` meanwhile. `false`
    /// stays hidden until shown explicitly, whatever `reveal` says.
    pub visible: bool,
    /// When a `visible: true` window first becomes visible to the viewer;
    /// see [`WindowReveal`] for who should pick what. Ignored for
    /// `visible: false`.
    pub reveal: WindowReveal,
    /// What frames the window. A desktop window that cannot take
    /// [`WindowFrame::AppTitleBar`] ([`WindowCapabilities::app_title_bar`](crate::WindowCapabilities::app_title_bar))
    /// opens with [`WindowFrame::System`]. Hosts without a system frame, such as
    /// mobile and web, retain their native frame policy.
    pub frame: WindowFrame,
    /// The smallest content size, if limited. A limit that encloses no area,
    /// or a minimum above [`max_size`](Self::max_size), refuses the open
    /// ([`geometry`](Self::geometry)).
    pub min_size: Option<WindowExtent>,
    /// The largest content size, if limited.
    pub max_size: Option<WindowExtent>,
    /// Initial outer position, on the display it names; `None` lets the
    /// system place the window. A backend that cannot place a window at open
    /// ignores it, and says so through its
    /// [`WindowCapabilities::places_at_open`](crate::WindowCapabilities::places_at_open).
    pub position: Option<WindowPosition>,
    /// The mode the window opens in. A backend that cannot enter it opens the
    /// window in [`WindowMode::Normal`], which its first snapshot shows.
    pub mode: WindowMode,
    /// The outer rectangle the window returns to from a maximized or fullscreen
    /// opening mode, so a restored maximized window keeps its earlier bounds.
    /// Its position, where it has one, names its own display, as
    /// [`position`](Self::position) does.
    pub normal_geometry: Option<WindowGeometry>,
    /// How early pointer samples reach the window; see [`InputLatency`].
    pub input_latency: InputLatency,
    /// What the window is created with on each operating system that has
    /// options of its own, each read only by that system's host; see
    /// [`OsWindowOptions`].
    pub os_options: OsWindowOptions,
}

impl WindowOptions {
    /// Set the title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the initial content size.
    #[must_use]
    pub const fn size(mut self, size: WindowExtent) -> Self {
        self.size = size;
        self
    }

    /// Set whether the window is resizable.
    #[must_use]
    pub const fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether the window is visible initially.
    #[must_use]
    pub const fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set when a visible window first becomes visible to the viewer.
    #[must_use]
    pub const fn reveal(mut self, reveal: WindowReveal) -> Self {
        self.reveal = reveal;
        self
    }

    /// Set what frames the window.
    #[must_use]
    pub const fn frame(mut self, frame: WindowFrame) -> Self {
        self.frame = frame;
        self
    }

    /// Set the minimum content size.
    #[must_use]
    pub const fn min_size(mut self, min_size: Option<WindowExtent>) -> Self {
        self.min_size = min_size;
        self
    }

    /// Set the maximum content size.
    #[must_use]
    pub const fn max_size(mut self, max_size: Option<WindowExtent>) -> Self {
        self.max_size = max_size;
        self
    }

    /// The numbers these options open a window at, each checked by the same
    /// check as the command that sets it later: [`size`](Self::size) as
    /// [`WindowCommand::SetSize`](crate::WindowCommand::SetSize),
    /// [`min_size`](Self::min_size) and [`max_size`](Self::max_size) as
    /// [`WindowCommand::SetConstraints`](crate::WindowCommand::SetConstraints).
    /// ([`position`](Self::position) and
    /// [`normal_geometry`](Self::normal_geometry) are valid by their types, as
    /// [`WindowCommand::SetPosition`](crate::WindowCommand::SetPosition)'s
    /// position is.)
    ///
    /// Every host opens a window through here, before it takes an id for it,
    /// and refuses the open for numbers a command would be refused for.
    ///
    /// # Errors
    /// [`InvalidWindowOption`], naming the option and the numbers: a size or
    /// limit that encloses no area, a minimum above the maximum.
    pub fn geometry(&self) -> Result<OpeningGeometry, InvalidWindowOption> {
        let refused = |option| move |refused| InvalidWindowOption { option, refused };
        let content = self.size.area().map_err(refused(WindowOption::Size))?;
        let min = self
            .min_size
            .map(crate::WindowExtent::area)
            .transpose()
            .map_err(refused(WindowOption::MinSize))?;
        let max = self
            .max_size
            .map(crate::WindowExtent::area)
            .transpose()
            .map_err(refused(WindowOption::MaxSize))?;
        let constraints = crate::SizeConstraints::new(min, max)
            .asked()
            .map_err(refused(WindowOption::Limits))?;
        Ok(OpeningGeometry {
            content,
            constraints,
            position: self.position,
        })
    }

    /// Set the initial outer position.
    #[must_use]
    pub const fn position(mut self, position: Option<WindowPosition>) -> Self {
        self.position = position;
        self
    }

    /// Set the mode the window opens in.
    #[must_use]
    pub const fn mode(mut self, mode: WindowMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the rectangle a maximized or fullscreen window returns to.
    #[must_use]
    pub const fn normal_geometry(mut self, normal_geometry: Option<WindowGeometry>) -> Self {
        self.normal_geometry = normal_geometry;
        self
    }

    /// Set how early pointer samples reach the window.
    #[must_use]
    pub const fn input_latency(mut self, input_latency: InputLatency) -> Self {
        self.input_latency = input_latency;
        self
    }

    /// Set what the window is created with on each operating system that has
    /// options of its own.
    #[must_use]
    pub fn os_options(mut self, os_options: OsWindowOptions) -> Self {
        self.os_options = os_options;
        self
    }
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "FLUI Window".to_string(),
            size: DEFAULT_SIZE,
            resizable: true,
            visible: true,
            reveal: WindowReveal::AtOpen,
            frame: WindowFrame::System,
            min_size: None,
            max_size: None,
            position: None,
            mode: WindowMode::Normal,
            normal_geometry: None,
            input_latency: InputLatency::Balanced,
            os_options: OsWindowOptions::default(),
        }
    }
}

/// The content size a window opens at when its options name none: 800 by 600
/// logical pixels.
const DEFAULT_SIZE: WindowExtent = match WindowExtent::try_new(Size::new(800.0, 600.0)) {
    Ok(size) => size,
    Err(_) => panic!("BUG: 800x600 is a finite, non-negative extent"),
};

/// The numbers a window opens at, each checked as the command that sets it
/// is ([`WindowOptions::geometry`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpeningGeometry {
    /// The content extent asked for ([`WindowOptions::size`]): finite and
    /// enclosing an area.
    pub content: crate::WindowExtent,
    /// The limits asked for ([`WindowOptions::min_size`],
    /// [`WindowOptions::max_size`]): each enclosing an area, the minimum not
    /// above the maximum.
    pub constraints: crate::SizeConstraints,
    /// The position asked for ([`WindowOptions::position`]).
    pub position: Option<WindowPosition>,
}

/// A [`WindowOptions`] field whose numbers no window can be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowOption {
    /// [`WindowOptions::size`].
    Size,
    /// [`WindowOptions::min_size`].
    MinSize,
    /// [`WindowOptions::max_size`].
    MaxSize,
    /// The two limits together: the minimum is above the maximum.
    Limits,
}

impl std::fmt::Display for WindowOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Size => "size",
            Self::MinSize => "min_size",
            Self::MaxSize => "max_size",
            Self::Limits => "min_size, max_size",
        })
    }
}

/// Why a window was not opened: an option asks for numbers that a command
/// setting them would be refused for. It names the option and the numbers,
/// so an application author knows what to change.
///
/// ```
/// use flui_foundation::geometry::Size;
/// use flui_platform::{WindowExtent, WindowOptions};
///
/// let options = WindowOptions::default()
///     .min_size(Some(WindowExtent::try_new(Size::new(0.0, 600.0))?));
/// let refused = options.geometry().expect_err("a minimum with no area");
/// assert_eq!(
///     refused.to_string(),
///     "the window option `min_size` describes no window: window size 0x600 encloses no area"
/// );
/// # Ok::<(), flui_platform::InvalidGeometry>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct InvalidWindowOption {
    /// The option refused.
    pub option: WindowOption,
    /// Its numbers, as the command would have refused them.
    pub refused: crate::InvalidGeometry,
}

impl std::fmt::Display for InvalidWindowOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let refused = self.refused;
        match self.option {
            WindowOption::Limits => write!(
                f,
                "the window options `min_size` and `max_size` describe no window: {refused}"
            ),
            option @ (WindowOption::Size | WindowOption::MinSize | WindowOption::MaxSize) => {
                write!(
                    f,
                    "the window option `{option}` describes no window: {refused}"
                )
            }
        }
    }
}

impl std::error::Error for InvalidWindowOption {}

// ==================== Identity and mode ====================

/// The identity of one window within the host that opened it.
///
/// Issued only by a [`WindowIdMint`], which the host holds and which never
/// issues a value twice: a window that closes takes its id with it, and the
/// next window gets another one even when the operating system hands it the
/// recycled handle or address of the closed one. So a late event addressed to
/// a closed window names no window, never its successor. A native handle is
/// never an id: a backend keyed by one keeps a map from the handle to the id
/// it issued, and a handle that is not in the map names no window.
///
/// There is no conversion from a number outside the `testing` feature's
/// `try_from_raw`, which a recording decoder uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(NonZeroU64);

impl WindowId {
    /// The issued value, for diagnostics and recordings. It means nothing
    /// outside the host that issued it.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }

    /// The id a recording or a test names by value, or `None` for 0, which
    /// no mint issues.
    ///
    /// Behind the `testing` feature: production code gets an id from the
    /// mint of the host that opened the window, and a value built here is a
    /// claim, not proof of a window: the host's own table still decides
    /// whether it names one.
    #[cfg(feature = "testing")]
    #[must_use]
    pub const fn try_from_raw(raw: u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }
}

/// Issues the [`WindowId`]s of one host, in increasing order from 1, and
/// never one twice.
///
/// Owner-local, as the windows are: a host keeps one in the state of the
/// thread that opens its windows. It never wraps: once its last value was
/// issued, every later [`next`](Self::next) refuses with [`IdExhausted`].
///
/// ```rust
/// use flui_platform::WindowIdMint;
///
/// let ids = WindowIdMint::new();
/// let first = ids.next()?;
/// let second = ids.next()?;
/// assert_ne!(first, second);
/// assert_eq!(first.get().get(), 1);
/// # Ok::<(), flui_foundation::IdExhausted>(())
/// ```
#[derive(Debug, Default)]
pub struct WindowIdMint(IdMint<NonZeroU64>);

impl WindowIdMint {
    /// A mint whose first id is 1.
    #[must_use]
    pub const fn new() -> Self {
        Self(IdMint::new())
    }

    /// Issue the id of a window about to open.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once every id was issued; the mint refuses from then
    /// on, so no window ever gets an id another window had.
    pub fn next(&self) -> Result<WindowId, IdExhausted> {
        self.0.next().map(WindowId)
    }
}

// ==================== Lifecycle state ====================

/// Native execution eligibility, independent of focus, visibility and GPU surface readiness.
///
/// `Detached` is a reversible native attachment observation. Terminal window
/// closure is a separate lifetime event and cannot be reversed with this state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowExecutionState {
    /// The platform permits UI execution; this does not guarantee a usable GPU surface.
    #[default]
    Running,
    /// UI execution is suspended, even if stale native focus/visibility remain true.
    Suspended,
    /// The native presentation is detached, but may subsequently attach again.
    Detached,
}

// ==================== Value Types ====================

/// The material the window system draws behind a window's content, which
/// shows through the window's frame and wherever its content is transparent
/// ([`WindowCommand::SetMaterial`](crate::WindowCommand::SetMaterial)).
///
/// The vocabulary every host shares: each material is one every window
/// system that has materials at all can draw, so a host whose
/// [`WindowCapabilities::material`](crate::WindowCapabilities::material) is
/// is available draws each of them. A material only one operating system
/// has (Mica Alt and Acrylic on Windows, the `NSVisualEffectView` materials
/// and Liquid Glass on macOS) is reached through that system's extension
/// (`flui::native::windows`, `flui::native::macos`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "every host maps every material: a new material is a change each host makes, not one it may skip"
)]
pub enum WindowMaterial {
    /// No material: the window's own background, opaque.
    #[default]
    Opaque,
    /// The window system's material for a long-lived window's background,
    /// tinted by the desktop behind it: Mica on Windows 11.
    System,
}
