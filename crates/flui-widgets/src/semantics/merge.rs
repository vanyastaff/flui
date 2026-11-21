//! `MergeSemantics`: merges the semantics of all descendants into one node.

#[cfg(doc)]
use super::Semantics;
use flui_foundation::CreationSite;
use flui_objects::RenderMergeSemantics;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Merges the semantics of all descendants into a single node.
#[derive(Clone, Debug)]
pub struct MergeSemantics {
    identifier: Option<std::sync::Arc<str>>,
    child: Child,
    site: CreationSite,
}

impl Default for MergeSemantics {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(CreationSite::here())
    }
}

impl MergeSemantics {
    fn at(site: CreationSite) -> Self {
        Self {
            identifier: None,
            child: Child::empty(),
            site,
        }
    }

    /// Creates a merge-semantics widget.
    #[track_caller]
    pub fn new() -> Self {
        Self::at(CreationSite::here())
    }

    /// Set the merged node's stable identifier for tests and automation
    /// (ADR-0240). The merge forms the node, so this, not a descendant's
    /// [`Semantics::identifier`], names it: a descendant's identifier is
    /// dropped by the merge. A merged node that publishes nothing (no role,
    /// label or text) is lifted into its parent with its identifier, as
    /// [`Semantics::identifier`] describes.
    #[must_use]
    pub fn identifier(mut self, identifier: impl Into<std::sync::Arc<str>>) -> Self {
        self.identifier = Some(identifier.into());
        self
    }

    /// [`identifier`](Self::identifier) when `identifier` is `Some`.
    #[must_use]
    pub(crate) fn identifier_if(mut self, identifier: Option<&std::sync::Arc<str>>) -> Self {
        self.identifier = identifier.cloned();
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for MergeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderMergeSemantics;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut object = RenderMergeSemantics::default();
        let _ = object.set_identifier(self.identifier.clone());
        object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_identifier(self.identifier.clone())
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(MergeSemantics, site = site);
