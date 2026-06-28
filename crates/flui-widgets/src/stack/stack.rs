//! [`Stack`] — overlaps children, aligning non-positioned ones.

use std::fmt;

use flui_objects::{RenderStack, StackFit};
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
    C: ViewSeq + Clone + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderStack;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderStack::new()
            .with_alignment(self.alignment)
            .with_fit(self.fit)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
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
