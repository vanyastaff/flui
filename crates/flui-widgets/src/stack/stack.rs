//! [`Stack`] and [`IndexedStack`] — overlap children, aligning non-positioned ones.

use std::fmt;

use flui_objects::{RenderIndexedStack, RenderStack, StackFit};
use flui_painting::Alignment;
use flui_painting::AlignmentGeometry;
use flui_painting::typography::TextDirection;
use flui_rendering::protocol::BoxProtocol;
use flui_view::BoxedView;
use flui_view::InheritedView;
use flui_view::seq::ViewSeq;

use crate::localization::Directionality;
use crate::support::generic_render_view_element;

/// Resolve the ambient [`Directionality`], defaulting to `Ltr`. The stack reads
/// it for both its alignment and the `start`/`end` edges of `Positioned`
/// children, so it registers the dependency unconditionally (T09).
fn resolve_text_direction(ctx: &flui_view::RenderObjectContext<'_>) -> TextDirection {
    ctx.depend_on::<Directionality, _>(|d| *d.data())
        .unwrap_or(TextDirection::Ltr)
}

/// Resolve `alignment` against an already-read `text_direction`: an absolute
/// geometry resolves to itself, a directional one to its physical mirror under
/// the direction (T09).
fn resolve_alignment_geometry(
    alignment: AlignmentGeometry,
    text_direction: TextDirection,
) -> Alignment {
    match alignment {
        AlignmentGeometry::Absolute(alignment) => alignment,
        AlignmentGeometry::Directional(directional) => directional.resolve(text_direction.is_ltr()),
    }
}

/// Overlaps its children, sizing itself to the largest non-positioned child and
/// aligning each by `alignment`.
///
/// Defaults are `alignment = Alignment::TOP_LEFT` and `fit = StackFit::Loose`.
/// Wrap a child in [`Positioned`](crate::Positioned) to place it at explicit
/// edges instead of being aligned.
///
/// Generic over `C: ViewSeq` — `stack!`-style tuples (via the shared
/// `column!`/`row!` macros) or a dynamic `Vec<BoxedView>`.
#[derive(Clone)]
pub struct Stack<C = Vec<BoxedView>> {
    alignment: AlignmentGeometry,
    fit: StackFit,
    children: C,
}

impl<C> Stack<C> {
    /// A stack of the given children with the default alignment and fit.
    pub fn new(children: C) -> Self {
        Self {
            alignment: Alignment::TOP_LEFT.into(),
            fit: StackFit::Loose,
            children,
        }
    }

    /// How non-positioned children are aligned within the stack — an
    /// [`Alignment`] converts to an absolute geometry, an
    /// [`AlignmentDirectional`](flui_painting::AlignmentDirectional) to a
    /// direction-sensitive one resolved against the ambient [`Directionality`].
    #[must_use]
    pub fn alignment(mut self, alignment: impl Into<AlignmentGeometry>) -> Self {
        self.alignment = alignment.into();
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

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let text_direction = resolve_text_direction(ctx);
        RenderStack::new()
            .with_alignment(resolve_alignment_geometry(self.alignment, text_direction))
            .with_text_direction(text_direction)
            .with_fit(self.fit)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let text_direction = resolve_text_direction(ctx);
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |=
            render_object.set_alignment(resolve_alignment_geometry(self.alignment, text_direction));
        impact |= render_object.set_text_direction(text_direction);
        impact |= render_object.set_fit(self.fit);
        impact
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
/// All children are still laid out, so the stack's size is
/// resolved exactly like [`Stack`]. Only the selected child participates in
/// paint, hit testing, semantics, and baseline reporting. `index = None`
/// displays no child.
#[derive(Clone)]
pub struct IndexedStack<C = Vec<BoxedView>> {
    alignment: AlignmentGeometry,
    fit: StackFit,
    index: Option<usize>,
    children: C,
}

impl<C> IndexedStack<C> {
    /// An indexed stack with the default alignment and fit, and `index = 0`.
    pub fn new(children: C) -> Self {
        Self {
            alignment: Alignment::TOP_LEFT.into(),
            fit: StackFit::Loose,
            index: Some(0),
            children,
        }
    }

    /// How non-positioned children are aligned within the stack — an
    /// [`Alignment`] converts to an absolute geometry, an
    /// [`AlignmentDirectional`](flui_painting::AlignmentDirectional) to a
    /// direction-sensitive one resolved against the ambient [`Directionality`].
    #[must_use]
    pub fn alignment(mut self, alignment: impl Into<AlignmentGeometry>) -> Self {
        self.alignment = alignment.into();
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

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let text_direction = resolve_text_direction(ctx);
        RenderIndexedStack::new()
            .with_alignment(resolve_alignment_geometry(self.alignment, text_direction))
            .with_text_direction(text_direction)
            .with_fit(self.fit)
            .with_index(self.index)
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let text_direction = resolve_text_direction(ctx);
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |=
            render_object.set_alignment(resolve_alignment_geometry(self.alignment, text_direction));
        impact |= render_object.set_text_direction(text_direction);
        impact |= render_object.set_fit(self.fit);
        impact |= render_object.set_index(self.index);
        impact
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn flui_view::View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}

generic_render_view_element!(IndexedStack);
