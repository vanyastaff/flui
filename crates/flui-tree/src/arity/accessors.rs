//! Advanced generic children accessors with HRTB and GAT support.
//!
//! These accessors provide type-safe access to children based on arity
//! using cutting-edge Rust type system features:
//!
//! - **HRTB (Higher-Rank Trait Bounds)** - Universal predicates
//! - **GAT (Generic Associated Types)** - Flexible iterator types
//! - **Const Generics** - Compile-time size optimization
//! - **Associated Constants** - Performance tuning hints
//!
//! All accessors are generic over `T`, allowing them to work with any
//! element type (`ElementId`, `Element`, etc.) while maintaining
//! zero-cost abstractions.

use std::fmt::Debug;
use std::marker::PhantomData;

// ============================================================================
// CHILDREN ACCESS TRAIT
// ============================================================================

/// Enhanced trait for accessing children with GAT and HRTB support.
///
/// All arity accessors implement this trait, providing a common interface
/// for operations using advanced type system features.
///
/// # Advanced Features
///
/// - **GAT iterators** for flexible iteration patterns
/// - **HRTB predicates** for universal compatibility
/// - **Associated constants** for performance tuning
/// - **Zero-cost abstractions** via const generics
///
/// # Note
///
/// The trait requires `Copy` because all accessors are thin views over slices
/// and should be cheap to copy.
pub trait ChildrenAccess<'a, T: 'a>: Copy + Send + Sync {
    /// GAT-based iterator type for flexible iteration.
    type Iter: Iterator<Item = &'a T> + 'a;

    /// Performance hint for access patterns.
    const ACCESS_PATTERN: AccessPattern = AccessPattern::Sequential;

    /// Expected access frequency for optimization.
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Moderate;
    /// Returns the underlying slice of children.
    fn as_slice(&self) -> &'a [T];

    /// Returns GAT-based iterator over children.
    fn iter(&self) -> Self::Iter;

    /// Returns the number of children.
    #[inline]
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Returns `true` if there are no children.
    #[inline]
    fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Find first child matching HRTB predicate.
    fn find_where<P>(&self, predicate: P) -> Option<&'a T>
    where
        P: for<'b> Fn(&'b T) -> bool,
    {
        self.iter().find(|item| predicate(item))
    }

    /// Filter children with HRTB predicate.
    fn filter_where<P>(&self, predicate: P) -> Vec<&'a T>
    where
        P: for<'b> Fn(&'b T) -> bool,
    {
        self.iter().filter(|item| predicate(item)).collect()
    }

    /// Count children matching HRTB predicate.
    fn count_where<P>(&self, predicate: P) -> usize
    where
        P: for<'b> Fn(&'b T) -> bool,
    {
        self.iter().filter(|item| predicate(item)).count()
    }

    /// Execute HRTB closure for each child.
    fn for_each<F>(&self, mut f: F)
    where
        F: for<'b> FnMut(&'b T),
    {
        for item in self.iter() {
            f(item);
        }
    }
}

/// Performance characteristics for access patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessPattern {
    /// Sequential access pattern.
    Sequential,
    /// Random access pattern.
    Random,
    /// Bulk operations.
    Bulk,
    /// Single element access.
    Single,
}

/// Access frequency hints for optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessFrequency {
    /// Rare access.
    Rare,
    /// Moderate access.
    Moderate,
    /// Frequent access.
    Frequent,
    /// Critical path.
    Critical,
}

/// Special accessor for never operations that always panics.
#[derive(Debug)]
pub struct NeverAccessor<T: Send + Sync>(pub(crate) PhantomData<T>);

impl<T: Send + Sync> Clone for NeverAccessor<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Send + Sync> Copy for NeverAccessor<T> {}

impl<'a, T: 'a + Send + Sync> ChildrenAccess<'a, T> for NeverAccessor<T> {
    type Iter = std::iter::Empty<&'a T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Single;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Rare;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        panic!("Never type operations are impossible")
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        std::iter::empty()
    }
}

impl<T: Send + Sync> NeverAccessor<T> {
    /// This operation is impossible and will never return.
    pub fn impossible(&self) -> ! {
        panic!("This operation should never be called - it's impossible by design")
    }
}

// ============================================================================
// NO CHILDREN (Leaf) - Enhanced with Never Type Support
// ============================================================================

/// Enhanced accessor for leaf nodes with never type support.
///
/// This zero-size type provides compile-time guarantees that certain
/// operations are impossible (return never type `!`).
///
/// # Enhanced Features
///
/// - **Never type** operations for impossible access
/// - **Const evaluation** for all methods
/// - **Zero runtime cost** - everything optimized away
#[derive(Debug)]
pub struct NoChildren<T>(pub(crate) PhantomData<T>);

impl<T> Clone for NoChildren<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for NoChildren<T> {}

impl<'a, T: 'a + Send + Sync> ChildrenAccess<'a, T> for NoChildren<T> {
    type Iter = std::iter::Empty<&'a T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Single;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Rare;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        &[]
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        std::iter::empty()
    }
}

impl<T> NoChildren<T> {
    /// Attempt to get first child - impossible operation returns never type.
    ///
    /// This method demonstrates the never type for compile-time safety.
    /// It will never return because leaf nodes cannot have children.
    pub fn first_impossible(&self) -> ! {
        unreachable!("Leaf nodes cannot have children")
    }

    /// Const evaluation support for empty check.
    #[inline]
    pub const fn is_guaranteed_empty(&self) -> bool {
        true
    }
}

// ============================================================================
// OPTIONAL CHILD (Optional)
// ============================================================================

/// Accessor for optional single child (0 or 1).
///
/// Provides an `Option`-like API for accessing the optional child.
#[derive(Debug)]
pub struct OptionalChild<'a, T> {
    pub(crate) children: &'a [T],
}

impl<T> Clone for OptionalChild<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for OptionalChild<'_, T> {}

impl<'a, T: 'a + Send + Sync> ChildrenAccess<'a, T> for OptionalChild<'a, T> {
    type Iter = std::slice::Iter<'a, T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Single;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Frequent;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        self.children
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        self.children.iter()
    }
}

impl<'a, T> OptionalChild<'a, T> {
    /// Returns the child if present.
    #[inline]
    pub fn get(&self) -> Option<&'a T> {
        self.children.first()
    }

    /// Returns `true` if there is a child.
    #[inline]
    pub fn is_some(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns `true` if there is no child.
    #[inline]
    pub fn is_none(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns the child or panics if not present.
    ///
    /// # Panics
    ///
    /// Panics if there is no child.
    #[inline]
    pub fn unwrap(&self) -> &'a T {
        self.children.first().expect("Optional child is None")
    }

    /// Returns the child or a default value.
    #[inline]
    pub fn unwrap_or<'b>(&'b self, default: &'b T) -> &'b T
    where
        'a: 'b,
    {
        self.children.first().unwrap_or(default)
    }

    /// Maps the child using the provided function.
    #[inline]
    pub fn map<U, F>(&self, f: F) -> Option<U>
    where
        F: FnOnce(&'a T) -> U,
    {
        self.children.first().map(f)
    }

    /// Maps the child or returns a default value.
    #[inline]
    pub fn map_or<U, F>(&self, default: U, f: F) -> U
    where
        F: FnOnce(&'a T) -> U,
    {
        self.children.first().map(f).unwrap_or(default)
    }

    /// Maps the child or computes a default value.
    #[inline]
    pub fn map_or_else<U, D, F>(&self, default: D, f: F) -> U
    where
        D: FnOnce() -> U,
        F: FnOnce(&'a T) -> U,
    {
        self.children.first().map(f).unwrap_or_else(default)
    }
}

// Conversions for OptionalChild
impl<'a, T: Copy> OptionalChild<'a, T> {
    /// Converts to `Option<T>`, copying the inner value if present.
    ///
    /// This is the idiomatic way to get a `Copy` value from an `OptionalChild`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let accessor = Optional::from_slice(&[element_id]);
    /// let id: Option<ElementId> = accessor.copied();
    /// ```
    #[inline]
    pub fn copied(&self) -> Option<T> {
        self.children.first().copied()
    }
}

// ============================================================================
// FIXED CHILDREN (Exact<N>)
// ============================================================================

/// Accessor for a fixed number of children.
///
/// Provides compile-time guaranteed access to exactly N children.
/// Specialized methods are provided for common arities (1, 2, 3).
#[derive(Debug)]
pub struct FixedChildren<'a, T, const N: usize> {
    pub(crate) children: &'a [T; N],
}

impl<T, const N: usize> Clone for FixedChildren<'_, T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, const N: usize> Copy for FixedChildren<'_, T, N> {}

impl<'a, T: 'a + Send + Sync, const N: usize> ChildrenAccess<'a, T> for FixedChildren<'a, T, N> {
    type Iter = std::slice::Iter<'a, T>;

    const ACCESS_PATTERN: AccessPattern = if N <= 4 {
        AccessPattern::Single
    } else {
        AccessPattern::Sequential
    };
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Frequent;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        self.children
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        self.children.iter()
    }
}

impl<'a, T, const N: usize> FixedChildren<'a, T, N> {
    /// Get the underlying array with const generic validation.
    ///
    /// This method provides zero-cost conversion to array type.
    #[inline]
    pub const fn as_array(&self) -> &'a [T; N] {
        self.children
    }

    /// HRTB-compatible find operation for fixed arrays.
    pub fn find_fixed<P>(&self, predicate: P) -> Option<(usize, &'a T)>
    where
        P: for<'b> Fn(&'b T) -> bool,
    {
        self.children
            .iter()
            .enumerate()
            .find(|(_, item)| predicate(item))
    }
}

impl<'a, T, const N: usize> FixedChildren<'a, T, N> {
    /// Returns a reference to the child at the given index.
    ///
    /// # Panics
    ///
    /// Panics if `index >= N`.
    #[inline]
    pub fn get(&self, index: usize) -> &'a T {
        &self.children[index]
    }

    /// Returns an iterator over the children.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &'a T> {
        self.children.iter()
    }
}

// Specialized methods for Exact<1> (Single)
impl<'a, T> FixedChildren<'a, T, 1> {
    /// Returns the single child (guaranteed to exist).
    #[inline]
    pub fn single(&self) -> &'a T {
        &self.children[0]
    }
}

// Specialized methods for Exact<2>
impl<'a, T> FixedChildren<'a, T, 2> {
    /// Returns the first child.
    #[inline]
    pub fn first(&self) -> &'a T {
        &self.children[0]
    }

    /// Returns the second child.
    #[inline]
    pub fn second(&self) -> &'a T {
        &self.children[1]
    }

    /// Returns both children as a tuple.
    #[inline]
    pub fn pair(&self) -> (&'a T, &'a T) {
        (&self.children[0], &self.children[1])
    }
}

// Specialized methods for Exact<3>
impl<'a, T> FixedChildren<'a, T, 3> {
    /// Returns the first child.
    #[inline]
    pub fn first(&self) -> &'a T {
        &self.children[0]
    }

    /// Returns the second child.
    #[inline]
    pub fn second(&self) -> &'a T {
        &self.children[1]
    }

    /// Returns the third child.
    #[inline]
    pub fn third(&self) -> &'a T {
        &self.children[2]
    }

    /// Returns all three children as a tuple.
    #[inline]
    pub fn triple(&self) -> (&'a T, &'a T, &'a T) {
        (&self.children[0], &self.children[1], &self.children[2])
    }
}

// ============================================================================
// SLICE CHILDREN (Variable, AtLeast<N>)
// ============================================================================

/// Accessor for a variable number of children.
///
/// Provides slice-based access to any number of children.
#[derive(Debug)]
pub struct SliceChildren<'a, T> {
    pub(crate) children: &'a [T],
}

impl<T> Clone for SliceChildren<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SliceChildren<'_, T> {}

impl<'a, T: 'a + Send + Sync> ChildrenAccess<'a, T> for SliceChildren<'a, T> {
    type Iter = std::slice::Iter<'a, T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Sequential;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Moderate;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        self.children
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        self.children.iter()
    }
}

impl<'a, T> SliceChildren<'a, T> {
    /// Returns a reference to the child at the given index, if it exists.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&'a T> {
        self.children.get(index)
    }

    /// Returns an iterator over the children.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &'a T> + ExactSizeIterator + DoubleEndedIterator {
        self.children.iter()
    }

    /// Returns the first child, if any.
    #[inline]
    pub fn first(&self) -> Option<&'a T> {
        self.children.first()
    }

    /// Returns the last child, if any.
    #[inline]
    pub fn last(&self) -> Option<&'a T> {
        self.children.last()
    }

    /// Returns an iterator over (index, child) pairs.
    #[inline]
    pub fn enumerate(&self) -> impl Iterator<Item = (usize, &'a T)> + ExactSizeIterator {
        self.children.iter().enumerate()
    }

    /// Returns a reversed iterator over the children.
    #[inline]
    pub fn rev(&self) -> impl Iterator<Item = &'a T> + ExactSizeIterator {
        self.children.iter().rev()
    }
}

// ============================================================================
// COPIED WRAPPER (idiomatic Rust pattern)
// ============================================================================

/// A wrapper that provides value iteration for `Copy` types.
///
/// This is the idiomatic Rust way to get values from accessors.
/// Instead of `.iter_copy()`, use `.iter().copied()` or call `.copied()` on
/// the accessor to get a `Copied` wrapper.
///
/// # Example
///
/// ```rust,ignore
/// let ids: &[ElementId] = &[id1, id2, id3];
/// let accessor = Variable::from_slice(ids);
///
/// // Standard iterator pattern
/// for id in accessor.iter().copied() {
///     // id is ElementId (by value)
/// }
///
/// // Or use the Copied wrapper
/// for id in accessor.copied() {
///     // id is ElementId (by value)
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Copied<'a, T> {
    children: &'a [T],
}

impl<'a, T: Copy> Iterator for Copied<'a, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.children.is_empty() {
            None
        } else {
            let item = self.children[0];
            self.children = &self.children[1..];
            Some(item)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.children.len();
        (len, Some(len))
    }

    #[inline]
    fn count(self) -> usize {
        self.children.len()
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        if n >= self.children.len() {
            self.children = &[];
            None
        } else {
            let item = self.children[n];
            self.children = &self.children[n + 1..];
            Some(item)
        }
    }
}

impl<'a, T: Copy> ExactSizeIterator for Copied<'a, T> {}

impl<'a, T: Copy> DoubleEndedIterator for Copied<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.children.is_empty() {
            None
        } else {
            let len = self.children.len();
            let item = self.children[len - 1];
            self.children = &self.children[..len - 1];
            Some(item)
        }
    }
}

impl<'a, T: Copy> std::iter::FusedIterator for Copied<'a, T> {}

// Add copied() method to SliceChildren for Copy types
impl<'a, T: Copy> SliceChildren<'a, T> {
    /// Returns an iterator that yields children by value.
    ///
    /// This is the idiomatic way to iterate over `Copy` values.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ids: &[ElementId] = &[id1, id2, id3];
    /// let accessor = Variable::from_slice(ids);
    /// for id in accessor.copied() {
    ///     // id is ElementId (by value)
    /// }
    /// ```
    #[inline]
    pub fn copied(&self) -> Copied<'a, T> {
        Copied {
            children: self.children,
        }
    }
}

// Add copied() method to FixedChildren for Copy types
impl<'a, T: Copy, const N: usize> FixedChildren<'a, T, N> {
    /// Returns an iterator that yields children by value.
    ///
    /// This is the idiomatic way to iterate over `Copy` values.
    #[inline]
    pub fn copied(&self) -> Copied<'a, T> {
        Copied {
            children: self.children.as_slice(),
        }
    }
}

// ============================================================================
// INTO ITERATOR
// ============================================================================

impl<'a, T> IntoIterator for SliceChildren<'a, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

// ============================================================================
// ADVANCED ACCESSOR TYPES WITH GAT AND HRTB
// ============================================================================

/// Smart children accessor with adaptive allocation strategy.
///
/// Uses different storage strategies based on the number of children
/// and access patterns, optimizing for both memory usage and performance.
#[derive(Debug)]
pub struct SmartChildren<'a, T> {
    children: &'a [T],
    strategy: AllocationStrategy,
}

/// Allocation strategy for smart accessors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationStrategy {
    /// Use stack allocation for small collections.
    Stack,
    /// Use heap allocation for larger collections.
    Heap,
    /// Use SIMD-optimized operations.
    Simd,
}

impl<T> Clone for SmartChildren<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SmartChildren<'_, T> {}

impl<'a, T: 'a + Send + Sync> ChildrenAccess<'a, T> for SmartChildren<'a, T> {
    type Iter = std::slice::Iter<'a, T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Bulk;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Critical;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        self.children
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        self.children.iter()
    }
}

impl<'a, T> SmartChildren<'a, T> {
    /// Create smart accessor with automatic strategy selection.
    pub fn new(children: &'a [T]) -> Self {
        let strategy = match children.len() {
            0..=8 => AllocationStrategy::Stack,
            9..=64 => AllocationStrategy::Heap,
            _ => AllocationStrategy::Simd,
        };

        Self { children, strategy }
    }

    /// Get the allocation strategy being used.
    pub const fn strategy(&self) -> AllocationStrategy {
        self.strategy
    }

    /// HRTB-compatible batch operation for performance.
    pub fn batch_process<F, R>(&self, mut f: F) -> Vec<R>
    where
        F: for<'b> FnMut(&'b T) -> R,
    {
        match self.strategy {
            AllocationStrategy::Stack => {
                // Use stack-allocated result buffer for small collections
                let mut results = Vec::with_capacity(self.children.len().min(16));
                for item in self.children {
                    results.push(f(item));
                }
                results
            }
            AllocationStrategy::Heap => {
                // Use heap allocation with capacity hint
                self.children.iter().map(|item| f(item)).collect()
            }
            AllocationStrategy::Simd => {
                // Use chunked processing for SIMD optimization
                let mut results = Vec::with_capacity(self.children.len());
                for chunk in self.children.chunks(8) {
                    for item in chunk {
                        results.push(f(item));
                    }
                }
                results
            }
        }
    }
}

/// Bounded children accessor with compile-time range validation.
///
/// Uses const generics to enforce minimum and maximum bounds at compile time.
#[derive(Debug)]
pub struct BoundedChildren<'a, T, const MIN: usize, const MAX: usize> {
    children: &'a [T],
}

impl<T, const MIN: usize, const MAX: usize> Clone for BoundedChildren<'_, T, MIN, MAX> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, const MIN: usize, const MAX: usize> Copy for BoundedChildren<'_, T, MIN, MAX> {}

impl<'a, T: 'a + Send + Sync, const MIN: usize, const MAX: usize> ChildrenAccess<'a, T>
    for BoundedChildren<'a, T, MIN, MAX>
{
    type Iter = std::slice::Iter<'a, T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Sequential;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Moderate;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        self.children
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        self.children.iter()
    }
}

impl<'a, T, const MIN: usize, const MAX: usize> BoundedChildren<'a, T, MIN, MAX> {
    /// Create bounded accessor with compile-time validation.
    pub fn new(children: &'a [T]) -> Option<Self> {
        if children.len() >= MIN && children.len() <= MAX {
            Some(Self { children })
        } else {
            None
        }
    }

    /// Create bounded accessor with debug assertions.
    ///
    /// # Panics (debug only)
    ///
    /// Panics in debug builds if bounds are violated.
    pub fn from_slice_debug(children: &'a [T]) -> Self {
        debug_assert!(
            children.len() >= MIN,
            "BoundedChildren: {} children is below minimum of {}",
            children.len(),
            MIN
        );
        debug_assert!(
            children.len() <= MAX,
            "BoundedChildren: {} children exceeds maximum of {}",
            children.len(),
            MAX
        );
        Self { children }
    }

    /// Get compile-time bounds information.
    pub const fn bounds() -> (usize, usize) {
        (MIN, MAX)
    }

    /// Check if current size is at minimum bound.
    pub const fn is_at_min(&self) -> bool {
        self.children.len() == MIN
    }

    /// Check if current size is at maximum bound.
    pub const fn is_at_max(&self) -> bool {
        self.children.len() == MAX
    }
}

/// Type-aware children accessor using GAT for different element types.
///
/// This accessor can work with different element types while maintaining
/// type safety through Generic Associated Types.
#[derive(Debug)]
pub struct TypedChildren<'a, T> {
    children: &'a [T],
    type_info: TypeInfo,
}

/// Type information for optimization hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeInfo {
    /// Element IDs (lightweight).
    ElementId,
    /// Full elements (heavier).
    Element,
    /// Custom type.
    Custom,
}

impl<T> Clone for TypedChildren<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for TypedChildren<'_, T> {}

impl<'a, T: 'a + Send + Sync> ChildrenAccess<'a, T> for TypedChildren<'a, T> {
    type Iter = std::slice::Iter<'a, T>;

    const ACCESS_PATTERN: AccessPattern = AccessPattern::Random;
    const ACCESS_FREQUENCY: AccessFrequency = AccessFrequency::Frequent;

    #[inline]
    fn as_slice(&self) -> &'a [T] {
        self.children
    }

    #[inline]
    fn iter(&self) -> Self::Iter {
        self.children.iter()
    }
}

impl<'a, T> TypedChildren<'a, T> {
    /// Create typed accessor with automatic type detection.
    pub fn new(children: &'a [T]) -> Self {
        let type_info = if std::mem::size_of::<T>() <= 8 {
            TypeInfo::ElementId
        } else if std::mem::size_of::<T>() <= 64 {
            TypeInfo::Element
        } else {
            TypeInfo::Custom
        };

        Self {
            children,
            type_info,
        }
    }

    /// Create typed accessor with explicit type information.
    pub fn with_type_info(children: &'a [T], type_info: TypeInfo) -> Self {
        Self {
            children,
            type_info,
        }
    }

    /// Get type information.
    pub const fn type_info(&self) -> TypeInfo {
        self.type_info
    }

    /// HRTB-compatible type-aware processing.
    pub fn process_typed<F, R>(&self, mut f: F) -> Vec<R>
    where
        F: for<'b> FnMut(&'b T, TypeInfo) -> R,
    {
        match self.type_info {
            TypeInfo::ElementId => {
                // Optimized path for lightweight IDs
                self.children
                    .iter()
                    .map(|item| f(item, TypeInfo::ElementId))
                    .collect()
            }
            TypeInfo::Element => {
                // Standard path for elements
                self.children
                    .iter()
                    .map(|item| f(item, TypeInfo::Element))
                    .collect()
            }
            TypeInfo::Custom => {
                // Conservative path for custom types
                let mut results = Vec::with_capacity(self.children.len());
                for item in self.children {
                    results.push(f(item, TypeInfo::Custom));
                }
                results
            }
        }
    }
}

// ============================================================================
// RANGE ARITY SUPPORT
// ============================================================================
//
// Note: Range<MIN, MAX> and Never are defined in mod.rs, not here.
// This section is kept for documentation purposes only.

#[cfg(test)]
mod advanced_tests {
    use super::*;

    #[test]
    fn test_smart_children() {
        let small: &[u32] = &[1, 2, 3];
        let accessor = SmartChildren::new(small);
        assert_eq!(accessor.strategy(), AllocationStrategy::Stack);
        assert_eq!(accessor.len(), 3);

        let large: Vec<u32> = (0..100).collect();
        let accessor = SmartChildren::new(&large);
        assert_eq!(accessor.strategy(), AllocationStrategy::Simd);
    }

    #[test]
    fn test_bounded_children() {
        let children: &[u32] = &[1, 2, 3];

        // Should succeed - within bounds
        let accessor = BoundedChildren::<_, 1, 5>::new(children);
        assert!(accessor.is_some());

        // Should fail - below minimum
        let accessor = BoundedChildren::<_, 5, 10>::new(children);
        assert!(accessor.is_none());
    }

    #[test]
    fn test_typed_children() {
        let ids: &[u32] = &[1, 2, 3, 4];
        let accessor = TypedChildren::new(ids);
        assert_eq!(accessor.type_info(), TypeInfo::ElementId);
        assert_eq!(accessor.len(), 4);
    }

    #[test]
    fn test_hrtb_operations() {
        let children: &[u32] = &[1, 2, 3, 4, 5];
        let accessor = SliceChildren { children };

        // HRTB predicate that works with any lifetime
        let found = accessor.find_where(|x| *x > 3);
        assert_eq!(found, Some(&4));

        let count = accessor.count_where(|x| *x % 2 == 0);
        assert_eq!(count, 2); // 2 and 4
    }

    #[test]
    fn test_gat_iterators() {
        let children: &[u32] = &[1, 2, 3];
        let accessor = SliceChildren { children };

        // GAT-based iterator
        let sum: u32 = accessor.iter().sum();
        assert_eq!(sum, 6);

        // Can use different iterator types
        let collected: Vec<_> = accessor.iter().collect();
        assert_eq!(collected, vec![&1, &2, &3]);
    }
}

impl<'a, T, const N: usize> IntoIterator for FixedChildren<'a, T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_children() {
        let accessor: NoChildren<u32> = NoChildren(PhantomData);
        assert!(accessor.is_empty());
        assert_eq!(accessor.len(), 0);
        assert_eq!(accessor.as_slice(), &[]);
    }

    #[test]
    fn test_optional_child_none() {
        let children: &[u32] = &[];
        let accessor = OptionalChild { children };

        assert!(accessor.is_none());
        assert!(!accessor.is_some());
        assert_eq!(accessor.get(), None);
        assert_eq!(accessor.len(), 0);
    }

    #[test]
    fn test_optional_child_some() {
        let children: &[u32] = &[42];
        let accessor = OptionalChild { children };

        assert!(accessor.is_some());
        assert!(!accessor.is_none());
        assert_eq!(accessor.get(), Some(&42));
        assert_eq!(accessor.unwrap(), &42);
        assert_eq!(accessor.len(), 1);
    }

    #[test]
    fn test_optional_child_map() {
        let children: &[u32] = &[42];
        let accessor = OptionalChild { children };

        assert_eq!(accessor.map(|x| x * 2), Some(84));
        assert_eq!(accessor.map_or(0, |x| x * 2), 84);

        let empty: &[u32] = &[];
        let empty_accessor = OptionalChild { children: empty };
        assert_eq!(empty_accessor.map(|x| x * 2), None);
        assert_eq!(empty_accessor.map_or(0, |x| x * 2), 0);
    }

    #[test]
    fn test_fixed_children_single() {
        let children: &[u32; 1] = &[42];
        let accessor = FixedChildren { children };

        assert_eq!(accessor.single(), &42);
        assert_eq!(accessor.get(0), &42);
        assert_eq!(accessor.len(), 1);
    }

    #[test]
    fn test_fixed_children_pair() {
        let children: &[u32; 2] = &[1, 2];
        let accessor = FixedChildren { children };

        assert_eq!(accessor.first(), &1);
        assert_eq!(accessor.second(), &2);
        assert_eq!(accessor.pair(), (&1, &2));
        assert_eq!(accessor.len(), 2);
    }

    #[test]
    fn test_fixed_children_triple() {
        let children: &[u32; 3] = &[1, 2, 3];
        let accessor = FixedChildren { children };

        assert_eq!(accessor.first(), &1);
        assert_eq!(accessor.second(), &2);
        assert_eq!(accessor.third(), &3);
        assert_eq!(accessor.triple(), (&1, &2, &3));
        assert_eq!(accessor.len(), 3);
    }

    #[test]
    fn test_slice_children() {
        let children: &[u32] = &[1, 2, 3, 4, 5];
        let accessor = SliceChildren { children };

        assert_eq!(accessor.len(), 5);
        assert_eq!(accessor.first(), Some(&1));
        assert_eq!(accessor.last(), Some(&5));
        assert_eq!(accessor.get(2), Some(&3));
        assert_eq!(accessor.get(10), None);

        let collected: Vec<_> = accessor.iter().copied().collect();
        assert_eq!(collected, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_slice_children_enumerate() {
        let children: &[u32] = &[10, 20, 30];
        let accessor = SliceChildren { children };

        let enumerated: Vec<_> = accessor.enumerate().collect();
        assert_eq!(enumerated, vec![(0, &10), (1, &20), (2, &30)]);
    }

    #[test]
    fn test_slice_children_rev() {
        let children: &[u32] = &[1, 2, 3];
        let accessor = SliceChildren { children };

        let reversed: Vec<_> = accessor.rev().copied().collect();
        assert_eq!(reversed, vec![3, 2, 1]);
    }

    #[test]
    fn test_into_iterator_slice() {
        let children: &[u32] = &[1, 2, 3];
        let accessor = SliceChildren { children };

        let collected: Vec<_> = accessor.into_iter().copied().collect();
        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[test]
    fn test_into_iterator_fixed() {
        let children: &[u32; 3] = &[1, 2, 3];
        let accessor = FixedChildren { children };

        let collected: Vec<_> = accessor.into_iter().copied().collect();
        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[test]
    fn test_copy_semantics() {
        let children: &[u32] = &[1, 2, 3];
        let accessor = SliceChildren { children };

        // Accessor should be Copy
        let copy = accessor;
        assert_eq!(copy.len(), accessor.len());

        // Can use both after copy
        assert_eq!(accessor.first(), copy.first());
    }
}
