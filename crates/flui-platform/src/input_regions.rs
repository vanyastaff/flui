//! Screen areas the application claims for its own input, handed to the OS per window
//! (ADR-0217 §5).
//!
//! The framework collects [`InputRegion`]s in window-logical coordinates from layout and
//! passes them to [`PlatformWindow::set_input_regions`](crate::PlatformWindow::set_input_regions)
//! on every composited frame. The backend projects them to device pixels once, with the
//! window's current scale ([`DeviceInputRegion::project`]), and updates the OS only when
//! the projection changed.

use flui_foundation::geometry::{
    DevicePixelRatio, DevicePoint, DeviceRect, Rect, device_rect_covering,
};

/// What the OS should do with an input region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputRegionKind {
    /// The OS does not start its own edge gestures (Android back and home swipes, iOS
    /// screen-edge gestures) inside the region, so the application's gesture there wins.
    SystemGestureExclusion,
    /// A press here moves the window, as a press on the system's title bar does: the system
    /// runs the move, its snapping and its double-click, and a secondary press there shows
    /// the window menu. For a title bar the application draws
    /// ([`WindowFrame::AppTitleBar`](crate::WindowFrame::AppTitleBar)).
    WindowMove,
    /// A press here resizes the window from `edge`, run by the system, for a window whose
    /// system gives it no border to resize it by ([`WindowFrame::None`](crate::WindowFrame::None)).
    WindowResize(ResizeEdge),
    /// The application's drawing of a caption button. The system treats the area as its own
    /// button would be treated where that matters: on Windows 11 the maximize button opens
    /// the snap layouts over it. The application's button still takes the press.
    CaptionButton(CaptionButton),
    /// Taken back out of a [`WindowMove`](Self::WindowMove) or
    /// [`WindowResize`](Self::WindowResize) region below it: a press here reaches the
    /// application.
    WindowContent,
}

/// The edge or corner a [`InputRegionKind::WindowResize`] region resizes the window from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[derive(flui_foundation::inspect::Inspect)]
pub enum ResizeEdge {
    /// The top edge.
    Top,
    /// The bottom edge.
    Bottom,
    /// The left edge.
    Left,
    /// The right edge.
    Right,
    /// The top-left corner.
    TopLeft,
    /// The top-right corner.
    TopRight,
    /// The bottom-left corner.
    BottomLeft,
    /// The bottom-right corner.
    BottomRight,
}

/// The caption button an [`InputRegionKind::CaptionButton`] region draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[derive(flui_foundation::inspect::Inspect)]
pub enum CaptionButton {
    /// Minimizes the window.
    Minimize,
    /// Maximizes the window, or restores it when maximized.
    Maximize,
    /// Closes the window.
    Close,
}

/// One region in window-logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputRegion {
    kind: InputRegionKind,
    rect: Rect,
}

impl InputRegion {
    /// A `kind` region covering `rect`, in window-logical pixels.
    #[must_use]
    pub const fn new(kind: InputRegionKind, rect: Rect) -> Self {
        Self { kind, rect }
    }

    /// What the region is for.
    #[must_use]
    pub const fn kind(&self) -> InputRegionKind {
        self.kind
    }

    /// Where it is, in window-logical pixels.
    #[must_use]
    pub const fn rect(&self) -> Rect {
        self.rect
    }
}

/// One region in device pixels, as a backend hands it to the OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceInputRegion {
    kind: InputRegionKind,
    rect: DeviceRect,
}

/// Why a region has no device-pixel form ([`DeviceInputRegion::project`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NotProjected {
    /// The region covers nothing.
    #[error("the region is empty")]
    Empty,
    /// An edge is not finite in device pixels.
    #[error("its device-pixel edges are not finite")]
    NotFinite,
    /// A rounded edge, or the width or height, does not fit in `i32`.
    #[error("its device-pixel edges or size do not fit in i32")]
    OutOfRange,
}

impl DeviceInputRegion {
    /// A `kind` region covering the device pixels of `rect`.
    #[must_use]
    pub const fn new(kind: InputRegionKind, rect: DeviceRect) -> Self {
        Self { kind, rect }
    }

    /// `region` (window-logical) in device pixels at `ratio`: each edge rounds outward
    /// (floor of the minima, ceiling of the maxima), so a thin region at a fractional scale
    /// keeps at least one pixel.
    ///
    /// # Errors
    ///
    /// [`NotProjected`] for an empty region, and for one whose device edges are not finite
    /// or whose rounded edges, width or height do not fit in `i32`: such a region is
    /// dropped, never saturated or wrapped.
    pub fn project(region: InputRegion, ratio: DevicePixelRatio) -> Result<Self, NotProjected> {
        let rect = region.rect;
        if rect.is_empty() {
            return Err(NotProjected::Empty);
        }
        let edges =
            [rect.left(), rect.top(), rect.right(), rect.bottom()].map(|e| ratio.to_device(e));
        if edges.iter().any(|edge| !edge.is_finite()) {
            return Err(NotProjected::NotFinite);
        }
        let [left, top, right, bottom] = edges;
        let [left, top, right, bottom] = [left.floor(), top.floor(), right.ceil(), bottom.ceil()];
        let in_range = |value: f64| (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value);
        let representable = [left, top, right, bottom].into_iter().all(in_range)
            && right - left <= f64::from(i32::MAX)
            && bottom - top <= f64::from(i32::MAX);
        if !representable {
            return Err(NotProjected::OutOfRange);
        }
        Ok(Self::new(region.kind, device_rect_covering(rect, ratio)))
    }

    /// What the region is for.
    #[must_use]
    pub const fn kind(&self) -> InputRegionKind {
        self.kind
    }

    /// Where it is, in window device pixels.
    #[must_use]
    pub const fn rect(&self) -> DeviceRect {
        self.rect
    }
}

/// What a window's chrome regions say is under a point: the answer a backend gives the
/// system when it asks what a press there does ([`chrome_hit`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChromeHit {
    /// The application's content: the press reaches the application.
    Content,
    /// The window's title bar: the system moves the window.
    Move,
    /// A resize border: the system resizes the window from the edge.
    Resize(ResizeEdge),
    /// A caption button the application draws.
    CaptionButton(CaptionButton),
}

/// What `regions` say is at `point`, both in window device pixels, as a backend answers the
/// system's hit test (`WM_NCHITTEST`, a press on an AppKit or Wayland window, an X11 button
/// press) from the set the last composited frame handed it, never by running the
/// framework's hit test inside the system's callback.
///
/// The regions are in paint order, the order
/// [`set_input_regions`](crate::PlatformWindow::set_input_regions) receives them in, so a
/// region painted later lies over one painted earlier: the last chrome region whose
/// rectangle holds the point decides, and a [`InputRegionKind::WindowContent`] region over a
/// move area gives the point back to the application. A rectangle holds the points from its
/// minimum edges up to, not including, its maximum ones. Regions of other kinds
/// ([`InputRegionKind::SystemGestureExclusion`]) are not chrome and are passed over; a point
/// no chrome region holds is [`ChromeHit::Content`].
#[must_use]
pub fn chrome_hit(regions: &[DeviceInputRegion], point: DevicePoint) -> ChromeHit {
    let holds = |rect: DeviceRect| {
        rect.min.x <= point.x
            && point.x < rect.max.x
            && rect.min.y <= point.y
            && point.y < rect.max.y
    };
    regions
        .iter()
        .rev()
        .filter(|region| holds(region.rect))
        .find_map(|region| match region.kind {
            InputRegionKind::SystemGestureExclusion => None,
            InputRegionKind::WindowMove => Some(ChromeHit::Move),
            InputRegionKind::WindowResize(edge) => Some(ChromeHit::Resize(edge)),
            InputRegionKind::CaptionButton(button) => Some(ChromeHit::CaptionButton(button)),
            InputRegionKind::WindowContent => Some(ChromeHit::Content),
        })
        .unwrap_or(ChromeHit::Content)
}

impl flui_foundation::inspect::ToValue for InputRegionKind {
    fn to_value(&self) -> flui_foundation::inspect::Value<'_> {
        let name = match self {
            Self::SystemGestureExclusion => "SystemGestureExclusion",
            Self::WindowMove => "WindowMove",
            Self::WindowResize(ResizeEdge::Top) => "WindowResize(Top)",
            Self::WindowResize(ResizeEdge::Bottom) => "WindowResize(Bottom)",
            Self::WindowResize(ResizeEdge::Left) => "WindowResize(Left)",
            Self::WindowResize(ResizeEdge::Right) => "WindowResize(Right)",
            Self::WindowResize(ResizeEdge::TopLeft) => "WindowResize(TopLeft)",
            Self::WindowResize(ResizeEdge::TopRight) => "WindowResize(TopRight)",
            Self::WindowResize(ResizeEdge::BottomLeft) => "WindowResize(BottomLeft)",
            Self::WindowResize(ResizeEdge::BottomRight) => "WindowResize(BottomRight)",
            Self::CaptionButton(CaptionButton::Minimize) => "CaptionButton(Minimize)",
            Self::CaptionButton(CaptionButton::Maximize) => "CaptionButton(Maximize)",
            Self::CaptionButton(CaptionButton::Close) => "CaptionButton(Close)",
            Self::WindowContent => "WindowContent",
        };
        flui_foundation::inspect::Value::Variant {
            ty: "InputRegionKind",
            name,
        }
    }
}
