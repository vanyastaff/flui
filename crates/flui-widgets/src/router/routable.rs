//! [`Routable`] — a typed set of locations.

use super::path::{RouteParseError, RoutePath};

/// A typed set of locations: the route type a [`Router`](super::Router) keeps
/// a stack of.
///
/// # Contract
///
/// For every value `r`, `Self::from_path(&r.to_path()) == Ok(r)`. A path no
/// value matches is [`RouteParseError::NoMatch`] — never a panic and never a
/// fallback to some default value. `#[derive(Routable)]` will guarantee this;
/// a hand-written impl must.
///
/// # Example
///
/// ```
/// use flui_widgets::{Routable, RouteParseError, RoutePath};
///
/// #[derive(Debug, Clone, PartialEq)]
/// enum AppRoute {
///     Home,
///     Note { id: u32 },
/// }
///
/// impl Routable for AppRoute {
///     fn to_path(&self) -> RoutePath {
///         match self {
///             Self::Home => RoutePath::root(),
///             Self::Note { id } => RoutePath::root().join("note").join(id),
///         }
///     }
///
///     fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
///         let segments: Vec<_> = path.segments().collect();
///         match segments.as_slice() {
///             [] => Ok(Self::Home),
///             [note, id] if note == "note" => id
///                 .parse()
///                 .map(|id| Self::Note { id })
///                 .map_err(|_| RouteParseError::Param {
///                     path: path.clone(),
///                     field: "id",
///                     segment: id.to_string(),
///                 }),
///             _ => Err(RouteParseError::NoMatch { path: path.clone() }),
///         }
///     }
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
    /// with `path` itself — Flutter's `Navigator.defaultGenerateInitialRoutes`:
    /// a prefix that matches nothing is a gap and is skipped, but the full path
    /// must match.
    ///
    /// # Errors
    ///
    /// Whatever [`from_path`](Self::from_path) reports for the full path; the
    /// prefixes' errors are gaps, not failures.
    fn back_stack(path: &RoutePath) -> Result<Vec<Self>, RouteParseError> {
        let full = Self::from_path(path)?;
        let mut stack: Vec<Self> = path
            .prefixes()
            .filter(|prefix| prefix != path)
            .filter_map(|prefix| Self::from_path(&prefix).ok())
            .collect();
        stack.push(full);
        Ok(stack)
    }

    /// The label assistive technology announces for this value's page. `None`
    /// (the default) leaves the page's route unnamed.
    fn semantics_label(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The hand-written route type the router tests share.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) enum AppRoute {
        Home,
        Note { id: u32 },
        Tag { name: String },
    }

    impl Routable for AppRoute {
        fn to_path(&self) -> RoutePath {
            match self {
                Self::Home => RoutePath::root(),
                Self::Note { id } => RoutePath::root().join("note").join(id),
                Self::Tag { name } => RoutePath::root().join("tag").join(name),
            }
        }

        fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
            let segments: Vec<_> = path.segments().collect();
            match segments.as_slice() {
                [] => Ok(Self::Home),
                [head, id] if head == "note" => {
                    id.parse()
                        .map(|id| Self::Note { id })
                        .map_err(|_| RouteParseError::Param {
                            path: path.clone(),
                            field: "id",
                            segment: id.to_string(),
                        })
                }
                [head, name] if head == "tag" => Ok(Self::Tag {
                    name: name.to_string(),
                }),
                _ => Err(RouteParseError::NoMatch { path: path.clone() }),
            }
        }
    }

    #[test]
    fn route_path_round_trips_a_hand_written_routable() {
        let tag = AppRoute::Tag {
            name: "a b/c".to_owned(),
        };
        assert_eq!(tag.to_path().as_str(), "/tag/a%20b%2Fc");
        for route in [AppRoute::Home, AppRoute::Note { id: 42 }, tag] {
            assert_eq!(AppRoute::from_path(&route.to_path()), Ok(route));
        }
    }

    #[test]
    fn parse_reports_no_match_and_bad_params() {
        assert!(matches!(
            AppRoute::parse("/nope"),
            Err(RouteParseError::NoMatch { .. })
        ));
        assert!(matches!(
            AppRoute::parse("/note/x"),
            Err(RouteParseError::Param { field: "id", .. })
        ));
        assert!(matches!(
            AppRoute::parse("note"),
            Err(RouteParseError::Malformed { .. })
        ));
    }

    #[test]
    fn back_stack_is_the_matching_prefix_chain() {
        // Flutter oracle: 'Initial route can have gaps' — `/note` is a gap.
        let path = RoutePath::parse("/note/1").expect("parses");
        assert_eq!(
            AppRoute::back_stack(&path),
            Ok(vec![AppRoute::Home, AppRoute::Note { id: 1 }])
        );
        // 'The full initial route has to be matched'.
        let path = RoutePath::parse("/settings/x").expect("parses");
        assert!(matches!(
            AppRoute::back_stack(&path),
            Err(RouteParseError::NoMatch { .. })
        ));
        assert_eq!(
            AppRoute::back_stack(&RoutePath::root()),
            Ok(vec![AppRoute::Home])
        );
    }
}
