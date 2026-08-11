//! [`Flexible`] and [`Expanded`] — give a child of a [`Row`]/[`Column`]/[`Flex`]
//! a share of the main axis.
//!
//! Both are [`ParentDataView`]s: they contribute a [`FlexParentData`] (flex
//! factor + fit) to their child's render node, which the parent `RenderFlex`
//! reads during layout. The child is **required** — a flex child with no
//! subtree is meaningless — so it is taken by the constructor rather than a
//! fallible `.child()` builder.
//!
//! [`Row`]: crate::Row
//! [`Column`]: crate::Column
//! [`Flex`]: crate::Flex

use flui_rendering::parent_data::{FlexFit, FlexParentData};
use flui_types::Offset;
use flui_view::{BoxedView, IntoView, ParentDataView, View, ViewExt, impl_parent_data_view};

/// Gives its child a share of the main axis of a [`Row`]/[`Column`]/[`Flex`],
/// proportional to `flex` relative to the other flexible siblings.
///
/// Flutter parity: `widgets/basic.dart` `Flexible`. The default `fit` is
/// [`FlexFit::Loose`] (the child may be smaller than its share); [`Expanded`]
/// is the [`FlexFit::Tight`] specialization that forces the child to fill it.
///
/// [`Row`]: crate::Row
/// [`Column`]: crate::Column
/// [`Flex`]: crate::Flex
#[derive(Clone, Debug)]
pub struct Flexible {
    flex: i32,
    fit: FlexFit,
    child: BoxedView,
}

impl Flexible {
    /// A flexible child with `flex == 1` and a [loose](FlexFit::Loose) fit.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            flex: 1,
            fit: FlexFit::Loose,
            child: child.into_view().boxed(),
        }
    }

    /// Set the flex factor — this child's share of the main axis relative to
    /// its flexible siblings.
    #[must_use]
    pub fn flex(mut self, flex: i32) -> Self {
        self.flex = flex;
        self
    }

    /// Set how the child fits the space allotted to it
    /// ([`Loose`](FlexFit::Loose) lets it shrink, [`Tight`](FlexFit::Tight)
    /// forces it to fill).
    #[must_use]
    pub fn fit(mut self, fit: FlexFit) -> Self {
        self.fit = fit;
        self
    }
}

impl ParentDataView for Flexible {
    type ParentData = FlexParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        FlexParentData::new(Offset::ZERO, Some(self.flex), self.fit)
    }

    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        let flex = Some(self.flex);
        if parent_data.flex == flex && parent_data.fit == self.fit {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        parent_data.flex = flex;
        parent_data.fit = self.fit;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl_parent_data_view!(Flexible);

/// A [`Flexible`] that forces its child to fill its share of the main axis
/// (a [`FlexFit::Tight`] fit).
///
/// Flutter parity: `widgets/basic.dart` `Expanded` — `Flexible` fixed to
/// `FlexFit.tight`.
#[derive(Clone, Debug)]
pub struct Expanded {
    flex: i32,
    child: BoxedView,
}

impl Expanded {
    /// An expanded child with `flex == 1`.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            flex: 1,
            child: child.into_view().boxed(),
        }
    }

    /// Set the flex factor — this child's share of the main axis relative to
    /// its flexible siblings.
    #[must_use]
    pub fn flex(mut self, flex: i32) -> Self {
        self.flex = flex;
        self
    }
}

impl ParentDataView for Expanded {
    type ParentData = FlexParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        FlexParentData::new(Offset::ZERO, Some(self.flex), FlexFit::Tight)
    }

    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        let flex = Some(self.flex);
        if parent_data.flex == flex && parent_data.fit == FlexFit::Tight {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        parent_data.flex = flex;
        parent_data.fit = FlexFit::Tight;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl_parent_data_view!(Expanded);

#[cfg(test)]
mod tests {
    use flui_foundation::RenderId;
    use flui_types::geometry::px;

    use super::*;
    use crate::SizedBox;

    fn seeded_data() -> FlexParentData {
        let mut data = FlexParentData::new(Offset::new(px(8.0), px(13.0)), Some(1), FlexFit::Loose);
        data.container.previous_sibling = Some(RenderId::new(7));
        data.container.next_sibling = Some(RenderId::new(9));
        data
    }

    fn assert_layout_fields_preserved(data: &FlexParentData) {
        assert_eq!(data.offset, Offset::new(px(8.0), px(13.0)));
        assert_eq!(data.container.previous_sibling, Some(RenderId::new(7)));
        assert_eq!(data.container.next_sibling, Some(RenderId::new(9)));
    }

    #[test]
    fn flexible_parent_data_reports_exact_impact_and_preserves_layout_fields() {
        let mut data = seeded_data();
        let unchanged = Flexible::new(SizedBox::shrink());
        assert_eq!(
            unchanged.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::NONE
        );

        let mut flex_only_data = seeded_data();
        let flex_only = Flexible::new(SizedBox::shrink()).flex(3);
        assert_eq!(
            flex_only.apply_parent_data(&mut flex_only_data),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
        assert_eq!(flex_only_data.flex, Some(3));
        assert_eq!(flex_only_data.fit, FlexFit::Loose);
        assert_layout_fields_preserved(&flex_only_data);

        let mut fit_only_data = seeded_data();
        let fit_only = Flexible::new(SizedBox::shrink()).fit(FlexFit::Tight);
        assert_eq!(
            fit_only.apply_parent_data(&mut fit_only_data),
            flui_rendering::RenderUpdateImpact::LAYOUT,
        );
        assert_eq!(fit_only_data.flex, Some(1));
        assert_eq!(fit_only_data.fit, FlexFit::Tight);
        assert_layout_fields_preserved(&fit_only_data);

        let changed = Flexible::new(SizedBox::shrink())
            .flex(3)
            .fit(FlexFit::Tight);
        assert_eq!(
            changed.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(data.flex, Some(3));
        assert_eq!(data.fit, FlexFit::Tight);
        assert_layout_fields_preserved(&data);
    }

    #[test]
    fn expanded_parent_data_reports_exact_impact_and_preserves_layout_fields() {
        let mut data = seeded_data();
        data.fit = FlexFit::Tight;
        let unchanged = Expanded::new(SizedBox::shrink());
        assert_eq!(
            unchanged.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::NONE
        );
        let changed = Expanded::new(SizedBox::shrink()).flex(4);
        assert_eq!(
            changed.apply_parent_data(&mut data),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
        assert_eq!(data.flex, Some(4));
        assert_eq!(data.fit, FlexFit::Tight);
        assert_layout_fields_preserved(&data);
    }
}
