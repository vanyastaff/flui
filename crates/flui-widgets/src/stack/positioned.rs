//! [`Positioned`] — place a child of a [`Stack`] at explicit edge offsets.
//!
//! A [`ParentDataView`] contributing a [`StackParentData`] (edge insets +
//! explicit size) to its child's render node, which the parent `RenderStack`
//! reads to position the child. The child is **required** — a positioned child
//! with no subtree is meaningless — so it is taken by the constructor.
//!
//! [`Stack`]: crate::Stack

use flui_rendering::parent_data::StackParentData;
use flui_view::{BoxedView, IntoView, ParentDataView, View, ViewExt, impl_parent_data_view};

/// Positions its child within a [`Stack`] relative to the stack's edges.
///
/// Any subset of `left` / `top` / `right` / `bottom` may be set; setting both
/// edges of an axis (e.g. `left` and `right`) stretches the child along it,
/// otherwise `width` / `height` size it. An unset axis leaves the child at the
/// stack's alignment (a "non-positioned" child).
///
/// Flutter parity: `widgets/basic.dart` `Positioned`.
///
/// [`Stack`]: crate::Stack
#[derive(Clone, Debug)]
pub struct Positioned {
    left: Option<f32>,
    top: Option<f32>,
    right: Option<f32>,
    bottom: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    child: BoxedView,
}

impl Positioned {
    /// A child with no positioning set yet — chain the edge/size builders to
    /// place it. With nothing set the child stays non-positioned (aligned).
    pub fn new(child: impl IntoView) -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
            child: child.into_view().boxed(),
        }
    }

    /// Pin the child to all four edges of the stack (Flutter's
    /// `Positioned.fill` with the default zero insets).
    pub fn fill(child: impl IntoView) -> Self {
        Self::new(child).left(0.0).top(0.0).right(0.0).bottom(0.0)
    }

    /// Distance between the child's left edge and the stack's left edge.
    #[must_use]
    pub fn left(mut self, left: f32) -> Self {
        self.left = Some(left);
        self
    }

    /// Distance between the child's top edge and the stack's top edge.
    #[must_use]
    pub fn top(mut self, top: f32) -> Self {
        self.top = Some(top);
        self
    }

    /// Distance between the child's right edge and the stack's right edge.
    #[must_use]
    pub fn right(mut self, right: f32) -> Self {
        self.right = Some(right);
        self
    }

    /// Distance between the child's bottom edge and the stack's bottom edge.
    #[must_use]
    pub fn bottom(mut self, bottom: f32) -> Self {
        self.bottom = Some(bottom);
        self
    }

    /// The child's explicit width (ignored when both `left` and `right` are
    /// set, which already determine it).
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// The child's explicit height (ignored when both `top` and `bottom` are
    /// set, which already determine it).
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }
}

impl ParentDataView for Positioned {
    type ParentData = StackParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        let mut data = StackParentData::new();
        if let Some(left) = self.left {
            data = data.with_left(left);
        }
        if let Some(top) = self.top {
            data = data.with_top(top);
        }
        if let Some(right) = self.right {
            data = data.with_right(right);
        }
        if let Some(bottom) = self.bottom {
            data = data.with_bottom(bottom);
        }
        if let Some(width) = self.width {
            data = data.with_width(width);
        }
        if let Some(height) = self.height {
            data = data.with_height(height);
        }
        data
    }
}

impl_parent_data_view!(Positioned);
