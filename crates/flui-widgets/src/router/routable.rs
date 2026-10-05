//! [`Routable`] — a typed set of locations.

use super::path::{RouteParseError, RoutePath};
use crate::support::retirement::{RetiredValues, Terminal};

/// A typed set of locations: the route type a [`Router`](super::Router) keeps
/// a stack of.
///
/// # Contract
///
/// For every value `r`, `Self::from_path(&r.to_path()) == Ok(r)`. A path no
/// value matches is [`RouteParseError::NoMatch`] — never a panic and never a
/// fallback to some default value.
///
/// `#[derive(Routable)]` writes `to_path` and `from_path` from one
/// `#[route("…")]` pattern per variant. It keeps the round trip for every
/// value whose fields print non-empty, whose `Display` output `FromStr` reads
/// back as the same value, and whose printed path no pattern tried before
/// its own also matches. Patterns are tried by specificity (a literal ranks
/// above a parameter at the first position where two differ), so a value
/// that prints a more specific pattern's literals parses as that pattern's
/// variant: beside `#[route("/s/new")]`, a `#[route("/s/:slug")]` value with
/// `slug == "new"`; and beside `#[route("/s/:b")]`, a `#[route("/:a/new")]`
/// value with `a == "s"`, whose `/s/new` fills `b` with `"new"`. A
/// hand-written impl keeps the contract itself.
///
/// # Example
///
/// ```
/// use flui_widgets::{Routable, RouteParseError, RoutePath};
///
/// #[derive(Routable, Debug, Clone, PartialEq)]
/// enum AppRoute {
///     #[route("/")]
///     Home,
///     #[route("/note/:id")]
///     Note { id: u32 },
/// }
///
/// let note = AppRoute::Note { id: 7 };
/// assert_eq!(note.to_path().as_str(), "/note/7");
/// assert_eq!(AppRoute::parse("/note/7"), Ok(note));
/// // `/note` matches nothing, so the stack `/note/7` opens with skips it.
/// let stack = AppRoute::back_stack(&RoutePath::parse("/note/7")?)?;
/// assert_eq!(stack, [AppRoute::Home, AppRoute::Note { id: 7 }]);
/// # Ok::<(), RouteParseError>(())
/// ```
pub trait Routable: Clone + PartialEq + 'static {
    /// The location this value names.
    fn to_path(&self) -> RoutePath;

    /// The value `path` names.
    ///
    /// # Errors
    ///
    /// [`RouteParseError::NoMatch`] when no value matches, and
    /// [`RouteParseError::Param`] when a pattern matches but a segment does not
    /// parse as its field.
    fn from_path(path: &RoutePath) -> Result<Self, RouteParseError>;

    /// [`RoutePath::parse`], then [`from_path`](Self::from_path).
    ///
    /// # Errors
    ///
    /// Whatever either step reports.
    fn parse(location: &str) -> Result<Self, RouteParseError> {
        Self::from_path(&RoutePath::parse(location)?)
    }

    /// The stack a location opens with, bottom to top.
    ///
    /// The default is every prefix of `path` that parses, root first, ending
    /// with `path` itself:
    /// a prefix that matches nothing is a gap and is skipped, but the full path
    /// must match.
    ///
    /// # Errors
    ///
    /// Whatever [`from_path`](Self::from_path) reports for the full path; the
    /// prefixes' errors are gaps, not failures.
    fn back_stack(path: &RoutePath) -> Result<Vec<Self>, RouteParseError> {
        let mut full = Terminal::new(Self::from_path(path)?);
        let mut stack = RetiredValues(Vec::new());
        for prefix in path.prefixes().filter(|prefix| prefix != path) {
            if let Ok(route) = Self::from_path(&prefix) {
                stack.0.push(route);
            }
        }
        stack.0.push(full.take_value());
        Ok(std::mem::take(&mut stack.0))
    }

    /// The label assistive technology announces for this value's page. `None`
    /// (the default) leaves the page's route unnamed.
    fn semantics_label(&self) -> Option<String> {
        None
    }
}
