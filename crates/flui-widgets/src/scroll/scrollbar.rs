//! [`Scrollbar`] — visual scrollbar track + thumb overlay.
//!
//! Wraps its child in a [`Stack`] and overlays a proportional thumb on the
//! trailing edge. The thumb geometry (`top`, `height`) is derived from the
//! [`ScrollController`]'s `thumb_fraction()` and `thumb_offset_fraction()`
//! helpers, which in turn depend on `viewport_dimension_pixels`,
//! `min_scroll_extent`, and `max_scroll_extent` — values the enclosing
//! `Scrollable` (or the app) sets via
//! [`ScrollController::update_dimensions`].
//!
//! Rebuilds are driven by [`AnimatedBuilder`] subscribed to the controller's
//! [`Listenable`](flui_foundation::Listenable), so only this inner subtree
//! rebuilds on position changes.
//!
//! # Deferred (v1)
//!
//! - Fade-in / fade-out animation when scrolling starts / stops.
//! - Horizontal scrollbar orientation.
//! - `ScrollbarTheme` look customization beyond `thumb_color`/`thumb_width`.
//!
//! # Flutter parity
//!
//! Corresponds to `widgets/scrollbar.dart` `Scrollbar`. FLUI's v1 version is
//! purely additive: it paints on top of the child without clipping or resizing
//! it. The thumb width defaults to 6 px (mobile), matching Flutter's
//! `ScrollbarThemeData.thickness`.

use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::Color;
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, Child, IntoView, ViewExt};

use crate::scroll::ScrollController;
use crate::{AnimatedBuilder, ColoredBox, GestureDetector, Positioned, Stack};

/// Minimum thumb extent in logical pixels — matches Flutter's
/// `ScrollbarPainter.minLength` default.
const MIN_THUMB_PX: f32 = 18.0;

/// Default thumb width in logical pixels.
const DEFAULT_THUMB_WIDTH_PX: f32 = 6.0;

/// Overlays a proportional scrollbar thumb on the trailing edge of its child.
///
/// The thumb height and vertical position are computed from the
/// [`ScrollController`]: pair a `Scrollbar` with the same controller that
/// drives the `Scrollable`(super::Scrollable) it sits alongside.
///
/// # Example
///
/// ```rust,ignore
/// let controller = ScrollController::new();
/// controller.update_dimensions(400.0, 0.0, 800.0);
///
/// Scrollbar::new()
///     .controller(controller.clone())
///     .child(
///         Scrollable::new()
///             .controller(controller)
///             .child(MyContent::new()),
///     )
/// ```
#[derive(Clone, StatelessView)]
pub struct Scrollbar {
    /// The position + notification source.
    controller: ScrollController,
    /// The colour of the thumb rectangle.
    thumb_color: Color,
    /// The width of the thumb in logical pixels.
    thumb_width: f32,
    /// The content to overlay the scrollbar onto.
    child: Child,
}

impl std::fmt::Debug for Scrollbar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scrollbar")
            .field("thumb_color", &self.thumb_color)
            .field("thumb_width", &self.thumb_width)
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

impl Default for Scrollbar {
    fn default() -> Self {
        Self {
            controller: ScrollController::new(),
            // Semi-transparent black — matches Flutter's CupertinoScrollbar default.
            thumb_color: Color::rgba(0, 0, 0, 128),
            thumb_width: DEFAULT_THUMB_WIDTH_PX,
            child: Child::empty(),
        }
    }
}

impl Scrollbar {
    /// A new `Scrollbar` with a fresh internal controller and default styling.
    /// Call `.controller(...)` to share the position with a `Scrollable`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the [`ScrollController`] whose position this scrollbar reflects.
    #[must_use]
    pub fn controller(mut self, controller: ScrollController) -> Self {
        self.controller = controller;
        self
    }

    /// Override the thumb colour (default: semi-transparent black).
    #[must_use]
    pub fn thumb_color(mut self, color: Color) -> Self {
        self.thumb_color = color;
        self
    }

    /// Override the thumb width in logical pixels (default: 6.0 px).
    #[must_use]
    pub fn thumb_width(mut self, width: f32) -> Self {
        self.thumb_width = width;
        self
    }

    /// The content to overlay the scrollbar onto.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl StatelessView for Scrollbar {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let thumb_color = self.thumb_color;
        let thumb_width = self.thumb_width;
        let child = self.child.clone();

        AnimatedBuilder::new(self.controller.as_listenable(), move || {
            let viewport_dim = controller.viewport_dimension_pixels();
            let fraction = controller.thumb_fraction();
            let offset_fraction = controller.thumb_offset_fraction();

            // Only render the thumb when layout dimensions are known and the
            // content is actually larger than the viewport.
            let show_thumb = viewport_dim > 0.0 && fraction < 1.0;
            let thumb_height = (viewport_dim * fraction).max(MIN_THUMB_PX);
            // Clamp thumb top so thumb never overflows the track.
            let available_track = (viewport_dim - thumb_height).max(0.0);
            let thumb_top = available_track * offset_fraction;

            // Stack children: content first (non-positioned, determines size),
            // then the thumb on top (positioned).
            let mut stack_children = Vec::new();
            if let Some(content) = child.clone().into_inner() {
                stack_children.push(content);
            }

            if show_thumb {
                let ctrl_drag = controller.clone();

                // Wrap the thumb in a GestureDetector so the user can drag it
                // to reposition the scroll. The delta in track-space maps to
                // content-space via:
                //   dP/d(thumb_top) = (viewport + scroll_extent) / available_track
                // (derived from the thumb_offset_fraction formula).
                let thumb_gesture = GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_pan_update(move |details| {
                        let delta_track_px = details.delta.dy.get();
                        if available_track > 0.0 {
                            let total_content_extent =
                                ctrl_drag.viewport_dimension_pixels() + ctrl_drag.scroll_extent();
                            let content_delta =
                                (delta_track_px / available_track) * total_content_extent;
                            let proposed = ctrl_drag.pixels() + content_delta;
                            ctrl_drag.set_pixels(proposed.clamp(
                                ctrl_drag.min_scroll_extent(),
                                ctrl_drag.max_scroll_extent(),
                            ));
                        }
                    })
                    .child(ColoredBox::new(thumb_color));

                let positioned_thumb = Positioned::new(thumb_gesture)
                    .top(thumb_top)
                    .right(0.0)
                    .width(thumb_width)
                    .height(thumb_height);
                // `ViewExt::boxed` converts the concrete `Positioned` to a
                // `BoxedView` so it can be pushed into `Vec<BoxedView>`.
                stack_children.push(positioned_thumb.boxed());
            }

            Stack::new(stack_children)
        })
    }
}
