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
}

impl_parent_data_view!(Expanded);
