//! Accessibility semantics widgets.
//!
//! These widgets are thin `RenderView` wrappers over the semantics proxy render
//! objects in `flui-objects`, matching Flutter's `Semantics`,
//! `MergeSemantics`, and `ExcludeSemantics` split.

use flui_objects::{RenderExcludeSemantics, RenderMergeSemantics, RenderSemanticsAnnotations};
use flui_rendering::{
    protocol::BoxProtocol,
    semantics::{SemanticsConfiguration, SemanticsProperties, SemanticsRole, TextDirection},
};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

#[derive(Clone, Copy, Debug, Default)]
struct SemanticsOptions {
    bits: u8,
}

impl SemanticsOptions {
    const CONTAINER: u8 = 1 << 0;
    const EXPLICIT_CHILD_NODES: u8 = 1 << 1;
    const EXCLUDE_DESCENDANTS: u8 = 1 << 2;
    const BLOCK_USER_ACTIONS: u8 = 1 << 3;

    #[inline]
    const fn contains(self, flag: u8) -> bool {
        (self.bits & flag) != 0
    }

    #[inline]
    fn set(&mut self, flag: u8, value: bool) {
        if value {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
    }
}

/// Annotates a subtree with accessibility semantics.
#[derive(Clone, Debug)]
// PORT-CHECK-OK-SP3: widget view type; `flui_rendering::pipeline::Semantics` is a typestate phase marker, not the accessibility widget/config object
pub struct Semantics {
    configuration: SemanticsConfiguration,
    options: SemanticsOptions,
    child: Child,
}

impl Default for Semantics {
    fn default() -> Self {
        Self {
            configuration: SemanticsConfiguration::new(),
            options: SemanticsOptions::default(),
            child: Child::empty(),
        }
    }
}

impl Semantics {
    /// Creates an empty semantics annotation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a semantics annotation from the shared properties bag.
    pub fn from_properties(properties: &SemanticsProperties) -> Self {
        Self {
            configuration: SemanticsConfiguration::from_properties(properties),
            ..Self::default()
        }
    }

    /// Creates a semantics annotation from a ready configuration.
    pub fn from_configuration(configuration: SemanticsConfiguration) -> Self {
        Self {
            configuration,
            ..Self::default()
        }
    }

    /// Set whether this widget introduces a new semantics node.
    #[must_use]
    pub fn container(mut self, container: bool) -> Self {
        self.options.set(SemanticsOptions::CONTAINER, container);
        self
    }

    /// Set whether descendants must create explicit semantics nodes.
    #[must_use]
    pub fn explicit_child_nodes(mut self, explicit_child_nodes: bool) -> Self {
        self.options
            .set(SemanticsOptions::EXPLICIT_CHILD_NODES, explicit_child_nodes);
        self
    }

    /// Set whether descendant semantics are ignored.
    #[must_use]
    pub fn exclude_semantics(mut self, exclude_semantics: bool) -> Self {
        self.options
            .set(SemanticsOptions::EXCLUDE_DESCENDANTS, exclude_semantics);
        self
    }

    /// Set whether user-action semantics are blocked for descendants.
    #[must_use]
    pub fn block_user_actions(mut self, block_user_actions: bool) -> Self {
        self.options
            .set(SemanticsOptions::BLOCK_USER_ACTIONS, block_user_actions);
        self
    }

    /// Set the accessible label.
    #[must_use]
    pub fn label(mut self, label: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_label(label);
        self
    }

    /// Set the accessible value.
    #[must_use]
    pub fn value(mut self, value: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_value(value);
        self
    }

    /// Set the accessible hint.
    #[must_use]
    pub fn hint(mut self, hint: impl Into<flui_rendering::semantics::AttributedString>) -> Self {
        self.configuration.set_hint(hint);
        self
    }

    /// Set whether this node is enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.configuration.set_enabled(Some(enabled));
        self
    }

    /// Set whether this node has checked state and is checked.
    #[must_use]
    pub fn checked(mut self, checked: bool) -> Self {
        self.configuration.set_checked(Some(checked));
        self
    }

    /// Set whether this node is in a mixed checkbox state.
    #[must_use]
    pub fn mixed(mut self, mixed: bool) -> Self {
        self.configuration.set_mixed(mixed);
        self
    }

    /// Set whether this node has toggled state and is toggled.
    #[must_use]
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.configuration.set_toggled(Some(toggled));
        self
    }

    /// Set whether this node is selected.
    #[must_use]
    pub fn selected(mut self, selected: bool) -> Self {
        self.configuration.set_selected(selected);
        self
    }

    /// Set whether this node has expanded state and is expanded.
    #[must_use]
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.configuration.set_expanded(expanded);
        self
    }

    /// Set whether this node is a button.
    #[must_use]
    pub fn button(mut self, button: bool) -> Self {
        self.configuration.set_button(button);
        self
    }

    /// Set whether this node is a link.
    #[must_use]
    pub fn link(mut self, link: bool) -> Self {
        self.configuration.set_link(link);
        self
    }

    /// Set whether this node is a slider.
    #[must_use]
    pub fn slider(mut self, slider: bool) -> Self {
        self.configuration.set_slider(slider);
        self
    }

    /// Set whether this node is a header.
    #[must_use]
    pub fn header(mut self, header: bool) -> Self {
        self.configuration.set_header(header);
        self
    }

    /// Set whether this node is an image.
    #[must_use]
    pub fn image(mut self, image: bool) -> Self {
        self.configuration.set_image(image);
        self
    }

    /// Set whether this node is a text field.
    #[must_use]
    pub fn text_field(mut self, text_field: bool) -> Self {
        self.configuration.set_text_field(text_field);
        self
    }

    /// Set whether this node is read-only.
    #[must_use]
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.configuration.set_read_only(read_only);
        self
    }

    /// Set whether this node is focusable.
    #[must_use]
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.configuration.set_focusable(focusable);
        self
    }

    /// Set whether this node is focused.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.configuration.set_focused(focused);
        self
    }

    /// Set whether this node is hidden from accessibility.
    #[must_use]
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.configuration.set_hidden(hidden);
        self
    }

    /// Set whether this node is obscured, such as a password field.
    #[must_use]
    pub fn obscured(mut self, obscured: bool) -> Self {
        self.configuration.set_obscured(obscured);
        self
    }

    /// Set whether this node is multiline.
    #[must_use]
    pub fn multiline(mut self, multiline: bool) -> Self {
        self.configuration.set_multiline(multiline);
        self
    }

    /// Set whether this node scopes a route.
    #[must_use]
    pub fn scopes_route(mut self, scopes_route: bool) -> Self {
        self.configuration.set_scopes_route(scopes_route);
        self
    }

    /// Set whether this node names a route.
    #[must_use]
    pub fn names_route(mut self, names_route: bool) -> Self {
        self.configuration.set_names_route(names_route);
        self
    }

    /// Set whether this node is a live region.
    #[must_use]
    pub fn live_region(mut self, live_region: bool) -> Self {
        self.configuration.set_live_region(live_region);
        self
    }

    /// Set the text direction used when merging text semantics.
    #[must_use]
    pub fn text_direction(mut self, text_direction: TextDirection) -> Self {
        self.configuration.set_text_direction(text_direction);
        self
    }

    /// Set the platform semantics role.
    #[must_use]
    pub fn role(mut self, role: SemanticsRole) -> Self {
        self.configuration.set_role(role);
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Semantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSemanticsAnnotations;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSemanticsAnnotations::from_configuration(self.configuration.clone())
            .with_container(self.options.contains(SemanticsOptions::CONTAINER))
            .with_explicit_child_nodes(
                self.options
                    .contains(SemanticsOptions::EXPLICIT_CHILD_NODES),
            )
            .with_exclude_semantics(self.options.contains(SemanticsOptions::EXCLUDE_DESCENDANTS))
            .with_block_user_actions(self.options.contains(SemanticsOptions::BLOCK_USER_ACTIONS))
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_configuration(self.configuration.clone());
        render_object.set_container(self.options.contains(SemanticsOptions::CONTAINER));
        render_object.set_explicit_child_nodes(
            self.options
                .contains(SemanticsOptions::EXPLICIT_CHILD_NODES),
        );
        render_object
            .set_exclude_semantics(self.options.contains(SemanticsOptions::EXCLUDE_DESCENDANTS));
        render_object
            .set_block_user_actions(self.options.contains(SemanticsOptions::BLOCK_USER_ACTIONS));
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(Semantics);

/// Merges the semantics of all descendants into a single node.
#[derive(Clone, Debug, Default)]
pub struct MergeSemantics {
    child: Child,
}

impl MergeSemantics {
    /// Creates a merge-semantics widget.
    pub fn new() -> Self {
        Self::default()
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

    fn create_render_object(&self) -> Self::RenderObject {
        RenderMergeSemantics::default()
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(MergeSemantics);

/// Drops descendant semantics while keeping layout, paint, and hit testing.
#[derive(Clone, Debug)]
pub struct ExcludeSemantics {
    excluding: bool,
    child: Child,
}

impl Default for ExcludeSemantics {
    fn default() -> Self {
        Self {
            excluding: true,
            child: Child::empty(),
        }
    }
}

impl ExcludeSemantics {
    /// Creates an exclude-semantics widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether descendants are excluded from semantics.
    #[must_use]
    pub fn excluding(mut self, excluding: bool) -> Self {
        self.excluding = excluding;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ExcludeSemantics {
    type Protocol = BoxProtocol;
    type RenderObject = RenderExcludeSemantics;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderExcludeSemantics::new(self.excluding)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_excluding(self.excluding);
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(ExcludeSemantics);

#[cfg(test)]
mod tests {
    use flui_rendering::RenderObject;
    use flui_rendering::semantics::{AttributedString, SemanticsRole};

    use super::*;

    // ------------------------------------------------------------------
    // Semantics -- builder methods reach the built SemanticsConfiguration.
    // ------------------------------------------------------------------

    #[test]
    fn semantics_builder_methods_reach_the_configuration() {
        let widget = Semantics::new()
            .label("a label")
            .value("a value")
            .hint("a hint")
            .enabled(true)
            .checked(true)
            .mixed(true)
            .toggled(true)
            .selected(true)
            .expanded(true)
            .button(true)
            .link(true)
            .slider(true)
            .header(true)
            .image(true)
            .text_field(true)
            .read_only(true)
            .focusable(true)
            .focused(true)
            .hidden(true)
            .obscured(true)
            .multiline(true)
            .scopes_route(true)
            .names_route(true)
            .live_region(true)
            .text_direction(TextDirection::Rtl)
            .role(SemanticsRole::Dialog);

        let render_object = widget.create_render_object();
        let config = render_object.configuration();

        assert_eq!(
            config.label().map(AttributedString::as_str),
            Some("a label")
        );
        assert_eq!(
            config.value().map(AttributedString::as_str),
            Some("a value")
        );
        assert_eq!(config.hint().map(AttributedString::as_str), Some("a hint"));
        assert_eq!(config.is_enabled(), Some(true));
        assert_eq!(config.is_checked(), Some(true));
        assert!(config.is_mixed());
        assert_eq!(config.is_toggled(), Some(true));
        assert!(config.is_selected());
        assert!(config.is_expanded());
        assert!(config.is_button());
        assert!(config.is_link());
        assert!(config.is_slider());
        assert!(config.is_header());
        assert!(config.is_image());
        assert!(config.is_text_field());
        assert!(config.is_read_only());
        assert!(config.is_focusable());
        assert!(config.is_focused());
        assert!(config.is_hidden());
        assert!(config.is_obscured());
        assert!(config.is_multiline());
        assert!(config.scopes_route());
        assert!(config.names_route());
        assert!(config.is_live_region());
        assert_eq!(config.text_direction(), Some(TextDirection::Rtl));
        assert_eq!(config.role(), SemanticsRole::Dialog);
    }

    #[test]
    fn semantics_options_reach_the_render_object() {
        let widget = Semantics::new()
            .container(true)
            .explicit_child_nodes(true)
            .exclude_semantics(true)
            .block_user_actions(true);

        let render_object = widget.create_render_object();

        assert!(render_object.container());
        assert!(render_object.explicit_child_nodes());
        assert!(render_object.exclude_semantics());
        assert!(render_object.block_user_actions());
    }

    #[test]
    fn semantics_defaults_are_all_off() {
        let render_object = Semantics::new().create_render_object();

        assert!(!render_object.container());
        assert!(!render_object.explicit_child_nodes());
        assert!(!render_object.exclude_semantics());
        assert!(!render_object.block_user_actions());
        assert_eq!(render_object.configuration().is_enabled(), None);
    }

    #[test]
    fn semantics_update_render_object_reapplies_configuration_and_options() {
        let mut render_object = Semantics::new().create_render_object();
        assert!(!render_object.container());

        let updated = Semantics::new()
            .label("updated")
            .container(true)
            .button(true);
        updated.update_render_object(&mut render_object);

        assert!(render_object.container());
        assert!(render_object.configuration().is_button());
        assert_eq!(
            render_object
                .configuration()
                .label()
                .map(AttributedString::as_str),
            Some("updated")
        );
    }

    #[test]
    fn semantics_from_properties_maps_the_shared_properties_bag() {
        use flui_rendering::semantics::SemanticsProperties;

        let properties = SemanticsProperties::new()
            .with_label("from properties")
            .with_button(true);
        let render_object = Semantics::from_properties(&properties).create_render_object();

        assert_eq!(
            render_object
                .configuration()
                .label()
                .map(AttributedString::as_str),
            Some("from properties")
        );
        assert!(render_object.configuration().is_button());
    }

    #[test]
    fn semantics_from_configuration_uses_the_given_configuration_directly() {
        let mut config = SemanticsConfiguration::new();
        config.set_selected(true);

        let render_object = Semantics::from_configuration(config).create_render_object();
        assert!(render_object.configuration().is_selected());
    }

    #[test]
    fn semantics_has_children_reflects_whether_a_child_was_set() {
        assert!(!Semantics::new().has_children());
        assert!(
            Semantics::new()
                .child(crate::SizedBox::new(10.0, 10.0))
                .has_children()
        );
    }

    // ------------------------------------------------------------------
    // MergeSemantics
    // ------------------------------------------------------------------

    #[test]
    fn merge_semantics_declares_a_boundary_that_merges_descendants() {
        let render_object = MergeSemantics::new().create_render_object();
        let mut config = SemanticsConfiguration::new();
        render_object.describe_semantics_configuration(&mut config);

        assert!(config.is_semantics_boundary());
        assert!(config.is_merging_semantics_of_descendants());
    }

    #[test]
    fn merge_semantics_has_children_reflects_whether_a_child_was_set() {
        assert!(!MergeSemantics::new().has_children());
        assert!(
            MergeSemantics::new()
                .child(crate::SizedBox::new(10.0, 10.0))
                .has_children()
        );
    }

    // ------------------------------------------------------------------
    // ExcludeSemantics
    // ------------------------------------------------------------------

    #[test]
    fn exclude_semantics_defaults_to_excluding_and_toggles_off() {
        let default_render_object = ExcludeSemantics::new().create_render_object();
        assert!(default_render_object.excludes_semantics_subtree());

        let disabled = ExcludeSemantics::new()
            .excluding(false)
            .create_render_object();
        assert!(!disabled.excludes_semantics_subtree());
    }

    #[test]
    fn exclude_semantics_update_render_object_reapplies_excluding() {
        let mut render_object = ExcludeSemantics::new().create_render_object();
        assert!(render_object.excludes_semantics_subtree());

        ExcludeSemantics::new()
            .excluding(false)
            .update_render_object(&mut render_object);
        assert!(!render_object.excludes_semantics_subtree());
    }

    #[test]
    fn exclude_semantics_has_children_reflects_whether_a_child_was_set() {
        assert!(!ExcludeSemantics::new().has_children());
        assert!(
            ExcludeSemantics::new()
                .child(crate::SizedBox::new(10.0, 10.0))
                .has_children()
        );
    }
}
