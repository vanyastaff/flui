//! [`ScrollViewConfig`] and [`ScrollView`]: the scroll configuration every
//! scrolling widget takes, said once.
//!
//! [`ListView`](crate::ListView), [`GridView`](crate::GridView),
//! [`CustomScrollView`](crate::CustomScrollView),
//! [`SingleChildScrollView`](crate::SingleChildScrollView), the animated lists
//! and [`Scrollable`](crate::Scrollable) itself each hold one
//! `ScrollViewConfig` and implement [`ScrollView`], whose provided methods are
//! the builders. A scroll widget a package writes does the same and gets the
//! whole vocabulary, and a new setting is one more provided method.

use flui_foundation::geometry::Axis;
use flui_interaction::PointerKindSet;

use crate::scroll::nested::OverscrollBehavior;
use crate::scroll::scroll_owner::SharedSnapPolicy;
use crate::scroll::{ScrollController, SharedScrollPhysics, WheelScrollDistances};

/// How a scroll view scrolls: its controller, axis, direction and the policy
/// of its gestures. Built through the [`ScrollView`] methods of the widget
/// that holds it.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ScrollViewConfig {
    /// The shared position and its commands, when the application holds it;
    /// `None` uses one the scrollable keeps for as long as it is mounted, so
    /// a rebuild of a parent never swaps the position under a drag.
    pub(crate) controller: Option<ScrollController>,
    /// The axis along which the content scrolls.
    pub(crate) scroll_direction: Axis,
    /// Whether offset `0.0` sits at the trailing edge of the axis.
    pub(crate) reverse: bool,
    /// An authored boundary and fling policy; `None` uses the owner's default.
    pub(crate) physics: Option<SharedScrollPhysics>,
    /// Where a release comes to rest, when not where the physics coasts.
    pub(crate) snap: Option<SharedSnapPolicy>,
    /// Whether this scrollable's remainder goes up its scroll chain.
    pub(crate) overscroll: OverscrollBehavior,
    /// The pointer kinds whose drags and presses the scrollable admits.
    pub(crate) devices: PointerKindSet,
    /// The logical distances wheel lines and characters resolve to.
    pub(crate) wheel_distances: WheelScrollDistances,
    /// What assistive technology reads for the scroll view.
    pub(crate) semantic_label: Option<String>,
    /// Take the keyboard focus when mounted.
    pub(crate) autofocus: bool,
}

impl Default for ScrollViewConfig {
    fn default() -> Self {
        Self {
            controller: None,
            scroll_direction: Axis::Vertical,
            reverse: false,
            physics: None,
            snap: None,
            overscroll: OverscrollBehavior::Auto,
            devices: PointerKindSet::all(),
            wheel_distances: WheelScrollDistances::default(),
            semantic_label: None,
            autofocus: false,
        }
    }
}

/// A widget that scrolls its content: the builders of its
/// [`ScrollViewConfig`].
///
/// Implementors provide [`ScrollView::scroll_config_mut`]; every other method
/// is provided.
pub trait ScrollView: Sized {
    /// The configuration the provided builders write.
    fn scroll_config_mut(&mut self) -> &mut ScrollViewConfig;

    /// Share the position with a [`ScrollController`] the application holds,
    /// to read the offset, jump or animate it, or drive a
    /// [`Scrollbar`](crate::Scrollbar) from it. Clones of one controller
    /// share one position. Without one, the scrollable keeps a position of
    /// its own for as long as it is mounted.
    #[must_use]
    fn controller(mut self, controller: ScrollController) -> Self {
        self.scroll_config_mut().controller = Some(controller);
        self
    }

    /// The scroll axis (default [`Axis::Vertical`]).
    #[must_use]
    fn scroll_direction(mut self, axis: Axis) -> Self {
        self.scroll_config_mut().scroll_direction = axis;
        self
    }

    /// Anchor offset `0.0` at the trailing edge of the axis (the bottom of a
    /// vertical view, the reading end of a horizontal one), so the content
    /// grows from there. Default `false`.
    #[must_use]
    fn reverse(mut self, reverse: bool) -> Self {
        self.scroll_config_mut().reverse = reverse;
        self
    }

    /// Override the boundary and fling behaviour (default:
    /// [`ClampingScrollPhysics`](crate::ClampingScrollPhysics)).
    #[must_use]
    fn physics(mut self, physics: SharedScrollPhysics) -> Self {
        self.scroll_config_mut().physics = Some(physics);
        self
    }

    /// Settle every release on the target `policy` picks — a page, an
    /// anchor — instead of where the physics would coast (ADR-0209 §9).
    /// A snapping scrollable keeps its fling: no residual goes up its chain.
    #[must_use]
    fn snap_policy(mut self, policy: SharedSnapPolicy) -> Self {
        self.scroll_config_mut().snap = Some(policy);
        self
    }

    /// Whether drag remainders and fling residuals this scrollable and the
    /// scrollables inside it leave go on up the nested scroll chain
    /// (default [`OverscrollBehavior::Auto`]) or stop here
    /// ([`OverscrollBehavior::Contain`]).
    #[must_use]
    fn overscroll_behavior(mut self, behavior: OverscrollBehavior) -> Self {
        self.scroll_config_mut().overscroll = behavior;
        self
    }

    /// Admit only drags and presses of these pointer kinds (default: all). A
    /// refused contact never reaches the scrollable's arena, so a pen that
    /// draws beside a finger that scrolls leaves the scrollable alone. Wheel
    /// and trackpad scrolling, keys, assistive actions and the controller are
    /// not contacts and are unaffected.
    #[must_use]
    fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.scroll_config_mut().devices = devices;
        self
    }

    /// Author the logical distance of one translated line or raw wheel
    /// character.
    ///
    /// These distances are application scroll policy, not measurements of
    /// glyphs or native typography. Raw detents additionally use system
    /// counts; translated line packets use only the line distance and pixel
    /// packets remain unchanged.
    #[must_use]
    fn wheel_distances(mut self, distances: WheelScrollDistances) -> Self {
        self.scroll_config_mut().wheel_distances = distances;
        self
    }

    /// What assistive technology reads for the scroll view, in place of
    /// nothing.
    #[must_use]
    fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.scroll_config_mut().semantic_label = Some(label.into());
        self
    }

    /// Take the keyboard focus when mounted, so the scroll keys work without
    /// a click first (default `false`).
    ///
    /// A focused scrollable, or one holding the focus in its subtree, scrolls
    /// by the arrow keys along its axis, PageUp/PageDown and Space/Shift+Space
    /// by a page, and Home/End to its extents. Each key press is a sequence
    /// from the keyboard device in the nested scroll chain; a scrollable at
    /// the end of its range lets the key bubble to the enclosing one. It is
    /// not a Tab stop, and a drag on it takes the focus.
    #[must_use]
    fn autofocus(mut self, autofocus: bool) -> Self {
        self.scroll_config_mut().autofocus = autofocus;
        self
    }
}
