//! Declared accessible collections and the provenance of their member indexes.

use flui_foundation::RenderId;

/// The accessible structure of a lazily materialized collection.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticsCollectionKind {
    /// A one-dimensional ordered set.
    List,
    /// A collection laid out in rows and columns.
    Grid,
}

/// Collection metadata independent of the number of materialized children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticsCollection {
    kind: SemanticsCollectionKind,
    count: Option<usize>,
}

impl SemanticsCollection {
    /// Declares a collection. `None` is an unknown total; totals outside the
    /// framework's signed set-index range also remain unknown.
    pub fn new(kind: SemanticsCollectionKind, count: Option<usize>) -> Self {
        Self {
            kind,
            count: count.filter(|count| i32::try_from(*count).is_ok()),
        }
    }

    /// The collection's accessible structure.
    #[must_use]
    pub fn kind(self) -> SemanticsCollectionKind {
        self.kind
    }

    /// The declared member count, including members outside the resident band.
    #[must_use]
    pub fn count(self) -> Option<usize> {
        self.count
    }
}

/// Which declaration an item's index belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticsIndexOrigin {
    /// Explicit numbering supplied by an annotation.
    Authored,
    /// Automatic membership in the named render host's declared collection.
    Collection(RenderId),
}

/// Who declared a collection's membership policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticsCollectionOrigin {
    /// Explicit indexes belong to the nearest authored collection.
    Authored,
    /// The lazy host stamps membership using its own typed render identity.
    Lazy,
}
