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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Children {
    /// No children (leaf nodes)
    ///
    /// Used by render objects that don't have children, such as:
    /// - Text rendering (RenderParagraph)
    /// - Image rendering (RenderImage)
    /// - Placeholder boxes (RenderSizedBox with no child)
    /// - Custom painters (RenderCustomPaint)
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

    /// Create Children::Multi from Vec
    #[inline]
    pub fn from_multi(ids: Vec<ElementId>) -> Self {
        Children::Multi(ids)
    }

    /// Create Children::Multi from slice
    #[inline]
    pub fn from_slice(ids: &[ElementId]) -> Self {
        Children::Multi(ids.to_vec())
    }
}

impl Default for Children {
    fn default() -> Self {
        Children::None
    }
}

impl From<ElementId> for Children {
    fn from(id: ElementId) -> Self {
        Children::Single(id)
    }
}

impl From<Vec<ElementId>> for Children {
    fn from(ids: Vec<ElementId>) -> Self {
        Children::Multi(ids)
    }
}

impl From<&[ElementId]> for Children {
    fn from(ids: &[ElementId]) -> Self {
        Children::Multi(ids.to_vec())
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
}
