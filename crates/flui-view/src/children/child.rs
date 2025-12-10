//! Optional single child wrapper using `ViewConfig` for deferred mounting.
//!
//! # Phase 5: Flutter-like Child Mounting API
//!
//! This is the new `Child` implementation that stores `ViewConfig` instead of
//! `ViewObject`, enabling:
//! - **Flutter-like API**: Pass views as config, not pre-created objects
//! - **Hot-reload**: Recreate view objects from configuration
//! - **Lazy mounting**: Delay state creation until mount time
//!
//! # Example
//!
//! ```rust,ignore
//! pub struct Padding {
//!     padding: EdgeInsets,
//!     child: Child,
//! }
//!
//! impl Padding {
//!     pub fn new(padding: EdgeInsets) -> Self {
//!         Self { padding, child: Child::none() }
//!     }
//!
//!     pub fn child(mut self, child: impl IntoViewConfig) -> Self {
//!         self.child = Child::new(child);
//!         self
//!     }
//! }
//!
//! // Later, during mount:
//! let child_handle = padding.child.mount_as_root();
//! ```

use crate::handle::{ViewConfig, ViewHandle};
use crate::IntoViewConfig;
use flui_foundation::ViewId;
use flui_tree::{Depth, Mounted};

/// Optional single child wrapper that stores view configuration.
///
/// This provides a cleaner API than `Option<ViewConfig>` for single-child widgets.
///
/// # Key Differences from Old `Child`
///
/// | Old Child | New Child |
/// |-----------|-----------|
/// | Stores `ViewObject` (state) | Stores `ViewConfig` (config) |
/// | Immediate object creation | Deferred until `mount()` |
/// | No hot-reload support | Full hot-reload support |
/// | `impl IntoView` | `impl IntoViewConfig` |
///
/// # Examples
///
/// ```rust,ignore
/// use flui_view::{Child, StatelessView, IntoViewConfig};
///
/// pub struct Container {
///     child: Child,
/// }
///
/// impl Container {
///     pub fn new() -> Self {
///         Self { child: Child::none() }
///     }
///
///     pub fn child(mut self, child: impl IntoViewConfig) -> Self {
///         self.child = Child::new(child);
///         self
///     }
/// }
/// ```
#[derive(Default)]
pub struct Child {
    inner: Option<ViewConfig>,
}

impl std::fmt::Debug for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child")
            .field("has_child", &self.inner.is_some())
            .finish()
    }
}

impl Child {
    /// Creates an empty child.
    #[inline]
    #[must_use]
    pub const fn none() -> Self {
        Self { inner: None }
    }

    /// Creates a child from a view configuration.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_view::{Child, IntoViewConfig};
    ///
    /// let child = Child::new(Text::new("Hello"));
    /// ```
    #[inline]
    pub fn new<V: IntoViewConfig>(view: V) -> Self {
        Self {
            inner: Some(view.into_view_config()),
        }
    }

    /// Creates a child from a `ViewConfig`.
    #[inline]
    #[must_use]
    pub const fn from_view_config(config: ViewConfig) -> Self {
        Self {
            inner: Some(config),
        }
    }

    /// Creates a child from an `Option<ViewConfig>`.
    #[inline]
    #[must_use]
    pub const fn from_option(option: Option<ViewConfig>) -> Self {
        Self { inner: option }
    }

    /// Returns `true` if empty.
    #[inline]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    /// Returns `true` if has child.
    #[inline]
    #[must_use]
    pub const fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Converts to `Option<ViewConfig>`.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> Option<ViewConfig> {
        self.inner
    }

    /// Takes the view config out of Child, leaving None in its place.
    #[inline]
    pub fn take(&mut self) -> Option<ViewConfig> {
        self.inner.take()
    }

    /// Replaces the child, returning the old one if present.
    #[inline]
    pub fn replace<V: IntoViewConfig>(&mut self, view: V) -> Option<ViewConfig> {
        self.inner.replace(view.into_view_config())
    }

    /// Maps the view config if present.
    #[inline]
    pub fn map<F, U>(self, f: F) -> Option<U>
    where
        F: FnOnce(ViewConfig) -> U,
    {
        self.inner.map(f)
    }

    /// Returns a reference to the inner view config if present.
    #[inline]
    #[must_use]
    pub fn as_ref(&self) -> Option<&ViewConfig> {
        self.inner.as_ref()
    }

    /// Unwraps the child, panicking if None.
    ///
    /// # Panics
    ///
    /// Panics if the child is None.
    #[inline]
    #[must_use]
    pub fn unwrap(self) -> ViewConfig {
        self.inner.unwrap()
    }

    /// Returns the child or the provided default.
    #[inline]
    #[must_use]
    pub fn unwrap_or<V: IntoViewConfig>(self, default: V) -> ViewConfig {
        self.inner.unwrap_or_else(|| default.into_view_config())
    }

    /// Returns the child or computes it from a closure.
    #[inline]
    pub fn unwrap_or_else<F>(self, f: F) -> ViewConfig
    where
        F: FnOnce() -> ViewConfig,
    {
        self.inner.unwrap_or_else(f)
    }
}

// ============================================================================
// MOUNTING API
// ============================================================================

impl Child {
    /// Mount the child view as root, creating a `ViewHandle<Mounted>`.
    ///
    /// # Returns
    ///
    /// `Some(ViewHandle<Mounted>)` if child exists, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let child = Child::new(Text::new("Hello"));
    /// if let Some(mounted) = child.mount_as_root() {
    ///     println!("Mounted child: {:?}", mounted);
    /// }
    /// ```
    pub fn mount_as_root(self) -> Option<ViewHandle<Mounted>> {
        self.inner.map(|config| {
            let handle = ViewHandle::from_config(config);
            handle.mount_as_root()
        })
    }

    /// Mount the child view as child of parent.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent node ID
    /// - `parent_depth`: Depth of the parent
    ///
    /// # Returns
    ///
    /// `Some(ViewHandle<Mounted>)` if child exists, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let child = Child::new(Text::new("Hello"));
    /// if let Some(mounted) = child.mount_as_child(parent_id, parent_depth) {
    ///     println!("Mounted child: {:?}", mounted);
    /// }
    /// ```
    pub fn mount_as_child(
        self,
        parent: ViewId,
        parent_depth: Depth,
    ) -> Option<ViewHandle<Mounted>> {
        self.inner.map(|config| {
            let handle = ViewHandle::from_config(config);
            handle.mount_as_child(parent, parent_depth)
        })
    }

    /// Mount the child view with explicit parent and depth.
    ///
    /// # Parameters
    ///
    /// - `parent`: Optional parent node ID (None for root)
    /// - `depth`: Depth in tree
    ///
    /// # Returns
    ///
    /// `Some(ViewHandle<Mounted>)` if child exists, `None` otherwise.
    pub fn mount(self, parent: Option<ViewId>, depth: Depth) -> Option<ViewHandle<Mounted>> {
        self.inner.map(|config| {
            let handle = ViewHandle::from_config(config);
            handle.mount(parent, depth)
        })
    }

    /// Check if the child config can update another config.
    ///
    /// Used during reconciliation to determine if views are compatible.
    pub fn can_update(&self, other: &Self) -> bool {
        match (&self.inner, &other.inner) {
            (Some(a), Some(b)) => a.can_update(b),
            (None, None) => true,
            _ => false,
        }
    }
}

// ============================================================================
// CONVERSIONS
// ============================================================================

impl From<Child> for Option<ViewConfig> {
    #[inline]
    fn from(child: Child) -> Self {
        child.inner
    }
}

impl<V: IntoViewConfig> From<Option<V>> for Child {
    #[inline]
    fn from(option: Option<V>) -> Self {
        match option {
            Some(view) => Child::new(view),
            None => Child::none(),
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
    fn test_child_none() {
        let child = Child::none();
        assert!(child.is_none());
        assert!(!child.is_some());
    }

    #[test]
    fn test_child_default() {
        let child = Child::default();
        assert!(child.is_none());
    }

    #[test]
    fn test_child_new() {
        let child = Child::new(TestView { value: 42 });
        assert!(child.is_some());
        assert!(!child.is_none());
    }

    #[test]
    fn test_child_from_option() {
        let child: Child = Some(TestView { value: 1 }).into();
        assert!(child.is_some());

        let child: Child = None::<TestView>.into();
        assert!(child.is_none());
    }

    #[test]
    fn test_child_replace() {
        let mut child = Child::new(TestView { value: 1 });
        assert!(child.is_some());

        let old = child.replace(TestView { value: 2 });
        assert!(old.is_some());
        assert!(child.is_some());
    }

    #[test]
    fn test_child_unwrap_or() {
        let child = Child::none();
        let config = child.unwrap_or(TestView { value: 99 });
        // Config should be created
        assert!(config.can_update(&TestView { value: 100 }.into_view_config()));
    }

    #[test]
    fn test_child_mount_as_root() {
        let child = Child::new(TestView { value: 42 });

        // Mount as root
        let mounted = child.mount_as_root();
        assert!(mounted.is_some());

        if let Some(handle) = mounted {
            assert!(handle.is_root());
            assert_eq!(handle.depth(), Depth::root());
        }
    }

    #[test]
    fn test_child_mount_as_child() {
        let child = Child::new(TestView { value: 42 });
        let parent_id = ViewId::new(10);

        let mounted = child.mount_as_child(parent_id, Depth::root());
        assert!(mounted.is_some());

        if let Some(handle) = mounted {
            assert!(!handle.is_root());
            assert_eq!(handle.parent(), Some(parent_id));
            assert_eq!(handle.depth(), Depth::new(1));
        }
    }

    #[test]
    fn test_child_mount_none() {
        let child = Child::none();
        let mounted = child.mount_as_root();
        assert!(mounted.is_none());
    }

    #[test]
    fn test_child_can_update() {
        let child1 = Child::new(TestView { value: 1 });
        let child2 = Child::new(TestView { value: 2 });
        let child3 = Child::none();

        // Same type, should be able to update
        assert!(child1.can_update(&child2));

        // None vs Some = cannot update
        assert!(!child1.can_update(&child3));
        assert!(!child3.can_update(&child1));

        // None vs None = can update
        assert!(child3.can_update(&Child::none()));
    }
}
