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

    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        let changed = parent_data.left != self.left
            || parent_data.top != self.top
            || parent_data.right != self.right
            || parent_data.bottom != self.bottom
            || parent_data.width != self.width
            || parent_data.height != self.height;
        if !changed {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        parent_data.left = self.left;
        parent_data.top = self.top;
        parent_data.right = self.right;
        parent_data.bottom = self.bottom;
        parent_data.width = self.width;
        parent_data.height = self.height;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl_parent_data_view!(Positioned);

#[cfg(test)]
mod tests {
    use flui_foundation::RenderId;
    use flui_types::{Offset, geometry::px};

    use super::*;
    use crate::SizedBox;

    #[test]
    fn positioned_parent_data_reports_exact_impact_and_preserves_layout_fields() {
        let mut data = StackParentData::new().with_left(1.0).with_top(2.0);
        data.offset = Offset::new(px(8.0), px(13.0));
        data.container.previous_sibling = Some(RenderId::new(7));
        data.container.next_sibling = Some(RenderId::new(9));
        let unchanged = Positioned::new(SizedBox::shrink()).left(1.0).top(2.0);
        assert_eq!(
            unchanged.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::NONE
        );
        let changed = Positioned::new(SizedBox::shrink())
            .right(3.0)
            .bottom(4.0)
            .width(5.0)
            .height(6.0);
        assert_eq!(
            changed.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(data.offset, Offset::new(px(8.0), px(13.0)));
        assert_eq!(data.container.previous_sibling, Some(RenderId::new(7)));
        assert_eq!(data.container.next_sibling, Some(RenderId::new(9)));
    }

    #[test]
    fn positioned_each_geometry_field_independently_requires_layout() {
        let configurations = [
            Positioned::new(SizedBox::shrink()).left(1.0),
            Positioned::new(SizedBox::shrink()).top(2.0),
            Positioned::new(SizedBox::shrink()).right(3.0),
            Positioned::new(SizedBox::shrink()).bottom(4.0),
            Positioned::new(SizedBox::shrink()).width(5.0),
            Positioned::new(SizedBox::shrink()).height(6.0),
        ];

        for configuration in configurations {
            let mut data = StackParentData::new();
            data.offset = Offset::new(px(8.0), px(13.0));
            data.container.previous_sibling = Some(RenderId::new(7));
            data.container.next_sibling = Some(RenderId::new(9));
            assert_eq!(
                configuration.apply_parent_data(&mut data),
                flui_rendering::RenderUpdateImpact::LAYOUT,
            );
            assert_eq!(data.offset, Offset::new(px(8.0), px(13.0)));
            assert_eq!(data.container.previous_sibling, Some(RenderId::new(7)));
            assert_eq!(data.container.next_sibling, Some(RenderId::new(9)));
        }
    }
}
