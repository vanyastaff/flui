//! [`CustomMultiChildLayout`] and [`LayoutId`] — delegate multi-child layout
//! to a [`MultiChildLayoutDelegate`].

use std::fmt;
use std::sync::Arc;

use flui_objects::RenderCustomMultiChildLayoutBox;
use flui_rendering::delegates::MultiChildLayoutDelegate;
use flui_rendering::parent_data::MultiChildLayoutParentData;
use flui_rendering::protocol::BoxProtocol;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, IntoView, ParentDataView, View, ViewExt, impl_parent_data_view};

use crate::support::generic_render_view_element;

/// Metadata for identifying one child inside a [`CustomMultiChildLayout`].
///
/// Flutter parity: `widgets/basic.dart` `LayoutId`.
#[derive(Clone, Debug)]
pub struct LayoutId {
    id: String,
    child: BoxedView,
}

impl LayoutId {
    /// Marks `child` with a layout identifier.
    pub fn new(id: impl Into<String>, child: impl IntoView) -> Self {
        Self {
            id: id.into(),
            child: child.into_view().boxed(),
        }
    }

    /// Returns this child's layout identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl ParentDataView for LayoutId {
    type ParentData = MultiChildLayoutParentData;

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn create_parent_data(&self) -> Self::ParentData {
        MultiChildLayoutParentData::zero().with_id(self.id.clone())
    }
}

impl_parent_data_view!(LayoutId);

/// A widget that sizes and positions multiple children using a layout delegate.
///
/// Each child must be wrapped in [`LayoutId`] so the delegate can address it by
/// id. Flutter parity: `widgets/basic.dart` `CustomMultiChildLayout` over
/// `RenderCustomMultiChildLayoutBox`.
#[derive(Clone)]
pub struct CustomMultiChildLayout<C = Vec<BoxedView>> {
    delegate: Arc<dyn MultiChildLayoutDelegate>,
    children: C,
}

impl<C> CustomMultiChildLayout<C> {
    /// Creates a custom multi-child layout.
    pub fn new(delegate: Arc<dyn MultiChildLayoutDelegate>, children: C) -> Self {
        Self { delegate, children }
    }
}

impl<C: ViewSeq> fmt::Debug for CustomMultiChildLayout<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomMultiChildLayout")
            .field("delegate", &self.delegate)
            .field("children", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<C> flui_view::RenderView for CustomMultiChildLayout<C>
where
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderCustomMultiChildLayoutBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderCustomMultiChildLayoutBox::new(self.delegate.clone())
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_delegate(self.delegate.clone());
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(CustomMultiChildLayout);
