//! Diagnostics and debugging support
//!
//! This module provides types for debugging and introspection.

use std::{fmt, str::FromStr};

/// The level of importance of a diagnostic message.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticLevel;
///
/// let level = DiagnosticLevel::Info;
/// assert!(level > DiagnosticLevel::Debug);
/// assert_eq!(level.to_string(), "info");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum DiagnosticLevel {
    /// Hidden diagnostic level.
    Hidden,
    /// A diagnostic that is likely to be low-value but may provide debugging
    /// value.
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
            _ => Err(ParseDiagnosticLevelError(s.into())),
        }
    }
}

/// Error type for parsing `DiagnosticLevel`.
///
/// Audit I-19: payload is `Box<str>` rather than `String` — the
/// invalid-input description is read-only after construction, so
/// the heap layout of `String` (16-byte triple-pointer for the
/// always-empty growth space) wastes 8 bytes per error compared to
/// `Box<str>` (single thin pointer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnosticLevelError(Box<str>);

impl fmt::Display for ParseDiagnosticLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid diagnostic level: '{}'", self.0)
    }
}

impl std::error::Error for ParseDiagnosticLevelError {}

/// How a tree should be rendered.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsTreeStyle;
///
/// let style = DiagnosticsTreeStyle::Sparse;
/// assert_eq!(style.to_string(), "sparse");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[non_exhaustive]
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
            _ => Err(ParseDiagnosticsTreeStyleError(s.into())),
        }
    }
}

/// Error type for parsing `DiagnosticsTreeStyle`.
///
/// Audit I-19: payload `Box<str>` not `String` — same rationale as
/// `ParseDiagnosticLevelError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnosticsTreeStyleError(Box<str>);

impl fmt::Display for ParseDiagnosticsTreeStyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid diagnostics tree style: '{}'", self.0)
    }
}

impl std::error::Error for ParseDiagnosticsTreeStyleError {}

/// The kind of a diagnostics property, determining how it is displayed.
///
/// Each kind of property (enum, flag, iterable, etc.) is an enum variant
/// rather than a subclass.
///
/// The `Generic` variant is the fallback for all types not explicitly listed.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsPropertyKind;
///
/// let kind = DiagnosticsPropertyKind::Iterable { count: 3 };
/// assert_eq!(kind, DiagnosticsPropertyKind::Iterable { count: 3 });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum DiagnosticsPropertyKind {
    /// A generic property displayed as `{name}: {value:?}`.
    Generic,
    /// An enum property; `description` overrides the formatted value string.
    Enum {
        /// Optional human-readable description of the current enum variant.
        description: Option<std::borrow::Cow<'static, str>>,
    },
    /// A boolean flag property; displayed as `{name}` (true) or omitted (false).
    Flag,
    /// An iterable property; `count` is the number of elements.
    Iterable {
        /// The number of elements in the iterable.
        count: usize,
    },
    /// An optional reference; displayed as `{name}: <null>` when absent.
    OptionalRef,
    /// A stack of strings (e.g. stack traces).
    Stack,
    /// A double/float with an optional unit (e.g. `"dp"`, `"px"`).
    Double {
        /// Optional unit label appended to the formatted value.
        unit: Option<std::borrow::Cow<'static, str>>,
    },
    /// An integer with an optional unit.
    Int {
        /// Optional unit label appended to the formatted value.
        unit: Option<std::borrow::Cow<'static, str>>,
    },
    /// A color value (RGBA hex display).
    Color,
    /// An `Offset` / `Point2D` value.
    Offset,
    /// A `Rect` value.
    Rect,
    /// A `Size` value.
    Size,
}

impl Default for DiagnosticsPropertyKind {
    #[inline]
    fn default() -> Self {
        Self::Generic
    }
}

/// A diagnostic property
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsProperty;
///
/// let prop = DiagnosticsProperty::new("width", 100);
/// assert_eq!(prop.name(), "width");
/// assert_eq!(prop.value(), "100");
/// assert_eq!(prop.to_string(), "width: 100");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiagnosticsProperty {
    name: String,
    value: String,
    #[cfg_attr(feature = "serde", serde(default))]
    level: DiagnosticLevel,
    /// The typed kind of this property, determining how it is displayed.
    ///
    /// Defaults to [`DiagnosticsPropertyKind::Generic`] for properties built
    /// via [`DiagnosticsProperty::new`], preserving backwards compatibility.
    #[cfg_attr(feature = "serde", serde(default))]
    pub kind: DiagnosticsPropertyKind,
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
            kind: DiagnosticsPropertyKind::Generic,
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

    /// Set the typed property kind (builder pattern)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_foundation::{DiagnosticsProperty, DiagnosticsPropertyKind};
    ///
    /// let prop = DiagnosticsProperty::new("visible", "true")
    ///     .with_kind(DiagnosticsPropertyKind::Flag);
    /// assert_eq!(prop.kind, DiagnosticsPropertyKind::Flag);
    /// ```
    #[must_use]
    pub fn with_kind(mut self, kind: DiagnosticsPropertyKind) -> Self {
        self.kind = kind;
        self
    }

    /// Returns the typed property kind
    #[must_use]
    #[inline]
    pub const fn kind(&self) -> &DiagnosticsPropertyKind {
        &self.kind
    }

    /// Hide the property name (builder pattern)
    #[must_use]
    pub const fn value_only(mut self) -> Self {
        self.show_name = false;
        self
    }

    /// Omit the `name: value` separator (builder pattern).
    ///
    /// Used by [`DiagnosticsPropertyKind::Flag`] so true flags render as the
    /// property name only.
    #[must_use]
    pub const fn without_separator(mut self) -> Self {
        self.show_separator = false;
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
        if self.is_hidden() {
            return String::new();
        }

        match &self.kind {
            DiagnosticsPropertyKind::Flag => {
                if self.show_name {
                    if self.show_separator {
                        format!("{}: {}", self.name, self.value)
                    } else {
                        self.name.clone()
                    }
                } else {
                    self.value.clone()
                }
            }
            _ => match style {
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
            },
        }
    }
}

/// Failure modes for [`DiagnosticsNode::find_descendant_unique`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescendantLookupError {
    /// No descendant matched the requested name.
    NotFound,
    /// More than one descendant matched the requested name.
    Ambiguous,
}

impl std::error::Error for DescendantLookupError {}

impl fmt::Display for DescendantLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("diagnostics descendant not found"),
            Self::Ambiguous => f.write_str("diagnostics descendant name is ambiguous"),
        }
    }
}

fn walk_descendant<'a>(
    node: &'a DiagnosticsNode,
    name: &str,
    found: &mut Option<&'a DiagnosticsNode>,
) -> bool {
    // Compare base names (before any `<...>`): a diagnostics node's own name
    // keeps full generic fidelity (e.g. "RenderViewport<ScrollPosition>"),
    // but a caller querying "by render type" wants the base name regardless
    // of which generic argument a render object happens to be monomorphized
    // over — normalizing both sides also lets a caller pass the full
    // generic name through unharmed if it wants to.
    if node.name.as_deref().map(base_type_name) == Some(base_type_name(name)) {
        if found.is_some() {
            return true;
        }
        *found = Some(node);
    }
    for child in &node.children {
        if walk_descendant(child, name, found) {
            return true;
        }
    }
    false
}

/// The part of `type_name` before its first `<`, if any — the base name
/// ignoring generic parameters (`"RenderViewport<ScrollPosition>"` ->
/// `"RenderViewport"`; `"RenderColoredBox"` -> `"RenderColoredBox"` unchanged).
fn base_type_name(type_name: &str) -> &str {
    type_name.split('<').next().unwrap_or(type_name)
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
/// # Examples
///
/// ```rust
/// use flui_foundation::{DiagnosticsNode, DiagnosticsProperty};
///
/// let mut node = DiagnosticsNode::new("MyView");
/// node.add_property(DiagnosticsProperty::new("width", 100));
/// let rendered = node.to_string();
/// assert!(rendered.contains("MyView"));
/// assert!(rendered.contains("width"));
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

    /// Returns the value of the first property named `name`, if present.
    ///
    /// Convenience for structured assertions over a diagnostics tree
    /// (instead of substring-matching the rendered dump).
    #[must_use]
    pub fn get_property(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|property| property.name() == name)
            .map(DiagnosticsProperty::value)
    }

    /// Returns the first child node named `name`, if present. Compares base
    /// names (before any `<...>`) — see [`find_descendant`](Self::find_descendant).
    #[must_use]
    pub fn find_child(&self, name: &str) -> Option<&Self> {
        self.children
            .iter()
            .find(|child| child.name().map(base_type_name) == Some(base_type_name(name)))
    }

    /// Returns the first descendant node named `name` (depth-first), if present.
    #[must_use]
    pub fn find_descendant(&self, name: &str) -> Option<&Self> {
        self.find_descendant_unique(name).ok()
    }

    /// Returns the sole descendant named `name` (depth-first).
    ///
    /// # Errors
    ///
    /// - [`DescendantLookupError::NotFound`] when no node matches `name`.
    /// - [`DescendantLookupError::Ambiguous`] when more than one node matches.
    pub fn find_descendant_unique(&self, name: &str) -> Result<&Self, DescendantLookupError> {
        let mut found: Option<&DiagnosticsNode> = None;
        if walk_descendant(self, name, &mut found) {
            Err(DescendantLookupError::Ambiguous)
        } else {
            found.ok_or(DescendantLookupError::NotFound)
        }
    }

    /// Returns the named property record, if present.
    #[must_use]
    pub fn find_property(&self, name: &str) -> Option<&DiagnosticsProperty> {
        self.properties
            .iter()
            .find(|property| property.name() == name)
    }

    /// Parses the named property as `f64`, if present and parseable.
    ///
    /// Respects [`DiagnosticsPropertyKind::Double`] / [`DiagnosticsPropertyKind::Int`]
    /// unit suffixes (e.g. `"25px"` → `25.0`).
    #[must_use]
    pub fn get_property_f64(&self, name: &str) -> Option<f64> {
        self.find_property(name)
            .and_then(parse_numeric_property_value)
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

    /// Checks if this node should be displayed at the given minimum level.
    #[must_use]
    #[inline]
    pub const fn is_visible_at_level(&self, min_level: DiagnosticLevel) -> bool {
        self.level as u8 >= min_level as u8
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

    /// Convert to a deep string representation (all non-hidden properties).
    #[must_use]
    pub fn format_deep(&self, indent: usize) -> String {
        self.format_deep_filtered(indent, DiagnosticLevel::Hidden)
    }

    /// Convert to a deep string representation, omitting properties and nodes
    /// below `min_level` and properties equal to their default value.
    #[must_use]
    pub fn format_deep_filtered(&self, indent: usize, min_level: DiagnosticLevel) -> String {
        use std::fmt::Write;

        if !self.is_visible_at_level(min_level) {
            return String::new();
        }

        let mut result = String::new();
        let prefix = "  ".repeat(indent);

        if let Some(ref name) = self.name {
            let _ = writeln!(result, "{prefix}{name}");
        }

        for prop in &self.properties {
            if prop.is_hidden() || !prop.is_visible_at_level(min_level) {
                continue;
            }
            let formatted = prop.format_with_style(self.style);
            if !formatted.is_empty() {
                let _ = writeln!(result, "{prefix}  {formatted}");
            }
        }

        for child in &self.children {
            result.push_str(&child.format_deep_filtered(indent + 1, min_level));
        }

        result
    }

    /// Renders the full tree from the root (same as [`fmt::Display`]).
    #[must_use]
    pub fn to_string_deep(&self) -> String {
        self.format_deep(0)
    }

    /// Renders the tree, omitting diagnostics below `min_level`.
    #[must_use]
    pub fn to_string_deep_at_level(&self, min_level: DiagnosticLevel) -> String {
        self.format_deep_filtered(0, min_level)
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
/// # Examples
///
/// ```rust
/// use flui_foundation::{
///     Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, DiagnosticsProperty,
/// };
///
/// #[derive(Debug)]
/// struct MyView {
///     width: i32,
///     height: i32,
/// }
///
/// impl Diagnosticable for MyView {
///     fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
///         builder.add("width", self.width);
///         builder.add("height", self.height);
///     }
/// }
/// ```
pub trait Diagnosticable: fmt::Debug {
    /// Create a diagnostics node for this object.
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let type_name = short_diagnostic_type_name(std::any::type_name::<Self>());
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

/// Strips the module path from `full` (a `std::any::type_name::<Self>()`
/// output) and from every generic argument nested inside it, keeping the
/// bare type names — the same shape Dart's `runtimeType` prints (generics
/// visible, no library prefixes).
///
/// Examples: `"a::B"` -> `"B"`; `"a::B<c::D>"` -> `"B<D>"`;
/// `"a::B<c::D, e::F>"` -> `"B<D, F>"`; `"a::B<c::D<e::F>>"` -> `"B<D<F>>"`;
/// a lifetime argument passes through unchanged (`"Borrowed<'_>"` stays
/// `"Borrowed<'_>"` — it has no module path to strip).
///
/// A plain `rsplit("::")` over the whole string gets this wrong: the LAST
/// `::` in `"a::B<c::D>"` sits inside the generic argument, so a bare
/// rsplit yields `"D>"`, not `"B<D>"`. And truncating at the first `<`
/// (an earlier version of this function did) throws the generic away
/// entirely, which loses real information — `RenderViewport<ScrollPosition>`
/// stops being distinguishable from any other `RenderViewport<O>`.
///
/// Implementation: a single left-to-right scan. `full` is split into
/// maximal runs of "path characters" (`[A-Za-z0-9_:']`) — each run is one
/// path segment or one lifetime — separated by structural characters
/// (`<`, `>`, `,`, whitespace, …), which pass through unchanged. Each path
/// run starting with `'` is a lifetime and passes through as-is; every
/// other run is reduced to the text after its rightmost `::` (or kept
/// whole if it has none, e.g. `u32`). No regex, no recursion — the `<`/`>`
/// nesting doesn't need to be tracked because runs never cross a
/// structural character, so "nested" generics fall out of the scan for
/// free.
fn short_diagnostic_type_name(full: &str) -> String {
    let mut short = String::with_capacity(full.len());
    let mut run = String::new();
    for ch in full.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == ':' || ch == '\'' {
            run.push(ch);
            continue;
        }
        push_shortened_run(&mut short, &run);
        run.clear();
        short.push(ch);
    }
    push_shortened_run(&mut short, &run);
    short
}

/// Appends `run` to `short`, shortened to its bare name unless it's a
/// lifetime (starts with `'`, passed through verbatim). A no-op on an
/// empty `run` (two structural characters in direct succession, e.g. `>>`).
fn push_shortened_run(short: &mut String, run: &str) {
    if run.is_empty() {
        return;
    }
    if let Some(lifetime) = run.strip_prefix('\'') {
        short.push('\'');
        short.push_str(lifetime);
    } else {
        short.push_str(run.rsplit("::").next().unwrap_or(run));
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

    /// Add a flag property (bool). Omitted when `value` is false.
    ///
    /// Uses [`DiagnosticsPropertyKind::Flag`] so tree renderers can format the
    /// property without a redundant `true` suffix.
    pub fn add_flag(&mut self, name: impl Into<String>, value: bool, if_true: &str) -> &mut Self {
        if value {
            self.properties.push(
                DiagnosticsProperty::new(name, if_true)
                    .with_kind(DiagnosticsPropertyKind::Flag)
                    .without_separator(),
            );
        }
        self
    }

    /// Add a property that is hidden when equal to `default`.
    pub fn add_default(
        &mut self,
        name: impl Into<String>,
        value: impl fmt::Display,
        default: impl Into<String>,
    ) -> &mut Self {
        self.properties
            .push(DiagnosticsProperty::new(name, value).with_default(default.into()));
        self
    }

    /// Add an enum-like property (`Debug` formatted) with [`DiagnosticsPropertyKind::Enum`].
    pub fn add_enum(&mut self, name: impl Into<String>, value: impl fmt::Debug) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new(name, format!("{value:?}"))
                .with_kind(DiagnosticsPropertyKind::Enum { description: None }),
        );
        self
    }

    /// Add an enum property hidden when it equals `default`.
    pub fn add_default_enum<T: fmt::Debug>(
        &mut self,
        name: impl Into<String>,
        value: T,
        default: T,
    ) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new(name, format!("{value:?}"))
                .with_default(format!("{default:?}"))
                .with_kind(DiagnosticsPropertyKind::Enum { description: None }),
        );
        self
    }

    /// Add a floating-point property with an optional unit suffix.
    pub fn add_double(
        &mut self,
        name: impl Into<String>,
        value: f64,
        unit: Option<&'static str>,
    ) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new(name, format_double(value, unit)).with_kind(
                DiagnosticsPropertyKind::Double {
                    unit: unit.map(std::borrow::Cow::Borrowed),
                },
            ),
        );
        self
    }

    /// Add a floating-point property hidden when equal to `default`.
    pub fn add_default_double(
        &mut self,
        name: impl Into<String>,
        value: f64,
        default: f64,
        unit: Option<&'static str>,
    ) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new(name, format_double(value, unit))
                .with_default(format_double(default, unit))
                .with_kind(DiagnosticsPropertyKind::Double {
                    unit: unit.map(std::borrow::Cow::Borrowed),
                }),
        );
        self
    }

    /// Add an integer property with an optional unit suffix.
    pub fn add_int(
        &mut self,
        name: impl Into<String>,
        value: i64,
        unit: Option<&'static str>,
    ) -> &mut Self {
        let formatted = match unit {
            Some(u) => format!("{value}{u}"),
            None => format!("{value}"),
        };
        self.properties
            .push(DiagnosticsProperty::new(name, formatted).with_kind(
                DiagnosticsPropertyKind::Int {
                    unit: unit.map(std::borrow::Cow::Borrowed),
                },
            ));
        self
    }

    /// Add a size property (`width x height`).
    pub fn add_size(
        &mut self,
        name: impl Into<String>,
        width: impl fmt::Display,
        height: impl fmt::Display,
    ) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new(name, format!("{width} x {height}"))
                .with_kind(DiagnosticsPropertyKind::Size),
        );
        self
    }

    /// Add a color property (RGBA display).
    pub fn add_color(&mut self, name: impl Into<String>, value: impl fmt::Display) -> &mut Self {
        self.properties
            .push(DiagnosticsProperty::new(name, value).with_kind(DiagnosticsPropertyKind::Color));
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

#[inline]
fn format_double(value: f64, unit: Option<&str>) -> String {
    match unit {
        Some(u) => format!("{value}{u}"),
        None => format!("{value}"),
    }
}

/// Parses a numeric diagnostics property, stripping a typed unit suffix when
/// present.
fn parse_numeric_property_value(property: &DiagnosticsProperty) -> Option<f64> {
    let raw = property.value();
    let numeric = match property.kind() {
        DiagnosticsPropertyKind::Double { unit } | DiagnosticsPropertyKind::Int { unit } => {
            match unit {
                Some(suffix) if raw.ends_with(suffix.as_ref()) => &raw[..raw.len() - suffix.len()],
                _ => raw,
            }
        }
        _ => raw,
    };
    numeric.trim().parse().ok()
}

// ============================================================================
// DEBUG PAINT CONFIGURATION
// ============================================================================

/// Configuration for visual debug overlays during painting.
///
/// When enabled, the paint pipeline draws additional visual indicators
/// to help debug layout and hit-testing issues:
///
/// - **Paint bounds**: a colored rectangle around each render object
/// - **Baseline indicators**: horizontal lines at baseline positions
/// - **Overflow indicators**: yellow/black stripes for overflowing content
/// - **Hit-test areas**: semi-transparent overlays for hittable regions
///
/// # Usage
///
/// ```ignore
/// use flui_foundation::DebugPaintConfig;
///
/// let config = DebugPaintConfig::all_enabled();
/// if config.show_paint_bounds {
///     // Draw paint bounds rectangle
/// }
/// ```
///
/// # Feature Gate
///
/// Debug paint overlays are only active when the `debug-paint` feature
/// is enabled on `flui-foundation`. In release builds without the
/// feature, all fields are `false` and the config is a zero-cost
/// no-op.
// Four independent debug-overlay toggles, not a state machine — each
// overlay is orthogonal and combined freely. A bitflags/enum would obscure,
// not clarify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools)]
pub struct DebugPaintConfig {
    /// Draw a colored rectangle around each render object's paint bounds.
    pub show_paint_bounds: bool,
    /// Draw horizontal lines at baseline positions.
    pub show_baselines: bool,
    /// Draw yellow/black stripes for overflowing content.
    pub show_overflow: bool,
    /// Draw semi-transparent overlays for hittable regions.
    pub show_hit_test_areas: bool,
}

impl DebugPaintConfig {
    /// All overlays disabled (default for release builds).
    pub const NONE: Self = Self {
        show_paint_bounds: false,
        show_baselines: false,
        show_overflow: false,
        show_hit_test_areas: false,
    };

    /// All overlays enabled (typical for debug builds).
    pub const ALL: Self = Self {
        show_paint_bounds: true,
        show_baselines: true,
        show_overflow: true,
        show_hit_test_areas: true,
    };

    /// Creates a config with all overlays enabled.
    #[must_use]
    pub const fn all_enabled() -> Self {
        Self::ALL
    }

    /// Creates a config with all overlays disabled.
    #[must_use]
    pub const fn all_disabled() -> Self {
        Self::NONE
    }

    /// Returns `true` if any overlay is enabled.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.show_paint_bounds
            || self.show_baselines
            || self.show_overflow
            || self.show_hit_test_areas
    }
}

impl Default for DebugPaintConfig {
    fn default() -> Self {
        // Default to enabled in debug builds, disabled in release.
        #[cfg(debug_assertions)]
        {
            Self::ALL
        }
        #[cfg(not(debug_assertions))]
        {
            Self::NONE
        }
    }
}
