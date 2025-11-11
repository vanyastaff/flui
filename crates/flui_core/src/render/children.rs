//! Children enum - unified representation of child count patterns
//!
//! This module provides the `Children` enum which replaces the three-trait
//! system (LeafRender, SingleRender, MultiRender) with a single unified approach.

use crate::element::ElementId;

/// Children enum - unified representation of child count patterns
///
/// Replaces the need for three separate render traits by encoding
/// child count as an enum instead of separate trait methods.
///
/// # Variants
///
/// - `None`: No children (leaf nodes like Text, Image)
/// - `Single`: Exactly one child (wrappers like Padding, Opacity)
/// - `Multi`: Multiple children (layouts like Flex, Stack)
///
/// # Examples
///
/// ```rust,ignore
/// // Leaf render - no children
/// let children = Children::None;
///
/// // Single child render
/// let children = Children::Single(child_id);
///
/// // Multi-child render
/// let children = Children::Multi(vec![child1, child2, child3]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Children {
    /// No children (leaf nodes)
    ///
    /// Used by render objects that don't have children, such as:
    /// - Text rendering (RenderParagraph)
    /// - Image rendering (RenderImage)
    /// - Placeholder boxes (RenderSizedBox with no child)
    /// - Custom painters (RenderCustomPaint)
    #[default]
    None,

    /// Exactly one child
    ///
    /// Used by render objects that wrap a single child, such as:
    /// - Padding (RenderPadding)
    /// - Transform (RenderTransform)
    /// - Opacity (RenderOpacity)
    /// - Clipping (RenderClipRect, RenderClipRRect)
    Single(ElementId),

    /// Multiple children (0 or more)
    ///
    /// Used by render objects that arrange multiple children, such as:
    /// - Flex layouts (RenderFlex - Row/Column)
    /// - Stack layouts (RenderStack)
    /// - Wrap layouts (RenderWrap)
    /// - Grid layouts (RenderGrid)
    Multi(Vec<ElementId>),
}

impl Children {
    /// Check if empty (no children)
    ///
    /// Returns `true` for:
    /// - `Children::None`
    /// - `Children::Multi(vec)` where vec is empty
    ///
    /// Returns `false` for:
    /// - `Children::Single(_)`
    /// - `Children::Multi(vec)` where vec is not empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Children::None => true,
            Children::Single(_) => false,
            Children::Multi(v) => v.is_empty(),
        }
    }

    /// Get child count
    ///
    /// Returns:
    /// - `0` for `Children::None`
    /// - `1` for `Children::Single(_)`
    /// - `n` for `Children::Multi(vec)` where n = vec.len()
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Children::None => 0,
            Children::Single(_) => 1,
            Children::Multi(v) => v.len(),
        }
    }

    /// Get children as slice (for iteration)
    ///
    /// Returns:
    /// - Empty slice for `Children::None`
    /// - Single-element slice for `Children::Single(id)`
    /// - Full slice for `Children::Multi(vec)`
    ///
    /// This is the preferred way to iterate over children in a unified manner.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for &child_id in children.as_slice() {
    ///     ctx.layout_child(child_id, constraints);
    /// }
    /// ```
    #[inline]
    pub fn as_slice(&self) -> &[ElementId] {
        match self {
            Children::None => &[],
            Children::Single(id) => std::slice::from_ref(id),
            Children::Multi(v) => v.as_slice(),
        }
    }

    /// Get single child (panics if not Single variant)
    ///
    /// # Panics
    ///
    /// Panics if called on `Children::None` or `Children::Multi`.
    /// Use `try_single()` for a non-panicking version.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // For Single-child render objects:
    /// let child_id = ctx.children.single();
    /// ctx.layout_child(child_id, constraints);
    /// ```
    #[inline]
    pub fn single(&self) -> ElementId {
        match self {
            Children::Single(id) => *id,
            Children::None => panic!("Expected Children::Single, got Children::None"),
            Children::Multi(v) => panic!(
                "Expected Children::Single, got Children::Multi with {} children",
                v.len()
            ),
        }
    }

    /// Try get single child (returns None if not Single)
    ///
    /// Returns:
    /// - `Some(id)` if this is `Children::Single(id)`
    /// - `None` for `Children::None` or `Children::Multi`
    ///
    /// This is the non-panicking version of `single()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(child_id) = ctx.children.try_single() {
    ///     // Handle single child case
    /// }
    /// ```
    #[inline]
    pub fn try_single(&self) -> Option<ElementId> {
        match self {
            Children::Single(id) => Some(*id),
            _ => None,
        }
    }

    /// Get multi children (returns empty slice if not Multi)
    ///
    /// Returns:
    /// - The child slice for `Children::Multi(vec)`
    /// - Empty slice for `Children::None` or `Children::Single`
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for &child_id in ctx.children.multi() {
    ///     // Process each child
    /// }
    /// ```
    #[inline]
    pub fn multi(&self) -> &[ElementId] {
        match self {
            Children::Multi(v) => v.as_slice(),
            _ => &[],
        }
    }

    /// Get mutable multi children (returns empty slice if not Multi)
    ///
    /// Returns:
    /// - The mutable child slice for `Children::Multi(vec)`
    /// - Empty slice for `Children::None` or `Children::Single`
    #[inline]
    pub fn multi_mut(&mut self) -> &mut [ElementId] {
        match self {
            Children::Multi(v) => v.as_mut_slice(),
            _ => &mut [],
        }
    }

    /// Create Children::None
    #[inline]
    pub const fn none() -> Self {
        Children::None
    }

    /// Create Children::Single from element ID
    #[inline]
    pub const fn from_single(id: ElementId) -> Self {
        Children::Single(id)
    }

    /// Create Children from Vec
    ///
    /// Returns appropriate variant based on vec length:
    /// - Empty vec → Children::None
    /// - Single element → Children::Single
    /// - Multiple elements → Children::Multi
    #[inline]
    pub fn from_multi(ids: Vec<ElementId>) -> Self {
        match ids.len() {
            0 => Children::None,
            1 => Children::Single(ids[0]),
            _ => Children::Multi(ids),
        }
    }

    /// Create Children from slice
    ///
    /// Returns appropriate variant based on slice length:
    /// - Empty slice → Children::None
    /// - Single element → Children::Single
    /// - Multiple elements → Children::Multi
    #[inline]
    pub fn from_slice(ids: &[ElementId]) -> Self {
        match ids.len() {
            0 => Children::None,
            1 => Children::Single(ids[0]),
            _ => Children::Multi(ids.to_vec()),
        }
    }

    /// Check if this is the None variant
    ///
    /// Returns `true` only for `Children::None`.
    ///
    /// Note: `is_empty()` also returns true for empty Multi variant.
    #[inline]
    pub fn is_none(&self) -> bool {
        matches!(self, Children::None)
    }

    /// Check if this is the Single variant
    ///
    /// Returns `true` only for `Children::Single(_)`.
    #[inline]
    pub fn is_single(&self) -> bool {
        matches!(self, Children::Single(_))
    }

    /// Check if this is the Multi variant
    ///
    /// Returns `true` only for `Children::Multi(_)`, regardless of vec length.
    #[inline]
    pub fn is_multi(&self) -> bool {
        matches!(self, Children::Multi(_))
    }

    /// Get first child ID
    ///
    /// Returns:
    /// - `None` for `Children::None`
    /// - `Some(id)` for `Children::Single(id)`
    /// - `Some(ids[0])` for `Children::Multi(ids)` if not empty
    /// - `None` for `Children::Multi(vec![])` (empty vec)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(first) = children.first() {
    ///     ctx.layout_child(first, constraints);
    /// }
    /// ```
    #[inline]
    pub fn first(&self) -> Option<ElementId> {
        match self {
            Children::None => None,
            Children::Single(id) => Some(*id),
            Children::Multi(v) => v.first().copied(),
        }
    }

    /// Get last child ID
    ///
    /// Returns:
    /// - `None` for `Children::None`
    /// - `Some(id)` for `Children::Single(id)`
    /// - `Some(ids[n-1])` for `Children::Multi(ids)` if not empty
    /// - `None` for `Children::Multi(vec![])` (empty vec)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(last) = children.last() {
    ///     // Special handling for last child
    /// }
    /// ```
    #[inline]
    pub fn last(&self) -> Option<ElementId> {
        match self {
            Children::None => None,
            Children::Single(id) => Some(*id),
            Children::Multi(v) => v.last().copied(),
        }
    }

    /// Get child at specific index
    ///
    /// Returns:
    /// - `None` for `Children::None`
    /// - `Some(id)` for `Children::Single(id)` if index == 0
    /// - `Some(ids[index])` for `Children::Multi(ids)` if index < ids.len()
    /// - `None` if index out of bounds
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(child) = children.get(2) {
    ///     // Handle third child
    /// }
    /// ```
    #[inline]
    pub fn get(&self, index: usize) -> Option<ElementId> {
        match self {
            Children::None => None,
            Children::Single(id) if index == 0 => Some(*id),
            Children::Single(_) => None,
            Children::Multi(v) => v.get(index).copied(),
        }
    }

    /// Iterate over children
    ///
    /// Returns an iterator over all child IDs.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for child_id in children.iter() {
    ///     ctx.layout_child(child_id, constraints);
    /// }
    /// ```
    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, ElementId> {
        self.as_slice().iter()
    }

    /// Check if contains specific child ID
    ///
    /// Returns `true` if the given ID is present in the children.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if children.contains(&child_id) {
    ///     // Child is present
    /// }
    /// ```
    #[inline]
    pub fn contains(&self, id: &ElementId) -> bool {
        self.as_slice().contains(id)
    }

    /// Map over children with a function
    ///
    /// Applies a function to each child ID and collects results.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let sizes = children.map(|child_id| {
    ///     ctx.get_child_size(child_id)
    /// });
    /// ```
    #[inline]
    pub fn map<F, T>(&self, f: F) -> Vec<T>
    where
        F: FnMut(&ElementId) -> T,
    {
        self.as_slice().iter().map(f).collect()
    }

    /// Filter children with a predicate
    ///
    /// Returns a vector of child IDs that match the predicate.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let visible_children = children.filter(|child_id| {
    ///     ctx.is_child_visible(*child_id)
    /// });
    /// ```
    #[inline]
    pub fn filter<F>(&self, mut f: F) -> Vec<ElementId>
    where
        F: FnMut(&ElementId) -> bool,
    {
        self.as_slice().iter().filter(|id| f(id)).copied().collect()
    }

    /// Enumerate children with indices
    ///
    /// Returns an iterator of (index, child_id) pairs.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for (i, child_id) in children.enumerate() {
    ///     println!("Child {} has ID {:?}", i, child_id);
    /// }
    /// ```
    #[inline]
    pub fn enumerate(&self) -> impl Iterator<Item = (usize, &ElementId)> {
        self.as_slice().iter().enumerate()
    }

    /// Convert to Vec<ElementId>
    ///
    /// Returns a vector containing all child IDs.
    /// Allocates a new Vec even for Single variant.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let vec = children.to_vec();
    /// ```
    #[inline]
    pub fn to_vec(&self) -> Vec<ElementId> {
        self.as_slice().to_vec()
    }

    /// Take ownership of Multi children (returns empty vec if not Multi)
    ///
    /// Consumes self and returns the Vec<ElementId> if Multi,
    /// otherwise returns an empty vec.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let ids = children.into_vec();
    /// ```
    #[inline]
    pub fn into_vec(self) -> Vec<ElementId> {
        match self {
            Children::Multi(v) => v,
            Children::Single(id) => vec![id],
            Children::None => vec![],
        }
    }

    // ============================================================================
    // Arity-Specific Helper Methods
    // ============================================================================

    /// Get optional single child (for Arity::Optional)
    ///
    /// Returns `Some(id)` if single child present, `None` if no children.
    ///
    /// # Panics
    ///
    /// Panics if called on Multi variant with 2+ children.
    /// Use with `Arity::Optional` only (0 or 1 child).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // For Optional arity (like SizedBox)
    /// impl Render for RenderSizedBox {
    ///     fn arity(&self) -> Arity { Arity::Optional }
    ///
    ///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
    ///         let size = self.compute_size(ctx.constraints);
    ///
    ///         // Safe - Optional guarantees 0 or 1
    ///         if let Some(child) = ctx.children.single_opt() {
    ///             ctx.layout_child(child, BoxConstraints::tight(size));
    ///         }
    ///
    ///         size
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn single_opt(&self) -> Option<ElementId> {
        match self {
            Children::None => None,
            Children::Single(id) => Some(*id),
            Children::Multi(vec) => match vec.len() {
                0 => None,
                1 => Some(vec[0]),
                n => panic!(
                    "single_opt() called on Multi with {} children (expected 0 or 1)",
                    n
                ),
            },
        }
    }

    /// Get first child (for AtLeast arities)
    ///
    /// Returns the first child ID.
    ///
    /// # Panics
    ///
    /// Panics if no children. Use with `Arity::AtLeast(1+)` only.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // For AtLeast(1) arity (like Table with header)
    /// impl Render for RenderTable {
    ///     fn arity(&self) -> Arity { Arity::AtLeast(1) }
    ///
    ///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
    ///         let header = ctx.children.first_child();
    ///         let data_rows = ctx.children.rest();
    ///
    ///         // Layout header
    ///         let header_size = ctx.layout_child(header, ctx.constraints);
    ///
    ///         // Layout data rows
    ///         for &row in data_rows {
    ///             ctx.layout_child(row, ctx.constraints);
    ///         }
    ///
    ///         Size::ZERO
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn first_child(&self) -> ElementId {
        match self {
            Children::None => panic!("first_child() called on empty children (use with AtLeast(1+))"),
            Children::Single(id) => *id,
            Children::Multi(vec) => {
                vec.first().copied()
                    .expect("first_child() called on empty Multi (use with AtLeast(1+))")
            }
        }
    }

    /// Get all children except the first (for AtLeast arities)
    ///
    /// Returns slice of remaining children after the first.
    /// Useful for patterns like "header + data rows" or "first + rest".
    ///
    /// # Panics
    ///
    /// Panics if no children. Use with `Arity::AtLeast(1+)` only.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Table with header + data rows
    /// let header = ctx.children.first_child();
    /// let data_rows = ctx.children.rest();
    ///
    /// // header is ElementId
    /// // data_rows is &[ElementId] (can be empty)
    /// ```
    #[inline]
    pub fn rest(&self) -> &[ElementId] {
        match self {
            Children::None => panic!("rest() called on empty children (use with AtLeast(1+))"),
            Children::Single(_) => &[],
            Children::Multi(vec) => {
                if vec.is_empty() {
                    panic!("rest() called on empty Multi (use with AtLeast(1+))");
                }
                &vec[1..]
            }
        }
    }

    /// Split first child from rest (for AtLeast arities)
    ///
    /// Returns `(first, rest)` tuple where:
    /// - `first` is the first child ID
    /// - `rest` is slice of remaining children (can be empty)
    ///
    /// # Panics
    ///
    /// Panics if no children. Use with `Arity::AtLeast(1+)` only.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl Render for RenderTable {
    ///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
    ///         let (header, data_rows) = ctx.children.split_first_child();
    ///
    ///         // header: ElementId
    ///         // data_rows: &[ElementId]
    ///
    ///         let header_size = ctx.layout_child(header, ctx.constraints);
    ///
    ///         let mut y = header_size.height;
    ///         for &row in data_rows {
    ///             let row_size = ctx.layout_child(row, ctx.constraints);
    ///             y += row_size.height;
    ///         }
    ///
    ///         Size::new(header_size.width, y)
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn split_first_child(&self) -> (ElementId, &[ElementId]) {
        (self.first_child(), self.rest())
    }

    /// Check if has at least n children (for AtLeast/Range validation)
    ///
    /// Returns `true` if child count >= n.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Runtime check for minimum children
    /// if !ctx.children.has_at_least(2) {
    ///     panic!("Comparison needs at least 2 children");
    /// }
    /// ```
    #[inline]
    pub fn has_at_least(&self, n: usize) -> bool {
        self.len() >= n
    }

    /// Check if has at most n children (for Range validation)
    ///
    /// Returns `true` if child count <= n.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Runtime check for maximum children
    /// if !ctx.children.has_at_most(5) {
    ///     panic!("Dialog supports at most 5 children");
    /// }
    /// ```
    #[inline]
    pub fn has_at_most(&self, n: usize) -> bool {
        self.len() <= n
    }

    /// Check if has exactly one child (for Optional arity)
    ///
    /// Returns `true` if exactly one child present.
    /// Useful for Optional arity checks.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if ctx.children.has_single() {
    ///     // SizedBox has child - layout it
    /// } else {
    ///     // SizedBox is empty - just return size
    /// }
    /// ```
    #[inline]
    pub fn has_single(&self) -> bool {
        matches!(self, Children::Single(_))
    }

    /// Check if child count is in range (for Range arity)
    ///
    /// Returns `true` if min <= count <= max.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Check range constraint
    /// if !ctx.children.in_range(3, 5) {
    ///     panic!("Dialog needs 3-5 children");
    /// }
    /// ```
    #[inline]
    pub fn in_range(&self, min: usize, max: usize) -> bool {
        let len = self.len();
        len >= min && len <= max
    }

    /// Get child at specific index, panic if out of bounds
    ///
    /// # Panics
    ///
    /// Panics if index >= len(). Use `get()` for safe access.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // For Range(3, 5) - Dialog with title, content, 1-3 actions
    /// let title = ctx.children.at(0);
    /// let content = ctx.children.at(1);
    /// let actions = ctx.children.slice_from(2);  // 1-3 actions
    /// ```
    #[inline]
    pub fn at(&self, index: usize) -> ElementId {
        self.get(index)
            .unwrap_or_else(|| panic!("Child index {} out of bounds (len: {})", index, self.len()))
    }

    /// Get slice from index to end
    ///
    /// Returns slice starting at index. Panics if index > len.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Dialog: title, content, actions...
    /// let title = ctx.children.at(0);
    /// let content = ctx.children.at(1);
    /// let actions = ctx.children.slice_from(2);  // All remaining
    /// ```
    #[inline]
    pub fn slice_from(&self, index: usize) -> &[ElementId] {
        &self.as_slice()[index..]
    }

    /// Get slice range
    ///
    /// Returns slice [start..end]. Panics if range invalid.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Get middle children
    /// let middle = ctx.children.slice_range(1, 3);  // [1..3]
    /// ```
    #[inline]
    pub fn slice_range(&self, start: usize, end: usize) -> &[ElementId] {
        &self.as_slice()[start..end]
    }
}

// Default is now derived with #[default] annotation above

impl From<ElementId> for Children {
    fn from(id: ElementId) -> Self {
        Children::Single(id)
    }
}

impl From<Vec<ElementId>> for Children {
    fn from(ids: Vec<ElementId>) -> Self {
        Self::from_multi(ids)
    }
}

impl From<&[ElementId]> for Children {
    fn from(ids: &[ElementId]) -> Self {
        Self::from_slice(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none() {
        let children = Children::None;
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
        assert_eq!(children.as_slice(), &[]);
    }

    #[test]
    fn test_single() {
        let id = ElementId::new(1);
        let children = Children::Single(id);
        assert!(!children.is_empty());
        assert_eq!(children.len(), 1);
        assert_eq!(children.as_slice(), &[id]);
        assert_eq!(children.single(), id);
        assert_eq!(children.try_single(), Some(id));
    }

    #[test]
    fn test_multi_empty() {
        let children = Children::Multi(vec![]);
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
        assert_eq!(children.as_slice(), &[]);
    }

    #[test]
    fn test_multi() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let children = Children::Multi(ids.clone());
        assert!(!children.is_empty());
        assert_eq!(children.len(), 3);
        assert_eq!(children.as_slice(), ids.as_slice());
        assert_eq!(children.multi(), ids.as_slice());
    }

    #[test]
    #[should_panic(expected = "Expected Children::Single, got Children::None")]
    fn test_single_panic_none() {
        let children = Children::None;
        children.single();
    }

    #[test]
    #[should_panic(expected = "Expected Children::Single, got Children::Multi")]
    fn test_single_panic_multi() {
        let children = Children::Multi(vec![ElementId::new(1), ElementId::new(2)]);
        children.single();
    }

    #[test]
    fn test_default() {
        let children = Children::default();
        assert!(matches!(children, Children::None));
    }

    #[test]
    fn test_from_element_id() {
        let id = ElementId::new(42);
        let children: Children = id.into();
        assert_eq!(children.single(), id);
    }

    #[test]
    fn test_from_vec() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let children: Children = ids.clone().into();
        assert_eq!(children.multi(), ids.as_slice());
    }

    #[test]
    fn test_from_slice() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let children: Children = ids.as_slice().into();
        assert_eq!(children.multi(), ids.as_slice());
    }

    #[test]
    fn test_is_variants() {
        let none = Children::None;
        assert!(none.is_none());
        assert!(!none.is_single());
        assert!(!none.is_multi());

        let single = Children::Single(ElementId::new(1));
        assert!(!single.is_none());
        assert!(single.is_single());
        assert!(!single.is_multi());

        let multi = Children::Multi(vec![ElementId::new(1), ElementId::new(2)]);
        assert!(!multi.is_none());
        assert!(!multi.is_single());
        assert!(multi.is_multi());
    }

    #[test]
    fn test_first() {
        let none = Children::None;
        assert_eq!(none.first(), None);

        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.first(), Some(id));

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.first(), Some(ids[0]));

        let empty_multi = Children::Multi(vec![]);
        assert_eq!(empty_multi.first(), None);
    }

    #[test]
    fn test_last() {
        let none = Children::None;
        assert_eq!(none.last(), None);

        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.last(), Some(id));

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.last(), Some(ids[2]));

        let empty_multi = Children::Multi(vec![]);
        assert_eq!(empty_multi.last(), None);
    }

    #[test]
    fn test_get() {
        let none = Children::None;
        assert_eq!(none.get(0), None);
        assert_eq!(none.get(1), None);

        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.get(0), Some(id));
        assert_eq!(single.get(1), None);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.get(0), Some(ids[0]));
        assert_eq!(multi.get(1), Some(ids[1]));
        assert_eq!(multi.get(2), Some(ids[2]));
        assert_eq!(multi.get(3), None);
    }

    #[test]
    fn test_iter() {
        let none = Children::None;
        assert_eq!(none.iter().count(), 0);

        let id = ElementId::new(42);
        let single = Children::Single(id);
        let collected: Vec<_> = single.iter().copied().collect();
        assert_eq!(collected, vec![id]);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        let collected: Vec<_> = multi.iter().copied().collect();
        assert_eq!(collected, ids);
    }

    #[test]
    fn test_contains() {
        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);
        let id3 = ElementId::new(3);

        let none = Children::None;
        assert!(!none.contains(&id1));

        let single = Children::Single(id1);
        assert!(single.contains(&id1));
        assert!(!single.contains(&id2));

        let multi = Children::Multi(vec![id1, id2]);
        assert!(multi.contains(&id1));
        assert!(multi.contains(&id2));
        assert!(!multi.contains(&id3));
    }

    #[test]
    fn test_map() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());

        let doubled: Vec<_> = multi.map(|id| id.get() * 2);
        assert_eq!(doubled, vec![2, 4, 6]);
    }

    #[test]
    fn test_filter() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3), ElementId::new(4)];
        let multi = Children::Multi(ids.clone());

        let even: Vec<_> = multi.filter(|&id| id.get() % 2 == 0);
        assert_eq!(even, vec![ElementId::new(2), ElementId::new(4)]);
    }

    #[test]
    fn test_enumerate() {
        let ids = vec![ElementId::new(10), ElementId::new(20), ElementId::new(30)];
        let multi = Children::Multi(ids.clone());

        let enumerated: Vec<_> = multi.enumerate().collect();
        assert_eq!(enumerated.len(), 3);
        assert_eq!(enumerated[0], (0, &ids[0]));
        assert_eq!(enumerated[1], (1, &ids[1]));
        assert_eq!(enumerated[2], (2, &ids[2]));
    }

    #[test]
    fn test_to_vec() {
        let none = Children::None;
        assert_eq!(none.to_vec(), Vec::<ElementId>::new());

        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.to_vec(), vec![id]);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.to_vec(), ids);
    }

    #[test]
    fn test_into_vec() {
        let none = Children::None;
        assert_eq!(none.into_vec(), Vec::<ElementId>::new());

        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.into_vec(), vec![id]);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.into_vec(), ids);
    }

    // Arity-specific helper method tests

    #[test]
    fn test_single_opt() {
        let none = Children::None;
        assert_eq!(none.single_opt(), None);

        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.single_opt(), Some(id));

        let empty_multi = Children::Multi(vec![]);
        assert_eq!(empty_multi.single_opt(), None);

        let one_multi = Children::Multi(vec![id]);
        assert_eq!(one_multi.single_opt(), Some(id));
    }

    #[test]
    #[should_panic(expected = "single_opt() called on Multi with 2 children")]
    fn test_single_opt_panic_multi() {
        let multi = Children::Multi(vec![ElementId::new(1), ElementId::new(2)]);
        multi.single_opt();
    }

    #[test]
    fn test_first_child() {
        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.first_child(), id);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.first_child(), ids[0]);
    }

    #[test]
    #[should_panic(expected = "first_child() called on empty children")]
    fn test_first_child_panic_none() {
        let none = Children::None;
        none.first_child();
    }

    #[test]
    #[should_panic(expected = "first_child() called on empty Multi")]
    fn test_first_child_panic_empty_multi() {
        let empty = Children::Multi(vec![]);
        empty.first_child();
    }

    #[test]
    fn test_rest() {
        let id = ElementId::new(42);
        let single = Children::Single(id);
        assert_eq!(single.rest(), &[]);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.rest(), &ids[1..]);
    }

    #[test]
    #[should_panic(expected = "rest() called on empty children")]
    fn test_rest_panic_none() {
        let none = Children::None;
        none.rest();
    }

    #[test]
    fn test_split_first_child() {
        let id = ElementId::new(42);
        let single = Children::Single(id);
        let (first, rest) = single.split_first_child();
        assert_eq!(first, id);
        assert_eq!(rest, &[]);

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        let (first, rest) = multi.split_first_child();
        assert_eq!(first, ids[0]);
        assert_eq!(rest, &ids[1..]);
    }

    #[test]
    fn test_has_at_least() {
        let none = Children::None;
        assert!(none.has_at_least(0));
        assert!(!none.has_at_least(1));

        let single = Children::Single(ElementId::new(1));
        assert!(single.has_at_least(0));
        assert!(single.has_at_least(1));
        assert!(!single.has_at_least(2));

        let multi = Children::Multi(vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)]);
        assert!(multi.has_at_least(0));
        assert!(multi.has_at_least(1));
        assert!(multi.has_at_least(3));
        assert!(!multi.has_at_least(4));
    }

    #[test]
    fn test_has_at_most() {
        let none = Children::None;
        assert!(none.has_at_most(0));
        assert!(none.has_at_most(1));

        let single = Children::Single(ElementId::new(1));
        assert!(!single.has_at_most(0));
        assert!(single.has_at_most(1));
        assert!(single.has_at_most(2));

        let multi = Children::Multi(vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)]);
        assert!(!multi.has_at_most(2));
        assert!(multi.has_at_most(3));
        assert!(multi.has_at_most(4));
    }

    #[test]
    fn test_has_single() {
        let none = Children::None;
        assert!(!none.has_single());

        let single = Children::Single(ElementId::new(1));
        assert!(single.has_single());

        let multi = Children::Multi(vec![ElementId::new(1)]);
        assert!(!multi.has_single());
    }

    #[test]
    fn test_in_range() {
        let none = Children::None;
        assert!(none.in_range(0, 2));
        assert!(!none.in_range(1, 2));

        let single = Children::Single(ElementId::new(1));
        assert!(single.in_range(0, 2));
        assert!(single.in_range(1, 1));
        assert!(!single.in_range(2, 3));

        let multi = Children::Multi(vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)]);
        assert!(multi.in_range(1, 5));
        assert!(multi.in_range(3, 3));
        assert!(!multi.in_range(4, 5));
        assert!(!multi.in_range(0, 2));
    }

    #[test]
    fn test_at() {
        let single = Children::Single(ElementId::new(42));
        assert_eq!(single.at(0), ElementId::new(42));

        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());
        assert_eq!(multi.at(0), ids[0]);
        assert_eq!(multi.at(1), ids[1]);
        assert_eq!(multi.at(2), ids[2]);
    }

    #[test]
    #[should_panic(expected = "Child index 1 out of bounds")]
    fn test_at_panic() {
        let single = Children::Single(ElementId::new(42));
        single.at(1);
    }

    #[test]
    fn test_slice_from() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let multi = Children::Multi(ids.clone());

        assert_eq!(multi.slice_from(0), &ids[..]);
        assert_eq!(multi.slice_from(1), &ids[1..]);
        assert_eq!(multi.slice_from(2), &ids[2..]);
        assert_eq!(multi.slice_from(3), &[]);
    }

    #[test]
    fn test_slice_range() {
        let ids = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3), ElementId::new(4)];
        let multi = Children::Multi(ids.clone());

        assert_eq!(multi.slice_range(0, 2), &ids[0..2]);
        assert_eq!(multi.slice_range(1, 3), &ids[1..3]);
        assert_eq!(multi.slice_range(2, 4), &ids[2..4]);
    }
}
