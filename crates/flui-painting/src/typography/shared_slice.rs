//! An immutable list a text style shares with its copies.
//!
//! A style is copied wherever it is inherited, merged or resolved (each `DefaultTextStyle`
//! read, each paragraph a theme styles, each icon), so its lists are shared rather than owned:
//! a copy costs a reference count, not an allocation. A style travels with the painter that
//! lays it out, which may move between threads, so the sharing is `Arc`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;

/// An immutable list shared by every copy of it. The empty list holds no allocation.
///
/// Built from a `Vec`, an array or an iterator; read as a slice.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        from = "Vec<T>",
        into = "Vec<T>",
        bound(
            serialize = "T: Clone + serde::Serialize",
            deserialize = "T: serde::Deserialize<'de>"
        )
    )
)]
pub struct SharedSlice<T>(Option<Arc<[T]>>);

impl<T> SharedSlice<T> {
    /// The empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self(None)
    }

    /// The items, in order.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        self.0.as_deref().unwrap_or_default()
    }

    /// This list with `item` appended: a new list, which the copies of this one do not see.
    #[must_use]
    pub fn with(&self, item: T) -> Self
    where
        T: Clone,
    {
        self.iter().cloned().chain(std::iter::once(item)).collect()
    }
}

impl<T> Default for SharedSlice<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for SharedSlice<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: PartialEq> PartialEq for SharedSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq> Eq for SharedSlice<T> {}

impl<T: Hash> Hash for SharedSlice<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<T> Deref for SharedSlice<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, I: Into<T>> FromIterator<I> for SharedSlice<T> {
    fn from_iter<It: IntoIterator<Item = I>>(items: It) -> Self {
        let items: Vec<T> = items.into_iter().map(Into::into).collect();
        Self::from(items)
    }
}

impl<T> From<Vec<T>> for SharedSlice<T> {
    fn from(items: Vec<T>) -> Self {
        if items.is_empty() {
            Self(None)
        } else {
            Self(Some(Arc::from(items)))
        }
    }
}

impl<T, I: Into<T>, const LEN: usize> From<[I; LEN]> for SharedSlice<T> {
    fn from(items: [I; LEN]) -> Self {
        items.into_iter().collect()
    }
}

impl<T: Clone> From<SharedSlice<T>> for Vec<T> {
    fn from(items: SharedSlice<T>) -> Self {
        items.as_slice().to_vec()
    }
}

impl<'a, T> IntoIterator for &'a SharedSlice<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<T: fmt::Debug> fmt::Debug for SharedSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}
