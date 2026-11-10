//! [`Positioned`] and [`DirectionalPositioned`] — place a child of a [`Stack`]
//! at explicit edge offsets.
//!
//! Each is a [`ParentDataView`] contributing a [`StackParentData`] (edge
//! insets + explicit size) to its child's render node, which the parent
//! `RenderStack` reads to position the child. The child is **required** — a
//! positioned child with no subtree is meaningless — so it is taken by the
//! constructor.
//!
//! [`Positioned`] names the physical `left`/`right` edges and
//! [`DirectionalPositioned`] the reading `start`/`end` edges, so one child
//! cannot name both pairs.
//!
//! [`Stack`]: crate::Stack

use flui_rendering::parent_data::{HorizontalInsets, StackParentData};
use flui_view::{BoxedView, IntoView, ParentDataView, View, ViewExt, impl_parent_data_view};

/// The vertical insets and explicit size both positioned views share.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Placement {
    top: Option<f64>,
    bottom: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
}

impl Placement {
    fn create_parent_data(self, horizontal: HorizontalInsets) -> StackParentData {
        let mut data = StackParentData::new().with_horizontal(horizontal);
        data.top = self.top;
        data.bottom = self.bottom;
        data.width = self.width;
        data.height = self.height;
        data
    }

    fn apply(
        self,
        horizontal: HorizontalInsets,
        data: &mut StackParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        let current = Self {
            top: data.top,
            bottom: data.bottom,
            width: data.width,
            height: data.height,
        };
        if current == self && data.horizontal == horizontal {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        data.top = self.top;
        data.bottom = self.bottom;
        data.horizontal = horizontal;
        data.width = self.width;
        data.height = self.height;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

/// Positions its child within a [`Stack`] relative to the stack's edges.
///
/// Any subset of `left` / `top` / `right` / `bottom` may be set; setting both
/// edges of an axis (e.g. `left` and `right`) stretches the child along it,
/// otherwise `width` / `height` size it. An unset axis leaves the child at the
/// stack's alignment (a "non-positioned" child). For insets from the reading
/// edges instead of `left`/`right`, use [`Positioned::directional`].
///
/// [`Stack`]: crate::Stack
#[derive(Clone, Debug)]
pub struct Positioned {
    left: Option<f64>,
    right: Option<f64>,
    placement: Placement,
    child: BoxedView,
}

impl Positioned {
    /// A child with no positioning set yet — chain the edge/size builders to
    /// place it. With nothing set the child stays non-positioned (aligned).
    pub fn new(child: impl IntoView) -> Self {
        Self {
            left: None,
            right: None,
            placement: Placement::default(),
            child: child.into_view().boxed(),
        }
    }

    /// A child placed by its distances from the stack's reading edges
    /// (`start`/`end`) instead of its `left`/`right` edges; the stack resolves
    /// them against the ambient `Directionality`.
    pub fn directional(child: impl IntoView) -> DirectionalPositioned {
        DirectionalPositioned {
            start: None,
            end: None,
            placement: Placement::default(),
            child: child.into_view().boxed(),
        }
    }

    /// Pin the child to all four edges of the stack (zero insets).
    pub fn fill(child: impl IntoView) -> Self {
        Self::new(child).left(0.0).top(0.0).right(0.0).bottom(0.0)
    }

    /// Distance between the child's left edge and the stack's left edge.
    #[must_use]
    pub fn left(mut self, left: f64) -> Self {
        self.left = Some(left);
        self
    }

    /// Distance between the child's top edge and the stack's top edge.
    #[must_use]
    pub fn top(mut self, top: f64) -> Self {
        self.placement.top = Some(top);
        self
    }

    /// Distance between the child's right edge and the stack's right edge.
    #[must_use]
    pub fn right(mut self, right: f64) -> Self {
        self.right = Some(right);
        self
    }

    /// Distance between the child's bottom edge and the stack's bottom edge.
    #[must_use]
    pub fn bottom(mut self, bottom: f64) -> Self {
        self.placement.bottom = Some(bottom);
        self
    }

    /// The child's explicit width (ignored when both `left` and `right` are
    /// set, which already determine it).
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.placement.width = Some(width);
        self
    }

    /// The child's explicit height (ignored when both `top` and `bottom` are
    /// set, which already determine it).
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.placement.height = Some(height);
        self
    }

    const fn horizontal(&self) -> HorizontalInsets {
        HorizontalInsets::Physical {
            left: self.left,
            right: self.right,
        }
    }
}

impl ParentDataView for Positioned {
    type ParentData = StackParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        self.placement.create_parent_data(self.horizontal())
    }

    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        self.placement.apply(self.horizontal(), parent_data)
    }

    fn debug_type_name(&self) -> &'static str {
        "Positioned"
    }

    fn typical_ancestor_description(&self) -> &'static str {
        "Stack"
    }
}

impl_parent_data_view!(Positioned);

/// Positions its child within a [`Stack`] relative to the stack's reading
/// edges: `start` is the left edge in left-to-right text and the right edge in
/// right-to-left text, and `end` the opposite. Made by
/// [`Positioned::directional`].
///
/// Setting both `start` and `end` stretches the child between them, otherwise
/// `width` sizes it; `top` / `bottom` / `height` work as on [`Positioned`].
///
/// [`Stack`]: crate::Stack
#[derive(Clone, Debug)]
pub struct DirectionalPositioned {
    start: Option<f64>,
    end: Option<f64>,
    placement: Placement,
    child: BoxedView,
}

impl DirectionalPositioned {
    /// Distance between the child's reading-start edge and the stack's.
    #[must_use]
    pub fn start(mut self, start: f64) -> Self {
        self.start = Some(start);
        self
    }

    /// Distance between the child's top edge and the stack's top edge.
    #[must_use]
    pub fn top(mut self, top: f64) -> Self {
        self.placement.top = Some(top);
        self
    }

    /// Distance between the child's reading-end edge and the stack's.
    #[must_use]
    pub fn end(mut self, end: f64) -> Self {
        self.end = Some(end);
        self
    }

    /// Distance between the child's bottom edge and the stack's bottom edge.
    #[must_use]
    pub fn bottom(mut self, bottom: f64) -> Self {
        self.placement.bottom = Some(bottom);
        self
    }

    /// The child's explicit width (ignored when both `start` and `end` are
    /// set, which already determine it).
    #[must_use]
    pub fn width(mut self, width: f64) -> Self {
        self.placement.width = Some(width);
        self
    }

    /// The child's explicit height (ignored when both `top` and `bottom` are
    /// set, which already determine it).
    #[must_use]
    pub fn height(mut self, height: f64) -> Self {
        self.placement.height = Some(height);
        self
    }

    const fn horizontal(&self) -> HorizontalInsets {
        HorizontalInsets::Directional {
            start: self.start,
            end: self.end,
        }
    }
}

impl ParentDataView for DirectionalPositioned {
    type ParentData = StackParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        self.placement.create_parent_data(self.horizontal())
    }

    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        self.placement.apply(self.horizontal(), parent_data)
    }

    fn debug_type_name(&self) -> &'static str {
        "DirectionalPositioned"
    }

    fn typical_ancestor_description(&self) -> &'static str {
        "Stack"
    }
}

impl_parent_data_view!(DirectionalPositioned);
