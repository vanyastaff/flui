//! Multiple children wrapper using `ViewConfig` for deferred mounting.
//!
//! # Phase 5: Flutter-like Children Mounting API
//!
//! This is the new `Children` implementation that stores `Vec<ViewConfig>` instead of
//! `Vec<Box<dyn ViewObject>>`, enabling:
//! - **Flutter-like API**: Pass views as config, not pre-created objects
//! - **Hot-reload**: Recreate view objects from configuration
//! - **Lazy mounting**: Delay state creation until mount time
//!
//! # Example
//!
//! ```rust,ignore
//! pub struct Column {
//!     children: Children,
//! }
//!
//! impl Column {
//!     pub fn new() -> Self {
//!         Self { children: Children::new() }
//!     }
//!
//!     pub fn child(mut self, child: impl IntoViewConfig) -> Self {
//!         self.children.push(child);
//!         self
//!     }
//! }
//!
//! // Later, during mount:
//! let child_handles = column.children.mount_all(Some(parent_id));
//! ```

use crate::handle::{ViewConfig, ViewHandle};
use crate::IntoViewConfig;
use flui_tree::{Mountable, Mounted};

/// Multiple children wrapper that stores view configurations.
///
/// Provides a cleaner API than `Vec<ViewConfig>` for multi-child widgets.
///
/// # Key Differences from Old `Children`
///
/// | Old Children | New Children |
/// |--------------|--------------|
/// | Stores `Vec<Box<dyn ViewObject>>` | Stores `Vec<ViewConfig>` |
/// | Immediate object creation | Deferred until `mount_all()` |
/// | No hot-reload support | Full hot-reload support |
/// | `impl IntoView` | `impl IntoViewConfig` |
///
/// # Performance
///
/// - Uses `Vec` internally for cache-friendly access
/// - Reserves capacity when extending with sized iterators
/// - Provides `shrink_to_fit()` to reduce memory usage
#[derive(Default)]
pub struct Children {
    inner: Vec<ViewConfig>,
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("count", &self.inner.len())
            .finish()
    }
}

impl Children {
    /// Creates an empty list.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Creates with pre-allocated capacity.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    /// Adds a child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut children = Children::new();
    /// children.push(Text::new("Hello"));
    /// ```
    #[inline]
    pub fn push<V: IntoViewConfig>(&mut self, view: V) {
        self.inner.push(view.into_view_config());
    }

    /// Adds a `ViewConfig`.
    #[inline]
    pub fn push_view_config(&mut self, config: ViewConfig) {
        self.inner.push(config);
    }

    /// Extends with multiple children.
    ///
    /// Reserves capacity if the iterator provides a size hint for better performance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut children = Children::new();
    /// children.extend(vec![Text::new("A"), Text::new("B")]);
    /// ```
    #[inline]
    pub fn extend<V, I>(&mut self, views: I)
    where
        V: IntoViewConfig,
        I: IntoIterator<Item = V>,
    {
        let iter = views.into_iter();
        // Reserve capacity based on size hint for better performance
        let (lower, _) = iter.size_hint();
        self.inner.reserve(lower);

        for view in iter {
            self.push(view);
        }
    }

    /// Returns the number of children.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clears all children.
    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Converts to `Vec<ViewConfig>`.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> Vec<ViewConfig> {
        self.inner
    }

    /// Returns the current capacity.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Reserves capacity for at least `additional` more children.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    /// Shrinks capacity to fit the number of children.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.inner.shrink_to_fit();
    }

    /// Returns an iterator over the children configs.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &ViewConfig> + '_ {
        self.inner.iter()
    }

    /// Gets a reference to the config at the given index.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&ViewConfig> {
        self.inner.get(index)
    }
}

// ============================================================================
// MOUNTING API
// ============================================================================

impl Children {
    /// Mount all children, creating `Vec<ViewHandle<Mounted>>`.
    ///
    /// This converts all stored `ViewConfig` instances into mounted view handles
    /// with live `ViewObject` state.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent node ID for all children
    ///
    /// # Returns
    ///
    /// A vector of mounted view handles, one for each child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_view::Children;
    ///
    /// let mut children = Children::new();
    /// children.push(Text::new("A"));
    /// children.push(Text::new("B"));
    ///
    /// // Later, during mount:
    /// let mounted = children.mount_all(Some(parent_id));
    /// assert_eq!(mounted.len(), 2);
    /// ```
    pub fn mount_all(self, parent: Option<usize>) -> Vec<ViewHandle<Mounted>> {
        self.inner
            .into_iter()
            .map(|config| {
                let handle = ViewHandle::from_config(config);
                handle.mount(parent)
            })
            .collect()
    }

    /// Mount children at specific indices, creating handles for selected children.
    ///
    /// This is useful for partial updates during reconciliation.
    ///
    /// # Parameters
    ///
    /// - `indices`: Indices of children to mount
    /// - `parent`: Parent node ID for children
    ///
    /// # Returns
    ///
    /// A vector of (index, handle) pairs for mounted children.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut children = Children::new();
    /// children.push(Text::new("A"));
    /// children.push(Text::new("B"));
    /// children.push(Text::new("C"));
    ///
    /// // Mount only children at indices 0 and 2
    /// let mounted = children.mount_indices(&[0, 2], Some(parent_id));
    /// assert_eq!(mounted.len(), 2);
    /// ```
    pub fn mount_indices(
        &self,
        indices: &[usize],
        parent: Option<usize>,
    ) -> Vec<(usize, ViewHandle<Mounted>)> {
        indices
            .iter()
            .filter_map(|&idx| {
                self.get(idx).map(|config| {
                    let handle = ViewHandle::from_config(config.clone());
                    (idx, handle.mount(parent))
                })
            })
            .collect()
    }
}

// ============================================================================
// CONVERSIONS
// ============================================================================

impl<V: IntoViewConfig> FromIterator<V> for Children {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut children = Children::with_capacity(lower);
        children.extend(iter);
        children
    }
}

// Implement Extend trait for idiomatic Rust
impl<V: IntoViewConfig> Extend<V> for Children {
    #[inline]
    fn extend<I: IntoIterator<Item = V>>(&mut self, iter: I) {
        Children::extend(self, iter);
    }
}

impl IntoIterator for Children {
    type Item = ViewConfig;
    type IntoIter = std::vec::IntoIter<ViewConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl From<Children> for Vec<ViewConfig> {
    fn from(children: Children) -> Self {
        children.inner
    }
}

// Allow Vec<V> where V: IntoViewConfig to be converted to Children
impl<V: IntoViewConfig> From<Vec<V>> for Children {
    fn from(views: Vec<V>) -> Self {
        Children {
            inner: views
                .into_iter()
                .map(IntoViewConfig::into_view_config)
                .collect(),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::StatelessView;
    use crate::BuildContext;
    use flui_tree::NavigableHandle;

    #[derive(Clone)]
    struct TestView {
        value: i32,
    }

    impl StatelessView for TestView {
        fn build(self, _ctx: &dyn BuildContext) -> impl crate::IntoView {
            crate::EmptyView
        }
    }

    #[test]
    fn test_children_new() {
        let children = Children::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_children_with_capacity() {
        let children = Children::with_capacity(10);
        assert!(children.is_empty());
        assert!(children.capacity() >= 10);
    }

    #[test]
    fn test_children_push() {
        let mut children = Children::new();
        children.push(TestView { value: 1 });
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_children_from_vec() {
        let views = vec![TestView { value: 1 }, TestView { value: 2 }];
        let children: Children = views.into();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_extend() {
        let mut children = Children::new();
        children.extend(vec![TestView { value: 1 }, TestView { value: 2 }]);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_extend_trait() {
        let mut children = Children::new();
        // Test that Extend trait is implemented
        Extend::extend(
            &mut children,
            vec![TestView { value: 1 }, TestView { value: 2 }],
        );
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_iter() {
        let mut children = Children::new();
        children.push(TestView { value: 1 });
        children.push(TestView { value: 2 });

        let count = children.iter().count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_children_mount_all() {
        let mut children = Children::new();
        children.push(TestView { value: 1 });
        children.push(TestView { value: 2 });
        children.push(TestView { value: 3 });

        // Mount all as root
        let mounted = children.mount_all(None);
        assert_eq!(mounted.len(), 3);

        // All should be roots
        for handle in mounted {
            assert!(handle.is_root());
            assert_eq!(handle.depth(), 0);
        }
    }

    #[test]
    fn test_children_mount_all_with_parent() {
        let mut children = Children::new();
        children.push(TestView { value: 1 });
        children.push(TestView { value: 2 });

        // Mount with parent
        let mounted = children.mount_all(Some(42));

        assert_eq!(mounted.len(), 2);
        for handle in mounted {
            assert!(!handle.is_root());
            assert_eq!(handle.parent(), Some(42));
        }
    }

    #[test]
    fn test_children_mount_indices() {
        let mut children = Children::new();
        children.push(TestView { value: 1 });
        children.push(TestView { value: 2 });
        children.push(TestView { value: 3 });

        // Mount only indices 0 and 2
        let mounted = children.mount_indices(&[0, 2], Some(10));
        assert_eq!(mounted.len(), 2);

        let indices: Vec<_> = mounted.iter().map(|(idx, _)| *idx).collect();
        assert_eq!(indices, vec![0, 2]);

        for (_, handle) in mounted {
            assert_eq!(handle.parent(), Some(10));
        }
    }

    #[test]
    fn test_children_mount_empty() {
        let children = Children::new();
        let mounted = children.mount_all(None);
        assert!(mounted.is_empty());
    }

    #[test]
    fn test_children_get() {
        let mut children = Children::new();
        children.push(TestView { value: 1 });
        children.push(TestView { value: 2 });

        assert!(children.get(0).is_some());
        assert!(children.get(1).is_some());
        assert!(children.get(2).is_none());
    }
}
