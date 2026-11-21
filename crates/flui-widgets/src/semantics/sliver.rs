//! `SliverSemantics`: semantics annotation that keeps the sliver protocol.

#[cfg(doc)]
use super::Semantics;
use flui_foundation::CreationSite;
use flui_objects::RenderSliverSemanticsAnnotations;
use flui_rendering::protocol::SliverProtocol;
use flui_semantics::SemanticsConfiguration;
use flui_view::{Child, IntoView, RenderObjectContext, RenderView, impl_render_view};

/// Annotates an entire sliver subtree while retaining the sliver protocol.
/// Use [`Semantics`] for individual box children. A supplied configuration may
/// carry ordinary typed actions, collection metadata, relations or sort keys.
#[derive(Clone, Debug)]
pub struct SliverSemantics {
    configuration: SemanticsConfiguration,
    exclude_descendants: bool,
    child: Child,
    site: CreationSite,
}

impl Default for SliverSemantics {
    fn default() -> Self {
        // A trait fn cannot be `#[track_caller]`: the site is this line.
        Self::at(SemanticsConfiguration::default(), CreationSite::here())
    }
}

impl SliverSemantics {
    fn at(configuration: SemanticsConfiguration, site: CreationSite) -> Self {
        Self {
            configuration,
            exclude_descendants: false,
            child: Child::empty(),
            site,
        }
    }

    /// Creates an empty sliver annotation.
    #[track_caller]
    pub fn new() -> Self {
        Self::at(SemanticsConfiguration::default(), CreationSite::here())
    }

    /// Creates an annotation from a complete configuration.
    #[track_caller]
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self::at(configuration, CreationSite::here())
    }
    /// Supplies the accessible label.
    #[must_use]
    pub fn label(mut self, label: impl AsRef<str>) -> Self {
        self.configuration.set_label(label);
        self
    }

    /// Whether this sliver forms a semantics boundary.
    #[must_use]
    pub fn container(mut self, container: bool) -> Self {
        self.configuration.set_semantics_boundary(container);
        self
    }

    /// Keeps descendant contributions in explicit accessible nodes.
    #[must_use]
    pub fn explicit_child_nodes(mut self, explicit: bool) -> Self {
        self.configuration.set_explicit_child_nodes(explicit);
        self
    }

    /// Drops descendant accessibility without changing layout or painting.
    #[must_use]
    pub fn exclude_semantics(mut self, exclude: bool) -> Self {
        self.exclude_descendants = exclude;
        self
    }

    /// Supplies the sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverSemantics {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverSemanticsAnnotations;

    fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
        let mut object =
            RenderSliverSemanticsAnnotations::from_configuration(self.configuration.clone());
        let _ = object.set_exclude_descendants(self.exclude_descendants);
        object
    }

    fn update_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        object.set_configuration(self.configuration.clone())
            | object.set_exclude_descendants(self.exclude_descendants)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SliverSemantics, site = site);
