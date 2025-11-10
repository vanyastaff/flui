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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
    #[default]
    None,
}

impl SemanticsRole {
    /// Returns true if this role represents an interactive element
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsRole;
    ///
    /// assert!(SemanticsRole::Button.is_interactive());
    /// assert!(!SemanticsRole::Text.is_interactive());
    /// ```
    #[inline]
    #[must_use]
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
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsRole;
    ///
    /// assert!(SemanticsRole::List.is_container());
    /// assert!(!SemanticsRole::Button.is_container());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_container(&self) -> bool {
        matches!(
            self,
            Self::List | Self::Table | Self::Dialog | Self::Menu | Self::TabList | Self::Group
        )
    }

    /// Returns true if this role represents a form control
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsRole;
    ///
    /// assert!(SemanticsRole::TextField.is_form_control());
    /// assert!(SemanticsRole::Checkbox.is_form_control());
    /// assert!(!SemanticsRole::Text.is_form_control());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_form_control(&self) -> bool {
        matches!(
            self,
            Self::TextField | Self::Checkbox | Self::Radio | Self::Slider | Self::Switch
        )
    }

    /// Returns true if this role represents selectable content
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsRole;
    ///
    /// assert!(SemanticsRole::Checkbox.is_selectable());
    /// assert!(SemanticsRole::Radio.is_selectable());
    /// assert!(!SemanticsRole::Button.is_selectable());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_selectable(&self) -> bool {
        matches!(
            self,
            Self::Checkbox | Self::Radio | Self::Tab | Self::MenuItem
        )
    }

    /// Returns a human-readable name for this role
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::SemanticsRole;
    ///
    /// assert_eq!(SemanticsRole::Button.name(), "button");
    /// assert_eq!(SemanticsRole::TextField.name(), "textfield");
    /// ```
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Link => "link",
            Self::Image => "image",
            Self::TextField => "textfield",
            Self::Checkbox => "checkbox",
            Self::Radio => "radio",
            Self::Slider => "slider",
            Self::Switch => "switch",
            Self::Header => "header",
            Self::Text => "text",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Table => "table",
            Self::Dialog => "dialog",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
            Self::ProgressIndicator => "progressbar",
            Self::Tab => "tab",
            Self::TabList => "tablist",
            Self::Group => "group",
            Self::None => "none",
        }
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the role
    #[must_use]
    pub fn with_role(mut self, role: SemanticsRole) -> Self {
        self.role = role;
        self
    }

    /// Sets the label
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the value
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Sets the hint
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Sets whether this node is enabled
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets whether this node is focused
    #[must_use]
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Sets the text direction
    #[must_use]
    pub fn with_text_direction(mut self, direction: crate::typography::TextDirection) -> Self {
        self.text_direction = Some(direction);
        self
    }

    /// Returns the role
    #[inline]
    #[must_use]
    pub const fn role(&self) -> SemanticsRole {
        self.role
    }

    /// Returns the label, if set
    #[inline]
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Returns the value, if set
    #[inline]
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Returns the hint, if set
    #[inline]
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// Returns true if the node is enabled
    #[inline]
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns true if the node is focused
    #[inline]
    #[must_use]
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// Returns true if this node has a label
    #[inline]
    #[must_use]
    pub const fn has_label(&self) -> bool {
        self.label.is_some()
    }

    /// Returns true if this node has a value
    #[inline]
    #[must_use]
    pub const fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// Returns true if this node has a hint
    #[inline]
    #[must_use]
    pub const fn has_hint(&self) -> bool {
        self.hint.is_some()
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tap hint
    #[must_use]
    pub fn with_on_tap_hint(mut self, hint: impl Into<String>) -> Self {
        self.on_tap_hint = Some(hint.into());
        self
    }

    /// Sets the long press hint
    #[must_use]
    pub fn with_on_long_press_hint(mut self, hint: impl Into<String>) -> Self {
        self.on_long_press_hint = Some(hint.into());
        self
    }

    /// Returns the tap hint, if set
    #[inline]
    #[must_use]
    pub fn on_tap_hint(&self) -> Option<&str> {
        self.on_tap_hint.as_deref()
    }

    /// Returns the long press hint, if set
    #[inline]
    #[must_use]
    pub fn on_long_press_hint(&self) -> Option<&str> {
        self.on_long_press_hint.as_deref()
    }

    /// Returns true if this has any overrides
    #[inline]
    #[must_use]
    pub const fn has_overrides(&self) -> bool {
        self.on_tap_hint.is_some() || self.on_long_press_hint.is_some()
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
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::{SemanticsData, SemanticsProperties};
    /// use flui_types::Rect;
    ///
    /// let props = SemanticsProperties::new().with_label("Button");
    /// let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
    /// let data = SemanticsData::new(props, rect);
    /// assert_eq!(data.rect(), &rect);
    /// ```
    #[must_use]
    pub fn new(properties: SemanticsProperties, rect: Rect) -> Self {
        Self {
            properties,
            rect,
            transform: None,
        }
    }

    /// Creates new semantics data with a transform
    #[must_use]
    pub fn with_transform(mut self, transform: [f32; 16]) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Returns a reference to the properties
    #[inline]
    #[must_use]
    pub const fn properties(&self) -> &SemanticsProperties {
        &self.properties
    }

    /// Returns a reference to the bounding rect
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::{SemanticsData, SemanticsProperties};
    /// use flui_types::Rect;
    ///
    /// let props = SemanticsProperties::new();
    /// let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
    /// let data = SemanticsData::new(props, rect);
    /// assert_eq!(data.rect().left(), 10.0);
    /// assert_eq!(data.rect().width(), 100.0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn rect(&self) -> &Rect {
        &self.rect
    }

    /// Returns a reference to the transform matrix, if set
    #[inline]
    #[must_use]
    pub const fn transform(&self) -> Option<&[f32; 16]> {
        self.transform.as_ref()
    }

    /// Returns true if this has a transform applied
    #[inline]
    #[must_use]
    pub const fn has_transform(&self) -> bool {
        self.transform.is_some()
    }

    /// Returns the width of the bounding rect
    #[inline]
    #[must_use]
    pub fn width(&self) -> f32 {
        self.rect.width()
    }

    /// Returns the height of the bounding rect
    #[inline]
    #[must_use]
    pub fn height(&self) -> f32 {
        self.rect.height()
    }

    /// Returns the area of the bounding rect
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::semantics::{SemanticsData, SemanticsProperties};
    /// use flui_types::Rect;
    ///
    /// let props = SemanticsProperties::new();
    /// let rect = Rect::from_xywh(0.0, 0.0, 100.0, 50.0);
    /// let data = SemanticsData::new(props, rect);
    /// assert_eq!(data.area(), 5000.0);
    /// ```
    #[inline]
    #[must_use]
    pub fn area(&self) -> f32 {
        self.rect.width() * self.rect.height()
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
