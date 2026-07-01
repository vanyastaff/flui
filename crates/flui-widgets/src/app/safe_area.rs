//! [`SafeArea`] — pads its child to avoid OS-reserved screen intrusions.

use std::fmt;

use flui_geometry::EdgeInsets;
use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::app::MediaQuery;
use crate::layout::Padding;

/// Insets its child with sufficient padding to avoid operating-system
/// intrusions (notch, status bar, home indicator) reported by the nearest
/// ancestor [`MediaQuery`].
///
/// For each edge the effective inset is
/// `max(toggle ? media_padding.side : 0, minimum.side)`. When an edge's toggle
/// is `true` (the default) the OS-reported padding for that edge is honoured;
/// setting it to `false` leaves that edge unpadded (useful when a background
/// image bleeds to the edge or the child handles the intrusion itself).
///
/// The `minimum` [`EdgeInsets`] is applied even when a toggle is `false`,
/// so it acts as a guaranteed floor independent of the media data.
///
/// Flutter parity: `widgets/safe_area.dart` `SafeArea`.
///
/// **Divergence:** Flutter's `SafeArea` also wraps its child in
/// `MediaQuery.removePadding` to zero-out consumed edges in the subtree's
/// ambient [`MediaQueryData`](crate::MediaQueryData). FLUI defers
/// `MediaQuery.removePadding` (not yet implemented); nested `SafeArea`s
/// therefore over-pad. This divergence is documented and will be resolved when
/// `MediaQuery.removePadding` lands.
///
/// # Panics
///
/// Panics in `build` if there is no [`MediaQuery`] ancestor. Place a
/// `MediaQuery` near the root (e.g. from `flui_app::AppBinding`).
// Four independent per-edge toggle bools mirror Flutter's `SafeArea` API
// (left/top/right/bottom as separate constructor params). There is no semantic
// grouping that warrants a state machine or enum — each edge is truly
// independent. Suppress the lint rather than invent an artificial abstraction.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, StatelessView)]
pub struct SafeArea {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
    minimum: EdgeInsets,
    child: BoxedView,
}

impl SafeArea {
    /// A `SafeArea` with all four edge-toggles on and no minimum insets.
    ///
    /// Set the child with [`.child()`](Self::child); the default placeholder
    /// is a zero-size [`SizedBox::shrink`](crate::SizedBox).
    pub fn new() -> Self {
        Self {
            left: true,
            top: true,
            right: true,
            bottom: true,
            minimum: EdgeInsets::ZERO,
            child: crate::layout::SizedBox::shrink().boxed(),
        }
    }

    /// Whether to honour the left-edge OS padding (default `true`).
    #[must_use]
    pub fn left(mut self, left: bool) -> Self {
        self.left = left;
        self
    }

    /// Whether to honour the top-edge OS padding (default `true`).
    #[must_use]
    pub fn top(mut self, top: bool) -> Self {
        self.top = top;
        self
    }

    /// Whether to honour the right-edge OS padding (default `true`).
    #[must_use]
    pub fn right(mut self, right: bool) -> Self {
        self.right = right;
        self
    }

    /// Whether to honour the bottom-edge OS padding (default `true`).
    #[must_use]
    pub fn bottom(mut self, bottom: bool) -> Self {
        self.bottom = bottom;
        self
    }

    /// The minimum insets to apply on every edge regardless of the toggles.
    ///
    /// Defaults to [`EdgeInsets::ZERO`].
    #[must_use]
    pub fn minimum(mut self, minimum: EdgeInsets) -> Self {
        self.minimum = minimum;
        self
    }

    /// Set the widget laid out inside the safe-area insets.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = child.into_view().boxed();
        self
    }
}

impl Default for SafeArea {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SafeArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SafeArea")
            .field("left", &self.left)
            .field("top", &self.top)
            .field("right", &self.right)
            .field("bottom", &self.bottom)
            .field("minimum", &self.minimum)
            .finish_non_exhaustive()
    }
}

impl StatelessView for SafeArea {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // Flutter oracle: `safe_area.dart` lines 121-135.
        // Effective inset per edge: max(toggle ? media_side : 0, minimum_side).
        let media_padding = MediaQuery::of(ctx).padding;

        let effective_left = if self.left {
            media_padding.left.max(self.minimum.left)
        } else {
            self.minimum.left
        };
        let effective_top = if self.top {
            media_padding.top.max(self.minimum.top)
        } else {
            self.minimum.top
        };
        let effective_right = if self.right {
            media_padding.right.max(self.minimum.right)
        } else {
            self.minimum.right
        };
        let effective_bottom = if self.bottom {
            media_padding.bottom.max(self.minimum.bottom)
        } else {
            self.minimum.bottom
        };

        // `EdgeInsets::new(top, right, bottom, left)` — field order matches
        // `Edges::new` in `flui-geometry`.
        let insets = EdgeInsets::new(
            effective_top,
            effective_right,
            effective_bottom,
            effective_left,
        );
        Padding::new(insets).child(self.child.clone())
    }
}
