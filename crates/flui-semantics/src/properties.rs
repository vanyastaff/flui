//! Value types a configuration publishes or orders by: heading levels, text
//! direction and sort keys, and the join of two labels a merge makes.

use smol_str::SmolStr;

use crate::action::SemanticsAction;

// ============================================================================
// TextDirection
// ============================================================================

// Text flow direction lives in `flui_foundation::TextDirection`; the
// semantics path re-exports it so `flui_semantics::TextDirection` keeps working.
pub use flui_foundation::TextDirection;

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
#[expect(
    clippy::exhaustive_enums,
    reason = "the six heading levels are the ones every platform names, and a deeper outline nests at the sixth"
)]
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
// Label join
// ============================================================================

/// Joins two strings a merge combines (labels, values, hints) into one, with
/// a single ASCII space between them.
///
/// An empty operand returns the other unchanged, so the join never inserts a
/// leading or trailing space. Strings of opposite direction get no bidi
/// isolation: a screen reader reads the join in the order the platform's bidi
/// algorithm gives it.
#[must_use]
pub(crate) fn join_strings(this: &SmolStr, other: &SmolStr) -> SmolStr {
    if this.is_empty() {
        return other.clone();
    }
    if other.is_empty() {
        return this.clone();
    }
    let mut joined = String::with_capacity(this.len() + 1 + other.len());
    joined.push_str(this);
    joined.push(' ');
    joined.push_str(other);
    SmolStr::from(joined)
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
// SemanticsSortKey
// ============================================================================

/// Sort key for ordering siblings within a semantics boundary.
///
/// Keyed content precedes unkeyed content. Unnamed groups precede named groups;
/// group names sort lexically, then finite orders numerically. Equal keys keep
/// declarative order. Assembly refuses non-finite orders before publication.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticsSortKey {
    order: f64,
    name: Option<SmolStr>,
}

impl SemanticsSortKey {
    /// A key in the unnamed group, at `order`.
    #[must_use]
    pub fn new(order: f64) -> Self {
        Self { order, name: None }
    }

    /// A key in the group `name`, at `order` within it.
    #[must_use]
    pub fn named(order: f64, name: impl AsRef<str>) -> Self {
        Self {
            order,
            name: Some(SmolStr::new(name)),
        }
    }

    /// The order within the key's group.
    #[must_use]
    pub fn order(&self) -> f64 {
        self.order
    }

    /// The group the key orders within; `None` is the unnamed group.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
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
