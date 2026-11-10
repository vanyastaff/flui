//! An error with the place in a tree it happened at.
//!
//! The error values a tree's hot paths return carry ids only. The boundary that owns the tree (the
//! pipeline owner, the build owner) wraps one in a [`TreeError`] on its way out, with the path of
//! ancestors to the node and where the node's view was created. The wrapper costs nothing until
//! an error happens.

use std::error::Error;
use std::fmt;

use crate::CreationSite;
use crate::inspect::InspectId;

/// How many ancestors a [`TreeContext`] keeps, the failing node included.
pub const TREE_CONTEXT_DEPTH: usize = 8;

/// One node on a [`TreeContext`]'s path: what kind it is and which one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PathEntry {
    kind: &'static str,
    id: InspectId,
}

impl PathEntry {
    /// An entry for the node `id` of kind `kind`.
    #[must_use]
    pub const fn new(kind: &'static str, id: InspectId) -> Self {
        Self { kind, id }
    }

    /// The node's kind, as its tree names it (`RenderPadding`).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }

    /// The node's id in its tree.
    #[must_use]
    pub const fn id(&self) -> InspectId {
        self.id
    }
}

impl fmt::Display for PathEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.kind, self.id)
    }
}

/// Where in a tree something happened: the path from the node up through its ancestors, capped at
/// [`TREE_CONTEXT_DEPTH`] entries, and where the node was created.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TreeContext {
    path: Vec<PathEntry>,
    truncated: bool,
    site: CreationSite,
}

impl TreeContext {
    /// A context for a path that starts at the node and runs up to its ancestors. Entries past
    /// [`TREE_CONTEXT_DEPTH`] are dropped and the context remembers that it was cut.
    #[must_use]
    pub fn new(path: impl IntoIterator<Item = PathEntry>, site: CreationSite) -> Self {
        let mut path = path.into_iter();
        let kept: Vec<PathEntry> = path.by_ref().take(TREE_CONTEXT_DEPTH).collect();
        let truncated = path.next().is_some();
        Self {
            path: kept,
            truncated,
            site,
        }
    }

    /// The node first, then its parent, up to the cap.
    #[must_use]
    pub fn path(&self) -> &[PathEntry] {
        &self.path
    }

    /// Whether ancestors past the cap were left out.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Where the node's view was created.
    #[must_use]
    pub const fn site(&self) -> CreationSite {
        self.site
    }

    /// Whether the context says anything: no path and no site.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty() && self.site.location().is_none()
    }
}

/// `at RenderPadding (r12) < RenderFlex (r7), created at src/app.rs:41:13`, with each part left
/// out when it is not known.
impl fmt::Display for TreeContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut wrote = false;
        for (index, entry) in self.path.iter().enumerate() {
            f.write_str(if index == 0 { "at " } else { " < " })?;
            write!(f, "{entry}")?;
            wrote = true;
        }
        if self.truncated {
            f.write_str(" < ...")?;
        }
        if let Some(site) = self.site.location() {
            if wrote {
                f.write_str(", ")?;
            }
            write!(f, "created at {site}")?;
        }
        Ok(())
    }
}

/// An error `E` with the [`TreeContext`] of the node it happened at.
///
/// Displays as the error, a space, and the context: `layout cycle at RenderPadding (r12) <
/// RenderFlex (r7), created at src/app.rs:41:13`. It reads as the wrapped error through
/// [`Deref`](std::ops::Deref), and [`into_error`](Self::into_error) gives the error back.
#[derive(Debug)]
#[non_exhaustive]
pub struct TreeError<E> {
    error: E,
    context: TreeContext,
}

impl<E> TreeError<E> {
    /// `error` at `context`.
    #[must_use]
    pub const fn new(error: E, context: TreeContext) -> Self {
        Self { error, context }
    }

    /// The wrapped error.
    #[must_use]
    pub const fn error(&self) -> &E {
        &self.error
    }

    /// Where it happened.
    #[must_use]
    pub const fn context(&self) -> &TreeContext {
        &self.context
    }

    /// The wrapped error, without its context.
    #[must_use]
    pub fn into_error(self) -> E {
        self.error
    }

    /// The same context around the error `f` makes from this one.
    pub fn map<F>(self, f: impl FnOnce(E) -> F) -> TreeError<F> {
        TreeError {
            error: f(self.error),
            context: self.context,
        }
    }
}

/// An error from a place that knows no node: it carries an empty context.
impl<E> From<E> for TreeError<E> {
    fn from(error: E) -> Self {
        Self::new(error, TreeContext::default())
    }
}

impl<E> std::ops::Deref for TreeError<E> {
    type Target = E;

    fn deref(&self) -> &E {
        &self.error
    }
}

impl<E: fmt::Display> fmt::Display for TreeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;
        if !self.context.is_empty() {
            write!(f, " {}", self.context)?;
        }
        Ok(())
    }
}

impl<E: Error + 'static> Error for TreeError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderId;

    fn entry(kind: &'static str, slot: usize) -> PathEntry {
        PathEntry::new(
            kind,
            InspectId::from(RenderId::new_gen(
                u32::try_from(slot).expect("small slot"),
                crate::generation::Generation::FIRST,
            )),
        )
    }

    #[test]
    fn a_context_reads_from_the_node_up_to_its_ancestors() {
        let context = TreeContext::new(
            [entry("RenderPadding", 12), entry("RenderFlex", 7)],
            CreationSite::unknown(),
        );
        let text = TreeError::new("layout cycle", context).to_string();
        assert!(
            text.starts_with("layout cycle at RenderPadding (r12")
                && text.contains(" < RenderFlex (r7"),
            "{text}"
        );
    }

    #[test]
    fn a_path_past_the_cap_is_cut_and_says_so() {
        let long: Vec<_> = (0..TREE_CONTEXT_DEPTH + 3)
            .map(|i| entry("RenderBox", i))
            .collect();
        let context = TreeContext::new(long, CreationSite::unknown());
        assert_eq!(context.path().len(), TREE_CONTEXT_DEPTH);
        assert!(context.truncated());
        assert!(context.to_string().ends_with(" < ..."));

        let exact: Vec<_> = (0..TREE_CONTEXT_DEPTH)
            .map(|i| entry("RenderBox", i))
            .collect();
        assert!(!TreeContext::new(exact, CreationSite::unknown()).truncated());
    }

    #[test]
    fn an_empty_context_leaves_the_error_as_it_was() {
        let error = TreeError::new("boom", TreeContext::default());
        assert_eq!(error.to_string(), "boom");
        assert_eq!(*error, "boom");
        assert_eq!(error.into_error(), "boom");
    }

    #[test]
    fn the_site_follows_the_path_when_the_build_keeps_sites() {
        let site = CreationSite::here();
        let context = TreeContext::new([entry("RenderBox", 1)], site);
        let text = context.to_string();
        if CreationSite::KEPT {
            assert!(
                text.contains(", created at ") && text.contains("tree_error.rs"),
                "{text}"
            );
        } else {
            assert!(!text.contains("created at"), "{text}");
        }
    }
}
