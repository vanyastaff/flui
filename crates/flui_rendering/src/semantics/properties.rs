//! Semantics properties and flags.

use std::collections::HashSet;

// ============================================================================
// SemanticsFlag
// ============================================================================

/// Boolean properties of a semantics node.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsFlag` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u64)]
pub enum SemanticsFlag {
    /// Has checked state (for checkboxes, etc.).
    HasCheckedState = 1 << 0,

    /// Is checked.
    IsChecked = 1 << 1,

    /// Is selected.
    IsSelected = 1 << 2,

    /// Is button.
    IsButton = 1 << 3,

    /// Is link.
    IsLink = 1 << 4,

    /// Is text field.
    IsTextField = 1 << 5,

    /// Is slider.
    IsSlider = 1 << 6,

    /// Is keyboard key.
    IsKeyboardKey = 1 << 7,

    /// Is read-only.
    IsReadOnly = 1 << 8,

    /// Is focusable.
    IsFocusable = 1 << 9,

    /// Is focused.
    IsFocused = 1 << 10,

    /// Has enabled state.
    HasEnabledState = 1 << 11,

    /// Is enabled.
    IsEnabled = 1 << 12,

    /// Is in mutually exclusive group.
    IsInMutuallyExclusiveGroup = 1 << 13,

    /// Is header.
    IsHeader = 1 << 14,

    /// Is obscured (for password fields).
    IsObscured = 1 << 15,

    /// Scope route (modal barrier).
    ScopesRoute = 1 << 16,

    /// Names route.
    NamesRoute = 1 << 17,

    /// Is hidden.
    IsHidden = 1 << 18,

    /// Is image.
    IsImage = 1 << 19,

    /// Is live region.
    IsLiveRegion = 1 << 20,

    /// Has toggled state.
    HasToggledState = 1 << 21,

    /// Is toggled.
    IsToggled = 1 << 22,

    /// Has implicit scrolling.
    HasImplicitScrolling = 1 << 23,

    /// Is multiline.
    IsMultiline = 1 << 24,

    /// Is expanded.
    IsExpanded = 1 << 25,

    /// Is checkstate mixed (indeterminate).
    IsCheckStateMixed = 1 << 26,
}

impl SemanticsFlag {
    /// Returns the bitmask value for this flag.
    pub fn value(self) -> u64 {
        self as u64
    }

    /// Returns the name of this flag.
    pub fn name(self) -> &'static str {
        match self {
            Self::HasCheckedState => "hasCheckedState",
            Self::IsChecked => "isChecked",
            Self::IsSelected => "isSelected",
            Self::IsButton => "isButton",
            Self::IsLink => "isLink",
            Self::IsTextField => "isTextField",
            Self::IsSlider => "isSlider",
            Self::IsKeyboardKey => "isKeyboardKey",
            Self::IsReadOnly => "isReadOnly",
            Self::IsFocusable => "isFocusable",
            Self::IsFocused => "isFocused",
            Self::HasEnabledState => "hasEnabledState",
            Self::IsEnabled => "isEnabled",
            Self::IsInMutuallyExclusiveGroup => "isInMutuallyExclusiveGroup",
            Self::IsHeader => "isHeader",
            Self::IsObscured => "isObscured",
            Self::ScopesRoute => "scopesRoute",
            Self::NamesRoute => "namesRoute",
            Self::IsHidden => "isHidden",
            Self::IsImage => "isImage",
            Self::IsLiveRegion => "isLiveRegion",
            Self::HasToggledState => "hasToggledState",
            Self::IsToggled => "isToggled",
            Self::HasImplicitScrolling => "hasImplicitScrolling",
            Self::IsMultiline => "isMultiline",
            Self::IsExpanded => "isExpanded",
            Self::IsCheckStateMixed => "isCheckStateMixed",
        }
    }
}

// ============================================================================
// SemanticsFlags
// ============================================================================

/// A set of semantics flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticsFlags {
    /// The bitmask of flags.
    flags: u64,
}

impl SemanticsFlags {
    /// Creates an empty flags set.
    pub fn new() -> Self {
        Self { flags: 0 }
    }

    /// Creates flags from a bitmask.
    pub fn from_bits(bits: u64) -> Self {
        Self { flags: bits }
    }

    /// Returns the raw bitmask.
    pub fn bits(&self) -> u64 {
        self.flags
    }

    /// Returns whether the given flag is set.
    pub fn has(&self, flag: SemanticsFlag) -> bool {
        self.flags & flag.value() != 0
    }

    /// Sets a flag.
    pub fn set(&mut self, flag: SemanticsFlag) {
        self.flags |= flag.value();
    }

    /// Clears a flag.
    pub fn clear(&mut self, flag: SemanticsFlag) {
        self.flags &= !flag.value();
    }

    /// Toggles a flag.
    pub fn toggle(&mut self, flag: SemanticsFlag) {
        self.flags ^= flag.value();
    }

    /// Returns whether any flags are set.
    pub fn is_empty(&self) -> bool {
        self.flags == 0
    }

    /// Merges another flags set into this one.
    pub fn merge(&mut self, other: &Self) {
        self.flags |= other.flags;
    }
}

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
    pub name: String,
}

impl SemanticsTag {
    /// Creates a new semantics tag.
    pub fn new(name: impl Into<String>) -> Self {
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
    pub on_tap_hint: Option<String>,
    /// Override for the on-long-press hint.
    pub on_long_press_hint: Option<String>,
}

impl SemanticsHintOverrides {
    /// Creates empty hint overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the on-tap hint.
    pub fn with_tap_hint(mut self, hint: impl Into<String>) -> Self {
        self.on_tap_hint = Some(hint.into());
        self
    }

    /// Sets the on-long-press hint.
    pub fn with_long_press_hint(mut self, hint: impl Into<String>) -> Self {
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
    pub label: String,
    /// Optional hint for the action.
    pub hint: Option<String>,
}

impl CustomSemanticsAction {
    /// Creates a new custom action.
    pub fn new(id: i32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            hint: None,
        }
    }

    /// Sets the hint.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
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
    pub string: String,
    /// Attributes applied to ranges.
    pub attributes: Vec<StringAttribute>,
}

impl AttributedString {
    /// Creates a new attributed string.
    pub fn new(string: impl Into<String>) -> Self {
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
    Locale(String),
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
    pub tags: HashSet<SemanticsTag>,
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
    pub name: Option<String>,
}

impl SemanticsSortKey {
    /// Creates a new sort key.
    pub fn new(order: f64) -> Self {
        Self { order, name: None }
    }

    /// Creates a named sort key.
    pub fn named(order: f64, name: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flag_values() {
        assert_eq!(SemanticsFlag::HasCheckedState.value(), 1);
        assert_eq!(SemanticsFlag::IsChecked.value(), 2);
        assert_eq!(SemanticsFlag::IsSelected.value(), 4);
    }

    #[test]
    fn test_flags_operations() {
        let mut flags = SemanticsFlags::new();
        assert!(flags.is_empty());

        flags.set(SemanticsFlag::IsButton);
        assert!(flags.has(SemanticsFlag::IsButton));
        assert!(!flags.has(SemanticsFlag::IsLink));

        flags.set(SemanticsFlag::IsLink);
        assert!(flags.has(SemanticsFlag::IsLink));

        flags.clear(SemanticsFlag::IsButton);
        assert!(!flags.has(SemanticsFlag::IsButton));
    }

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
        assert_eq!(string.string, "Hello, World!");
        assert!(string.attributes.is_empty());
    }
}
