//! Semantics proxy render objects.
//!
//! These mirror Flutter's `RenderSemanticsAnnotations`,
//! `RenderMergeSemantics`, and `RenderExcludeSemantics` from
//! `rendering/proxy_box.dart`. Layout, paint, and hit-testing are transparent
//! single-child proxy behavior; only the semantics hooks differ.

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    semantics::{SemanticsConfiguration, SemanticsProperties},
    traits::RenderBox,
};

/// A render object that annotates its subtree with semantics properties.
#[derive(Debug, Clone)]
pub struct RenderSemanticsAnnotations {
    configuration: SemanticsConfiguration,
    container: bool,
    explicit_child_nodes: bool,
    exclude_semantics: bool,
    block_user_actions: bool,
    has_child: bool,
}

impl RenderSemanticsAnnotations {
    /// Creates a semantics-annotations render object from semantic properties.
    pub fn new(properties: SemanticsProperties) -> Self {
        Self::from_configuration(SemanticsConfiguration::from_properties(&properties))
    }

    /// Creates a semantics-annotations render object from a ready
    /// configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            container: false,
            explicit_child_nodes: false,
            exclude_semantics: false,
            block_user_actions: false,
            has_child: false,
        }
    }

    /// Returns the semantic properties configuration.
    pub fn configuration(&self) -> &SemanticsConfiguration {
        &self.configuration
    }

    /// Replaces the semantic properties configuration.
    pub fn set_configuration(&mut self, configuration: SemanticsConfiguration) {
        self.configuration = configuration;
    }

    /// Returns whether this object introduces a semantics boundary.
    #[inline]
    pub fn container(&self) -> bool {
        self.container
    }

    /// Sets whether this object introduces a semantics boundary.
    pub fn set_container(&mut self, container: bool) -> bool {
        if self.container == container {
            return false;
        }
        self.container = container;
        true
    }

    /// Chainable form of [`Self::set_container`].
    #[must_use]
    pub fn with_container(mut self, container: bool) -> Self {
        self.container = container;
        self
    }

    /// Returns whether descendants must create explicit semantics nodes.
    #[inline]
    pub fn explicit_child_nodes(&self) -> bool {
        self.explicit_child_nodes
    }

    /// Sets whether descendants must create explicit semantics nodes.
    pub fn set_explicit_child_nodes(&mut self, explicit_child_nodes: bool) -> bool {
        if self.explicit_child_nodes == explicit_child_nodes {
            return false;
        }
        self.explicit_child_nodes = explicit_child_nodes;
        true
    }

    /// Chainable form of [`Self::set_explicit_child_nodes`].
    #[must_use]
    pub fn with_explicit_child_nodes(mut self, explicit_child_nodes: bool) -> Self {
        self.explicit_child_nodes = explicit_child_nodes;
        self
    }

    /// Returns whether descendant semantics are ignored.
    #[inline]
    pub fn exclude_semantics(&self) -> bool {
        self.exclude_semantics
    }

    /// Sets whether descendant semantics are ignored.
    pub fn set_exclude_semantics(&mut self, exclude_semantics: bool) -> bool {
        if self.exclude_semantics == exclude_semantics {
            return false;
        }
        self.exclude_semantics = exclude_semantics;
        true
    }

    /// Chainable form of [`Self::set_exclude_semantics`].
    #[must_use]
    pub fn with_exclude_semantics(mut self, exclude_semantics: bool) -> Self {
        self.exclude_semantics = exclude_semantics;
        self
    }

    /// Returns whether user-action semantics are blocked for descendants.
    #[inline]
    pub fn block_user_actions(&self) -> bool {
        self.block_user_actions
    }

    /// Sets whether user-action semantics are blocked for descendants.
    pub fn set_block_user_actions(&mut self, block_user_actions: bool) -> bool {
        if self.block_user_actions == block_user_actions {
            return false;
        }
        self.block_user_actions = block_user_actions;
        true
    }

    /// Chainable form of [`Self::set_block_user_actions`].
    #[must_use]
    pub fn with_block_user_actions(mut self, block_user_actions: bool) -> Self {
        self.block_user_actions = block_user_actions;
        self
    }
}

impl Default for RenderSemanticsAnnotations {
    fn default() -> Self {
        Self::new(SemanticsProperties::new())
    }
}

impl flui_foundation::Diagnosticable for RenderSemanticsAnnotations {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("container", self.container, "container");
        builder.add_flag(
            "explicit_child_nodes",
            self.explicit_child_nodes,
            "explicit child nodes",
        );
        builder.add_flag(
            "exclude_semantics",
            self.exclude_semantics,
            "exclude semantics",
        );
        builder.add_flag(
            "block_user_actions",
            self.block_user_actions,
            "block user actions",
        );
        builder.add_flag(
            "has_semantics",
            self.configuration.has_content(),
            "has semantics",
        );
    }
}

impl RenderBox for RenderSemanticsAnnotations {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        *config = self.configuration.clone();
        config.set_semantics_boundary(self.container);
        config.set_explicit_children_are_traversal_groups(self.explicit_child_nodes);
        config.set_blocks_user_actions(self.block_user_actions);
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.exclude_semantics
    }
}

/// A render object that merges all descendant semantics into one node.
#[derive(Debug, Clone, Default)]
pub struct RenderMergeSemantics {
    has_child: bool,
}

impl RenderBox for RenderMergeSemantics {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn describe_semantics_configuration(&self, config: &mut SemanticsConfiguration) {
        config.set_semantics_boundary(true);
        config.set_merging_semantics_of_descendants(true);
    }
}

impl flui_foundation::Diagnosticable for RenderMergeSemantics {}

/// A render object that drops its descendant semantics while leaving layout,
/// paint, and hit testing unchanged.
#[derive(Debug, Clone)]
pub struct RenderExcludeSemantics {
    excluding: bool,
    has_child: bool,
}

impl RenderExcludeSemantics {
    /// Creates an exclude-semantics render object.
    pub const fn new(excluding: bool) -> Self {
        Self {
            excluding,
            has_child: false,
        }
    }

    /// Returns whether descendant semantics are excluded.
    #[inline]
    pub fn excluding(&self) -> bool {
        self.excluding
    }

    /// Sets whether descendant semantics are excluded.
    pub fn set_excluding(&mut self, excluding: bool) -> bool {
        if self.excluding == excluding {
            return false;
        }
        self.excluding = excluding;
        true
    }
}

impl Default for RenderExcludeSemantics {
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderExcludeSemantics {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("excluding", self.excluding, "excluding");
    }
}

impl RenderBox for RenderExcludeSemantics {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn excludes_semantics_subtree(&self) -> bool {
        self.excluding
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::semantics::{AttributedString, SemanticsFlag};

    #[test]
    fn annotations_describe_semantics_configuration() {
        let mut properties = SemanticsProperties::new()
            .with_label("Submit")
            .with_button(true)
            .with_enabled(true);
        properties.toggled = Some(false);

        let node = RenderSemanticsAnnotations::new(properties)
            .with_container(true)
            .with_explicit_child_nodes(true)
            .with_block_user_actions(true);

        let mut config = SemanticsConfiguration::new();
        node.describe_semantics_configuration(&mut config);

        assert!(config.is_semantics_boundary());
        assert!(config.explicit_children_are_traversal_groups());
        assert!(config.blocks_user_actions());
        assert_eq!(config.label().map(AttributedString::as_str), Some("Submit"));
        assert!(config.is_button());
        assert_eq!(config.is_enabled(), Some(true));
        assert_eq!(config.is_toggled(), Some(false));
    }

    #[test]
    fn merge_semantics_sets_boundary_and_descendant_merge() {
        let node = RenderMergeSemantics::default();
        let mut config = SemanticsConfiguration::new();
        node.describe_semantics_configuration(&mut config);

        assert!(config.is_semantics_boundary());
        assert!(config.is_merging_semantics_of_descendants());
    }

    #[test]
    fn exclude_semantics_only_controls_semantics_subtree() {
        let mut node = RenderExcludeSemantics::default();
        assert!(node.excluding());
        assert!(node.excludes_semantics_subtree());

        assert!(node.set_excluding(false));
        assert!(!node.excluding());
        assert!(!node.excludes_semantics_subtree());
    }

    #[test]
    fn properties_copy_all_supported_state_flags() {
        let mut properties = SemanticsProperties::new();
        properties.mixed = Some(true);
        properties.toggled = Some(false);
        properties.expanded = Some(false);
        properties.read_only = Some(true);
        properties.scopes_route = Some(true);
        properties.names_route = Some(true);
        properties.in_mutually_exclusive_group = Some(true);
        properties.increased_value = Some("More".into());
        properties.decreased_value = Some("Less".into());

        let config = SemanticsConfiguration::from_properties(&properties);

        assert!(config.is_mixed());
        assert_eq!(config.is_toggled(), Some(false));
        assert!(!config.is_expanded());
        assert!(config.has_flag(SemanticsFlag::HasExpandedState));
        assert!(config.is_read_only());
        assert!(config.scopes_route());
        assert!(config.names_route());
        assert!(config.is_in_mutually_exclusive_group());
        assert_eq!(
            config.increased_value().map(AttributedString::as_str),
            Some("More"),
        );
        assert_eq!(
            config.decreased_value().map(AttributedString::as_str),
            Some("Less"),
        );
    }
}
