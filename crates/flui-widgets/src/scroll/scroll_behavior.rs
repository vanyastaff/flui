//! [`ScrollBehavior`] and [`ScrollConfiguration`]: how the scroll views of a
//! subtree scroll where their own [`ScrollViewConfig`] says nothing.
//!
//! A scroll view resolves each setting it leaves open by asking the
//! behavior of the nearest [`ScrollConfiguration`] above it, with the
//! [`PlatformConvention`] it is built under and its axis. Without a
//! configuration it asks [`DefaultScrollBehavior`].
//!
//! [`ScrollViewConfig`]: crate::ScrollViewConfig
//! [`PlatformConvention`]: flui_view::environment::PlatformConvention

use std::rc::Rc;

use flui_foundation::geometry::Axis;
use flui_interaction::PointerKindSet;
use flui_platform::TargetPlatform;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, View};

use flui_rendering::constraints::AxisDirection;

use crate::scroll::{
    BouncingScrollPhysics, ClampingScrollPhysics, ScrollController, SharedScrollPhysics,
    WheelScrollDistances,
};

/// What a scroll view asks its [`ScrollBehavior`] about: the platform
/// convention it is built under and the axis it scrolls along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScrollBehaviorDetails {
    platform: TargetPlatform,
    axis: Axis,
}

impl ScrollBehaviorDetails {
    /// The details of a scroll view along `axis` built under `platform`.
    #[must_use]
    pub const fn new(platform: TargetPlatform, axis: Axis) -> Self {
        Self { platform, axis }
    }

    /// The platform whose conventions the scroll view follows.
    pub const fn platform(&self) -> TargetPlatform {
        self.platform
    }

    /// The axis the scroll view scrolls along.
    #[must_use]
    pub const fn axis(&self) -> Axis {
        self.axis
    }
}

/// The scroll view a [`ScrollBehavior`] decorates in
/// [`build_scrollbar`](ScrollBehavior::build_scrollbar): the platform
/// convention it is built under, the direction its offset grows in and the
/// controller that moves it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScrollableDetails {
    platform: TargetPlatform,
    axis_direction: AxisDirection,
    controller: ScrollController,
    wheel_distances: WheelScrollDistances,
}

impl ScrollableDetails {
    /// A scroll view moved by `controller` whose offset grows toward
    /// `axis_direction`, built under `platform`.
    #[must_use]
    pub fn new(
        platform: TargetPlatform,
        axis_direction: AxisDirection,
        controller: ScrollController,
    ) -> Self {
        Self {
            platform,
            axis_direction,
            controller,
            wheel_distances: WheelScrollDistances::default(),
        }
    }

    /// The platform whose conventions the scroll view follows.
    pub const fn platform(&self) -> TargetPlatform {
        self.platform
    }

    /// The direction the scroll view's offset grows in.
    #[must_use]
    pub const fn axis_direction(&self) -> AxisDirection {
        self.axis_direction
    }

    /// The controller that moves the scroll view.
    #[must_use]
    pub const fn controller(&self) -> &ScrollController {
        &self.controller
    }

    /// How far a line or detent of the wheel moves the scroll view.
    #[must_use]
    pub const fn wheel_distances(&self) -> WheelScrollDistances {
        self.wheel_distances
    }

    /// The same scroll view, moved by a line or detent of the wheel as
    /// `distances` say.
    #[must_use]
    pub(crate) const fn with_wheel_distances(mut self, distances: WheelScrollDistances) -> Self {
        self.wheel_distances = distances;
        self
    }
}

/// How the scroll views under a [`ScrollConfiguration`] scroll where their
/// own configuration says nothing.
///
/// Every method is provided; an implementation overrides the ones it
/// changes. A scroll view asks again whenever it rebuilds under another
/// behavior, platform or axis, and keeps the answer otherwise.
pub trait ScrollBehavior: std::fmt::Debug {
    /// The boundary and fling behaviour of a scroll view that names no
    /// physics: [`BouncingScrollPhysics`] on macOS and iOS,
    /// [`ClampingScrollPhysics`] elsewhere.
    fn physics(&self, details: &ScrollBehaviorDetails) -> SharedScrollPhysics {
        match details.platform() {
            TargetPlatform::MacOS | TargetPlatform::iOS => Rc::new(BouncingScrollPhysics::new()),
            _ => Rc::new(ClampingScrollPhysics::new()),
        }
    }

    /// The pointer kinds whose drags and presses a scroll view that names no
    /// devices admits: every kind but the mouse, which scrolls by its wheel
    /// and the scrollbar and leaves a drag to selection.
    fn drag_devices(&self, _details: &ScrollBehaviorDetails) -> PointerKindSet {
        PointerKindSet::TOUCH
            | PointerKindSet::PEN
            | PointerKindSet::TRACKPAD
            | PointerKindSet::VIRTUAL
    }

    /// What a scroll view shows over its content: `child` as it is. A
    /// behavior for a desktop convention wraps it in a
    /// [`Scrollbar`](crate::Scrollbar) attached to
    /// [`details`](ScrollableDetails::controller); `ctx` is the scroll
    /// view's own context.
    fn build_scrollbar(
        &self,
        _ctx: &dyn BuildContext,
        child: BoxedView,
        _details: &ScrollableDetails,
    ) -> BoxedView {
        child
    }
}

/// The behavior of a tree with no [`ScrollConfiguration`]: every provided
/// [`ScrollBehavior`] method as written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DefaultScrollBehavior;

impl ScrollBehavior for DefaultScrollBehavior {}

/// Sets the [`ScrollBehavior`] of the scroll views below it.
///
/// Dependents are told of a change only when the behavior is another `Rc`:
/// create it once (in a state, or a field the parent keeps) and pass clones
/// of it. A behavior allocated in `build` is a new one each time, and every
/// scroll view below asks it again.
#[derive(Clone)]
pub struct ScrollConfiguration {
    behavior: Rc<dyn ScrollBehavior>,
    child: BoxedView,
}

impl ScrollConfiguration {
    /// `behavior` for the scroll views in `child`.
    pub fn new(behavior: Rc<dyn ScrollBehavior>, child: impl IntoView) -> Self {
        Self {
            behavior,
            child: BoxedView::new(child.into_view()),
        }
    }

    /// The behavior of the nearest configuration above `ctx`, registering a
    /// dependency on it; `None` where there is none, which
    /// [`DefaultScrollBehavior`] answers for.
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Rc<dyn ScrollBehavior>> {
        ctx.depend_on::<Self, _>(|scope| Rc::clone(&scope.behavior))
    }
}

impl std::fmt::Debug for ScrollConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollConfiguration")
            .field("behavior", &self.behavior)
            .finish_non_exhaustive()
    }
}

impl InheritedView for ScrollConfiguration {
    type Data = Rc<dyn ScrollBehavior>;

    fn data(&self) -> &Self::Data {
        &self.behavior
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.behavior, &old.behavior)
    }
}

impl_inherited_view!(ScrollConfiguration);
