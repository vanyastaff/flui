//! Semantics properties and attributed strings.
//!
//! This module provides property types for accessibility information.

use rustc_hash::FxHashSet;
use smol_str::SmolStr;

use crate::flags::SemanticsFlag;

// ============================================================================
// TextDirection
// ============================================================================

/// Text direction for semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Right-to-left text.
    Rtl,
    /// Left-to-right text.
    #[default]
    Ltr,
}

// ============================================================================
// SemanticsTag
// ============================================================================

/// A tag that can be applied to semantics nodes for identification.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticsTag {
    /// The tag name.
    pub name: SmolStr,
}

impl SemanticsTag {
    /// Creates a new semantics tag.
    pub fn new(name: impl Into<SmolStr>) -> Self {
        Self { name: name.into() }
    }
}

// ============================================================================
// SemanticsHintOverrides
// ============================================================================

/// Overrides for semantics hints.
#[derive(Debug, Clone, Default)]
pub struct SemanticsHintOverrides {
    /// Override for the on-tap hint.
    pub on_tap_hint: Option<SmolStr>,
    /// Override for the on-long-press hint.
    pub on_long_press_hint: Option<SmolStr>,
}

impl SemanticsHintOverrides {
    /// Creates empty hint overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the on-tap hint.
    pub fn with_tap_hint(mut self, hint: impl Into<SmolStr>) -> Self {
        self.on_tap_hint = Some(hint.into());
        self
    }

    /// Sets the on-long-press hint.
    pub fn with_long_press_hint(mut self, hint: impl Into<SmolStr>) -> Self {
        self.on_long_press_hint = Some(hint.into());
        self
    }

    /// Returns whether any hints are overridden.
    pub fn is_empty(&self) -> bool {
        self.on_tap_hint.is_none() && self.on_long_press_hint.is_none()
    }
}

// ============================================================================
// CustomSemanticsAction
// ============================================================================

/// A custom semantics action.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CustomSemanticsAction {
    /// Unique identifier.
    pub id: i32,
    /// Label for the action.
    pub label: SmolStr,
    /// Optional hint for the action.
    pub hint: Option<SmolStr>,
}

impl CustomSemanticsAction {
    /// Creates a new custom action.
    pub fn new(id: i32, label: impl Into<SmolStr>) -> Self {
        Self {
            id,
            label: label.into(),
            hint: None,
        }
    }

    /// Sets the hint.
    pub fn with_hint(mut self, hint: impl Into<SmolStr>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

// ============================================================================
// AttributedString
// ============================================================================

/// A string with attributes for accessibility.
#[derive(Debug, Clone, Default)]
pub struct AttributedString {
    /// The string content.
    pub string: SmolStr,
    /// Attributes applied to ranges.
    pub attributes: Vec<StringAttribute>,
}

impl AttributedString {
    /// Creates a new attributed string.
    pub fn new(string: impl Into<SmolStr>) -> Self {
        Self {
            string: string.into(),
            attributes: Vec::new(),
        }
    }

    /// Adds an attribute.
    pub fn add_attribute(&mut self, attr: StringAttribute) {
        self.attributes.push(attr);
    }

    /// Returns whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.string.is_empty()
    }

    /// Returns the string as a str reference.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.string
    }
}

impl From<String> for AttributedString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for AttributedString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<SmolStr> for AttributedString {
    fn from(s: SmolStr) -> Self {
        Self {
            string: s,
            attributes: Vec::new(),
        }
    }
}

// ============================================================================
// StringAttribute
// ============================================================================

/// An attribute on a range of text.
#[derive(Debug, Clone)]
pub struct StringAttribute {
    /// Start index (inclusive).
    pub start: usize,
    /// End index (exclusive).
    pub end: usize,
    /// The attribute type.
    pub attribute_type: StringAttributeType,
}

/// Types of string attributes.
#[derive(Debug, Clone)]
pub enum StringAttributeType {
    /// Spell out the text character by character.
    SpellOut,
    /// Use a specific locale for pronunciation.
    Locale(SmolStr),
}

// ============================================================================
// SemanticsSortKey
// ============================================================================

/// Sort key for ordering semantics nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticsSortKey {
    /// The order value.
    pub order: f64,
    /// Optional name for grouping.
    pub name: Option<SmolStr>,
}

impl SemanticsSortKey {
    /// Creates a new sort key.
    pub fn new(order: f64) -> Self {
        Self { order, name: None }
    }

    /// Creates a named sort key.
    pub fn named(order: f64, name: impl Into<SmolStr>) -> Self {
        Self {
            order,
            name: Some(name.into()),
        }
    }
}

impl PartialOrd for SemanticsSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.order.partial_cmp(&other.order)
    }
}

// ============================================================================
// SemanticsProperties
// ============================================================================

/// All properties that can be set on a semantics node.
#[derive(Debug, Clone, Default)]
pub struct SemanticsProperties {
    /// Whether this node is enabled.
    pub enabled: Option<bool>,
    /// Whether this node is checked.
    pub checked: Option<bool>,
    /// Whether this node is mixed (indeterminate).
    pub mixed: Option<bool>,
    /// Whether this node is selected.
    pub selected: Option<bool>,
    /// Whether this node is toggled.
    pub toggled: Option<bool>,
    /// Whether this node is expanded.
    pub expanded: Option<bool>,
    /// Whether this node is focused.
    pub focused: Option<bool>,
    /// Whether this node is focusable.
    pub focusable: Option<bool>,
    /// Whether this node is a button.
    pub button: Option<bool>,
    /// Whether this node is a link.
    pub link: Option<bool>,
    /// Whether this node is a header.
    pub header: Option<bool>,
    /// Whether this node is an image.
    pub image: Option<bool>,
    /// Whether this node is a text field.
    pub text_field: Option<bool>,
    /// Whether this node is a slider.
    pub slider: Option<bool>,
    /// Whether this node is read-only.
    pub read_only: Option<bool>,
    /// Whether this node is hidden.
    pub hidden: Option<bool>,
    /// Whether this node is obscured (password).
    pub obscured: Option<bool>,
    /// Whether this node is multiline.
    pub multiline: Option<bool>,
    /// Whether this node scopes a route.
    pub scopes_route: Option<bool>,
    /// Whether this node names a route.
    pub names_route: Option<bool>,
    /// Whether this node is in a mutually exclusive group.
    pub in_mutually_exclusive_group: Option<bool>,
    /// Whether this node is a live region.
    pub live_region: Option<bool>,
    /// The label for this node.
    pub label: Option<AttributedString>,
    /// The value of this node.
    pub value: Option<AttributedString>,
    /// The increased value hint.
    pub increased_value: Option<AttributedString>,
    /// The decreased value hint.
    pub decreased_value: Option<AttributedString>,
    /// The hint for this node.
    pub hint: Option<AttributedString>,
    /// The text direction.
    pub text_direction: Option<TextDirection>,
    /// The sort key for ordering.
    pub sort_key: Option<SemanticsSortKey>,
    /// Tags for this node.
    pub tags: FxHashSet<SemanticsTag>,
    /// Custom actions.
    pub custom_actions: Vec<CustomSemanticsAction>,
    /// Hint overrides.
    pub hint_overrides: Option<SemanticsHintOverrides>,
}

impl SemanticsProperties {
    /// Creates empty properties.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the label.
    pub fn with_label(mut self, label: impl Into<AttributedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the value.
    pub fn with_value(mut self, value: impl Into<AttributedString>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets the hint.
    pub fn with_hint(mut self, hint: impl Into<AttributedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Sets whether this is a button.
    pub fn with_button(mut self, is_button: bool) -> Self {
        self.button = Some(is_button);
        self
    }

    /// Sets whether this is enabled.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Sets whether this is checked.
    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    /// Adds a tag.
    pub fn with_tag(mut self, tag: SemanticsTag) -> Self {
        self.tags.insert(tag);
        self
    }

    /// Returns whether any properties are set.
    pub fn is_empty(&self) -> bool {
        self.label.is_none()
            && self.value.is_none()
            && self.hint.is_none()
            && self.button.is_none()
            && self.enabled.is_none()
            && self.checked.is_none()
            && self.tags.is_empty()
    }

    /// Converts properties to flags.
    pub fn to_flags(&self) -> u64 {
        let mut flags = 0u64;

        if self.button == Some(true) {
            flags |= SemanticsFlag::IsButton.value();
        }
        if self.link == Some(true) {
            flags |= SemanticsFlag::IsLink.value();
        }
        if self.header == Some(true) {
            flags |= SemanticsFlag::IsHeader.value();
        }
        if self.image == Some(true) {
            flags |= SemanticsFlag::IsImage.value();
        }
        if self.text_field == Some(true) {
            flags |= SemanticsFlag::IsTextField.value();
        }
        if self.slider == Some(true) {
            flags |= SemanticsFlag::IsSlider.value();
        }
        if self.read_only == Some(true) {
            flags |= SemanticsFlag::IsReadOnly.value();
        }
        if self.hidden == Some(true) {
            flags |= SemanticsFlag::IsHidden.value();
        }
        if self.obscured == Some(true) {
            flags |= SemanticsFlag::IsObscured.value();
        }
        if self.multiline == Some(true) {
            flags |= SemanticsFlag::IsMultiline.value();
        }
        if self.scopes_route == Some(true) {
            flags |= SemanticsFlag::ScopesRoute.value();
        }
        if self.names_route == Some(true) {
            flags |= SemanticsFlag::NamesRoute.value();
        }
        if self.live_region == Some(true) {
            flags |= SemanticsFlag::IsLiveRegion.value();
        }
        if self.focusable == Some(true) {
            flags |= SemanticsFlag::IsFocusable.value();
        }
        if self.focused == Some(true) {
            flags |= SemanticsFlag::IsFocused.value();
        }
        if self.selected == Some(true) {
            flags |= SemanticsFlag::IsSelected.value();
        }
        if self.expanded == Some(true) {
            flags |= SemanticsFlag::IsExpanded.value();
        }

        // Checked state
        if self.checked.is_some() {
            flags |= SemanticsFlag::HasCheckedState.value();
            if self.checked == Some(true) {
                flags |= SemanticsFlag::IsChecked.value();
            }
        }
        if self.mixed == Some(true) {
            flags |= SemanticsFlag::IsCheckStateMixed.value();
        }

        // Enabled state
        if self.enabled.is_some() {
            flags |= SemanticsFlag::HasEnabledState.value();
            if self.enabled == Some(true) {
                flags |= SemanticsFlag::IsEnabled.value();
            }
        }

        // Toggled state
        if self.toggled.is_some() {
            flags |= SemanticsFlag::HasToggledState.value();
            if self.toggled == Some(true) {
                flags |= SemanticsFlag::IsToggled.value();
            }
        }

        flags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_properties_builder() {
        let props = SemanticsProperties::new()
            .with_label("Submit")
            .with_button(true)
            .with_enabled(true);

        assert!(props.label.is_some());
        assert_eq!(props.button, Some(true));
        assert_eq!(props.enabled, Some(true));
    }

    #[test]
    fn test_sort_key_ordering() {
        let key1 = SemanticsSortKey::new(1.0);
        let key2 = SemanticsSortKey::new(2.0);
        assert!(key1 < key2);
    }

    #[test]
    fn test_attributed_string() {
        let string = AttributedString::new("Hello, World!");
        assert_eq!(string.as_str(), "Hello, World!");
        assert!(string.attributes.is_empty());
    }

    #[test]
    fn test_smol_str_inline() {
        // Small strings should be inlined
        let small = SmolStr::from("Button");
        let cloned = small.clone(); // O(1) clone
        assert_eq!(small, cloned);
    }

    #[test]
    fn test_properties_to_flags() {
        let props = SemanticsProperties::new()
            .with_button(true)
            .with_enabled(true);

        let flags = props.to_flags();
        assert!(flags & SemanticsFlag::IsButton.value() != 0);
        assert!(flags & SemanticsFlag::HasEnabledState.value() != 0);
        assert!(flags & SemanticsFlag::IsEnabled.value() != 0);
    }

    #[test]
    fn test_semantics_tag() {
        let tag1 = SemanticsTag::new("test");
        let tag2 = SemanticsTag::new("test");
        assert_eq!(tag1, tag2);
    }
}
