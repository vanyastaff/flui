//! Semantics configuration for render objects.
//!
//! This module provides the configuration that render objects use to describe
//! their semantic properties for accessibility.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use smol_str::SmolStr;

use crate::action::{SemanticsAction, SemanticsActionHandler};
use crate::flags::{SemanticsFlag, SemanticsFlags};
use crate::properties::{
    AttributedString, CustomSemanticsAction, SemanticsHintOverrides, SemanticsProperties,
    SemanticsSortKey, SemanticsTag, TextDirection,
};

// ============================================================================
// SemanticsConfiguration
// ============================================================================

/// Configuration describing the semantic properties of a render object.
///
/// This is the primary way render objects communicate their accessibility
/// information to the semantics system. Each RenderObject can override
/// `describeSemanticsConfiguration` to fill out this configuration.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsConfiguration` class.
///
/// # Example
///
/// ```ignore
/// let mut config = SemanticsConfiguration::new();
/// config.set_label("Submit button");
/// config.set_button(true);
/// config.add_action(SemanticsAction::Tap, Arc::new(|_, _| {
///     println!("Button tapped!");
/// }));
/// ```
#[derive(Default, Clone)]
pub struct SemanticsConfiguration {
    /// Whether the semantic information in this configuration is complete.
    ///
    /// If true, children semantics won't be merged into this node.
    is_semantics_boundary: bool,

    /// Whether this configuration blocks semantics from descendant nodes.
    blocks_user_actions: bool,

    /// Whether this is explicitly tagged as a semantic boundary.
    explicit_children_are_traversal_groups: bool,

    /// Flags describing boolean properties.
    flags: SemanticsFlags,

    /// Actions that can be performed on this node.
    actions: FxHashMap<SemanticsAction, SemanticsActionHandler>,

    /// The label describing this node.
    label: Option<AttributedString>,

    /// The current value of this node.
    value: Option<AttributedString>,

    /// The value when increased.
    increased_value: Option<AttributedString>,

    /// The value when decreased.
    decreased_value: Option<AttributedString>,

    /// A hint about what will happen when the node is activated.
    hint: Option<AttributedString>,

    /// The tooltip for this node.
    tooltip: Option<SmolStr>,

    /// The text direction.
    text_direction: Option<TextDirection>,

    /// Custom semantic actions.
    custom_actions: SmallVec<[CustomSemanticsAction; 2]>,

    /// Tags for this node.
    tags: SmallVec<[SemanticsTag; 2]>,

    /// Sort key for ordering.
    sort_key: Option<SemanticsSortKey>,

    /// Hint overrides.
    hint_overrides: Option<SemanticsHintOverrides>,

    /// Scroll position.
    scroll_position: Option<f64>,

    /// Scroll extent maximum.
    scroll_extent_max: Option<f64>,

    /// Scroll extent minimum.
    scroll_extent_min: Option<f64>,

    /// The index of this node in a semantic list.
    index_in_parent: Option<i32>,

    /// The scroll index for this node.
    scroll_index: Option<i32>,

    /// The total scroll child count.
    scroll_child_count: Option<i32>,

    /// Platform view ID if this represents a platform view.
    platform_view_id: Option<i32>,

    /// Maximum character count for text field.
    max_value_length: Option<i32>,

    /// Current character count for text field.
    current_value_length: Option<i32>,

    /// Elevation for this node (z-order).
    elevation: f64,

    /// Thickness for this node.
    thickness: f64,
}

impl SemanticsConfiguration {
    /// Creates a new empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    // ========================================================================
    // Boundary Configuration
    // ========================================================================

    /// Returns whether this is a semantics boundary.
    #[inline]
    pub fn is_semantics_boundary(&self) -> bool {
        self.is_semantics_boundary
    }

    /// Sets whether this is a semantics boundary.
    ///
    /// When true, the semantic information from this node won't merge with
    /// its parent, creating a distinct semantics node.
    pub fn set_semantics_boundary(&mut self, value: bool) {
        self.is_semantics_boundary = value;
    }

    /// Returns whether this blocks user actions from descendants.
    #[inline]
    pub fn blocks_user_actions(&self) -> bool {
        self.blocks_user_actions
    }

    /// Sets whether this blocks user actions from descendants.
    pub fn set_blocks_user_actions(&mut self, value: bool) {
        self.blocks_user_actions = value;
    }

    /// Returns whether children are explicitly traversal groups.
    #[inline]
    pub fn explicit_children_are_traversal_groups(&self) -> bool {
        self.explicit_children_are_traversal_groups
    }

    /// Sets whether children are explicitly traversal groups.
    pub fn set_explicit_children_are_traversal_groups(&mut self, value: bool) {
        self.explicit_children_are_traversal_groups = value;
    }

    // ========================================================================
    // Flags
    // ========================================================================

    /// Returns whether a flag is set.
    #[inline]
    pub fn has_flag(&self, flag: SemanticsFlag) -> bool {
        self.flags.has(flag)
    }

    /// Sets a semantics flag.
    fn set_flag(&mut self, flag: SemanticsFlag, value: bool) {
        if value {
            self.flags.set(flag);
        } else {
            self.flags.clear(flag);
        }
    }

    /// Returns the flags.
    #[inline]
    pub fn flags(&self) -> &SemanticsFlags {
        &self.flags
    }

    // ========================================================================
    // Boolean Properties
    // ========================================================================

    /// Sets whether this is a button.
    pub fn set_button(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsButton, value);
    }

    /// Returns whether this is a button.
    #[inline]
    pub fn is_button(&self) -> bool {
        self.has_flag(SemanticsFlag::IsButton)
    }

    /// Sets whether this is a link.
    pub fn set_link(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsLink, value);
    }

    /// Returns whether this is a link.
    #[inline]
    pub fn is_link(&self) -> bool {
        self.has_flag(SemanticsFlag::IsLink)
    }

    /// Sets whether this is a text field.
    pub fn set_text_field(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsTextField, value);
    }

    /// Returns whether this is a text field.
    #[inline]
    pub fn is_text_field(&self) -> bool {
        self.has_flag(SemanticsFlag::IsTextField)
    }

    /// Sets whether this is a slider.
    pub fn set_slider(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsSlider, value);
    }

    /// Returns whether this is a slider.
    #[inline]
    pub fn is_slider(&self) -> bool {
        self.has_flag(SemanticsFlag::IsSlider)
    }

    /// Sets whether this is a header.
    pub fn set_header(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsHeader, value);
    }

    /// Returns whether this is a header.
    #[inline]
    pub fn is_header(&self) -> bool {
        self.has_flag(SemanticsFlag::IsHeader)
    }

    /// Sets whether this is an image.
    pub fn set_image(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsImage, value);
    }

    /// Returns whether this is an image.
    #[inline]
    pub fn is_image(&self) -> bool {
        self.has_flag(SemanticsFlag::IsImage)
    }

    /// Sets whether this is read-only.
    pub fn set_read_only(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsReadOnly, value);
    }

    /// Returns whether this is read-only.
    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.has_flag(SemanticsFlag::IsReadOnly)
    }

    /// Sets whether this is focusable.
    pub fn set_focusable(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsFocusable, value);
    }

    /// Returns whether this is focusable.
    #[inline]
    pub fn is_focusable(&self) -> bool {
        self.has_flag(SemanticsFlag::IsFocusable)
    }

    /// Sets whether this is focused.
    pub fn set_focused(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsFocused, value);
    }

    /// Returns whether this is focused.
    #[inline]
    pub fn is_focused(&self) -> bool {
        self.has_flag(SemanticsFlag::IsFocused)
    }

    /// Sets whether this is hidden.
    pub fn set_hidden(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsHidden, value);
    }

    /// Returns whether this is hidden.
    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.has_flag(SemanticsFlag::IsHidden)
    }

    /// Sets whether this is obscured (password field).
    pub fn set_obscured(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsObscured, value);
    }

    /// Returns whether this is obscured.
    #[inline]
    pub fn is_obscured(&self) -> bool {
        self.has_flag(SemanticsFlag::IsObscured)
    }

    /// Sets whether this is multiline.
    pub fn set_multiline(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsMultiline, value);
    }

    /// Returns whether this is multiline.
    #[inline]
    pub fn is_multiline(&self) -> bool {
        self.has_flag(SemanticsFlag::IsMultiline)
    }

    /// Sets whether this scopes a route.
    pub fn set_scopes_route(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::ScopesRoute, value);
    }

    /// Returns whether this scopes a route.
    #[inline]
    pub fn scopes_route(&self) -> bool {
        self.has_flag(SemanticsFlag::ScopesRoute)
    }

    /// Sets whether this names a route.
    pub fn set_names_route(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::NamesRoute, value);
    }

    /// Returns whether this names a route.
    #[inline]
    pub fn names_route(&self) -> bool {
        self.has_flag(SemanticsFlag::NamesRoute)
    }

    /// Sets whether this is a live region.
    pub fn set_live_region(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsLiveRegion, value);
    }

    /// Returns whether this is a live region.
    #[inline]
    pub fn is_live_region(&self) -> bool {
        self.has_flag(SemanticsFlag::IsLiveRegion)
    }

    // ========================================================================
    // Checked/Toggled State
    // ========================================================================

    /// Sets the checked state.
    ///
    /// Setting this also sets `HasCheckedState` flag.
    pub fn set_checked(&mut self, checked: Option<bool>) {
        if let Some(value) = checked {
            self.set_flag(SemanticsFlag::HasCheckedState, true);
            self.set_flag(SemanticsFlag::IsChecked, value);
        } else {
            self.set_flag(SemanticsFlag::HasCheckedState, false);
            self.set_flag(SemanticsFlag::IsChecked, false);
        }
    }

    /// Returns whether this is checked.
    pub fn is_checked(&self) -> Option<bool> {
        if self.has_flag(SemanticsFlag::HasCheckedState) {
            Some(self.has_flag(SemanticsFlag::IsChecked))
        } else {
            None
        }
    }

    /// Sets the mixed (indeterminate) state.
    pub fn set_mixed(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsCheckStateMixed, value);
    }

    /// Returns whether this is in mixed state.
    #[inline]
    pub fn is_mixed(&self) -> bool {
        self.has_flag(SemanticsFlag::IsCheckStateMixed)
    }

    /// Sets the toggled state.
    ///
    /// Setting this also sets `HasToggledState` flag.
    pub fn set_toggled(&mut self, toggled: Option<bool>) {
        if let Some(value) = toggled {
            self.set_flag(SemanticsFlag::HasToggledState, true);
            self.set_flag(SemanticsFlag::IsToggled, value);
        } else {
            self.set_flag(SemanticsFlag::HasToggledState, false);
            self.set_flag(SemanticsFlag::IsToggled, false);
        }
    }

    /// Returns whether this is toggled.
    pub fn is_toggled(&self) -> Option<bool> {
        if self.has_flag(SemanticsFlag::HasToggledState) {
            Some(self.has_flag(SemanticsFlag::IsToggled))
        } else {
            None
        }
    }

    // ========================================================================
    // Selected/Expanded/Enabled State
    // ========================================================================

    /// Sets whether this is selected.
    pub fn set_selected(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsSelected, value);
    }

    /// Returns whether this is selected.
    #[inline]
    pub fn is_selected(&self) -> bool {
        self.has_flag(SemanticsFlag::IsSelected)
    }

    /// Sets whether this is expanded.
    pub fn set_expanded(&mut self, value: bool) {
        self.set_flag(SemanticsFlag::IsExpanded, value);
    }

    /// Returns whether this is expanded.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.has_flag(SemanticsFlag::IsExpanded)
    }

    /// Sets the enabled state.
    ///
    /// Setting this also sets `HasEnabledState` flag.
    pub fn set_enabled(&mut self, enabled: Option<bool>) {
        if let Some(value) = enabled {
            self.set_flag(SemanticsFlag::HasEnabledState, true);
            self.set_flag(SemanticsFlag::IsEnabled, value);
        } else {
            self.set_flag(SemanticsFlag::HasEnabledState, false);
            self.set_flag(SemanticsFlag::IsEnabled, false);
        }
    }

    /// Returns whether this is enabled.
    pub fn is_enabled(&self) -> Option<bool> {
        if self.has_flag(SemanticsFlag::HasEnabledState) {
            Some(self.has_flag(SemanticsFlag::IsEnabled))
        } else {
            None
        }
    }

    // ========================================================================
    // Label, Value, Hint
    // ========================================================================

    /// Sets the label.
    pub fn set_label(&mut self, label: impl Into<AttributedString>) {
        self.label = Some(label.into());
    }

    /// Returns the label.
    #[inline]
    pub fn label(&self) -> Option<&AttributedString> {
        self.label.as_ref()
    }

    /// Sets the value.
    pub fn set_value(&mut self, value: impl Into<AttributedString>) {
        self.value = Some(value.into());
    }

    /// Returns the value.
    #[inline]
    pub fn value(&self) -> Option<&AttributedString> {
        self.value.as_ref()
    }

    /// Sets the increased value.
    pub fn set_increased_value(&mut self, value: impl Into<AttributedString>) {
        self.increased_value = Some(value.into());
    }

    /// Returns the increased value.
    #[inline]
    pub fn increased_value(&self) -> Option<&AttributedString> {
        self.increased_value.as_ref()
    }

    /// Sets the decreased value.
    pub fn set_decreased_value(&mut self, value: impl Into<AttributedString>) {
        self.decreased_value = Some(value.into());
    }

    /// Returns the decreased value.
    #[inline]
    pub fn decreased_value(&self) -> Option<&AttributedString> {
        self.decreased_value.as_ref()
    }

    /// Sets the hint.
    pub fn set_hint(&mut self, hint: impl Into<AttributedString>) {
        self.hint = Some(hint.into());
    }

    /// Returns the hint.
    #[inline]
    pub fn hint(&self) -> Option<&AttributedString> {
        self.hint.as_ref()
    }

    /// Sets the tooltip.
    pub fn set_tooltip(&mut self, tooltip: impl Into<SmolStr>) {
        self.tooltip = Some(tooltip.into());
    }

    /// Returns the tooltip.
    #[inline]
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    // ========================================================================
    // Text Direction
    // ========================================================================

    /// Sets the text direction.
    pub fn set_text_direction(&mut self, direction: TextDirection) {
        self.text_direction = Some(direction);
    }

    /// Returns the text direction.
    #[inline]
    pub fn text_direction(&self) -> Option<TextDirection> {
        self.text_direction
    }

    // ========================================================================
    // Actions
    // ========================================================================

    /// Adds an action handler.
    pub fn add_action(&mut self, action: SemanticsAction, handler: SemanticsActionHandler) {
        self.actions.insert(action, handler);
    }

    /// Removes an action.
    pub fn remove_action(&mut self, action: SemanticsAction) {
        self.actions.remove(&action);
    }

    /// Returns whether an action is available.
    #[inline]
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains_key(&action)
    }

    /// Returns the action handler for a given action.
    pub fn action_handler(&self, action: SemanticsAction) -> Option<&SemanticsActionHandler> {
        self.actions.get(&action)
    }

    /// Returns a bitmask of available actions.
    pub fn actions_as_bits(&self) -> u64 {
        self.actions
            .keys()
            .fold(0u64, |acc, action| acc | action.value())
    }

    // ========================================================================
    // Custom Actions
    // ========================================================================

    /// Adds a custom action.
    pub fn add_custom_action(&mut self, action: CustomSemanticsAction) {
        self.custom_actions.push(action);
    }

    /// Returns the custom actions.
    #[inline]
    pub fn custom_actions(&self) -> &[CustomSemanticsAction] {
        &self.custom_actions
    }

    // ========================================================================
    // Tags
    // ========================================================================

    /// Adds a tag.
    pub fn add_tag(&mut self, tag: SemanticsTag) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    /// Returns the tags.
    #[inline]
    pub fn tags(&self) -> &[SemanticsTag] {
        &self.tags
    }

    /// Returns whether a tag is present.
    pub fn has_tag(&self, tag: &SemanticsTag) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    // ========================================================================
    // Sort Key
    // ========================================================================

    /// Sets the sort key.
    pub fn set_sort_key(&mut self, key: SemanticsSortKey) {
        self.sort_key = Some(key);
    }

    /// Returns the sort key.
    #[inline]
    pub fn sort_key(&self) -> Option<&SemanticsSortKey> {
        self.sort_key.as_ref()
    }

    // ========================================================================
    // Hint Overrides
    // ========================================================================

    /// Sets hint overrides.
    pub fn set_hint_overrides(&mut self, overrides: SemanticsHintOverrides) {
        self.hint_overrides = Some(overrides);
    }

    /// Returns hint overrides.
    #[inline]
    pub fn hint_overrides(&self) -> Option<&SemanticsHintOverrides> {
        self.hint_overrides.as_ref()
    }

    // ========================================================================
    // Scroll Properties
    // ========================================================================

    /// Sets the scroll position.
    pub fn set_scroll_position(&mut self, position: f64) {
        self.scroll_position = Some(position);
    }

    /// Returns the scroll position.
    #[inline]
    pub fn scroll_position(&self) -> Option<f64> {
        self.scroll_position
    }

    /// Sets the scroll extent maximum.
    pub fn set_scroll_extent_max(&mut self, max: f64) {
        self.scroll_extent_max = Some(max);
    }

    /// Returns the scroll extent maximum.
    #[inline]
    pub fn scroll_extent_max(&self) -> Option<f64> {
        self.scroll_extent_max
    }

    /// Sets the scroll extent minimum.
    pub fn set_scroll_extent_min(&mut self, min: f64) {
        self.scroll_extent_min = Some(min);
    }

    /// Returns the scroll extent minimum.
    #[inline]
    pub fn scroll_extent_min(&self) -> Option<f64> {
        self.scroll_extent_min
    }

    /// Sets the scroll index.
    pub fn set_scroll_index(&mut self, index: i32) {
        self.scroll_index = Some(index);
    }

    /// Returns the scroll index.
    #[inline]
    pub fn scroll_index(&self) -> Option<i32> {
        self.scroll_index
    }

    /// Sets the scroll child count.
    pub fn set_scroll_child_count(&mut self, count: i32) {
        self.scroll_child_count = Some(count);
    }

    /// Returns the scroll child count.
    #[inline]
    pub fn scroll_child_count(&self) -> Option<i32> {
        self.scroll_child_count
    }

    // ========================================================================
    // Index
    // ========================================================================

    /// Sets the index in parent.
    pub fn set_index_in_parent(&mut self, index: i32) {
        self.index_in_parent = Some(index);
    }

    /// Returns the index in parent.
    #[inline]
    pub fn index_in_parent(&self) -> Option<i32> {
        self.index_in_parent
    }

    // ========================================================================
    // Platform View
    // ========================================================================

    /// Sets the platform view ID.
    pub fn set_platform_view_id(&mut self, id: i32) {
        self.platform_view_id = Some(id);
    }

    /// Returns the platform view ID.
    #[inline]
    pub fn platform_view_id(&self) -> Option<i32> {
        self.platform_view_id
    }

    // ========================================================================
    // Text Field Properties
    // ========================================================================

    /// Sets the maximum value length.
    pub fn set_max_value_length(&mut self, length: i32) {
        self.max_value_length = Some(length);
    }

    /// Returns the maximum value length.
    #[inline]
    pub fn max_value_length(&self) -> Option<i32> {
        self.max_value_length
    }

    /// Sets the current value length.
    pub fn set_current_value_length(&mut self, length: i32) {
        self.current_value_length = Some(length);
    }

    /// Returns the current value length.
    #[inline]
    pub fn current_value_length(&self) -> Option<i32> {
        self.current_value_length
    }

    // ========================================================================
    // Elevation
    // ========================================================================

    /// Sets the elevation.
    pub fn set_elevation(&mut self, elevation: f64) {
        self.elevation = elevation;
    }

    /// Returns the elevation.
    #[inline]
    pub fn elevation(&self) -> f64 {
        self.elevation
    }

    /// Sets the thickness.
    pub fn set_thickness(&mut self, thickness: f64) {
        self.thickness = thickness;
    }

    /// Returns the thickness.
    #[inline]
    pub fn thickness(&self) -> f64 {
        self.thickness
    }

    // ========================================================================
    // Merging and Copying
    // ========================================================================

    /// Returns whether this configuration has any semantic content.
    pub fn has_content(&self) -> bool {
        !self.flags.is_empty()
            || !self.actions.is_empty()
            || self.label.is_some()
            || self.value.is_some()
            || self.hint.is_some()
            || !self.custom_actions.is_empty()
    }

    /// Absorbs the semantic information from another configuration.
    ///
    /// This is used when merging child semantics into parent nodes.
    pub fn absorb(&mut self, other: &SemanticsConfiguration) {
        // Merge flags
        self.flags.merge(other.flags());

        // Merge actions (other's actions take precedence)
        for (action, handler) in &other.actions {
            self.actions.insert(*action, Arc::clone(handler));
        }

        // Merge custom actions
        self.custom_actions
            .extend(other.custom_actions.iter().cloned());

        // Merge tags
        for tag in &other.tags {
            self.add_tag(tag.clone());
        }

        // Use other's values if self doesn't have them
        if self.label.is_none() {
            self.label.clone_from(&other.label);
        }
        if self.value.is_none() {
            self.value.clone_from(&other.value);
        }
        if self.hint.is_none() {
            self.hint.clone_from(&other.hint);
        }
        if self.sort_key.is_none() {
            self.sort_key.clone_from(&other.sort_key);
        }
        if self.text_direction.is_none() {
            self.text_direction = other.text_direction;
        }
    }

    /// Creates a configuration from properties.
    pub fn from_properties(properties: &SemanticsProperties) -> Self {
        let mut config = Self::new();

        if let Some(enabled) = properties.enabled {
            config.set_enabled(Some(enabled));
        }
        if let Some(checked) = properties.checked {
            config.set_checked(Some(checked));
        }
        if let Some(selected) = properties.selected {
            config.set_selected(selected);
        }
        if let Some(button) = properties.button {
            config.set_button(button);
        }
        if let Some(link) = properties.link {
            config.set_link(link);
        }
        if let Some(header) = properties.header {
            config.set_header(header);
        }
        if let Some(image) = properties.image {
            config.set_image(image);
        }
        if let Some(text_field) = properties.text_field {
            config.set_text_field(text_field);
        }
        if let Some(slider) = properties.slider {
            config.set_slider(slider);
        }
        if let Some(focusable) = properties.focusable {
            config.set_focusable(focusable);
        }
        if let Some(focused) = properties.focused {
            config.set_focused(focused);
        }
        if let Some(hidden) = properties.hidden {
            config.set_hidden(hidden);
        }
        if let Some(obscured) = properties.obscured {
            config.set_obscured(obscured);
        }
        if let Some(multiline) = properties.multiline {
            config.set_multiline(multiline);
        }
        if let Some(live_region) = properties.live_region {
            config.set_live_region(live_region);
        }

        if let Some(ref label) = properties.label {
            config.set_label(label.clone());
        }
        if let Some(ref value) = properties.value {
            config.set_value(value.clone());
        }
        if let Some(ref hint) = properties.hint {
            config.set_hint(hint.clone());
        }
        if let Some(direction) = properties.text_direction {
            config.set_text_direction(direction);
        }
        if let Some(ref sort_key) = properties.sort_key {
            config.set_sort_key(sort_key.clone());
        }

        for tag in &properties.tags {
            config.add_tag(tag.clone());
        }
        for action in &properties.custom_actions {
            config.add_custom_action(action.clone());
        }

        if let Some(ref overrides) = properties.hint_overrides {
            config.set_hint_overrides(overrides.clone());
        }

        config
    }
}

impl std::fmt::Debug for SemanticsConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsConfiguration")
            .field("is_semantics_boundary", &self.is_semantics_boundary)
            .field("flags", &self.flags)
            .field("label", &self.label)
            .field("value", &self.value)
            .field("hint", &self.hint)
            .field("actions", &self.actions.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configuration_defaults() {
        let config = SemanticsConfiguration::new();
        assert!(!config.is_semantics_boundary());
        assert!(!config.has_content());
    }

    #[test]
    fn test_button_configuration() {
        let mut config = SemanticsConfiguration::new();
        config.set_label("Submit");
        config.set_button(true);
        config.set_enabled(Some(true));

        assert!(config.is_button());
        assert_eq!(config.is_enabled(), Some(true));
        assert_eq!(config.label().map(|l| l.as_str()), Some("Submit"));
        assert!(config.has_content());
    }

    #[test]
    fn test_checkbox_configuration() {
        let mut config = SemanticsConfiguration::new();
        config.set_label("Accept terms");
        config.set_checked(Some(false));

        assert!(config.has_flag(SemanticsFlag::HasCheckedState));
        assert_eq!(config.is_checked(), Some(false));

        config.set_checked(Some(true));
        assert_eq!(config.is_checked(), Some(true));
    }

    #[test]
    fn test_slider_configuration() {
        let mut config = SemanticsConfiguration::new();
        config.set_slider(true);
        config.set_value("50%");
        config.set_increased_value("55%");
        config.set_decreased_value("45%");

        assert!(config.is_slider());
        assert_eq!(config.value().map(|v| v.as_str()), Some("50%"));
        assert_eq!(config.increased_value().map(|v| v.as_str()), Some("55%"));
        assert_eq!(config.decreased_value().map(|v| v.as_str()), Some("45%"));
    }

    #[test]
    fn test_action_handling() {
        let mut config = SemanticsConfiguration::new();

        let handler: SemanticsActionHandler = Arc::new(|_action, _args| {});
        config.add_action(SemanticsAction::Tap, handler);

        assert!(config.has_action(SemanticsAction::Tap));
        assert!(!config.has_action(SemanticsAction::LongPress));
        assert_eq!(config.actions_as_bits(), SemanticsAction::Tap.value());
    }

    #[test]
    fn test_configuration_absorb() {
        let mut parent = SemanticsConfiguration::new();
        parent.set_button(true);

        let mut child = SemanticsConfiguration::new();
        child.set_label("Child label");
        child.set_enabled(Some(true));

        parent.absorb(&child);

        assert!(parent.is_button());
        assert_eq!(parent.label().map(|l| l.as_str()), Some("Child label"));
        assert_eq!(parent.is_enabled(), Some(true));
    }

    #[test]
    fn test_scroll_properties() {
        let mut config = SemanticsConfiguration::new();
        config.set_scroll_position(100.0);
        config.set_scroll_extent_min(0.0);
        config.set_scroll_extent_max(500.0);
        config.set_scroll_index(5);
        config.set_scroll_child_count(20);

        assert_eq!(config.scroll_position(), Some(100.0));
        assert_eq!(config.scroll_extent_min(), Some(0.0));
        assert_eq!(config.scroll_extent_max(), Some(500.0));
        assert_eq!(config.scroll_index(), Some(5));
        assert_eq!(config.scroll_child_count(), Some(20));
    }

    #[test]
    fn test_from_properties() {
        let props = SemanticsProperties::new()
            .with_label("Test")
            .with_button(true)
            .with_enabled(true);

        let config = SemanticsConfiguration::from_properties(&props);

        assert!(config.is_button());
        assert_eq!(config.is_enabled(), Some(true));
        assert_eq!(config.label().map(|l| l.as_str()), Some("Test"));
    }

    #[test]
    fn test_smallvec_inline() {
        let mut config = SemanticsConfiguration::new();

        // Add tags up to inline capacity
        config.add_tag(SemanticsTag::new("tag1"));
        config.add_tag(SemanticsTag::new("tag2"));

        assert_eq!(config.tags().len(), 2);
    }
}
