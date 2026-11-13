//! Navigation addressed to a presentation (ADR-0330 §5).
//!
//! A navigation that does not come from a widget's own handle, such as a URL
//! the platform opens for a window, is a [`NavigationIntent`] on the owner
//! thread, addressed to one presentation. The presentation's URL owner, its
//! outermost `Router`, applies it: it parses the [`RoutePath`] into its own
//! route type and edits its stack, or refuses with a [`NavigationRefused`].
//!
//! This is Flutter's route information pipeline. The runtime's per-presentation
//! slot is the `RouteInformationProvider` the platform reports a new location
//! to, the route type's `from_path` is the `RouteInformationParser`, and the
//! Router applies the parsed stack as its delegate's `setNewRoutePath` does.
//! The types live here, below the widget catalog, so the runtime names no
//! catalog type (ADR-0083).

mod path;
pub(crate) mod url_owner;

pub use path::{RouteParseError, RoutePath};

/// What a [`NavigationIntent`] asks the URL owner to do.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationOp {
    /// Push the page the path names over the current one.
    Push(RoutePath),
    /// Replace the top page with the page the path names.
    Replace(RoutePath),
    /// Pop the top route, as the Back key does.
    Pop,
    /// Set the stack the path opens with, as a deep link does: the pages it
    /// shares with the current stack stay, the rest are replaced.
    Go(RoutePath),
}

/// A navigation addressed to a presentation from outside its widgets: a URL
/// the platform opened for its window, or a location a development tool
/// asked for.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationIntent {
    /// What to do.
    pub op: NavigationOp,
}

impl NavigationIntent {
    /// An intent that does `op`.
    #[must_use]
    pub fn new(op: NavigationOp) -> Self {
        Self { op }
    }
}

impl From<NavigationOp> for NavigationIntent {
    fn from(op: NavigationOp) -> Self {
        Self::new(op)
    }
}

/// The owner of a presentation's URL: the one place a [`NavigationIntent`]
/// for that presentation is applied. The outermost `Router` of the
/// presentation's tree registers itself as it.
///
/// Owner-thread state: an implementation is `!Send`, and `navigate` runs on
/// the thread that owns the presentation.
pub trait UrlOwner: 'static {
    /// Apply `intent` to the owner's stack.
    ///
    /// # Errors
    ///
    /// [`NavigationRefused::NoMatch`] when no route matches the path, the
    /// stack then unchanged; [`NavigationRefused::NothingToPop`] for a pop of
    /// the last page; [`NavigationRefused::Busy`] when the owner is in
    /// the middle of another edit or its UI runtime is building; and
    /// [`NavigationRefused::NoUrlOwner`] when the owner has unmounted.
    fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused>;
}

/// Why a [`NavigationIntent`] was not applied. The stack is unchanged.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavigationRefused {
    /// The intent names a presentation the UI runtime does not host: it was
    /// never there, or it has closed.
    #[error("the intent names no presentation of this UI runtime")]
    NoPresentation,
    /// The presentation's tree has no `Router` to own its URL.
    #[error("the presentation has no URL owner")]
    NoUrlOwner,
    /// No route of the URL owner matches the path.
    #[error("no route matches {0}")]
    NoMatch(RoutePath),
    /// A [`NavigationOp::Pop`] found only the owner's last page, which it
    /// keeps: the platform handles the Back itself, as Flutter's `popRoute`
    /// answering `false` leaves it to the system.
    #[error("the URL owner has no route to pop")]
    NothingToPop,
    /// The URL owner is committing another edit, or its UI runtime is
    /// building, and an edit inside that one is refused rather than nested.
    #[error("the URL owner is in the middle of another edit")]
    Busy,
}
