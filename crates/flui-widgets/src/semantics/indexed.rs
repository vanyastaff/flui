//! `IndexedSemantics`: a zero-based sibling index on its child's semantics node.

use flui_foundation::CreationSite;
use flui_objects::RenderIndexedSemantics;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Annotates its child's semantics node with a zero-based index among its
/// siblings.
///
/// This is the "12" a screen reader announces in "item 12 of 100"; the "100"
/// comes from the enclosing scrollable's own child count, not from here.
///
/// Lazy sliver delegates do not wrap their materialised items in one — see
/// flui-rendering's `## Mapping decisions` — so this is for content you
/// index yourself: a hand-built list, a grid of cards, anything a lazy sliver
/// does not own.
///
/// The index is zero-based, as AccessKit's `position_in_set` is; the platform
/// adapters add one when they hand it to a screen reader.
#[derive(Clone, Debug)]
pub struct IndexedSemantics {
    index: usize,
    child: Child,
    site: CreationSite,
}

impl Default for IndexedSemantics {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(0, CreationSite::here())
    }
}

impl IndexedSemantics {
    fn at(index: usize, site: CreationSite) -> Self {
        Self {
            index,
            child: Child::empty(),
            site,
        }
    }

    /// Creates an indexed-semantics widget for a zero-based `index`.
    #[must_use]
    #[track_caller]
    pub fn new(index: usize) -> Self {
        Self::at(index, CreationSite::here())
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for IndexedSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIndexedSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIndexedSemantics::new(self.index)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_index(self.index)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IndexedSemantics, site = site);
