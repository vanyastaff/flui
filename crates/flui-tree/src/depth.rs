//! Tree depth abstraction with type safety and atomic support.
//!
//! This module provides `Depth` and `AtomicDepth` types for tracking
//! node position in tree hierarchies with compile-time guarantees.
//!
//! # Overview
//!
//! ```text
//! Root (depth=0)
//!   ├── Child A (depth=1)
//!   │     ├── Grandchild (depth=2)
//!   │     └── Grandchild (depth=2)
//!   └── Child B (depth=1)
//! ```
//!
//! # Why Type-Safe Depth?
//!
//! Using `Depth` instead of raw `usize` provides:
//!
//! - **Semantic clarity**: `depth.child_depth()` vs `depth + 1`
//! - **Validation**: Optional max depth enforcement
//! - **Consistency**: Same type across View, Element, and Render trees
//! - **Thread safety**: `AtomicDepth` for concurrent access
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_tree::{Depth, AtomicDepth, DepthAware};
//!
//! // Basic depth operations
//! let root = Depth::root();
//! assert!(root.is_root());
//!
//! let child = root.child_depth();
//! assert_eq!(child.get(), 1);
//!
//! // Atomic depth for thread-safe elements
//! let atomic = AtomicDepth::root();
//! atomic.set(Depth::new(5));
//! assert_eq!(atomic.get(), Depth::new(5));
//! ```
//!
//! # Thread Safety
//!
//! - `Depth`: `Copy + Send + Sync` (value type)
//! - `AtomicDepth`: `Send + Sync` (atomic operations)

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Maximum allowed tree depth.
///
/// This limit prevents:
/// - Stack overflow in recursive algorithms
/// - Pathological deep trees from consuming memory
/// - Integer overflow in depth calculations
///
/// Typical UI trees rarely exceed depth 32. This limit is conservative
/// but handles extreme cases.
pub const MAX_TREE_DEPTH: usize = 256;

/// Default depth for root nodes.
pub const ROOT_DEPTH: usize = 0;

// ============================================================================
// DEPTH
// ============================================================================

/// Type-safe tree depth wrapper.
///
/// Represents the distance from a node to the tree root:
/// - Root nodes have depth 0
/// - Direct children of root have depth 1
/// - And so on...
///
/// # Memory Layout
///
/// `Depth` is a zero-cost abstraction - same size as `usize`:
///
/// ```rust,ignore
/// assert_eq!(std::mem::size_of::<Depth>(), std::mem::size_of::<usize>());
/// ```
///
/// # Examples
///
/// ```rust,ignore
/// use flui_tree::Depth;
///
/// let root = Depth::root();
/// assert!(root.is_root());
/// assert_eq!(root.get(), 0);
///
/// let child = root.child_depth();
/// assert_eq!(child.get(), 1);
/// assert_eq!(child.distance_to(root), 1);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(transparent)]
pub struct Depth(usize);

impl Depth {
    // === CONSTRUCTORS ===

    /// Creates a new depth value.
    ///
    /// # Arguments
    ///
    /// * `value` - The depth value (0 = root)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let depth = Depth::new(3);
    /// assert_eq!(depth.get(), 3);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(value)
    }

    /// Creates a depth value with validation.
    ///
    /// Returns `None` if value exceeds `MAX_TREE_DEPTH`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert!(Depth::new_checked(100).is_some());
    /// assert!(Depth::new_checked(300).is_none());
    /// ```
    #[inline]
    #[must_use]
    pub const fn new_checked(value: usize) -> Option<Self> {
        if value <= MAX_TREE_DEPTH {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Creates a root depth (0).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let root = Depth::root();
    /// assert!(root.is_root());
    /// assert_eq!(root.get(), 0);
    /// ```
    #[inline]
    #[must_use]
    pub const fn root() -> Self {
        Self(ROOT_DEPTH)
    }

    // === ACCESSORS ===

    /// Returns the raw depth value.
    #[inline]
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }

    /// Returns true if this is the root depth (0).
    #[inline]
    #[must_use]
    pub const fn is_root(self) -> bool {
        self.0 == ROOT_DEPTH
    }

    /// Returns true if depth exceeds the maximum allowed.
    #[inline]
    #[must_use]
    pub const fn is_at_max(self) -> bool {
        self.0 >= MAX_TREE_DEPTH
    }

    // === NAVIGATION ===

    /// Returns the depth of a child node (self + 1).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let parent = Depth::new(5);
    /// let child = parent.child_depth();
    /// assert_eq!(child.get(), 6);
    /// ```
    #[inline]
    #[must_use]
    pub const fn child_depth(self) -> Self {
        Self(self.0 + 1)
    }

    /// Returns the depth of a child with validation.
    ///
    /// Returns `None` if result would exceed `MAX_TREE_DEPTH`.
    #[inline]
    #[must_use]
    pub const fn try_child_depth(self) -> Option<Self> {
        let new_depth = self.0 + 1;
        if new_depth <= MAX_TREE_DEPTH {
            Some(Self(new_depth))
        } else {
            None
        }
    }

    /// Returns the depth of the parent node (self - 1).
    ///
    /// Returns `None` if this is already root.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let child = Depth::new(3);
    /// let parent = child.parent_depth().unwrap();
    /// assert_eq!(parent.get(), 2);
    ///
    /// let root = Depth::root();
    /// assert!(root.parent_depth().is_none());
    /// ```
    #[inline]
    #[must_use]
    pub const fn parent_depth(self) -> Option<Self> {
        if self.0 == 0 {
            None
        } else {
            Some(Self(self.0 - 1))
        }
    }

    /// Returns the saturating child depth (clamps at `MAX_TREE_DEPTH`).
    #[inline]
    #[must_use]
    pub const fn saturating_child_depth(self) -> Self {
        if self.0 >= MAX_TREE_DEPTH {
            Self(MAX_TREE_DEPTH)
        } else {
            Self(self.0 + 1)
        }
    }

    /// Returns the saturating parent depth (clamps at 0).
    #[inline]
    #[must_use]
    pub const fn saturating_parent_depth(self) -> Self {
        if self.0 == 0 {
            Self(0)
        } else {
            Self(self.0 - 1)
        }
    }

    // === COMPARISON ===

    /// Calculates distance between two depths.
    ///
    /// Returns the absolute difference between depths.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let a = Depth::new(2);
    /// let b = Depth::new(7);
    /// assert_eq!(a.distance_to(b), 5);
    /// assert_eq!(b.distance_to(a), 5);
    /// ```
    #[inline]
    #[must_use]
    pub const fn distance_to(self, other: Self) -> usize {
        self.0.abs_diff(other.0)
    }

    /// Returns true if this depth is deeper than another.
    #[inline]
    #[must_use]
    pub const fn is_deeper_than(self, other: Self) -> bool {
        self.0 > other.0
    }

    /// Returns true if this depth is shallower than another.
    #[inline]
    #[must_use]
    pub const fn is_shallower_than(self, other: Self) -> bool {
        self.0 < other.0
    }

    /// Returns true if this depth is at the same level as another.
    #[inline]
    #[must_use]
    pub const fn is_same_level(self, other: Self) -> bool {
        self.0 == other.0
    }

    // === ARITHMETIC ===

    /// Adds levels to depth (for going deeper).
    ///
    /// Returns `None` on overflow or exceeding max.
    #[inline]
    #[must_use]
    pub const fn checked_add(self, levels: usize) -> Option<Self> {
        match self.0.checked_add(levels) {
            Some(result) if result <= MAX_TREE_DEPTH => Some(Self(result)),
            _ => None,
        }
    }

    /// Subtracts levels from depth (for going up).
    ///
    /// Returns `None` on underflow.
    #[inline]
    #[must_use]
    pub const fn checked_sub(self, levels: usize) -> Option<Self> {
        match self.0.checked_sub(levels) {
            Some(result) => Some(Self(result)),
            None => None,
        }
    }

    /// Saturating add (clamps at `MAX_TREE_DEPTH`).
    #[inline]
    #[must_use]
    pub const fn saturating_add(self, levels: usize) -> Self {
        let result = self.0.saturating_add(levels);
        if result > MAX_TREE_DEPTH {
            Self(MAX_TREE_DEPTH)
        } else {
            Self(result)
        }
    }

    /// Saturating sub (clamps at 0).
    #[inline]
    #[must_use]
    pub const fn saturating_sub(self, levels: usize) -> Self {
        Self(self.0.saturating_sub(levels))
    }
}

// === TRAIT IMPLEMENTATIONS ===

impl fmt::Debug for Depth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Depth({})", self.0)
    }
}

impl fmt::Display for Depth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for Depth {
    #[inline]
    fn from(value: usize) -> Self {
        Self::new(value)
    }
}

impl From<Depth> for usize {
    #[inline]
    fn from(depth: Depth) -> Self {
        depth.0
    }
}

// ============================================================================
// ATOMIC DEPTH
// ============================================================================

/// Thread-safe atomic depth for concurrent access.
///
/// Used in Element and RenderElement where depth may be accessed
/// from multiple threads during tree operations.
///
/// # Ordering
///
/// Uses `Acquire`/`Release` ordering for safe publication:
/// - `get()` uses `Acquire` to see writes from other threads
/// - `set()` uses `Release` to publish writes to other threads
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{AtomicDepth, Depth};
///
/// let depth = AtomicDepth::root();
/// assert!(depth.is_root());
///
/// depth.set(Depth::new(5));
/// assert_eq!(depth.get(), Depth::new(5));
///
/// // Atomic increment
/// let new_depth = depth.increment();
/// assert_eq!(new_depth, Depth::new(6));
/// ```
#[derive(Debug, Default)]
pub struct AtomicDepth(AtomicUsize);

impl AtomicDepth {
    // === CONSTRUCTORS ===

    /// Creates a new atomic depth.
    #[inline]
    #[must_use]
    pub const fn new(value: usize) -> Self {
        Self(AtomicUsize::new(value))
    }

    /// Creates an atomic root depth (0).
    #[inline]
    #[must_use]
    pub const fn root() -> Self {
        Self::new(ROOT_DEPTH)
    }

    /// Creates from a `Depth` value.
    #[inline]
    #[must_use]
    pub fn from_depth(depth: Depth) -> Self {
        Self::new(depth.get())
    }

    // === ACCESSORS ===

    /// Loads the current depth value.
    #[inline]
    #[must_use]
    pub fn get(&self) -> Depth {
        Depth::new(self.0.load(Ordering::Acquire))
    }

    /// Stores a new depth value.
    #[inline]
    pub fn set(&self, depth: Depth) {
        self.0.store(depth.get(), Ordering::Release);
    }

    /// Returns true if this is root depth.
    #[inline]
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.get().is_root()
    }

    /// Returns the raw atomic reference (for advanced use).
    #[inline]
    pub fn as_atomic(&self) -> &AtomicUsize {
        &self.0
    }

    // === ATOMIC OPERATIONS ===

    /// Atomically increments depth (for child).
    ///
    /// Returns the new depth value.
    #[inline]
    pub fn increment(&self) -> Depth {
        let new_val = self.0.fetch_add(1, Ordering::AcqRel) + 1;
        Depth::new(new_val)
    }

    /// Atomically decrements depth (for parent).
    ///
    /// Returns `None` if already at root.
    #[inline]
    pub fn decrement(&self) -> Option<Depth> {
        loop {
            let current = self.0.load(Ordering::Acquire);
            if current == 0 {
                return None;
            }

            if self
                .0
                .compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(Depth::new(current - 1));
            }
            // Retry on failure
        }
    }

    /// Compare and swap depth value.
    ///
    /// Returns `Ok(current)` if swap succeeded, `Err(actual)` if not.
    #[inline]
    pub fn compare_exchange(&self, current: Depth, new: Depth) -> Result<Depth, Depth> {
        self.0
            .compare_exchange(
                current.get(),
                new.get(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map(Depth::new)
            .map_err(Depth::new)
    }

    /// Updates depth from parent's depth (sets to parent + 1).
    #[inline]
    pub fn update_from_parent(&self, parent_depth: Depth) {
        self.set(parent_depth.child_depth());
    }

    /// Fetches current and replaces with new value.
    #[inline]
    pub fn swap(&self, new: Depth) -> Depth {
        Depth::new(self.0.swap(new.get(), Ordering::AcqRel))
    }
}

impl Clone for AtomicDepth {
    fn clone(&self) -> Self {
        Self::new(self.0.load(Ordering::Acquire))
    }
}

impl From<Depth> for AtomicDepth {
    fn from(depth: Depth) -> Self {
        Self::from_depth(depth)
    }
}

impl From<usize> for AtomicDepth {
    fn from(value: usize) -> Self {
        Self::new(value)
    }
}

// ============================================================================
// DEPTH ERROR
// ============================================================================

/// Errors that can occur during depth operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepthError {
    /// Attempted to exceed maximum tree depth.
    MaxDepthExceeded {
        /// The depth that was attempted.
        attempted: usize,
        /// The maximum allowed depth.
        max: usize,
    },
    /// Attempted to get parent of root.
    NoParentForRoot,
    /// Depth mismatch during validation.
    DepthMismatch {
        /// The expected depth.
        expected: usize,
        /// The actual depth.
        actual: usize,
    },
}

impl fmt::Display for DepthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxDepthExceeded { attempted, max } => {
                write!(f, "max depth exceeded: {attempted} > {max}")
            }
            Self::NoParentForRoot => {
                write!(f, "cannot get parent depth of root node")
            }
            Self::DepthMismatch { expected, actual } => {
                write!(f, "depth mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for DepthError {}

// ============================================================================
// DEPTH AWARE TRAIT
// ============================================================================

/// Trait for types that track their depth in a tree.
///
/// Implement this for node types that cache their depth for O(1) access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{Depth, DepthAware};
///
/// struct MyElement {
///     depth: Depth,
/// }
///
/// impl DepthAware for MyElement {
///     fn depth(&self) -> Depth {
///         self.depth
///     }
///
///     fn set_depth(&mut self, depth: Depth) {
///         self.depth = depth;
///     }
/// }
/// ```
pub trait DepthAware {
    /// Returns the node's depth in the tree.
    fn depth(&self) -> Depth;

    /// Sets the node's depth.
    fn set_depth(&mut self, depth: Depth);

    /// Returns true if this is a root node (depth 0).
    #[inline]
    fn is_at_root_depth(&self) -> bool {
        self.depth().is_root()
    }

    /// Updates depth based on parent's depth.
    #[inline]
    fn update_depth_from_parent(&mut self, parent_depth: Depth) {
        self.set_depth(parent_depth.child_depth());
    }

    /// Validates depth against expected value.
    #[inline]
    fn validate_depth(&self, expected: Depth) -> Result<(), DepthError> {
        let actual = self.depth();
        if actual == expected {
            Ok(())
        } else {
            Err(DepthError::DepthMismatch {
                expected: expected.get(),
                actual: actual.get(),
            })
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === DEPTH TESTS ===

    #[test]
    fn test_depth_new() {
        let depth = Depth::new(5);
        assert_eq!(depth.get(), 5);
    }

    #[test]
    fn test_depth_root() {
        let root = Depth::root();
        assert!(root.is_root());
        assert_eq!(root.get(), 0);
    }

    #[test]
    fn test_depth_child() {
        let parent = Depth::new(3);
        let child = parent.child_depth();
        assert_eq!(child.get(), 4);
        assert!(!child.is_root());
    }

    #[test]
    fn test_depth_parent() {
        let child = Depth::new(3);
        let parent = child.parent_depth().unwrap();
        assert_eq!(parent.get(), 2);

        let root = Depth::root();
        assert!(root.parent_depth().is_none());
    }

    #[test]
    fn test_depth_distance() {
        let a = Depth::new(2);
        let b = Depth::new(7);
        assert_eq!(a.distance_to(b), 5);
        assert_eq!(b.distance_to(a), 5);

        let same = Depth::new(5);
        assert_eq!(same.distance_to(same), 0);
    }

    #[test]
    fn test_depth_comparison() {
        let shallow = Depth::new(2);
        let deep = Depth::new(5);

        assert!(deep.is_deeper_than(shallow));
        assert!(shallow.is_shallower_than(deep));
        assert!(!shallow.is_deeper_than(deep));
        assert!(shallow.is_same_level(Depth::new(2)));
    }

    #[test]
    fn test_depth_checked() {
        assert!(Depth::new_checked(100).is_some());
        assert!(Depth::new_checked(MAX_TREE_DEPTH).is_some());
        assert!(Depth::new_checked(MAX_TREE_DEPTH + 1).is_none());
    }

    #[test]
    fn test_depth_try_child() {
        let normal = Depth::new(10);
        assert!(normal.try_child_depth().is_some());

        let at_max = Depth::new(MAX_TREE_DEPTH);
        assert!(at_max.try_child_depth().is_none());
    }

    #[test]
    fn test_depth_saturating() {
        let at_max = Depth::new(MAX_TREE_DEPTH);
        assert_eq!(at_max.saturating_child_depth().get(), MAX_TREE_DEPTH);

        let root = Depth::root();
        assert_eq!(root.saturating_parent_depth().get(), 0);
    }

    #[test]
    fn test_depth_arithmetic() {
        let depth = Depth::new(10);

        assert_eq!(depth.checked_add(5), Some(Depth::new(15)));
        assert_eq!(depth.checked_sub(5), Some(Depth::new(5)));
        assert_eq!(depth.checked_sub(15), None);

        assert_eq!(depth.saturating_add(1000).get(), MAX_TREE_DEPTH);
        assert_eq!(depth.saturating_sub(1000).get(), 0);
    }

    #[test]
    fn test_depth_display() {
        let depth = Depth::new(42);
        assert_eq!(format!("{}", depth), "42");
        assert_eq!(format!("{:?}", depth), "Depth(42)");
    }

    #[test]
    fn test_depth_from() {
        let depth: Depth = 5.into();
        assert_eq!(depth.get(), 5);

        let value: usize = depth.into();
        assert_eq!(value, 5);
    }

    // === ATOMIC DEPTH TESTS ===

    #[test]
    fn test_atomic_depth_new() {
        let depth = AtomicDepth::new(5);
        assert_eq!(depth.get(), Depth::new(5));
    }

    #[test]
    fn test_atomic_depth_root() {
        let depth = AtomicDepth::root();
        assert!(depth.is_root());
        assert_eq!(depth.get(), Depth::root());
    }

    #[test]
    fn test_atomic_depth_set_get() {
        let depth = AtomicDepth::new(0);
        depth.set(Depth::new(10));
        assert_eq!(depth.get(), Depth::new(10));
    }

    #[test]
    fn test_atomic_depth_increment() {
        let depth = AtomicDepth::new(5);
        let new = depth.increment();
        assert_eq!(new, Depth::new(6));
        assert_eq!(depth.get(), Depth::new(6));
    }

    #[test]
    fn test_atomic_depth_decrement() {
        let depth = AtomicDepth::new(5);
        let new = depth.decrement().unwrap();
        assert_eq!(new, Depth::new(4));

        let root = AtomicDepth::root();
        assert!(root.decrement().is_none());
    }

    #[test]
    fn test_atomic_depth_compare_exchange() {
        let depth = AtomicDepth::new(5);

        // Successful swap
        let result = depth.compare_exchange(Depth::new(5), Depth::new(10));
        assert_eq!(result, Ok(Depth::new(5)));
        assert_eq!(depth.get(), Depth::new(10));

        // Failed swap (wrong expected)
        let result = depth.compare_exchange(Depth::new(5), Depth::new(15));
        assert_eq!(result, Err(Depth::new(10)));
    }

    #[test]
    fn test_atomic_depth_update_from_parent() {
        let depth = AtomicDepth::root();
        depth.update_from_parent(Depth::new(3));
        assert_eq!(depth.get(), Depth::new(4));
    }

    #[test]
    fn test_atomic_depth_swap() {
        let depth = AtomicDepth::new(5);
        let old = depth.swap(Depth::new(10));
        assert_eq!(old, Depth::new(5));
        assert_eq!(depth.get(), Depth::new(10));
    }

    #[test]
    fn test_atomic_depth_clone() {
        let depth = AtomicDepth::new(42);
        let cloned = depth.clone();
        assert_eq!(cloned.get(), Depth::new(42));
    }

    // === DEPTH ERROR TESTS ===

    #[test]
    fn test_depth_error_display() {
        let err = DepthError::MaxDepthExceeded {
            attempted: 300,
            max: 256,
        };
        assert!(format!("{}", err).contains("300"));

        let err = DepthError::NoParentForRoot;
        assert!(format!("{}", err).contains("root"));

        let err = DepthError::DepthMismatch {
            expected: 5,
            actual: 10,
        };
        assert!(format!("{}", err).contains("5"));
        assert!(format!("{}", err).contains("10"));
    }

    // === DEPTH AWARE TESTS ===

    struct TestNode {
        depth: Depth,
    }

    impl DepthAware for TestNode {
        fn depth(&self) -> Depth {
            self.depth
        }

        fn set_depth(&mut self, depth: Depth) {
            self.depth = depth;
        }
    }

    #[test]
    fn test_depth_aware() {
        let mut node = TestNode {
            depth: Depth::root(),
        };

        assert!(node.is_at_root_depth());

        node.update_depth_from_parent(Depth::new(2));
        assert_eq!(node.depth(), Depth::new(3));

        assert!(node.validate_depth(Depth::new(3)).is_ok());
        assert!(node.validate_depth(Depth::new(5)).is_err());
    }
}
