//! Viewport offset for scroll position tracking.

use std::{
    cell::RefCell,
    fmt::Debug,
    rc::{Rc, Weak},
};

use flui_foundation::{ChangeNotifier, Subscription};

/// The direction of a scroll, relative to the positive scroll offset axis.
///
/// This indicates the direction that the user is scrolling, not the direction
/// that the scroll offset is changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a scroll is idle or moves one of two ways along its axis"
)]
pub enum ScrollDirection {
    /// No scrolling is underway.
    #[default]
    Idle,

    /// Scrolling is happening in the negative scroll offset direction.
    ///
    /// For a vertical list with `AxisDirection::down`, this means the content
    /// is moving down, exposing earlier content.
    Forward,

    /// Scrolling is happening in the positive scroll offset direction.
    ///
    /// For a vertical list with `AxisDirection::down`, this means the content
    /// is moving up, exposing later content.
    Reverse,
}

impl ScrollDirection {
    /// Returns the opposite scroll direction.
    pub fn flip(self) -> Self {
        match self {
            Self::Idle => Self::Idle,
            Self::Forward => Self::Reverse,
            Self::Reverse => Self::Forward,
        }
    }
}

/// Which part of the content inside the viewport should be visible.
///
/// The `pixels` value determines the scroll offset that the viewport uses to
/// select which part of its content to display. As the user scrolls the
/// viewport, this value changes, which changes the content that is displayed.
///
/// This trait is a `ChangeNotifier`-like that notifies its listeners when
/// `pixels` changes.
///
/// It is the viewport's layout protocol, not a way to scroll: it reads the
/// offset, reports extents and applies layout-time corrections. Moving a
/// shared offset is the business of its one
/// [`ScrollOffsetWriter`](super::ScrollOffsetWriter).
pub trait ViewportOffset: Debug {
    /// Detached layout state; it contains no listeners or delivery obligations.
    type Layout: ViewportLayout;

    /// Starts a layout proposal from the currently accepted position.
    fn begin_layout(&self) -> Self::Layout;

    /// Publishes a successful proposal, refusing it if accepted input changed
    /// while descendants were being measured.
    fn accept_layout(&mut self, layout: Self::Layout) -> crate::RenderResult<()>;

    /// The number of pixels to offset the children in the opposite of the axis
    /// direction.
    ///
    /// For example, if the axis direction is down, then the pixel value
    /// represents the number of logical pixels to move the children _up_ the
    /// screen.
    fn pixels(&self) -> f64;

    /// Whether the `pixels` property is available.
    fn has_pixels(&self) -> bool;

    /// Called when the viewport's extents are established.
    ///
    /// The argument is the dimension of the viewport in the main axis
    /// (e.g., the height for a vertical viewport).
    ///
    /// If applying the viewport dimension changes the scroll offset, return
    /// `false`. Otherwise, return `true`.
    fn apply_viewport_dimension(&mut self, viewport_dimension: f64) -> bool;

    /// Called when the viewport's content extents are established.
    ///
    /// The arguments are the minimum and maximum scroll extents respectively.
    ///
    /// If applying the content dimensions changes the scroll offset, return
    /// `false`. Otherwise, return `true`.
    fn apply_content_dimensions(&mut self, min_scroll_extent: f64, max_scroll_extent: f64) -> bool;

    /// Apply a layout-time correction to the scroll offset.
    ///
    /// This method should change the `pixels` value by `correction`, but
    /// without calling the notification callbacks.
    fn correct_by(&mut self, correction: f64);

    /// The direction in which the user is trying to change `pixels`.
    fn user_scroll_direction(&self) -> ScrollDirection;

    /// Adds a listener that will be called when `pixels` changes.
    fn add_listener(&self, listener: Rc<dyn Fn()>);

    /// Removes a listener.
    fn remove_listener(&self, listener: &Rc<dyn Fn()>);
}

/// Scroll corrections and dimensions under consideration by a viewport.
///
/// A proposal is local to one layout walk. These methods neither publish metrics
/// nor invoke listeners; dropping a rejected proposal leaves accepted state alone.
pub trait ViewportLayout: Debug {
    /// Proposed scroll position.
    fn pixels(&self) -> f64;
    /// Applies a proposed viewport extent, returning whether another pass is unnecessary.
    fn apply_viewport_dimension(&mut self, dimension: f64) -> bool;
    /// Applies proposed content bounds, returning whether another pass is unnecessary.
    fn apply_content_dimensions(&mut self, min: f64, max: f64) -> bool;
    /// Applies a correction derived from the current child pass.
    fn correct_by(&mut self, correction: f64);
}

/// Detached layout proposal for a fixed position.
#[derive(Debug)]
pub struct FixedViewportLayout {
    original: f64,
    pixels: f64,
}

impl ViewportLayout for FixedViewportLayout {
    fn pixels(&self) -> f64 {
        self.pixels
    }
    fn apply_viewport_dimension(&mut self, _dimension: f64) -> bool {
        true
    }
    fn apply_content_dimensions(&mut self, _min: f64, _max: f64) -> bool {
        true
    }
    fn correct_by(&mut self, correction: f64) {
        self.pixels += correction;
    }
}

/// A simple fixed viewport offset that doesn't change.
///
/// The `pixels` value does not change unless the viewport issues a correction.
pub struct FixedViewportOffset {
    pixels: f64,
}

impl Debug for FixedViewportOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FixedViewportOffset")
            .field("pixels", &self.pixels)
            .finish()
    }
}

impl FixedViewportOffset {
    /// Creates a fixed viewport offset with the given pixels value.
    pub fn new(pixels: f64) -> Self {
        Self { pixels }
    }

    /// Creates a fixed viewport offset at zero.
    pub fn zero() -> Self {
        Self::new(0.0)
    }
}

impl Default for FixedViewportOffset {
    fn default() -> Self {
        Self::zero()
    }
}

impl ViewportOffset for FixedViewportOffset {
    type Layout = FixedViewportLayout;

    fn begin_layout(&self) -> Self::Layout {
        FixedViewportLayout {
            original: self.pixels,
            pixels: self.pixels,
        }
    }

    fn accept_layout(&mut self, layout: Self::Layout) -> crate::RenderResult<()> {
        if self.pixels.to_bits() != layout.original.to_bits() {
            return Err(crate::RenderError::ViewportOffsetChanged);
        }
        self.pixels = layout.pixels;
        Ok(())
    }

    fn pixels(&self) -> f64 {
        self.pixels
    }

    fn has_pixels(&self) -> bool {
        true
    }

    fn apply_viewport_dimension(&mut self, _viewport_dimension: f64) -> bool {
        true
    }

    fn apply_content_dimensions(
        &mut self,
        _min_scroll_extent: f64,
        _max_scroll_extent: f64,
    ) -> bool {
        true
    }

    fn correct_by(&mut self, correction: f64) {
        self.pixels += correction;
    }

    fn user_scroll_direction(&self) -> ScrollDirection {
        ScrollDirection::Idle
    }

    fn add_listener(&self, _listener: Rc<dyn Fn()>) {
        // No-op: nothing moves a fixed offset outside layout, which never
        // notifies, so no listener could ever fire.
    }

    fn remove_listener(&self, _listener: &Rc<dyn Fn()>) {
        // No-op for the same reason as `add_listener`.
    }
}

/// The listener set behind [`ViewportOffset::add_listener`]: a [`ChangeNotifier`] whose
/// subscriptions are found again by the callback's `Rc` identity, which is the trait's removal
/// contract. Delivery, a panicking listener and removal during delivery follow the notifier's
/// rules, so one failed listener neither silences the offset nor skips the rest.
#[derive(Default)]
pub(crate) struct OffsetListeners {
    identities: RefCell<Vec<(Subscription, Weak<dyn Fn()>)>>,
    notifier: ChangeNotifier,
}

impl OffsetListeners {
    pub(crate) fn add(&self, listener: Rc<dyn Fn()>) {
        let identity = Rc::downgrade(&listener);
        let listening = self.notifier.listen(listener);
        self.identities.borrow_mut().push((listening, identity));
    }

    /// Withdraws the first subscription of `listener` without detaching it, for an
    /// owner that retires it under the failure of another delivery in progress.
    #[must_use]
    pub(crate) fn take(&self, listener: &Rc<dyn Fn()>) -> Option<Subscription> {
        let mut identities = self.identities.borrow_mut();
        let probe = Rc::downgrade(listener);
        let index = identities
            .iter()
            .position(|(_, identity)| identity.ptr_eq(&probe))?;
        Some(identities.remove(index).0)
    }

    pub(crate) fn notifier(&self) -> &ChangeNotifier {
        &self.notifier
    }
}
