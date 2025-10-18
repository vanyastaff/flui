//! Semantic data structures

use crate::Rect;

/// The role of a semantic node for accessibility
///
/// Similar to Flutter's implicit roles. These help screen readers
/// understand what kind of UI element this is.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::SemanticsRole;
///
/// let role = SemanticsRole::Button;
/// assert!(role.is_interactive());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SemanticsRole {
    /// A button that can be tapped
    Button,

    /// A link that navigates somewhere
    Link,

    /// An image
    Image,

    /// A text field for user input
    TextField,

    /// A checkbox
    Checkbox,

    /// A radio button
    Radio,

    /// A slider for selecting a value in a range
    Slider,

    /// A switch (on/off toggle)
    Switch,

    /// A header (like a section title)
    Header,

    /// Generic text content
    Text,

    /// A list container
    List,

    /// A list item
    ListItem,

    /// A table
    Table,

    /// A dialog or modal
    Dialog,

    /// A menu
    Menu,

    /// A menu item
    MenuItem,

    /// A progress indicator
    ProgressIndicator,

    /// A tab
    Tab,

    /// A tab list
    TabList,

    /// A generic group of elements
    Group,

    /// No specific role
    None,
}

impl SemanticsRole {
    /// Returns true if this role represents an interactive element
    pub const fn is_interactive(&self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::Link
                | Self::TextField
                | Self::Checkbox
                | Self::Radio
                | Self::Slider
                | Self::Switch
                | Self::MenuItem
                | Self::Tab
        )
    }

    /// Returns true if this role represents a container
    pub const fn is_container(&self) -> bool {
        matches!(
            self,
            Self::List | Self::Table | Self::Dialog | Self::Menu | Self::TabList | Self::Group
        )
    }
}

impl Default for SemanticsRole {
    fn default() -> Self {
        Self::None
    }
}

/// Properties that describe semantics for accessibility
///
/// Similar to Flutter's `SemanticsProperties`. These are the properties
/// that can be set on a semantic node to describe it for accessibility.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::{SemanticsProperties, SemanticsRole};
///
/// let props = SemanticsProperties::new()
///     .with_label("Submit button")
///     .with_role(SemanticsRole::Button)
///     .with_enabled(true);
///
/// assert_eq!(props.label(), Some("Submit button"));
/// assert_eq!(props.role(), SemanticsRole::Button);
/// ```
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticsProperties {
    /// The role of this semantic node
    pub role: SemanticsRole,

    /// A short label describing this node
    pub label: Option<String>,

    /// The value of this node (for text fields, sliders, etc.)
    pub value: Option<String>,

    /// A hint about what will happen if the user interacts with this node
    pub hint: Option<String>,

    /// Whether this node is enabled for interaction
    pub enabled: bool,

    /// Whether this node is currently focused
    pub focused: bool,

    /// The text direction for this node's text
    pub text_direction: Option<crate::typography::TextDirection>,
}

impl SemanticsProperties {
    /// Creates new empty semantics properties
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the role
    pub fn with_role(mut self, role: SemanticsRole) -> Self {
        self.role = role;
        self
    }

    /// Sets the label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the value
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets the hint
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Sets whether this node is enabled
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether this node is focused
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Returns the role
    pub fn role(&self) -> SemanticsRole {
        self.role
    }

    /// Returns the label, if set
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Returns the value, if set
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Returns the hint, if set
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }
}

/// Overrides for semantic hints
///
/// Similar to Flutter's `SemanticsHintOverrides`. Allows customizing
/// the hints that screen readers provide.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticsHintOverrides {
    /// Override for the hint when the node is tappable
    pub on_tap_hint: Option<String>,

    /// Override for the hint when the node is long-pressable
    pub on_long_press_hint: Option<String>,
}

impl SemanticsHintOverrides {
    /// Creates new empty hint overrides
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tap hint
    pub fn with_on_tap_hint(mut self, hint: impl Into<String>) -> Self {
        self.on_tap_hint = Some(hint.into());
        self
    }

    /// Sets the long press hint
    pub fn with_on_long_press_hint(mut self, hint: impl Into<String>) -> Self {
        self.on_long_press_hint = Some(hint.into());
        self
    }
}

/// Summary data about a semantic node
///
/// Similar to Flutter's `SemanticsData`. This is a snapshot of the
/// semantic information for a node.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SemanticsData {
    /// The properties of this node
    pub properties: SemanticsProperties,

    /// The bounding rect of this node
    pub rect: Rect,

    /// The transform applied to this node (not implemented yet)
    pub transform: Option<[f32; 16]>,
}

impl SemanticsData {
    /// Creates new semantics data
    pub fn new(properties: SemanticsProperties, rect: Rect) -> Self {
        Self {
            properties,
            rect,
            transform: None,
        }
    }

    /// Creates new semantics data with a transform
    pub fn with_transform(mut self, transform: [f32; 16]) -> Self {
        self.transform = Some(transform);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_role_default() {
        let role = SemanticsRole::default();
        assert_eq!(role, SemanticsRole::None);
    }

    #[test]
    fn test_semantics_role_is_interactive() {
        assert!(SemanticsRole::Button.is_interactive());
        assert!(SemanticsRole::Link.is_interactive());
        assert!(!SemanticsRole::Text.is_interactive());
        assert!(!SemanticsRole::Image.is_interactive());
    }

    #[test]
    fn test_semantics_role_is_container() {
        assert!(SemanticsRole::List.is_container());
        assert!(SemanticsRole::Table.is_container());
        assert!(!SemanticsRole::Button.is_container());
        assert!(!SemanticsRole::Text.is_container());
    }

    #[test]
    fn test_semantics_properties_new() {
        let props = SemanticsProperties::new();
        assert_eq!(props.role(), SemanticsRole::None);
        assert_eq!(props.label(), None);
        assert_eq!(props.value(), None);
    }

    #[test]
    fn test_semantics_properties_builder() {
        let props = SemanticsProperties::new()
            .with_role(SemanticsRole::Button)
            .with_label("Click me")
            .with_enabled(true);

        assert_eq!(props.role(), SemanticsRole::Button);
        assert_eq!(props.label(), Some("Click me"));
        assert!(props.enabled);
    }

    #[test]
    fn test_semantics_hint_overrides() {
        let overrides = SemanticsHintOverrides::new()
            .with_on_tap_hint("Tap to submit")
            .with_on_long_press_hint("Long press for options");

        assert_eq!(overrides.on_tap_hint.as_deref(), Some("Tap to submit"));
        assert_eq!(
            overrides.on_long_press_hint.as_deref(),
            Some("Long press for options")
        );
    }

    #[test]
    fn test_semantics_data_new() {
        let props = SemanticsProperties::new().with_label("Test");
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
        let data = SemanticsData::new(props, rect);

        assert_eq!(data.properties.label(), Some("Test"));
        assert_eq!(data.rect, rect);
        assert_eq!(data.transform, None);
    }
}
