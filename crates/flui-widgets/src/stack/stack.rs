//! [`Stack`] and [`IndexedStack`] — overlap children, aligning non-positioned ones.

use std::fmt;

use flui_objects::{RenderIndexedStack, RenderStack, StackFit};
use flui_rendering::protocol::BoxProtocol;
use flui_types::Alignment;
use flui_view::BoxedView;
use flui_view::seq::ViewSeq;

use crate::support::generic_render_view_element;

/// Overlaps its children, sizing itself to the largest non-positioned child and
/// aligning each by `alignment`.
///
/// Flutter parity: `widgets/basic.dart` `Stack` over `RenderStack`. Defaults
/// match Flutter: `alignment = Alignment::TOP_LEFT`, `fit = StackFit::Loose`.
/// Wrap a child in [`Positioned`](crate::Positioned) to place it at explicit
/// edges instead of being aligned.
///
/// Generic over `C: ViewSeq` — `stack!`-style tuples (via the shared
/// `column!`/`row!` macros) or a dynamic `Vec<BoxedView>`.
#[derive(Clone)]
pub struct Stack<C = Vec<BoxedView>> {
    alignment: Alignment,
    fit: StackFit,
    children: C,
}

impl<C> Stack<C> {
    /// A stack of the given children with Flutter's default alignment/fit.
    pub fn new(children: C) -> Self {
        Self {
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::Loose,
            children,
        }
    }

    /// How non-positioned children are aligned within the stack.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// How non-positioned children are sized (`Loose` = own size, `Expand` =
    /// fill the stack).
    #[must_use]
    pub fn fit(mut self, fit: StackFit) -> Self {
        self.fit = fit;
        self
    }
}

impl<C: ViewSeq> fmt::Debug for Stack<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stack")
            .field("alignment", &self.alignment)
            .field("fit", &self.fit)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for Stack<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderStack;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderStack::new()
            .with_alignment(self.alignment)
            .with_fit(self.fit)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_alignment(self.alignment);
        render_object.set_fit(self.fit);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(Stack);

/// Overlaps its children like [`Stack`], but displays only one child by index.
///
/// Flutter parity: all children are still laid out, so the stack's size is
/// resolved exactly like [`Stack`]. Only the selected child participates in
/// paint, hit testing, semantics, and baseline reporting. `index = None`
/// displays no child.
#[derive(Clone)]
pub struct IndexedStack<C = Vec<BoxedView>> {
    alignment: Alignment,
    fit: StackFit,
    index: Option<usize>,
    children: C,
}

impl<C> IndexedStack<C> {
    /// An indexed stack with Flutter's default alignment, fit, and `index = 0`.
    pub fn new(children: C) -> Self {
        Self {
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::Loose,
            index: Some(0),
            children,
        }
    }

    /// How non-positioned children are aligned within the stack.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// How non-positioned children are sized (`Loose` = own size, `Expand` =
    /// fill the stack).
    #[must_use]
    pub fn fit(mut self, fit: StackFit) -> Self {
        self.fit = fit;
        self
    }

    /// Which child is displayed. `None` displays no child.
    #[must_use]
    pub fn index(mut self, index: Option<usize>) -> Self {
        self.index = index;
        self
    }
}

impl<C: ViewSeq> fmt::Debug for IndexedStack<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IndexedStack")
            .field("alignment", &self.alignment)
            .field("fit", &self.fit)
            .field("index", &self.index)
            .field("children", &self.children.len())
            .finish()
    }
}

impl<C> flui_view::RenderView for IndexedStack<C>
where
    C: ViewSeq + Clone + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderIndexedStack;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIndexedStack::new()
            .with_alignment(self.alignment)
            .with_fit(self.fit)
            .with_index(self.index)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_alignment(self.alignment);
        render_object.set_fit(self.fit);
        render_object.set_index(self.index);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(IndexedStack);

#[cfg(test)]
mod tests {
    use flui_types::Alignment;
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn stack_create_render_object_defaults_to_top_left_and_loose_fit() {
        let stack: Stack = Stack::new(Vec::new());
        let render_object = stack.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);
        assert_eq!(render_object.fit(), StackFit::Loose);
    }

    #[test]
    fn stack_create_render_object_applies_overridden_alignment_and_fit() {
        let stack: Stack = Stack::new(Vec::new())
            .alignment(Alignment::CENTER)
            .fit(StackFit::Expand);
        let render_object = stack.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.alignment(), Alignment::CENTER);
        assert_eq!(render_object.fit(), StackFit::Expand);
    }

    #[test]
    fn stack_update_render_object_reconfigures_alignment_and_fit() {
        let mut render_object = Stack::<Vec<BoxedView>>::new(Vec::new())
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);

        let updated: Stack = Stack::new(Vec::new())
            .alignment(Alignment::BOTTOM_RIGHT)
            .fit(StackFit::Expand);
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.alignment(), Alignment::BOTTOM_RIGHT);
        assert_eq!(render_object.fit(), StackFit::Expand);
    }

    #[test]
    fn stack_debug_reports_alignment_fit_and_child_count() {
        use flui_view::ViewExt;

        let stack = Stack::new(vec![SizedBox::shrink().boxed()]);
        let debug = format!("{stack:?}");
        assert!(
            debug.contains("alignment:") && debug.contains("children: 1"),
            "Debug output must include alignment and children count, got: {debug}",
        );
    }

    #[test]
    fn stack_has_children_reflects_an_empty_child_list() {
        let empty: Stack = Stack::new(Vec::new());
        assert!(!empty.has_children());
    }

    #[test]
    fn indexed_stack_create_render_object_defaults_to_index_zero() {
        let stack: IndexedStack = IndexedStack::new(Vec::new());
        let render_object = stack.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.index(), Some(0));
        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);
        assert_eq!(render_object.fit(), StackFit::Loose);
    }

    #[test]
    fn indexed_stack_create_render_object_applies_overridden_index_alignment_and_fit() {
        let stack: IndexedStack = IndexedStack::new(Vec::new())
            .index(None)
            .alignment(Alignment::CENTER)
            .fit(StackFit::Expand);
        let render_object = stack.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.index(), None);
        assert_eq!(render_object.alignment(), Alignment::CENTER);
        assert_eq!(render_object.fit(), StackFit::Expand);
    }

    #[test]
    fn indexed_stack_update_render_object_reconfigures_index_alignment_and_fit() {
        let mut render_object = IndexedStack::<Vec<BoxedView>>::new(Vec::new())
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.index(), Some(0));

        let updated: IndexedStack = IndexedStack::new(Vec::new())
            .index(Some(3))
            .alignment(Alignment::BOTTOM_RIGHT)
            .fit(StackFit::Expand);
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.index(), Some(3));
        assert_eq!(render_object.alignment(), Alignment::BOTTOM_RIGHT);
        assert_eq!(render_object.fit(), StackFit::Expand);
    }

    #[test]
    fn indexed_stack_debug_reports_index_and_child_count() {
        use flui_view::ViewExt;

        let stack = IndexedStack::new(vec![SizedBox::shrink().boxed()]).index(None);
        let debug = format!("{stack:?}");
        assert!(
            debug.contains("index: None") && debug.contains("children: 1"),
            "Debug output must include index and children count, got: {debug}",
        );
    }

    #[test]
    fn indexed_stack_has_children_reflects_an_empty_child_list() {
        let empty: IndexedStack = IndexedStack::new(Vec::new());
        assert!(!empty.has_children());
    }
}
