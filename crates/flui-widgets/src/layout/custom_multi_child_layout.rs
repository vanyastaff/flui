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
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderCustomMultiChildLayoutBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderCustomMultiChildLayoutBox::new(self.delegate.clone())
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
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

#[cfg(test)]
mod tests {
    use std::any::{Any, TypeId};

    use flui_rendering::delegates::MultiChildLayoutContext;
    use flui_types::{Offset, Size};
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    /// A minimal delegate -- only needed to satisfy `MultiChildLayoutDelegate`'s
    /// object safety; these tests exercise the widget's own wiring (which
    /// delegate instance gets installed), not delegate layout behavior
    /// (covered by `tests/custom_multi_child_layout.rs`).
    #[derive(Debug)]
    struct NoopDelegate;

    impl MultiChildLayoutDelegate for NoopDelegate {
        fn perform_layout(&self, _context: &mut dyn MultiChildLayoutContext, _size: Size) {}

        fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
            false
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[derive(Debug)]
    struct OtherDelegate;

    impl MultiChildLayoutDelegate for OtherDelegate {
        fn perform_layout(&self, _context: &mut dyn MultiChildLayoutContext, _size: Size) {}

        fn should_relayout(&self, _old_delegate: &dyn MultiChildLayoutDelegate) -> bool {
            true
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn noop_delegate() -> Arc<dyn MultiChildLayoutDelegate> {
        Arc::new(NoopDelegate)
    }

    #[test]
    fn layout_id_returns_the_configured_identifier() {
        let widget = LayoutId::new("header", SizedBox::shrink());
        assert_eq!(widget.id(), "header");
    }

    #[test]
    fn layout_id_create_parent_data_carries_the_id_and_zero_offset() {
        let widget = LayoutId::new("body", SizedBox::shrink());
        let parent_data = widget.create_parent_data();

        assert_eq!(parent_data.id.as_deref(), Some("body"));
        assert_eq!(parent_data.offset, Offset::ZERO);
        assert!(parent_data.has_id());
    }

    #[test]
    fn layout_id_child_returns_the_wrapped_view() {
        let widget = LayoutId::new("header", SizedBox::new(10.0, 20.0));
        assert_eq!(
            widget.child().view_type_id(),
            TypeId::of::<SizedBox>(),
            "child() must return the wrapped SizedBox view",
        );
    }

    #[test]
    fn layout_id_debug_reports_the_id() {
        let widget = LayoutId::new("header", SizedBox::shrink());
        let debug = format!("{widget:?}");
        assert!(
            debug.contains(r#"id: "header""#),
            "Debug output must report the id, got: {debug}",
        );
    }

    #[test]
    fn create_render_object_installs_the_given_delegate() {
        let render_object = CustomMultiChildLayout::new(noop_delegate(), Vec::<BoxedView>::new())
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(
            render_object.delegate().as_any().is::<NoopDelegate>(),
            "create_render_object must install the exact delegate passed to new()",
        );
    }

    #[test]
    fn update_render_object_replaces_the_delegate() {
        let mut render_object =
            CustomMultiChildLayout::new(noop_delegate(), Vec::<BoxedView>::new())
                .create_render_object(&flui_view::RenderObjectContext::detached());
        assert!(render_object.delegate().as_any().is::<NoopDelegate>());

        let other: Arc<dyn MultiChildLayoutDelegate> = Arc::new(OtherDelegate);
        CustomMultiChildLayout::new(other, Vec::<BoxedView>::new()).update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert!(
            render_object.delegate().as_any().is::<OtherDelegate>(),
            "update_render_object must replace the delegate with the new instance",
        );
    }

    #[test]
    fn has_children_reflects_whether_children_were_set() {
        let empty = CustomMultiChildLayout::new(noop_delegate(), Vec::<BoxedView>::new());
        assert!(!empty.has_children());

        let with_children = CustomMultiChildLayout::new(
            noop_delegate(),
            vec![SizedBox::shrink().boxed(), SizedBox::shrink().boxed()],
        );
        assert!(with_children.has_children());
    }

    #[test]
    fn debug_reports_the_delegate_and_child_count() {
        let empty = CustomMultiChildLayout::new(noop_delegate(), Vec::<BoxedView>::new());
        let debug = format!("{empty:?}");
        assert!(
            debug.contains("children: 0"),
            "Debug output must report zero children, got: {debug}",
        );

        let with_children = CustomMultiChildLayout::new(
            noop_delegate(),
            vec![SizedBox::shrink().boxed(), SizedBox::shrink().boxed()],
        );
        let debug = format!("{with_children:?}");
        assert!(
            debug.contains("children: 2"),
            "Debug output must report the child count, got: {debug}",
        );
    }

    #[test]
    fn visit_child_views_visits_each_child_exactly_once() {
        let widget = CustomMultiChildLayout::new(
            noop_delegate(),
            vec![
                SizedBox::shrink().boxed(),
                SizedBox::shrink().boxed(),
                SizedBox::shrink().boxed(),
            ],
        );

        let mut visit_count = 0usize;
        widget.visit_child_views(&mut |_child| visit_count += 1);

        assert_eq!(visit_count, 3);
    }
}
