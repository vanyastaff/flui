//! Diagnostics and debugging support
//!
//! This module provides types for debugging and introspection,
//! similar to Flutter's diagnostics system.

use std::fmt;
use std::str::FromStr;

/// The level of importance of a diagnostic message.
///
/// Similar to Flutter's `DiagnosticLevel`.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticLevel;
///
/// let level = DiagnosticLevel::Info;
/// assert!(level > DiagnosticLevel::Debug);
/// println!("{}", level); // "info"
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum DiagnosticLevel {
    /// Hidden diagnostic level.
    Hidden,
    /// A diagnostic that is likely to be low-value but may provide debugging value.
    Fine,
    /// A diagnostic useful for debugging.
    Debug,
    /// Diagnostics that are probably useful for debugging.
    Info,
    /// A diagnostic that is informational.
    Warning,
    /// A diagnostic that we want to bring to the user's attention.
    Hint,
    /// A diagnostic that indicates an error.
    Error,
}

impl DiagnosticLevel {
    /// Returns the level as a lowercase string
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Fine => "fine",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Hint => "hint",
            Self::Error => "error",
        }
    }

    /// Checks if this is an error level
    #[must_use]
    #[inline]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error)
    }

    /// Checks if this is a warning level
    #[must_use]
    #[inline]
    pub const fn is_warning(&self) -> bool {
        matches!(self, Self::Warning)
    }

    /// Checks if this level should be visible in normal output
    #[must_use]
    #[inline]
    pub const fn is_visible(&self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

impl Default for DiagnosticLevel {
    #[inline]
    fn default() -> Self {
        Self::Info
    }
}

impl fmt::Display for DiagnosticLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for DiagnosticLevel {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for DiagnosticLevel {
    type Err = ParseDiagnosticLevelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "hidden" => Ok(Self::Hidden),
            "fine" => Ok(Self::Fine),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warning" | "warn" => Ok(Self::Warning),
            "hint" => Ok(Self::Hint),
            "error" | "err" => Ok(Self::Error),
            _ => Err(ParseDiagnosticLevelError(s.to_string())),
        }
    }
}

/// Error type for parsing `DiagnosticLevel`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnosticLevelError(String);

impl fmt::Display for ParseDiagnosticLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid diagnostic level: '{}'", self.0)
    }
}

impl std::error::Error for ParseDiagnosticLevelError {}

/// How a tree should be rendered.
///
/// Similar to Flutter's `DiagnosticsTreeStyle`.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsTreeStyle;
///
/// let style = DiagnosticsTreeStyle::Sparse;
/// println!("{}", style); // "sparse"
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum DiagnosticsTreeStyle {
    /// A style that is appropriate for displaying sparse trees.
    Sparse,
    /// A style that is appropriate for displaying the properties of an object.
    Shallow,
    /// A style that is appropriate for displaying a tree.
    Dense,
    /// A style that is appropriate for displaying a single line.
    #[cfg_attr(feature = "serde", serde(rename = "singleline"))]
    SingleLine,
    /// A style that is appropriate for displaying an error.
    #[cfg_attr(feature = "serde", serde(rename = "errorproperty"))]
    ErrorProperty,
}

impl DiagnosticsTreeStyle {
    /// Returns the style as a lowercase string
    #[must_use]
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Sparse => "sparse",
            Self::Shallow => "shallow",
            Self::Dense => "dense",
            Self::SingleLine => "singleline",
            Self::ErrorProperty => "errorproperty",
        }
    }

    /// Checks if this is a compact style
    #[must_use]
    #[inline]
    pub const fn is_compact(&self) -> bool {
        matches!(self, Self::SingleLine | Self::Shallow)
    }
}

impl Default for DiagnosticsTreeStyle {
    #[inline]
    fn default() -> Self {
        Self::Sparse
    }
}

impl fmt::Display for DiagnosticsTreeStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for DiagnosticsTreeStyle {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for DiagnosticsTreeStyle {
    type Err = ParseDiagnosticsTreeStyleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sparse" => Ok(Self::Sparse),
            "shallow" => Ok(Self::Shallow),
            "dense" => Ok(Self::Dense),
            "singleline" | "single_line" | "single-line" => Ok(Self::SingleLine),
            "errorproperty" | "error_property" | "error-property" => Ok(Self::ErrorProperty),
            _ => Err(ParseDiagnosticsTreeStyleError(s.to_string())),
        }
    }
}

/// Error type for parsing `DiagnosticsTreeStyle`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnosticsTreeStyleError(String);

impl fmt::Display for ParseDiagnosticsTreeStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid diagnostics tree style: '{}'", self.0)
    }
}

impl std::error::Error for ParseDiagnosticsTreeStyleError {}

/// A diagnostic property
///
/// Similar to Flutter's `DiagnosticsProperty`.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsProperty;
///
/// let prop = DiagnosticsProperty::new("width", 100);
/// assert_eq!(prop.name(), "width");
/// assert_eq!(prop.value(), "100");
/// println!("{}", prop); // "width: 100"
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiagnosticsProperty {
    name: String,
    value: String,
    #[cfg_attr(feature = "serde", serde(default))]
    level: DiagnosticLevel,
    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    show_name: bool,
    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    show_separator: bool,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    default_value: Option<String>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    tooltip: Option<String>,
}

#[cfg(feature = "serde")]
const fn default_true() -> bool {
    true
}

impl DiagnosticsProperty {
    /// Create a new diagnostics property
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::DiagnosticsProperty;
    ///
    /// let prop = DiagnosticsProperty::new("width", 100);
    /// assert_eq!(prop.name(), "width");
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl fmt::Display) -> Self {
        Self {
            name: name.into(),
            value: value.to_string(),
            level: DiagnosticLevel::Info,
            show_name: true,
            show_separator: true,
            default_value: None,
            tooltip: None,
        }
    }

    /// Returns the property name
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the property value as a string
    #[must_use]
    #[inline]
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the diagnostic level
    #[must_use]
    #[inline]
    pub const fn level(&self) -> DiagnosticLevel {
        self.level
    }

    /// Returns the tooltip if present
    #[must_use]
    #[inline]
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    /// Checks if the property name should be shown
    #[must_use]
    #[inline]
    pub const fn shows_name(&self) -> bool {
        self.show_name
    }

    /// Checks if the separator should be shown
    #[must_use]
    #[inline]
    pub const fn shows_separator(&self) -> bool {
        self.show_separator
    }

    /// Set the diagnostic level (builder pattern)
    #[must_use]
    pub const fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    /// Hide the property name (builder pattern)
    #[must_use]
    pub const fn value_only(mut self) -> Self {
        self.show_name = false;
        self
    }

    /// Set a default value (builder pattern)
    #[must_use]
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }

    /// Set a tooltip (builder pattern)
    #[must_use]
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Checks if this property is hidden based on its default value
    #[must_use]
    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.default_value
            .as_ref()
            .is_some_and(|default| &self.value == default)
    }

    /// Checks if this property should be displayed at the given level
    #[must_use]
    #[inline]
    pub const fn is_visible_at_level(&self, min_level: DiagnosticLevel) -> bool {
        self.level as u8 >= min_level as u8
    }

    /// Format the property as a string with given style
    #[must_use]
    pub fn format_with_style(&self, style: DiagnosticsTreeStyle) -> String {
        match style {
            DiagnosticsTreeStyle::SingleLine => {
                if self.show_name {
                    if self.show_separator {
                        format!("{}: {}", self.name, self.value)
                    } else {
                        format!("{} {}", self.name, self.value)
                    }
                } else {
                    self.value.clone()
                }
            }
            _ => {
                if self.show_name {
                    format!("{}: {}", self.name, self.value)
                } else {
                    self.value.clone()
                }
            }
        }
    }
}

impl fmt::Display for DiagnosticsProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.format_with_style(DiagnosticsTreeStyle::SingleLine)
        )
    }
}

/// A node in the diagnostics tree
///
/// Similar to Flutter's `DiagnosticsNode`.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{DiagnosticsNode, DiagnosticsProperty};
///
/// let mut node = DiagnosticsNode::new("MyView");
/// node.add_property(DiagnosticsProperty::new("width", 100));
/// println!("{}", node);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiagnosticsNode {
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    name: Option<String>,
    #[cfg_attr(feature = "serde", serde(default))]
    properties: Vec<DiagnosticsProperty>,
    #[cfg_attr(feature = "serde", serde(default))]
    children: Vec<DiagnosticsNode>,
    #[cfg_attr(feature = "serde", serde(default))]
    level: DiagnosticLevel,
    #[cfg_attr(feature = "serde", serde(default))]
    style: DiagnosticsTreeStyle,
}

impl DiagnosticsNode {
    /// Create a new diagnostics node
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            properties: Vec::new(),
            children: Vec::new(),
            level: DiagnosticLevel::Info,
            style: DiagnosticsTreeStyle::Sparse,
        }
    }

    /// Create a node without a name
    #[must_use]
    pub const fn anonymous() -> Self {
        Self {
            name: None,
            properties: Vec::new(),
            children: Vec::new(),
            level: DiagnosticLevel::Info,
            style: DiagnosticsTreeStyle::Sparse,
        }
    }

    /// Returns the node name
    #[must_use]
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the properties
    #[must_use]
    #[inline]
    pub fn properties(&self) -> &[DiagnosticsProperty] {
        &self.properties
    }

    /// Returns mutable access to properties
    #[inline]
    pub const fn properties_mut(&mut self) -> &mut Vec<DiagnosticsProperty> {
        &mut self.properties
    }

    /// Returns the children
    #[must_use]
    #[inline]
    pub fn children(&self) -> &[Self] {
        &self.children
    }

    /// Returns mutable access to children
    #[inline]
    pub const fn children_mut(&mut self) -> &mut Vec<Self> {
        &mut self.children
    }

    /// Returns the diagnostic level
    #[must_use]
    #[inline]
    pub const fn level(&self) -> DiagnosticLevel {
        self.level
    }

    /// Returns the rendering style
    #[must_use]
    #[inline]
    pub const fn style(&self) -> DiagnosticsTreeStyle {
        self.style
    }

    /// Checks if this node has any properties
    #[must_use]
    #[inline]
    pub const fn has_properties(&self) -> bool {
        !self.properties.is_empty()
    }

    /// Checks if this node has any children
    #[must_use]
    #[inline]
    pub const fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Add a property
    pub fn add_property(&mut self, property: DiagnosticsProperty) {
        self.properties.push(property);
    }

    /// Add a child node
    pub fn add_child(&mut self, child: Self) {
        self.children.push(child);
    }

    /// Set the diagnostic level (builder pattern)
    #[must_use]
    pub const fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    /// Set the rendering style (builder pattern)
    #[must_use]
    pub const fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.style = style;
        self
    }

    /// Add a property (builder pattern)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::{DiagnosticsNode, DiagnosticsProperty};
    ///
    /// let node = DiagnosticsNode::new("MyView")
    ///     .property("width", 100)
    ///     .property("height", 50);
    /// ```
    #[must_use]
    pub fn property(mut self, name: impl Into<String>, value: impl fmt::Display) -> Self {
        self.properties.push(DiagnosticsProperty::new(name, value));
        self
    }

    /// Add a property with a custom `DiagnosticsProperty` (builder pattern)
    #[must_use]
    pub fn with_property(mut self, property: DiagnosticsProperty) -> Self {
        self.properties.push(property);
        self
    }

    /// Add a child node (builder pattern)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::DiagnosticsNode;
    ///
    /// let node = DiagnosticsNode::new("Parent")
    ///     .child(DiagnosticsNode::new("Child1"))
    ///     .child(DiagnosticsNode::new("Child2"));
    /// ```
    #[must_use]
    pub fn child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Add multiple children (builder pattern)
    #[must_use]
    pub fn with_children(mut self, children: impl IntoIterator<Item = Self>) -> Self {
        self.children.extend(children);
        self
    }

    /// Add a flag property (builder pattern)
    ///
    /// Only adds the property if the condition is true.
    #[must_use]
    pub fn flag(
        mut self,
        name: impl Into<String>,
        condition: bool,
        value: impl fmt::Display,
    ) -> Self {
        if condition {
            self.properties.push(DiagnosticsProperty::new(name, value));
        }
        self
    }

    /// Add an optional property (builder pattern)
    ///
    /// Only adds the property if the value is Some.
    #[must_use]
    pub fn optional<T: fmt::Display>(mut self, name: impl Into<String>, value: Option<T>) -> Self {
        if let Some(v) = value {
            self.properties.push(DiagnosticsProperty::new(name, v));
        }
        self
    }

    /// Convert to a deep string representation
    #[must_use]
    pub fn format_deep(&self, indent: usize) -> String {
        use std::fmt::Write;

        let mut result = String::new();
        let prefix = "  ".repeat(indent);

        if let Some(ref name) = self.name {
            let _ = writeln!(result, "{prefix}{name}");
        }

        for prop in &self.properties {
            if !prop.is_hidden() {
                let formatted = prop.format_with_style(self.style);
                let _ = writeln!(result, "{prefix}  {formatted}");
            }
        }

        for child in &self.children {
            result.push_str(&child.format_deep(indent + 1));
        }

        result
    }
}

impl Default for DiagnosticsNode {
    #[inline]
    fn default() -> Self {
        Self::anonymous()
    }
}

impl fmt::Display for DiagnosticsNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_deep(0))
    }
}

/// Trait for objects that can provide diagnostics information.
///
/// Similar to Flutter's `Diagnosticable`.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{Diagnosticable, DiagnosticsNode, DiagnosticsProperty};
///
/// #[derive(Debug)]
/// struct MyView {
///     width: i32,
///     height: i32,
/// }
///
/// impl Diagnosticable for MyView {
///     fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
///         properties.push(DiagnosticsProperty::new("width", self.width));
///         properties.push(DiagnosticsProperty::new("height", self.height));
///     }
/// }
/// ```
pub trait Diagnosticable: fmt::Debug {
    /// Create a diagnostics node for this object.
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let type_name = std::any::type_name::<Self>();
        let mut node = DiagnosticsNode::new(type_name);
        let mut builder = DiagnosticsBuilder::new();
        self.debug_fill_properties(&mut builder);
        *node.properties_mut() = builder.build();
        node
    }

    /// Collect diagnostic properties.
    fn debug_fill_properties(&self, _properties: &mut DiagnosticsBuilder) {
        // Override in implementations
    }
}

/// Helper builder for diagnostic properties.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsBuilder;
///
/// let mut builder = DiagnosticsBuilder::new();
/// builder.add("width", 100);
/// builder.add("height", 50);
/// builder.add_optional("title", Some("Test"));
/// let properties = builder.build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsBuilder {
    properties: Vec<DiagnosticsProperty>,
}

impl DiagnosticsBuilder {
    /// Create a new builder.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            properties: Vec::new(),
        }
    }

    /// Create a builder with capacity
    #[must_use]
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            properties: Vec::with_capacity(capacity),
        }
    }

    /// Add a property.
    pub fn add(&mut self, name: impl Into<String>, value: impl fmt::Display) -> &mut Self {
        self.properties.push(DiagnosticsProperty::new(name, value));
        self
    }

    /// Add a property with a specific level.
    pub fn add_with_level(
        &mut self,
        name: impl Into<String>,
        value: impl fmt::Display,
        level: DiagnosticLevel,
    ) -> &mut Self {
        self.properties
            .push(DiagnosticsProperty::new(name, value).with_level(level));
        self
    }

    /// Add a flag property (bool).
    pub fn add_flag(&mut self, name: impl Into<String>, value: bool, if_true: &str) -> &mut Self {
        if value {
            self.properties
                .push(DiagnosticsProperty::new(name, if_true));
        }
        self
    }

    /// Add an optional property.
    pub fn add_optional<T: fmt::Display>(
        &mut self,
        name: impl Into<String>,
        value: Option<T>,
    ) -> &mut Self {
        if let Some(v) = value {
            self.add(name, v);
        }
        self
    }

    /// Returns the number of properties
    #[must_use]
    #[inline]
    pub const fn len(&self) -> usize {
        self.properties.len()
    }

    /// Checks if the builder is empty
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// Build the properties list.
    #[must_use]
    pub fn build(self) -> Vec<DiagnosticsProperty> {
        self.properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_level_default() {
        assert_eq!(DiagnosticLevel::default(), DiagnosticLevel::Info);
    }

    #[test]
    fn test_diagnostic_level_display() {
        assert_eq!(format!("{}", DiagnosticLevel::Info), "info");
        assert_eq!(format!("{}", DiagnosticLevel::Error), "error");
    }

    #[test]
    fn test_diagnostic_level_as_str() {
        assert_eq!(DiagnosticLevel::Debug.as_str(), "debug");
        assert_eq!(DiagnosticLevel::Warning.as_str(), "warning");
    }

    #[test]
    fn test_diagnostic_level_from_str() {
        assert_eq!(
            "info".parse::<DiagnosticLevel>().unwrap(),
            DiagnosticLevel::Info
        );
        assert_eq!(
            "ERROR".parse::<DiagnosticLevel>().unwrap(),
            DiagnosticLevel::Error
        );
        assert_eq!(
            "warn".parse::<DiagnosticLevel>().unwrap(),
            DiagnosticLevel::Warning
        );
        assert!("invalid".parse::<DiagnosticLevel>().is_err());
    }

    #[test]
    fn test_diagnostic_level_predicates() {
        assert!(DiagnosticLevel::Error.is_error());
        assert!(!DiagnosticLevel::Info.is_error());
        assert!(DiagnosticLevel::Warning.is_warning());
        assert!(DiagnosticLevel::Info.is_visible());
        assert!(!DiagnosticLevel::Hidden.is_visible());
    }

    #[test]
    fn test_diagnostics_tree_style_default() {
        assert_eq!(
            DiagnosticsTreeStyle::default(),
            DiagnosticsTreeStyle::Sparse
        );
    }

    #[test]
    fn test_diagnostics_tree_style_display() {
        assert_eq!(format!("{}", DiagnosticsTreeStyle::Sparse), "sparse");
        assert_eq!(
            format!("{}", DiagnosticsTreeStyle::SingleLine),
            "singleline"
        );
    }

    #[test]
    fn test_diagnostics_tree_style_from_str() {
        assert_eq!(
            "sparse".parse::<DiagnosticsTreeStyle>().unwrap(),
            DiagnosticsTreeStyle::Sparse
        );
        assert_eq!(
            "single-line".parse::<DiagnosticsTreeStyle>().unwrap(),
            DiagnosticsTreeStyle::SingleLine
        );
    }

    #[test]
    fn test_diagnostics_tree_style_is_compact() {
        assert!(DiagnosticsTreeStyle::SingleLine.is_compact());
        assert!(DiagnosticsTreeStyle::Shallow.is_compact());
        assert!(!DiagnosticsTreeStyle::Dense.is_compact());
    }

    #[test]
    fn test_diagnostics_property() {
        let prop = DiagnosticsProperty::new("width", 100);
        assert_eq!(prop.name(), "width");
        assert_eq!(prop.value(), "100");
        assert_eq!(prop.level(), DiagnosticLevel::Info);
        assert!(!prop.is_hidden());
    }

    #[test]
    fn test_diagnostics_property_display() {
        let prop = DiagnosticsProperty::new("width", 100);
        assert_eq!(format!("{}", prop), "width: 100");
    }

    #[test]
    fn test_diagnostics_property_equality() {
        let prop1 = DiagnosticsProperty::new("width", 100);
        let prop2 = DiagnosticsProperty::new("width", 100);
        let prop3 = DiagnosticsProperty::new("height", 100);

        assert_eq!(prop1, prop2);
        assert_ne!(prop1, prop3);
    }

    #[test]
    fn test_diagnostics_property_with_default() {
        let prop = DiagnosticsProperty::new("width", 100).with_default("100");
        assert!(prop.is_hidden());

        let prop2 = DiagnosticsProperty::new("width", 200).with_default("100");
        assert!(!prop2.is_hidden());
    }

    #[test]
    fn test_diagnostics_node() {
        let mut node = DiagnosticsNode::new("MyView");
        node.add_property(DiagnosticsProperty::new("width", 100));
        node.add_property(DiagnosticsProperty::new("height", 50));

        assert_eq!(node.properties().len(), 2);
        assert_eq!(node.name().unwrap(), "MyView");
        assert!(node.has_properties());
        assert!(!node.has_children());
    }

    #[test]
    fn test_diagnostics_node_default() {
        let node = DiagnosticsNode::default();
        assert_eq!(node.name(), None);
        assert!(node.properties.is_empty());
        assert!(node.children.is_empty());
    }

    #[test]
    fn test_diagnostics_node_equality() {
        let mut node1 = DiagnosticsNode::new("Element");
        node1.add_property(DiagnosticsProperty::new("width", 100));

        let mut node2 = DiagnosticsNode::new("Element");
        node2.add_property(DiagnosticsProperty::new("width", 100));

        assert_eq!(node1, node2);
    }

    #[test]
    fn test_diagnostics_node_with_children() {
        let mut parent = DiagnosticsNode::new("Parent");
        parent.add_property(DiagnosticsProperty::new("id", 1));

        let mut child = DiagnosticsNode::new("Child");
        child.add_property(DiagnosticsProperty::new("name", "test"));

        parent = parent.child(child);

        assert_eq!(parent.children().len(), 1);
        assert!(parent.has_children());
        assert_eq!(parent.children()[0].name().unwrap(), "Child");
    }

    #[test]
    fn test_diagnostics_builder() {
        let mut builder = DiagnosticsBuilder::new();
        builder.add("width", 100);
        builder.add("height", 50);
        builder.add_optional("title", Some("Test"));
        builder.add_optional::<String>("empty", None);
        builder.add_flag("visible", true, "VISIBLE");
        builder.add_flag("hidden", false, "HIDDEN");

        assert_eq!(builder.len(), 4);
        assert!(!builder.is_empty());

        let props = builder.build();
        assert_eq!(props.len(), 4);
    }

    #[test]
    fn test_diagnostic_level_ordering() {
        assert!(DiagnosticLevel::Hidden < DiagnosticLevel::Debug);
        assert!(DiagnosticLevel::Debug < DiagnosticLevel::Info);
        assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Warning < DiagnosticLevel::Error);
    }

    #[test]
    fn test_diagnostics_tree_string() {
        let mut root = DiagnosticsNode::new("Root");
        root.add_property(DiagnosticsProperty::new("id", 1));

        let mut child = DiagnosticsNode::new("Child");
        child.add_property(DiagnosticsProperty::new("name", "test"));
        root = root.child(child);

        let output = root.format_deep(0);
        assert!(output.contains("Root"));
        assert!(output.contains("id: 1"));
        assert!(output.contains("Child"));
        assert!(output.contains("name: test"));
    }

    #[test]
    fn test_diagnostics_node_builder_pattern() {
        let node = DiagnosticsNode::new("MyView")
            .property("width", 100)
            .property("height", 50)
            .flag("visible", true, "VISIBLE")
            .flag("hidden", false, "HIDDEN")
            .optional("title", Some("Test"))
            .optional::<String>("empty", None)
            .with_level(DiagnosticLevel::Info)
            .with_style(DiagnosticsTreeStyle::Dense);

        assert_eq!(node.name().unwrap(), "MyView");
        assert_eq!(node.properties().len(), 4); // width, height, visible flag, title
        assert_eq!(node.level(), DiagnosticLevel::Info);
        assert_eq!(node.style(), DiagnosticsTreeStyle::Dense);
    }

    #[test]
    fn test_diagnostics_node_builder_with_children() {
        let node = DiagnosticsNode::new("Parent")
            .property("id", 1)
            .child(DiagnosticsNode::new("Child1").property("name", "first"))
            .child(DiagnosticsNode::new("Child2").property("name", "second"));

        assert_eq!(node.children().len(), 2);
        assert_eq!(node.children()[0].name().unwrap(), "Child1");
        assert_eq!(node.children()[1].name().unwrap(), "Child2");
    }

    #[test]
    fn test_diagnostics_node_builder_complex() {
        let tree = DiagnosticsNode::new("Container")
            .property("width", 800)
            .property("height", 600)
            .with_level(DiagnosticLevel::Info)
            .child(
                DiagnosticsNode::new("Row")
                    .property("spacing", 8)
                    .child(DiagnosticsNode::new("Text").property("content", "Hello"))
                    .child(DiagnosticsNode::new("Button").property("label", "Click")),
            )
            .child(
                DiagnosticsNode::new("Column")
                    .property("alignment", "center")
                    .child(DiagnosticsNode::new("Image").property("src", "logo.png")),
            );

        assert_eq!(tree.name().unwrap(), "Container");
        assert_eq!(tree.properties().len(), 2);
        assert_eq!(tree.children().len(), 2);

        let row = &tree.children()[0];
        assert_eq!(row.name().unwrap(), "Row");
        assert_eq!(row.children().len(), 2);

        let column = &tree.children()[1];
        assert_eq!(column.name().unwrap(), "Column");
        assert_eq!(column.children().len(), 1);
    }
}
