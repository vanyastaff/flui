//! Describing elements instead of holding them (ADR-0238 §4): the [`Locator`] a caller writes,
//! what [`semantics.locate`](crate::method::SemanticsLocate) answers, and the [`Target`] an
//! action takes.
//!
//! A locator is re-resolved at every use, in the app, over the published semantics tree. It
//! never caches a node, so it cannot act on one that has since been replaced.

use crate::geometry::LogicalPoint;
use crate::tree::{ElementRef, Node, TreeRevision, WindowId};
use crate::wire::{Checked, Role};

/// How a locator's text criterion compares with a node's text.
///
/// `{"match": "exact", "text": "Save"}`; `ignore_case` folds both sides by Unicode case
/// first. There is no regular expression, so no regex engine enters a debug app (ADR-0238,
/// "Alternatives").
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "match", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum TextMatch {
    /// The whole text.
    Exact {
        /// The text to compare with.
        text: String,
        /// Compare after Unicode case folding.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        ignore_case: bool,
    },
    /// Anywhere in the text.
    Contains {
        /// The text to look for.
        text: String,
        /// Compare after Unicode case folding.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        ignore_case: bool,
    },
    /// At the start of the text.
    Prefix {
        /// The text it starts with.
        text: String,
        /// Compare after Unicode case folding.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        ignore_case: bool,
    },
}

impl TextMatch {
    /// The whole text, case-sensitive.
    #[must_use]
    pub fn exact(text: impl Into<String>) -> Self {
        Self::Exact {
            text: text.into(),
            ignore_case: false,
        }
    }

    /// Anywhere in the text, case-sensitive.
    #[must_use]
    pub fn contains(text: impl Into<String>) -> Self {
        Self::Contains {
            text: text.into(),
            ignore_case: false,
        }
    }

    /// At the start of the text, case-sensitive.
    #[must_use]
    pub fn prefix(text: impl Into<String>) -> Self {
        Self::Prefix {
            text: text.into(),
            ignore_case: false,
        }
    }

    /// The same comparison after Unicode case folding.
    #[must_use]
    pub fn ignoring_case(self) -> Self {
        match self {
            Self::Exact { text, .. } => Self::Exact {
                text,
                ignore_case: true,
            },
            Self::Contains { text, .. } => Self::Contains {
                text,
                ignore_case: true,
            },
            Self::Prefix { text, .. } => Self::Prefix {
                text,
                ignore_case: true,
            },
        }
    }
}

/// The states a locator or an expectation requires; a state left `None` is not compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct StateFilter {
    /// The checked state.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub checked: Option<Checked>,
    /// Disabled, itself or through an ancestor.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub disabled: Option<bool>,
    /// Holding keyboard focus.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub focused: Option<bool>,
    /// Expanded.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub expanded: Option<bool>,
    /// Selected.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub selected: Option<bool>,
}

impl StateFilter {
    /// Whether nothing is required.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Which of several matches a locator means.
///
/// `"strict"`, `"first"`, `"last"` or `{"nth": 2}` (from 0). Strict is the default: an action
/// through a locator with more than one match fails `ambiguous` instead of guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Pick {
    /// Exactly one match, or `ambiguous`.
    #[default]
    Strict,
    /// The first match in tree order.
    First,
    /// The last match in tree order.
    Last,
    /// The match at this index in tree order, from 0.
    Nth(u32),
}

/// A description of the elements meant: every criterion given must hold.
///
/// Built from one criterion and refined: `Locator::role(Role::Button).name("Increment")`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Locator {
    /// Only this window; every window of the session when `None`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub window: Option<WindowId>,
    /// The role.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub role: Option<Role>,
    /// The accessible name.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name: Option<TextMatch>,
    /// The value.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<TextMatch>,
    /// The description.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub description: Option<TextMatch>,
    /// The author's identifier (ADR-0240), compared exactly.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub identifier: Option<String>,
    /// The states required.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "StateFilter::is_empty")
    )]
    pub state: StateFilter,
    /// Only under an element this locator matches.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub within: Option<Box<Locator>>,
    /// Only elements with a descendant this locator matches.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub has: Option<Box<Locator>>,
    /// Which match is meant.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub pick: Pick,
}

impl Locator {
    /// Elements of `role`.
    #[must_use]
    pub fn role(role: Role) -> Self {
        Self {
            role: Some(role),
            ..Self::default()
        }
    }

    /// The element whose identifier is `identifier`.
    #[must_use]
    pub fn identifier(identifier: impl Into<String>) -> Self {
        Self {
            identifier: Some(identifier.into()),
            ..Self::default()
        }
    }

    /// Elements whose name is exactly `name`.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self::default().name(name)
    }

    /// Also: the name is exactly `name`.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(TextMatch::exact(name));
        self
    }

    /// Also: the name matches `name`.
    #[must_use]
    pub fn name_matching(mut self, name: TextMatch) -> Self {
        self.name = Some(name);
        self
    }

    /// Also: the value matches `value`.
    #[must_use]
    pub fn value(mut self, value: TextMatch) -> Self {
        self.value = Some(value);
        self
    }

    /// Also: the description matches `description`.
    #[must_use]
    pub fn description(mut self, description: TextMatch) -> Self {
        self.description = Some(description);
        self
    }

    /// Also: the states in `state` hold.
    #[must_use]
    pub fn state(mut self, state: StateFilter) -> Self {
        self.state = state;
        self
    }

    /// Also: in `window` only.
    #[must_use]
    pub fn in_window(mut self, window: WindowId) -> Self {
        self.window = Some(window);
        self
    }

    /// Also: under an element `ancestor` matches.
    #[must_use]
    pub fn within(mut self, ancestor: Locator) -> Self {
        self.within = Some(Box::new(ancestor));
        self
    }

    /// Also: with a descendant `descendant` matches.
    #[must_use]
    pub fn has(mut self, descendant: Locator) -> Self {
        self.has = Some(Box::new(descendant));
        self
    }

    /// The match `pick` names instead of exactly one.
    #[must_use]
    pub fn pick(mut self, pick: Pick) -> Self {
        self.pick = pick;
        self
    }
}

/// What an action is aimed at.
///
/// `{"element": "w3/e12"}`, `{"locator": {…}}` or `{"point": {"window": "w3", "at": {…}}}`.
/// A point is the explicit last resort (ADR-0238, "Consequences"): codegen never writes one
/// from a locator-based trace.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Target {
    /// An element already resolved.
    Element(ElementRef),
    /// The element a locator resolves to at the time of the action.
    Locator(Locator),
    /// A point in a window.
    Point {
        /// The window.
        window: WindowId,
        /// Where, in the window's logical surface coordinates.
        at: LogicalPoint,
    },
}

impl From<ElementRef> for Target {
    fn from(element: ElementRef) -> Self {
        Self::Element(element)
    }
}

impl From<Locator> for Target {
    fn from(locator: Locator) -> Self {
        Self::Locator(locator)
    }
}

/// `semantics.locate`: the elements a locator matches now.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct LocateParams {
    /// What to look for. Its `pick` applies: a strict locator with several matches answers
    /// `ambiguous`.
    pub locator: Locator,
    /// Matches to report at most; the rest are counted in `truncated`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_matches: Option<u32>,
}

impl LocateParams {
    /// Every match of `locator`.
    #[must_use]
    pub fn new(locator: Locator) -> Self {
        Self {
            locator,
            max_matches: None,
        }
    }
}

/// One element a locator matched.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Match {
    /// The element, across windows.
    pub element: ElementRef,
    /// The revision of its window's tree the match was made on.
    pub revision: TreeRevision,
    /// The node as matched, without children.
    pub node: Node,
}

impl Match {
    /// `node` in `element`'s window at `revision`.
    #[must_use]
    pub fn new(element: ElementRef, revision: TreeRevision, node: Node) -> Self {
        Self {
            element,
            revision,
            node,
        }
    }
}

/// What `semantics.locate` answers: the matches in tree order, window by window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Matches {
    /// The matches reported.
    pub matches: Vec<Match>,
    /// Matches past `max_matches`, not reported.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub truncated: u32,
}

impl Matches {
    /// `matches`, all reported.
    #[must_use]
    pub fn new(matches: Vec<Match>) -> Self {
        Self {
            matches,
            truncated: 0,
        }
    }
}
