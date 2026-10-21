//! Value types a configuration publishes: attributed strings, heading levels,
//! text direction, tags, hint overrides and sort keys.

use smol_str::SmolStr;

use crate::action::SemanticsAction;

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
// HeadingLevel
// ============================================================================

/// The outline level of a heading, as `aria-level` numbers it: [`Self::H1`]
/// is the top of the outline.
///
/// Screen readers announce the level and let a user jump between headings of
/// one level. The six levels are the ones every platform names; a deeper
/// outline nests its headings at [`Self::H6`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeadingLevel {
    /// The top level, a page or screen title.
    H1 = 1,
    /// A section heading.
    H2 = 2,
    /// A subsection heading.
    H3 = 3,
    /// A fourth-level heading.
    H4 = 4,
    /// A fifth-level heading.
    H5 = 5,
    /// A sixth-level heading.
    H6 = 6,
}

impl HeadingLevel {
    /// The one-based level, as `aria-level` gives it.
    #[must_use]
    pub const fn get(self) -> u8 {
        self as u8
    }
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
// AttributedString
// ============================================================================

/// A string with attributes for accessibility.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
// AttributedString concat helper
// ============================================================================

/// Concatenates two attributed strings in a text-direction-aware manner.
///
/// Used during
/// [`SemanticsConfiguration::absorb`](crate::configuration::SemanticsConfiguration::absorb) when the parent and child each carry
/// a label or hint that must be joined into a single string for the
/// platform accessibility surface.
///
/// Empty-input fast paths return the non-empty operand directly so the
/// join never inserts a leading or trailing space.
///
/// Strings join with a single ASCII space whatever their direction: two
/// strings of opposite direction get no bidi isolation, so a screen reader
/// reads the join in the order the platform's bidi algorithm gives it.
/// Attribute offsets carry over from the left operand; right-operand
/// attributes are shifted by `left.string.len() + 1` for the inserted
/// space.
#[must_use]
pub fn concat_attributed_string(
    this: &AttributedString,
    _this_dir: TextDirection,
    other: &AttributedString,
    _other_dir: TextDirection,
) -> AttributedString {
    if this.is_empty() {
        return other.clone();
    }
    if other.is_empty() {
        return this.clone();
    }

    let mut joined = String::with_capacity(this.string.len() + 1 + other.string.len());
    joined.push_str(&this.string);
    joined.push(' ');
    joined.push_str(&other.string);

    let mut attributes = this.attributes.clone();
    let shift = this.string.len() + 1; // " "
    for a in &other.attributes {
        attributes.push(StringAttribute {
            start: a.start + shift,
            end: a.end + shift,
            attribute_type: a.attribute_type.clone(),
        });
    }

    AttributedString {
        string: SmolStr::new(&joined),
        attributes,
    }
}

// ============================================================================
// SemanticsAction mask constants
// ============================================================================

/// Bitmask of `SemanticsAction` variants that can be absorbed from a child
/// configuration into its parent **even when** the child has set
/// `blocks_user_actions = true`.
///
/// Giving up keyboard focus remains available: a blocked node can always be
/// left. Activation, scrolling, text editing and custom actions are blocked.
pub(crate) const UNBLOCKED_USER_ACTIONS_MASK: u64 = SemanticsAction::Blur.value();

// ============================================================================
// StringAttribute
// ============================================================================

/// An attribute on a range of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringAttribute {
    /// Start index (inclusive).
    pub start: usize,
    /// End index (exclusive).
    pub end: usize,
    /// The attribute type.
    pub attribute_type: StringAttributeType,
}

/// Types of string attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringAttributeType {
    /// Spell out the text character by character.
    SpellOut,
    /// Use a specific locale for pronunciation.
    Locale(SmolStr),
}

// ============================================================================
// SemanticsSortKey
// ============================================================================

/// Sort key for ordering siblings within a semantics boundary.
///
/// Keyed content precedes unkeyed content. Unnamed groups precede named groups;
/// group names sort lexically, then finite orders numerically. Equal keys keep
/// declarative order. Assembly refuses non-finite orders before publication.
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
        self.name.partial_cmp(&other.name).and_then(|order| {
            if order.is_eq() {
                self.order.partial_cmp(&other.order)
            } else {
                Some(order)
            }
        })
    }
}
