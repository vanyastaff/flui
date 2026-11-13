//! [`Scrollbar`]: a thumb, and optionally a track, along the edge of the
//! scroll view below it.
//!
//! The counterpart of Flutter's `RawScrollbar` and its `ScrollbarPainter`.
//! The bar finds its scroll view through a scroll observer scope: the
//! nearest [`Scrollable`](super::Scrollable) under it attaches its position
//! and the direction it scrolls in, and a list nested inside that one stays
//! out. [`controller`](Scrollbar::controller) names the scroll view instead.
//!
//! The bar is a foreground painter whose repaint source is the position and
//! the fade, so a scroll frame repaints the bar and rebuilds nothing. Where
//! the painter reports the thumb or the track, the bar takes the contact,
//! from any device: dragging the thumb moves the position, and a tap on the
//! track pages by most of a viewport.
//!
//! Colours and a hover policy belong to the design systems over it; the bar
//! holds the geometry Flutter's raw bar holds.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, DrivenController, Interval, MotionPolicy, Vsync, curves,
};
use flui_foundation::geometry::{Axis, Offset, Rect, Size};
use flui_foundation::{ChangeNotifier, Listenable, Subscription};
use flui_interaction::pointer::EventPropagation;
use flui_interaction::{PointerEvent, PointerEventExt as _, PointerKindSet};
use flui_painting::typography::TextDirection;
use flui_painting::{Canvas, Paint, styling::Color};
use flui_platform::WheelPreferences;
use flui_platform::pointer::ScrollEvent;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::delegates::CustomPainter;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::view::ViewportOffset as _;
use flui_view::environment::{GestureArenaScope, MediaQuery, VsyncScope};
use flui_view::prelude::StatefulView;
use flui_view::{BoxedView, BuildContext, IntoView, LifecycleContext, ViewState};

use super::scroll_observer::{ObservedScroll, ScrollObserver, ScrollObserverScope};
use super::{ScrollController, ScrollableDetails, WheelScrollDistances, wheel_axis_delta};
use crate::interaction::ContactAdmission;
use crate::{CustomPaint, Directionality, DragSlot, GestureDetector, Listener};

/// The share of the viewport a tap on the track pages by.
const TRACK_PAGE: f64 = 0.8;
/// How long a tap on the track takes to page.
const TRACK_PAGE_DURATION: Duration = Duration::from_millis(100);

/// The edge of its scroll view a [`Scrollbar`] sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(clippy::exhaustive_enums, reason = "the four edges of a scroll view")]
pub enum ScrollbarOrientation {
    /// The left edge of a vertical scroll view.
    Left,
    /// The right edge of a vertical scroll view.
    Right,
    /// The top edge of a horizontal scroll view.
    Top,
    /// The bottom edge of a horizontal scroll view.
    Bottom,
}

impl ScrollbarOrientation {
    /// The axis of the scroll views this edge belongs to.
    #[must_use]
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Vertical,
            Self::Top | Self::Bottom => Axis::Horizontal,
        }
    }

    /// The edge a bar takes when none is named: the trailing side of a
    /// vertical scroll view for `text_direction`, the bottom of a horizontal
    /// one.
    #[must_use]
    pub const fn default_for(axis: Axis, text_direction: TextDirection) -> Self {
        match (axis, text_direction) {
            (Axis::Vertical, TextDirection::Rtl) => Self::Left,
            (Axis::Vertical, TextDirection::Ltr) => Self::Right,
            (Axis::Horizontal, _) => Self::Bottom,
        }
    }
}

/// The geometry, visibility and input knobs of a bar, read by its painter.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Knobs {
    thumb_visibility: bool,
    track_visibility: bool,
    interactive: bool,
    thickness: f64,
    radius: f64,
    main_axis_margin: f64,
    cross_axis_margin: f64,
    min_thumb_length: f64,
    thumb_color: Color,
    track_color: Color,
    orientation: Option<ScrollbarOrientation>,
    fade_duration: Duration,
    time_to_fade: Duration,
}

impl Default for Knobs {
    fn default() -> Self {
        Self {
            thumb_visibility: false,
            track_visibility: false,
            interactive: true,
            thickness: Scrollbar::DEFAULT_THICKNESS,
            radius: 0.0,
            main_axis_margin: 0.0,
            cross_axis_margin: 0.0,
            min_thumb_length: Scrollbar::DEFAULT_MIN_THUMB_LENGTH,
            thumb_color: Color::rgba(0, 0, 0, 102),
            track_color: Color::rgba(0, 0, 0, 20),
            orientation: None,
            fade_duration: Scrollbar::DEFAULT_FADE_DURATION,
            time_to_fade: Scrollbar::DEFAULT_TIME_TO_FADE,
        }
    }
}

/// A thumb, and optionally a track, along the edge of the scroll view in
/// `child`.
///
/// The bar follows the nearest scroll view under it: a list nested inside
/// that one has a bar of its own or none. It is hidden until the position
/// scrolls, fades out [`time_to_fade`](Self::time_to_fade) after it stops,
/// and shows only while the content is longer than the viewport.
///
/// # Example
///
/// ```rust,ignore
/// Scrollbar::new(ListView::builder(rows, |_, index| row(index)))
///     .thumb_visibility(true)
/// ```
#[derive(Clone, StatefulView)]
pub struct Scrollbar {
    child: BoxedView,
    followed: Option<ObservedScroll>,
    knobs: Knobs,
}

impl fmt::Debug for Scrollbar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scrollbar")
            .field("followed", &self.followed)
            .field("knobs", &self.knobs)
            .finish_non_exhaustive()
    }
}

impl Scrollbar {
    /// The thickness of a bar that names none, in logical pixels.
    pub const DEFAULT_THICKNESS: f64 = 6.0;
    /// The shortest a thumb gets, in logical pixels, unless the track is
    /// shorter.
    pub const DEFAULT_MIN_THUMB_LENGTH: f64 = 18.0;
    /// How long the bar takes to fade in or out.
    pub const DEFAULT_FADE_DURATION: Duration = Duration::from_millis(300);
    /// How long after the scroll stops the bar starts to fade out.
    pub const DEFAULT_TIME_TO_FADE: Duration = Duration::from_millis(600);

    /// A bar over the scroll view in `child`.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView::new(child.into_view()),
            followed: None,
            knobs: Knobs::default(),
        }
    }

    /// A bar over `child` following the scroll view `details` describe: the
    /// form a [`ScrollBehavior::build_scrollbar`](super::ScrollBehavior::build_scrollbar)
    /// returns.
    #[must_use]
    pub fn for_scrollable(details: &ScrollableDetails, child: impl IntoView) -> Self {
        Self::new(child).controller(details.controller().clone(), details.axis_direction())
    }

    /// Follow the scroll view `controller` moves, whose offset grows toward
    /// `axis_direction`, instead of the one under the bar: a bar beside its
    /// list rather than over it.
    #[must_use]
    pub fn controller(
        mut self,
        controller: ScrollController,
        axis_direction: AxisDirection,
    ) -> Self {
        self.followed = Some(ObservedScroll {
            controller,
            axis_direction,
        });
        self
    }

    /// Show the thumb at all times instead of only while scrolling (default
    /// `false`). Debug builds panic when such a bar paints with no scroll
    /// view to follow.
    #[must_use]
    pub fn thumb_visibility(mut self, visible: bool) -> Self {
        self.knobs.thumb_visibility = visible;
        self
    }

    /// Paint the track under the thumb whenever the thumb shows (default
    /// `false`).
    #[must_use]
    pub fn track_visibility(mut self, visible: bool) -> Self {
        self.knobs.track_visibility = visible;
        self
    }

    /// Take drags of the thumb and taps on the track (default `true`).
    #[must_use]
    pub fn interactive(mut self, interactive: bool) -> Self {
        self.knobs.interactive = interactive;
        self
    }

    /// The bar's extent across the scroll axis, in logical pixels (default
    /// [`DEFAULT_THICKNESS`](Self::DEFAULT_THICKNESS)). A value that is not
    /// finite and positive paints no bar.
    #[must_use]
    pub fn thickness(mut self, thickness: f64) -> Self {
        self.knobs.thickness = thickness;
        self
    }

    /// The corner radius of the thumb and the track (default square).
    #[must_use]
    pub fn radius(mut self, radius: f64) -> Self {
        self.knobs.radius = radius;
        self
    }

    /// Space kept free at both ends of the track (default `0`).
    #[must_use]
    pub fn main_axis_margin(mut self, margin: f64) -> Self {
        self.knobs.main_axis_margin = margin;
        self
    }

    /// Space between the bar and the edge it sits on (default `0`).
    #[must_use]
    pub fn cross_axis_margin(mut self, margin: f64) -> Self {
        self.knobs.cross_axis_margin = margin;
        self
    }

    /// The shortest the thumb gets while the position is in range (default
    /// [`DEFAULT_MIN_THUMB_LENGTH`](Self::DEFAULT_MIN_THUMB_LENGTH)); past
    /// either end it shrinks further as the position overscrolls.
    #[must_use]
    pub fn min_thumb_length(mut self, length: f64) -> Self {
        self.knobs.min_thumb_length = length;
        self
    }

    /// The thumb's colour.
    #[must_use]
    pub fn thumb_color(mut self, color: Color) -> Self {
        self.knobs.thumb_color = color;
        self
    }

    /// The track's colour, painted when
    /// [`track_visibility`](Self::track_visibility) is set.
    #[must_use]
    pub fn track_color(mut self, color: Color) -> Self {
        self.knobs.track_color = color;
        self
    }

    /// The edge the bar sits on, instead of the trailing side of a vertical
    /// scroll view or the bottom of a horizontal one. An edge of the other
    /// axis is a mistake: debug builds panic, release builds use the default.
    #[must_use]
    pub fn orientation(mut self, orientation: ScrollbarOrientation) -> Self {
        self.knobs.orientation = Some(orientation);
        self
    }

    /// How long the bar takes to fade in and out (default
    /// [`DEFAULT_FADE_DURATION`](Self::DEFAULT_FADE_DURATION)).
    #[must_use]
    pub fn fade_duration(mut self, duration: Duration) -> Self {
        self.knobs.fade_duration = duration;
        self
    }

    /// How long after the scroll stops the bar starts to fade out (default
    /// [`DEFAULT_TIME_TO_FADE`](Self::DEFAULT_TIME_TO_FADE)).
    #[must_use]
    pub fn time_to_fade(mut self, duration: Duration) -> Self {
        self.knobs.time_to_fade = duration;
        self
    }
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Where the track and the thumb lie in the bar's box.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarGeometry {
    track: Rect,
    thumb: Rect,
}

/// What the painter reads off the position.
#[derive(Debug, Clone, Copy)]
struct Extents {
    pixels: f64,
    min: f64,
    max: f64,
    viewport: f64,
}

fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// The track and the thumb of a bar of `knobs` in a box of `size`, or none
/// when the content fits or the geometry is degenerate.
fn bar_geometry(
    knobs: &Knobs,
    size: Size,
    extents: Extents,
    axis_direction: AxisDirection,
    text_direction: TextDirection,
) -> Option<BarGeometry> {
    let Extents {
        pixels,
        min,
        max,
        viewport,
    } = extents;
    if ![pixels, min, max, viewport, size.width, size.height]
        .iter()
        .all(|value| value.is_finite())
        || max <= min
        || viewport <= 0.0
    {
        return None;
    }
    let axis = axis_direction.axis();
    let orientation = match knobs.orientation {
        Some(orientation) if orientation.axis() == axis => orientation,
        Some(orientation) => {
            debug_assert!(
                false,
                "a {orientation:?} scrollbar cannot sit on a {axis:?} scroll view"
            );
            ScrollbarOrientation::default_for(axis, text_direction)
        }
        None => ScrollbarOrientation::default_for(axis, text_direction),
    };
    let thickness = finite_non_negative(knobs.thickness);
    let main_margin = finite_non_negative(knobs.main_axis_margin);
    let cross_margin = finite_non_negative(knobs.cross_axis_margin);
    let (main_extent, cross_extent) = match axis {
        Axis::Vertical => (size.height, size.width),
        Axis::Horizontal => (size.width, size.height),
    };
    let track_length = main_extent - 2.0 * main_margin;
    if thickness <= 0.0 || track_length <= 0.0 || thickness + cross_margin > cross_extent {
        return None;
    }

    // How much of the content shows: the viewport less what overscrolls.
    let overscroll = (min - pixels).max(0.0) + (pixels - max).max(0.0);
    let inside = (viewport - overscroll).clamp(0.0, viewport);
    let content = max - min + viewport;
    let fraction_visible = (inside / content).clamp(0.0, 1.0);
    let before = (pixels - min).max(0.0);
    let after = (max - pixels).max(0.0);
    let safe_min = finite_non_negative(knobs.min_thumb_length).min(track_length);
    let min_length = if before > 0.0 && after > 0.0 {
        safe_min
    } else {
        // Past an end, the minimum shrinks to nothing by a fifth of a
        // viewport of overscroll.
        let overscrolled = (1.0 - inside / viewport).clamp(0.0, 0.2);
        safe_min * (1.0 - overscrolled / 0.2)
    };
    let thumb_length = (track_length * fraction_visible).clamp(min_length, track_length);
    let travel = track_length - thumb_length;
    let scrolled = ((pixels - min) / (max - min)).clamp(0.0, 1.0);
    let along = if axis_direction.is_reversed() {
        1.0 - scrolled
    } else {
        scrolled
    };
    let thumb_start = main_margin + travel * along;

    let cross_start = match orientation {
        ScrollbarOrientation::Left | ScrollbarOrientation::Top => cross_margin,
        ScrollbarOrientation::Right | ScrollbarOrientation::Bottom => {
            cross_extent - cross_margin - thickness
        }
    };
    let (track, thumb) = match axis {
        Axis::Vertical => (
            Rect::from_ltwh(cross_start, main_margin, thickness, track_length),
            Rect::from_ltwh(cross_start, thumb_start, thickness, thumb_length),
        ),
        Axis::Horizontal => (
            Rect::from_ltwh(main_margin, cross_start, track_length, thickness),
            Rect::from_ltwh(thumb_start, cross_start, thumb_length, thickness),
        ),
    };
    Some(BarGeometry { track, thumb })
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// What the state, the painter and the gestures share.
struct Bar {
    knobs: Cell<Knobs>,
    observer: Rc<ScrollObserver>,
    /// The scroll view the bar names, if it names one.
    followed: RefCell<Option<ObservedScroll>>,
    /// The scroll view the bar shows: the named one, else the observed one.
    shown: RefCell<Option<ObservedScroll>>,
    text_direction: Cell<TextDirection>,
    /// The size of the bar's box at the last paint.
    size: Cell<Size>,
    fade: AnimationController,
    /// Whether the fade runs on a frame clock.
    fade_bound: Cell<bool>,
    reduced_motion: Cell<bool>,
    /// Where the latest contact the bar took went down.
    pressed: Cell<Option<Landing>>,
    /// Whether a drag of the thumb is under way.
    dragging: Cell<bool>,
    /// The pixels the bar last saw, so a change of extents alone shows nothing.
    last_pixels: Cell<f64>,
    repaint: ChangeNotifier,
    position: RefCell<Option<Subscription>>,
    activity: RefCell<Option<Subscription>>,
}

impl fmt::Debug for Bar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bar")
            .field("shown", &*self.shown.borrow())
            .field("opacity", &self.fade.value())
            .finish_non_exhaustive()
    }
}

impl Bar {
    fn shown(&self) -> Option<ObservedScroll> {
        self.shown.borrow().clone()
    }

    fn geometry(&self) -> Option<BarGeometry> {
        let shown = self.shown()?;
        let position = shown.controller.position();
        bar_geometry(
            &self.knobs.get(),
            self.size.get(),
            Extents {
                pixels: position.pixels(),
                min: position.min_scroll_extent(),
                max: position.max_scroll_extent(),
                viewport: position.viewport_dimension(),
            },
            shown.axis_direction,
            self.text_direction.get(),
        )
    }

    fn opacity(&self) -> f64 {
        if self.knobs.get().thumb_visibility || self.dragging.get() {
            1.0
        } else {
            self.fade.value()
        }
    }

    /// Where a contact at `local` lands: the bar's box, the thumb or neither.
    fn takes(&self, local: Offset) -> Option<Landing> {
        if !self.knobs.get().interactive || self.opacity() <= 0.0 {
            return None;
        }
        let geometry = self.geometry()?;
        if geometry.thumb.contains_offset(local) {
            Some(Landing::Thumb)
        } else if geometry.track.contains_offset(local) {
            Some(Landing::Track)
        } else {
            None
        }
    }

    /// Follow the named scroll view, else the observed one, and subscribe
    /// to it when it changed.
    fn resolve(self: &Rc<Self>) {
        let next = self
            .followed
            .borrow()
            .clone()
            .or_else(|| self.observer.observed());
        let unchanged = match (&*self.shown.borrow(), &next) {
            (None, None) => true,
            (Some(current), Some(next)) => {
                current.axis_direction == next.axis_direction
                    && current
                        .controller
                        .position()
                        .ptr_eq(&next.controller.position())
            }
            _ => false,
        };
        if unchanged {
            return;
        }
        let (position, activity) = match &next {
            Some(scroll) => {
                let position = scroll.controller.position();
                let weak = Rc::downgrade(self);
                let moved = position.listenable().listen(Rc::new(move || {
                    if let Some(bar) = weak.upgrade() {
                        bar.moved();
                    }
                }));
                let weak = Rc::downgrade(self);
                let watched = position.clone();
                let activity = position.add_activity_listener(Rc::new(move || {
                    if let Some(bar) = weak.upgrade() {
                        bar.scrolling(watched.is_scrolling());
                    }
                }));
                (Some(moved), Some(activity))
            }
            None => (None, None),
        };
        if let Some(scroll) = &next {
            self.last_pixels.set(scroll.controller.position().pixels());
        }
        let replaced = (
            self.shown.replace(next),
            self.position.replace(position),
            self.activity.replace(activity),
        );
        // The old subscriptions and controller drop with nothing borrowed.
        drop(replaced);
        // A position already scrolling raises no activity change to hear:
        // show the bar for the run under way.
        if self
            .shown()
            .is_some_and(|shown| shown.controller.position().is_scrolling())
        {
            self.scrolling(true);
        }
        self.repaint.notify_listeners();
    }

    /// The position changed: repaint, and show the bar for a while when it
    /// moved outside a scroll activity, as a jump does.
    fn moved(&self) {
        let pixels = self
            .shown()
            .map(|shown| shown.controller.position().pixels());
        let jumped = pixels.is_some_and(|pixels| self.last_pixels.replace(pixels) != pixels);
        let quiet = !self.dragging.get()
            && self
                .shown()
                .is_some_and(|shown| !shown.controller.position().is_scrolling());
        if jumped && quiet && !self.knobs.get().thumb_visibility {
            if let Err(error) = self.fade.set_value(1.0) {
                tracing::error!(%error, "Scrollbar could not show its thumb");
            }
            self.disappear();
        }
        self.repaint.notify_listeners();
    }

    /// Show the bar while the position scrolls; fade it out once it stops.
    fn scrolling(&self, scrolling: bool) {
        if scrolling {
            self.appear();
        } else if !self.dragging.get() {
            self.disappear();
        }
    }

    fn appear(&self) {
        let knobs = self.knobs.get();
        let at_once =
            self.reduced_motion.get() || !self.fade_bound.get() || knobs.fade_duration.is_zero();
        let shown = if at_once {
            self.fade.set_value(1.0).map(drop)
        } else {
            self.fade
                .animate_to(1.0, Some(knobs.fade_duration))
                .map(drop)
        };
        if let Err(error) = shown {
            tracing::error!(%error, "Scrollbar could not show its thumb");
        }
    }

    fn disappear(&self) {
        let knobs = self.knobs.get();
        let hidden = if self.reduced_motion.get() || !self.fade_bound.get() {
            self.fade.set_value(0.0).map(drop)
        } else {
            let total = knobs.time_to_fade + knobs.fade_duration;
            if total.is_zero() {
                self.fade.set_value(0.0).map(drop)
            } else {
                // The thumb stays for `time_to_fade`, then fades.
                let hold = knobs.time_to_fade.as_secs_f64() / total.as_secs_f64();
                match Interval::try_new(hold, 1.0, curves::LINEAR) {
                    Ok(interval) => self
                        .fade
                        .animate_to_curved(0.0, Some(total), interval)
                        .map(drop),
                    Err(_) => self.fade.set_value(0.0).map(drop),
                }
            }
        };
        if let Err(error) = hidden {
            tracing::error!(%error, "Scrollbar could not hide its thumb");
        }
    }

    /// Move the position by `delta` along the track, in the bar's box.
    fn drag_by(&self, delta: f64) {
        let Some(shown) = self.shown() else { return };
        let Some(geometry) = self.geometry() else {
            return;
        };
        let position = shown.controller.position();
        let (min, max) = (position.min_scroll_extent(), position.max_scroll_extent());
        let (track, thumb) = match shown.axis_direction.axis() {
            Axis::Vertical => (geometry.track.height(), geometry.thumb.height()),
            Axis::Horizontal => (geometry.track.width(), geometry.thumb.width()),
        };
        let travel = track - thumb;
        if travel <= 0.0 || !delta.is_finite() {
            return;
        }
        let along = if shown.axis_direction.is_reversed() {
            -delta
        } else {
            delta
        };
        let target = (position.pixels() + along * (max - min) / travel).clamp(min, max);
        if target.is_finite() {
            // Taking the thumb takes the position from any running fling or
            // animation before it moves.
            let _ = shown.controller.jump_to(target);
        }
    }

    /// Move the position by a wheel or trackpad tick that reached the bar;
    /// the tick goes on outward when the position cannot move its way.
    fn wheel(&self, event: &ScrollEvent, preferences: &WheelPreferences) -> EventPropagation {
        let Some(shown) = self.shown() else {
            return EventPropagation::Continue;
        };
        // Through the scrollable's own wheel handling: the same distances
        // and the same nested chain as a tick over its content.
        if let Some(port) = shown.controller.wheel_port() {
            return port.claim(event);
        }
        // A controller no scrollable drives: move the position itself.
        let position = shown.controller.position();
        let mut delta = wheel_axis_delta(
            shown.axis_direction.axis(),
            event,
            position.viewport_dimension(),
            preferences,
            WheelScrollDistances::default(),
        );
        if shown.axis_direction.is_reversed() {
            delta = -delta;
        }
        let (min, max) = (position.min_scroll_extent(), position.max_scroll_extent());
        let target = (position.pixels() + delta).clamp(min, max);
        if delta == 0.0 || !target.is_finite() || target == position.pixels() {
            return EventPropagation::Continue;
        }
        let _ = shown.controller.jump_to(target);
        EventPropagation::Stop
    }

    /// Page toward `local`, a tap on the track off the thumb.
    fn page_toward(&self, local: Offset) {
        let Some(shown) = self.shown() else { return };
        let Some(geometry) = self.geometry() else {
            return;
        };
        let (at, thumb_start, thumb_end) = match shown.axis_direction.axis() {
            Axis::Vertical => (local.dy, geometry.thumb.top(), geometry.thumb.bottom()),
            Axis::Horizontal => (local.dx, geometry.thumb.left(), geometry.thumb.right()),
        };
        let toward_end = if at > thumb_end {
            true
        } else if at < thumb_start {
            false
        } else {
            return;
        };
        let forward = toward_end != shown.axis_direction.is_reversed();
        let position = shown.controller.position();
        let page = position.viewport_dimension() * TRACK_PAGE;
        let target = if forward {
            position.pixels() + page
        } else {
            position.pixels() - page
        }
        .clamp(position.min_scroll_extent(), position.max_scroll_extent());
        if target.is_finite() {
            let _ = shown.controller.animate_to(
                target,
                TRACK_PAGE_DURATION,
                flui_animation::SharedCurve::new(curves::EASE_IN_OUT),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landing {
    Thumb,
    Track,
}

// ---------------------------------------------------------------------------
// Painter
// ---------------------------------------------------------------------------

struct ScrollbarPainter {
    bar: Rc<Bar>,
    repaint: Rc<dyn Listenable>,
}

impl fmt::Debug for ScrollbarPainter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollbarPainter")
            .field("bar", &self.bar)
            .finish_non_exhaustive()
    }
}
impl CustomPainter for ScrollbarPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let bar = &self.bar;
        bar.size.set(size);
        let knobs = bar.knobs.get();
        if knobs.thumb_visibility {
            debug_assert!(
                bar.shown.borrow().is_some(),
                "a Scrollbar with thumb_visibility found no scroll view: put a Scrollable \
                 under it or name one with Scrollbar::controller"
            );
        }
        let opacity = bar.opacity();
        if opacity <= 0.0 {
            return;
        }
        let Some(geometry) = bar.geometry() else {
            return;
        };
        let radius = finite_non_negative(knobs.radius);
        let fill = |rect: Rect, color: Color| {
            let color = color.with_opacity(f64::from(color.opacity()) * opacity);
            (rect, Paint::fill(color))
        };
        if knobs.track_visibility {
            let (rect, paint) = fill(geometry.track, knobs.track_color);
            draw(canvas, rect, radius, &paint);
        }
        let (rect, paint) = fill(geometry.thumb, knobs.thumb_color);
        draw(canvas, rect, radius, &paint);
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        old.as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| !Rc::ptr_eq(&old.bar, &self.bar))
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(Rc::clone(&self.repaint))
    }

    fn hit_test(&self, position: Offset) -> Option<bool> {
        Some(self.bar.takes(position).is_some())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn draw(canvas: &mut Canvas, rect: Rect, radius: f64, paint: &Paint) {
    if radius > 0.0 {
        canvas.draw_rrect(
            flui_foundation::geometry::RRect::from_rect_circular(rect, radius),
            paint,
        );
    } else {
        canvas.draw_rect(rect, paint);
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Persistent state for [`Scrollbar`]: the observer its scroll view
/// attaches to, the fade, and what the painter reads.
pub struct ScrollbarState {
    bar: Rc<Bar>,
    fade: DrivenController,
    vsync: Option<Vsync>,
    observed: Option<Subscription>,
    fade_listener: Option<Subscription>,
}

impl fmt::Debug for ScrollbarState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollbarState")
            .field("bar", &self.bar)
            .finish_non_exhaustive()
    }
}

impl StatefulView for Scrollbar {
    type State = ScrollbarState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        let fade = AnimationController::builder(self.knobs.fade_duration)
            .debug_label("Scrollbar.fade")
            .build_on(None);
        let bar = Rc::new(Bar {
            knobs: Cell::new(self.knobs),
            observer: Rc::new(ScrollObserver::default()),
            followed: RefCell::new(self.followed.clone()),
            shown: RefCell::new(None),
            text_direction: Cell::new(TextDirection::Ltr),
            size: Cell::new(Size::ZERO),
            fade: fade.controller().clone(),
            fade_bound: Cell::new(false),
            reduced_motion: Cell::new(false),
            pressed: Cell::new(None),
            dragging: Cell::new(false),
            last_pixels: Cell::new(0.0),
            repaint: ChangeNotifier::new(),
            position: RefCell::new(None),
            activity: RefCell::new(None),
        });
        ScrollbarState {
            bar,
            fade,
            vsync: None,
            observed: None,
            fade_listener: None,
        }
    }
}

impl ScrollbarState {
    fn bind_vsync(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = VsyncScope::maybe_of(ctx);
        if VsyncScope::same_registry(self.vsync.as_ref(), incoming.as_ref()) {
            return;
        }
        self.vsync = incoming;
        // A fade on the old clock ends where it is.
        let _ = self.fade.controller().stop();
        let rebound = self.fade.rebind(self.vsync.as_ref());
        self.bar.fade_bound.set(self.fade.is_bound());
        if let Err(error) = rebound {
            tracing::error!(%error, "Scrollbar lost its frame registry");
        }
    }
}

impl ViewState<Scrollbar> for ScrollbarState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let weak = Rc::downgrade(&self.bar);
        self.observed = Some(self.bar.observer.listen(Rc::new(move || {
            if let Some(bar) = weak.upgrade() {
                bar.resolve();
            }
        })));
        let weak = Rc::downgrade(&self.bar);
        self.fade_listener = Some(self.fade.controller().listen(Rc::new(move || {
            if let Some(bar) = weak.upgrade() {
                bar.repaint.notify_listeners();
            }
        })));
        self.did_change_dependencies(ctx);
        self.bar.resolve();
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.bar
            .text_direction
            .set(Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr));
        self.bar
            .reduced_motion
            .set(MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce));
        self.bind_vsync(ctx);
    }

    fn build(&self, view: &Scrollbar, ctx: &dyn BuildContext) -> impl IntoView {
        let painter = ScrollbarPainter {
            bar: Rc::clone(&self.bar),
            repaint: Rc::new(self.bar.repaint.clone()),
        };
        let admitted = Rc::clone(&self.bar);
        let tapped = Rc::clone(&self.bar);
        let (started, moved, ended) = (
            Rc::clone(&self.bar),
            Rc::clone(&self.bar),
            Rc::clone(&self.bar),
        );
        let wheeled = Rc::clone(&self.bar);
        let pressed = Rc::clone(&self.bar);
        let wheel_preferences = GestureArenaScope::wheel_preferences_of(ctx);
        // A pan, so a bar whose scroll view attaches after this build drags
        // along whichever axis it follows.
        let slot = DragSlot::pan()
            // The bar takes the mouse whatever its scroll view admits.
            .allowed_devices(PointerKindSet::all())
            // The contact the drag follows: a second finger that lands
            // elsewhere does not change where this one went down.
            .on_down(move |_ctx, details| {
                pressed.pressed.set(pressed.takes(details.local_position));
            })
            .on_start(move |_ctx, _details| {
                if started.pressed.get() == Some(Landing::Thumb) {
                    started.dragging.set(true);
                    started.appear();
                }
            })
            .on_update(move |_ctx, details| {
                if moved.dragging.get()
                    && let Some(shown) = moved.shown()
                {
                    moved.drag_by(match shown.axis_direction.axis() {
                        Axis::Vertical => details.delta.dy,
                        Axis::Horizontal => details.delta.dx,
                    });
                }
            })
            .on_end(move |_ctx, _details| {
                if ended.dragging.replace(false) {
                    let scrolling = ended
                        .shown()
                        .is_some_and(|shown| shown.controller.position().is_scrolling());
                    ended.scrolling(scrolling);
                }
            });
        let detector =
            GestureDetector::new()
                .admission(ContactAdmission::new(move |dispatch| {
                    match dispatch.local {
                        PointerEvent::Down(_) => dispatch
                            .local
                            .position()
                            .is_some_and(|local| admitted.takes(local).is_some()),
                        _ => true,
                    }
                }))
                .behavior(HitTestBehavior::DeferToChild)
                .drag(slot)
                .on_tap_up(move |_ctx, details| tapped.page_toward(details.local_position))
                .child(CustomPaint::new().foreground_painter(painter).child(
                    ScrollObserverScope::new(
                        ctx,
                        Rc::clone(&self.bar.observer),
                        // A bar with a controller of its own shows that one,
                        // not the scroll view below it.
                        view.followed.is_none(),
                        view.child.clone(),
                    ),
                ));
        // Over the bar the scroll view is off the hit path, so the bar moves
        // it by the wheel; over the content the scroll view claims first.
        Listener::new()
            .on_scroll_claim(move |event| wheeled.wheel(event, &wheel_preferences.snapshot()))
            .child(detector)
    }

    fn did_update_view(&mut self, _old_view: &Scrollbar, new_view: &Scrollbar) {
        self.bar.knobs.set(new_view.knobs);
        let outgoing = self.bar.followed.replace(new_view.followed.clone());
        drop(outgoing);
        self.bar.resolve();
        self.bar.repaint.notify_listeners();
    }

    fn dispose(&mut self) {
        drop(self.observed.take());
        drop(self.fade_listener.take());
        let subscriptions = (
            self.bar.position.take(),
            self.bar.activity.take(),
            self.bar.shown.take(),
        );
        drop(subscriptions);
        self.fade.dispose();
    }
}
