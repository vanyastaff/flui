//! Diagnostics and debugging support
//!
//! This module provides types for debugging and introspection,
//! similar to Flutter's diagnostics system.

use std::{fmt, str::FromStr};

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
/// assert_eq!(level.to_string(), "info");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
/// Similar to Flutter's `DiagnosticsTreeStyle`.
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
/// Mirrors Flutter's typed `DiagnosticsProperty<T>` subclass hierarchy
/// (`EnumProperty`, `FlagProperty`, `IterableProperty`, etc.) but as an
/// enum variant instead of class inheritance.
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

/// The typed value of a [`DiagnosticsProperty`].
///
/// Carries the structured data so the inspector's JSON serialization is
/// faithful (full precision, typed shapes) while the text renderer normalises
/// at the [`fmt::Display`] boundary only (floats → 2 decimal places,
/// colors → `#RRGGBBAA`, etc.).
///
/// The `Str` variant is the back-compat path: [`DiagnosticsProperty::new`]
/// always constructs it, so all existing `Diagnosticable` impls continue to
/// compile and behave identically.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::DiagnosticsValue;
///
/// let v = DiagnosticsValue::Float(0.333_333);
/// assert_eq!(v.to_string(), "0.33");           // normalised for text
/// assert_eq!(DiagnosticsValue::Str("hello".into()).to_string(), "hello");
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum DiagnosticsValue {
    /// Absent / null value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Signed 64-bit integer.
    Int(i64),
    /// 64-bit float — serialised at full precision; displayed at 2 d.p.
    Float(f64),
    /// Generic string (the back-compat variant produced by
    /// [`DiagnosticsProperty::new`]).
    Str(String),
    /// RGBA colour, each channel `0–255`.
    Color {
        /// Red channel `0–255`.
        r: u8,
        /// Green channel `0–255`.
        g: u8,
        /// Blue channel `0–255`.
        b: u8,
        /// Alpha channel `0–255` (255 = fully opaque).
        a: u8,
    },
    /// Axis-aligned rectangle: origin (`x`, `y`) + extent (`w`, `h`).
    Rect {
        /// Left edge (origin x).
        x: f64,
        /// Top edge (origin y).
        y: f64,
        /// Width.
        w: f64,
        /// Height.
        h: f64,
    },
    /// 2-D offset / point.
    Offset {
        /// Horizontal component.
        x: f64,
        /// Vertical component.
        y: f64,
    },
    /// 2-D size.
    Size {
        /// Width.
        w: f64,
        /// Height.
        h: f64,
    },
    /// Ordered list of diagnostic values.
    List(Vec<DiagnosticsValue>),
    /// Inline nested properties (for sub-objects that don't warrant a full
    /// child node).
    Nested(Vec<DiagnosticsProperty>),
}

impl fmt::Display for DiagnosticsValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            // Two decimal places for human-readable text; JSON gets full precision
            // via the typed serialisation.
            Self::Float(v) => write!(f, "{v:.2}"),
            Self::Str(s) => f.write_str(s),
            Self::Color { r, g, b, a } => write!(f, "#{r:02X}{g:02X}{b:02X}{a:02X}"),
            Self::Rect { x, y, w, h } => write!(f, "({x:.1},{y:.1},{w:.1},{h:.1})"),
            Self::Offset { x, y } => write!(f, "({x:.1},{y:.1})"),
            Self::Size { w, h } => write!(f, "{w:.1}×{h:.1}"),
            Self::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Self::Nested(props) => {
                write!(f, "{{")?;
                for (i, prop) in props.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", prop.name())?;
                    write!(f, ": {}", prop.value())?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl From<&str> for DiagnosticsValue {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}

impl From<String> for DiagnosticsValue {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

impl From<f64> for DiagnosticsValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<i64> for DiagnosticsValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<bool> for DiagnosticsValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

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
/// assert_eq!(prop.to_string(), "width: 100");
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct DiagnosticsProperty {
    name: String,
    value: DiagnosticsValue,
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
    /// The display string to compare against when checking
    /// [`DiagnosticsProperty::is_hidden`]. Stored as a plain string so
    /// callers can pass `"0"` / `"false"` etc. without knowing the typed
    /// variant.
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
            value: DiagnosticsValue::Str(value.to_string()),
            level: DiagnosticLevel::Info,
            kind: DiagnosticsPropertyKind::Generic,
            show_name: true,
            show_separator: true,
            default_value: None,
            tooltip: None,
        }
    }

    /// Creates a property with an explicit typed value.
    ///
    /// Prefer the typed [`DiagnosticsBuilder`] methods (`add_rect`,
    /// `add_color_rgba`, etc.) over calling this directly.
    #[must_use]
    pub fn new_typed(name: impl Into<String>, value: DiagnosticsValue) -> Self {
        Self {
            name: name.into(),
            value,
            level: DiagnosticLevel::Info,
            kind: DiagnosticsPropertyKind::Generic,
            show_name: true,
            show_separator: true,
            default_value: None,
            tooltip: None,
        }
    }

    /// Returns the property name.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the property value as a normalised display string.
    ///
    /// This is the back-compat accessor: it always returns the same text
    /// that [`fmt::Display`] would produce for the property line (the
    /// `name: value` format uses this). For faithful typed access use
    /// [`value_typed`](Self::value_typed).
    #[must_use]
    #[inline]
    pub fn value(&self) -> String {
        self.value.to_string()
    }

    /// Returns the typed value, giving the inspector faithful structured data.
    #[must_use]
    #[inline]
    pub fn value_typed(&self) -> &DiagnosticsValue {
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

    /// Checks if this property is hidden based on its default value.
    ///
    /// Comparison is against the normalised display string so callers can
    /// pass `"0"`, `"false"`, etc. without coupling to the typed variant.
    #[must_use]
    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.default_value
            .as_ref()
            .is_some_and(|default| self.value.to_string() == *default)
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
                    self.value()
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
                        self.value()
                    }
                }
                _ => {
                    if self.show_name {
                        format!("{}: {}", self.name, self.value)
                    } else {
                        self.value()
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
    if node.name.as_deref() == Some(name) {
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
/// let rendered = node.to_string();
/// assert!(rendered.contains("MyView"));
/// assert!(rendered.contains("width"));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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

    /// Returns the display string of the first property named `name`, if present.
    ///
    /// Convenience for structured assertions over a diagnostics tree
    /// (instead of substring-matching the rendered dump). For typed access
    /// use [`find_property`](Self::find_property) and then
    /// [`value_typed`](DiagnosticsProperty::value_typed).
    #[must_use]
    pub fn get_property(&self, name: &str) -> Option<String> {
        self.properties
            .iter()
            .find(|property| property.name() == name)
            .map(DiagnosticsProperty::value)
    }

    /// Returns the first child node named `name`, if present.
    #[must_use]
    pub fn find_child(&self, name: &str) -> Option<&Self> {
        self.children
            .iter()
            .find(|child| child.name() == Some(name))
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

    /// Exports this diagnostics tree as a JSON string.
    ///
    /// Produces a structured JSON representation suitable for devtools
    /// consumption. Each node has `name`, `properties`, `children`,
    /// and `level` fields.
    ///
    /// # Example output
    ///
    /// ```json
    /// {
    ///   "name": "RenderPadding",
    ///   "level": "info",
    ///   "properties": {"padding": "16px"},
    ///   "children": []
    /// }
    /// ```
    #[must_use]
    pub fn to_json(&self) -> String {
        fn write_json(node: &DiagnosticsNode, buf: &mut String, indent: usize) -> std::fmt::Result {
            use std::fmt::Write as _;

            let pad = " ".repeat(indent);
            writeln!(buf, "{pad}{{")?;
            writeln!(
                buf,
                "{pad}  \"name\": \"{}\",",
                escape_json(node.name().unwrap_or(""))
            )?;
            writeln!(buf, "{pad}  \"level\": \"{}\",", node.level.as_str())?;

            // Properties
            write!(buf, "{pad}  \"properties\": {{")?;
            let props = node.properties();
            for (i, prop) in props.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                buf.push('\n');
                write!(
                    buf,
                    "{pad}    \"{}\": \"{}\"",
                    escape_json(prop.name()),
                    escape_json(&prop.value())
                )?;
            }
            if !props.is_empty() {
                buf.push('\n');
                write!(buf, "{pad}  ")?;
            }
            buf.push_str("},\n");

            // Children
            write!(buf, "{pad}  \"children\": [")?;
            let children = node.children();
            for (i, child) in children.iter().enumerate() {
                if i > 0 {
                    buf.push(',');
                }
                buf.push('\n');
                write_json(child, buf, indent + 4)?;
            }
            if !children.is_empty() {
                buf.push('\n');
                write!(buf, "{pad}  ")?;
            }
            buf.push_str("]\n");
            write!(buf, "{pad}}}")
        }

        let mut buf = String::with_capacity(256);
        // Writing to a `String` through `fmt::Write` is infallible.
        let _ = write_json(self, &mut buf, 0);
        buf
    }
}

impl Default for DiagnosticsNode {
    #[inline]
    fn default() -> Self {
        Self::anonymous()
    }
}

/// Escapes a string for JSON embedding.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
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
        // F27: strip the module path, keeping only the final type segment.
        // "flui_rendering::objects::RenderPadding" -> "RenderPadding".
        let full = std::any::type_name::<Self>();
        let type_name = full.rsplit("::").next().unwrap_or(full);
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
        value: f32,
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
        value: f32,
        default: f32,
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

    /// Add a color property (RGBA display) from a pre-formatted string.
    ///
    /// For a typed RGBA value use [`add_color_rgba`](Self::add_color_rgba).
    pub fn add_color(&mut self, name: impl Into<String>, value: impl fmt::Display) -> &mut Self {
        self.properties
            .push(DiagnosticsProperty::new(name, value).with_kind(DiagnosticsPropertyKind::Color));
        self
    }

    // ---- Typed-value additions (ADR-0005 Decision 1) -------------------------

    /// Add a typed `f64` property.
    pub fn add_f64(&mut self, name: impl Into<String>, value: f64) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new_typed(name, DiagnosticsValue::Float(value))
                .with_kind(DiagnosticsPropertyKind::Double { unit: None }),
        );
        self
    }

    /// Add a typed `i64` property.
    pub fn add_i64(&mut self, name: impl Into<String>, value: i64) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new_typed(name, DiagnosticsValue::Int(value))
                .with_kind(DiagnosticsPropertyKind::Int { unit: None }),
        );
        self
    }

    /// Add a typed `bool` property.
    pub fn add_bool(&mut self, name: impl Into<String>, value: bool) -> &mut Self {
        self.properties.push(DiagnosticsProperty::new_typed(
            name,
            DiagnosticsValue::Bool(value),
        ));
        self
    }

    /// Add a typed RGBA colour property.
    pub fn add_color_rgba(
        &mut self,
        name: impl Into<String>,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    ) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new_typed(name, DiagnosticsValue::Color { r, g, b, a })
                .with_kind(DiagnosticsPropertyKind::Color),
        );
        self
    }

    /// Add a typed axis-aligned rectangle property.
    pub fn add_rect(
        &mut self,
        name: impl Into<String>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new_typed(name, DiagnosticsValue::Rect { x, y, w, h })
                .with_kind(DiagnosticsPropertyKind::Rect),
        );
        self
    }

    /// Add a typed 2-D offset property.
    pub fn add_offset(&mut self, name: impl Into<String>, x: f64, y: f64) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new_typed(name, DiagnosticsValue::Offset { x, y })
                .with_kind(DiagnosticsPropertyKind::Offset),
        );
        self
    }

    /// Add a typed 2-D size property.
    ///
    /// For a display-string size (e.g. `"100 x 50"`) use
    /// [`add_size`](Self::add_size).
    pub fn add_size_f64(&mut self, name: impl Into<String>, w: f64, h: f64) -> &mut Self {
        self.properties.push(
            DiagnosticsProperty::new_typed(name, DiagnosticsValue::Size { w, h })
                .with_kind(DiagnosticsPropertyKind::Size),
        );
        self
    }

    /// Add a property with an arbitrary typed [`DiagnosticsValue`].
    pub fn add_typed(&mut self, name: impl Into<String>, value: DiagnosticsValue) -> &mut Self {
        self.properties
            .push(DiagnosticsProperty::new_typed(name, value));
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
fn format_double(value: f32, unit: Option<&str>) -> String {
    match unit {
        Some(u) => format!("{value}{u}"),
        None => format!("{value}"),
    }
}

/// Parses a numeric diagnostics property, stripping a typed unit suffix when
/// present.
///
/// For typed `Float`/`Int` variants the value is read directly rather than
/// parsing text; for `Str` (back-compat path) text parsing with optional unit
/// stripping is used.
fn parse_numeric_property_value(property: &DiagnosticsProperty) -> Option<f64> {
    // Fast path for floats: avoids display-string formatting + re-parsing.
    // Integers are not short-circuited: i64→f64 loses precision outside
    // ±2^53; the display-string parse below is exact for the integer
    // range that diagnostic properties carry.
    if let DiagnosticsValue::Float(v) = property.value_typed() {
        return Some(*v);
    }

    // Back-compat path: display string with optional unit suffix.
    let raw = property.value();
    let numeric = match property.kind() {
        DiagnosticsPropertyKind::Double { unit } | DiagnosticsPropertyKind::Int { unit } => {
            match unit {
                Some(suffix) if raw.ends_with(suffix.as_ref()) => {
                    raw[..raw.len() - suffix.len()].to_owned()
                }
                _ => raw,
            }
        }
        _ => raw,
    };
    numeric.trim().parse().ok()
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
        assert_eq!(format!("{prop}"), "width: 100");
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
    fn diagnostics_property_kind_field_exists() {
        let prop = DiagnosticsProperty::new("width", "100.0");
        assert_eq!(prop.kind, DiagnosticsPropertyKind::Generic);
    }

    #[test]
    fn diagnostics_property_flag_kind() {
        let prop =
            DiagnosticsProperty::new("visible", "true").with_kind(DiagnosticsPropertyKind::Flag);
        assert_eq!(prop.kind, DiagnosticsPropertyKind::Flag);
    }

    #[test]
    fn diagnostics_property_iterable_kind() {
        let prop = DiagnosticsProperty::new("children", "[..]")
            .with_kind(DiagnosticsPropertyKind::Iterable { count: 3 });
        assert_eq!(prop.kind, DiagnosticsPropertyKind::Iterable { count: 3 });
    }

    #[test]
    fn to_diagnostics_node_uses_short_type_name() {
        #[derive(Debug)]
        struct MyWidget;
        impl Diagnosticable for MyWidget {}

        let node = MyWidget.to_diagnostics_node();
        // `type_name::<MyWidget>()` includes the full module path
        // (e.g. `flui_foundation::debug::tests::...::MyWidget`); after the
        // F27 fix the node name must be stripped to just "MyWidget".
        assert_eq!(
            node.name(),
            Some("MyWidget"),
            "type_name should be short (no module path), got: {:?}",
            node.name()
        );
    }

    #[test]
    fn test_diagnostics_builder_typed_helpers() {
        let mut builder = DiagnosticsBuilder::new();
        builder.add_enum("direction", "Horizontal");
        builder.add_default("spacing", 0, "0");
        builder.add_default_double("opacity", 1.0, 1.0, None);
        builder.add_default_double("gap", 8.0, 0.0, Some("px"));
        builder.add_size("size", 100, 50);
        builder.add_flag("visible", true, "visible");

        let props = builder.build();
        assert_eq!(props.len(), 6);
        assert_eq!(
            props[0].kind(),
            &DiagnosticsPropertyKind::Enum { description: None }
        );
        assert!(props[1].is_hidden());
        assert!(props[2].is_hidden());
        assert!(!props[3].is_hidden());
        assert_eq!(props[4].kind(), &DiagnosticsPropertyKind::Size);
        assert_eq!(props[5].kind(), &DiagnosticsPropertyKind::Flag);
    }

    #[test]
    fn test_diagnostics_node_find_descendant() {
        let tree = DiagnosticsNode::new("Root").child(
            DiagnosticsNode::new("RenderFlex")
                .property("direction", "Horizontal")
                .child(DiagnosticsNode::new("RenderPadding")),
        );

        let flex = tree.find_descendant("RenderFlex").expect("flex");
        assert_eq!(
            flex.get_property("direction").as_deref(),
            Some("Horizontal")
        );
        assert!(tree.find_descendant("RenderPadding").is_some());
        assert!(tree.find_descendant("Missing").is_none());
    }

    #[test]
    fn test_diagnostics_node_format_deep_filtered() {
        let node = DiagnosticsNode::new("Box")
            .property("opacity", 1.0)
            .with_property(
                DiagnosticsProperty::new("debug_only", "trace").with_level(DiagnosticLevel::Debug),
            );

        let full = node.format_deep(0);
        assert!(full.contains("opacity"));
        assert!(full.contains("debug_only"));

        let info_only = node.to_string_deep_at_level(DiagnosticLevel::Info);
        assert!(info_only.contains("opacity"));
        assert!(!info_only.contains("debug_only"));
    }

    #[test]
    fn test_diagnostics_node_get_property_f64() {
        let node = DiagnosticsNode::new("Box").property("opacity", "0.5");
        assert_eq!(node.get_property_f64("opacity"), Some(0.5));
        assert_eq!(node.get_property_f64("missing"), None);
    }

    #[test]
    fn test_diagnostics_node_get_property_f64_strips_unit_suffix() {
        let mut builder = DiagnosticsBuilder::new();
        builder.add_double("item_extent", 25.0, Some("px"));
        let [property] = builder.build().try_into().ok().unwrap();
        let node = DiagnosticsNode::new("RenderSliverFixedExtentList").with_property(property);
        assert_eq!(node.get_property_f64("item_extent"), Some(25.0));
    }

    // ---- Task 3 typed-value tests (TDD: written before implementation) ----

    #[test]
    fn string_property_back_compat() {
        let prop = DiagnosticsProperty::new("width", 100);
        assert_eq!(prop.value(), "100");
        assert_eq!(prop.to_string(), "width: 100");
    }

    #[test]
    fn typed_rect_value_is_structured() {
        let mut builder = DiagnosticsBuilder::new();
        builder.add_rect("bounds", 0.0, 0.0, 40.0, 40.0);
        let props = builder.build();
        assert_eq!(props.len(), 1);
        assert_eq!(
            props[0].value_typed(),
            &DiagnosticsValue::Rect {
                x: 0.0,
                y: 0.0,
                w: 40.0,
                h: 40.0
            }
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn faithful_vs_display() {
        let val = DiagnosticsValue::Float(0.333_333);
        // serde_json serializes with full precision
        let json = serde_json::to_string(&val).unwrap();
        assert!(
            json.contains("0.333333"),
            "expected full-precision float in JSON, got: {json}"
        );
        // Display shows 2 decimal places
        assert_eq!(val.to_string(), "0.33");
    }

    #[test]
    fn test_diagnostics_node_find_descendant_unique_rejects_ambiguous() {
        let tree = DiagnosticsNode::new("Root")
            .child(DiagnosticsNode::new("RenderPadding"))
            .child(DiagnosticsNode::new("RenderPadding"));
        assert_eq!(
            tree.find_descendant_unique("RenderPadding"),
            Err(DescendantLookupError::Ambiguous),
        );
        assert!(tree.find_descendant("RenderPadding").is_none());
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
// overlay is orthogonal and combined freely (mirrors Flutter's separate
// `debugPaint*Enabled` flags). A bitflags/enum would obscure, not clarify.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
