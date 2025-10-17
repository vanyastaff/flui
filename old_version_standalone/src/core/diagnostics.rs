//! Diagnostics and debugging support
//!
//! This module provides types for debugging and introspection,
//! similar to Flutter's diagnostics system.

use std::fmt;

/// The level of importance of a diagnostic message.
///
/// Similar to Flutter's `DiagnosticLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticLevel {
    /// Hidden diagnostic level.
    Hidden,

    /// A diagnostic that is likely to be low-value but where the diagnostic
    /// may provide value for debugging.
    Fine,

    /// A diagnostic that is likely to be low-value but where the diagnostic
    /// may provide value for debugging.
    Debug,

    /// Diagnostics that are probably useful for debugging but do not rise
    /// to the level of being "informational".
    Info,

    /// A diagnostic that is informational.
    Warning,

    /// A diagnostic that we want to bring to the attention of the user.
    Hint,

    /// A diagnostic that indicates an error.
    Error,
}

/// How a tree should be rendered.
///
/// Similar to Flutter's `DiagnosticsTreeStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticsTreeStyle {
    /// A style that is appropriate for displaying sparse trees.
    Sparse,

    /// A style that is appropriate for displaying the properties of an object.
    Shallow,

    /// A style that is appropriate for displaying a tree.
    Dense,

    /// A style that is appropriate for displaying a single line.
    SingleLine,

    /// A style that is appropriate for displaying an error.
    ErrorProperty,
}

/// A diagnostic property.
///
/// Similar to Flutter's `DiagnosticsProperty`.
#[derive(Debug, Clone)]
pub struct DiagnosticsProperty {
    /// Name of the property
    pub name: String,

    /// Value of the property as a string
    pub value: String,

    /// Diagnostic level
    pub level: DiagnosticLevel,

    /// Whether to show the name
    pub show_name: bool,

    /// Whether to show separator between name and value
    pub show_separator: bool,

    /// Default value (if this matches, the property may be hidden)
    pub default_value: Option<String>,

    /// Tooltip or description
    pub tooltip: Option<String>,
}

impl DiagnosticsProperty {
    /// Create a new diagnostics property.
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

    /// Create a property with a specific level.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    /// Create a property without showing the name.
    pub fn value_only(mut self) -> Self {
        self.show_name = false;
        self
    }

    /// Set a default value.
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }

    /// Set a tooltip.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Check if this property is hidden based on its default value.
    pub fn is_hidden(&self) -> bool {
        if let Some(ref default) = self.default_value {
            &self.value == default
        } else {
            false
        }
    }

    /// Format the property as a string.
    pub fn to_string_with_style(&self, style: DiagnosticsTreeStyle) -> String {
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

/// A node in the diagnostics tree.
///
/// Similar to Flutter's `DiagnosticsNode`.
#[derive(Debug, Clone)]
pub struct DiagnosticsNode {
    /// Name of this node
    pub name: Option<String>,

    /// Properties of this node
    pub properties: Vec<DiagnosticsProperty>,

    /// Child nodes
    pub children: Vec<DiagnosticsNode>,

    /// The level of this diagnostic
    pub level: DiagnosticLevel,

    /// The style for rendering this node
    pub style: DiagnosticsTreeStyle,
}

impl DiagnosticsNode {
    /// Create a new diagnostics node.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            properties: Vec::new(),
            children: Vec::new(),
            level: DiagnosticLevel::Info,
            style: DiagnosticsTreeStyle::Sparse,
        }
    }

    /// Create a node without a name.
    pub fn anonymous() -> Self {
        Self {
            name: None,
            properties: Vec::new(),
            children: Vec::new(),
            level: DiagnosticLevel::Info,
            style: DiagnosticsTreeStyle::Sparse,
        }
    }

    /// Add a property.
    pub fn add_property(&mut self, property: DiagnosticsProperty) {
        self.properties.push(property);
    }

    /// Add a child node.
    pub fn add_child(&mut self, child: DiagnosticsNode) {
        self.children.push(child);
    }

    /// Set the diagnostic level.
    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    /// Set the rendering style.
    pub fn with_style(mut self, style: DiagnosticsTreeStyle) -> Self {
        self.style = style;
        self
    }

    /// Convert to a string representation.
    pub fn to_string_deep(&self, indent: usize) -> String {
        let mut result = String::new();
        let prefix = "  ".repeat(indent);

        // Add name
        if let Some(ref name) = self.name {
            result.push_str(&format!("{}{}\n", prefix, name));
        }

        // Add properties
        for prop in &self.properties {
            if !prop.is_hidden() {
                result.push_str(&format!(
                    "{}  {}\n",
                    prefix,
                    prop.to_string_with_style(self.style)
                ));
            }
        }

        // Add children
        for child in &self.children {
            result.push_str(&child.to_string_deep(indent + 1));
        }

        result
    }
}

impl fmt::Display for DiagnosticsNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_deep(0))
    }
}

/// Trait for objects that can provide diagnostics information.
///
/// Similar to Flutter's `Diagnosticable`.
pub trait Diagnosticable: fmt::Debug {
    /// Create a diagnostics node for this object.
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let type_name = std::any::type_name::<Self>();
        DiagnosticsNode::new(type_name)
    }

    /// Collect diagnostic properties.
    fn debug_fill_properties(&self, _properties: &mut Vec<DiagnosticsProperty>) {
        // Override in implementations
    }
}

/// Helper builder for diagnostic properties.
pub struct DiagnosticsBuilder {
    properties: Vec<DiagnosticsProperty>,
}

impl DiagnosticsBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            properties: Vec::new(),
        }
    }

    /// Add a property.
    pub fn add(&mut self, name: impl Into<String>, value: impl fmt::Display) {
        self.properties.push(DiagnosticsProperty::new(name, value));
    }

    /// Add a property with a specific level.
    pub fn add_with_level(
        &mut self,
        name: impl Into<String>,
        value: impl fmt::Display,
        level: DiagnosticLevel,
    ) {
        self.properties.push(
            DiagnosticsProperty::new(name, value).with_level(level)
        );
    }

    /// Add a flag property (bool).
    pub fn add_flag(&mut self, name: impl Into<String>, value: bool, if_true: &str) {
        if value {
            self.properties.push(
                DiagnosticsProperty::new(name, if_true)
            );
        }
    }

    /// Add an optional property.
    pub fn add_optional<T: fmt::Display>(
        &mut self,
        name: impl Into<String>,
        value: Option<T>,
    ) {
        if let Some(v) = value {
            self.add(name, v);
        }
    }

    /// Build the properties list.
    pub fn build(self) -> Vec<DiagnosticsProperty> {
        self.properties
    }
}

impl Default for DiagnosticsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostics_property() {
        let prop = DiagnosticsProperty::new("width", 100);
        assert_eq!(prop.name, "width");
        assert_eq!(prop.value, "100");
        assert_eq!(prop.level, DiagnosticLevel::Info);
        assert!(!prop.is_hidden());
    }

    #[test]
    fn test_diagnostics_property_with_default() {
        let prop = DiagnosticsProperty::new("width", 100)
            .with_default("100");
        assert!(prop.is_hidden());

        let prop2 = DiagnosticsProperty::new("width", 200)
            .with_default("100");
        assert!(!prop2.is_hidden());
    }

    #[test]
    fn test_diagnostics_node() {
        let mut node = DiagnosticsNode::new("MyWidget");
        node.add_property(DiagnosticsProperty::new("width", 100));
        node.add_property(DiagnosticsProperty::new("height", 50));

        assert_eq!(node.properties.len(), 2);
        assert_eq!(node.name.as_ref().unwrap(), "MyWidget");
    }

    #[test]
    fn test_diagnostics_node_with_children() {
        let mut parent = DiagnosticsNode::new("Parent");
        parent.add_property(DiagnosticsProperty::new("id", 1));

        let mut child = DiagnosticsNode::new("Child");
        child.add_property(DiagnosticsProperty::new("name", "test"));

        parent.add_child(child);

        assert_eq!(parent.children.len(), 1);
        assert_eq!(parent.children[0].name.as_ref().unwrap(), "Child");
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

        let props = builder.build();
        assert_eq!(props.len(), 4); // width, height, title, visible flag
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
        root.add_child(child);

        let output = root.to_string_deep(0);
        assert!(output.contains("Root"));
        assert!(output.contains("id: 1"));
        assert!(output.contains("Child"));
        assert!(output.contains("name: test"));
    }
}
