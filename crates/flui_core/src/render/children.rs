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
}
