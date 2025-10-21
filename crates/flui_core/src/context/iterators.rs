//! Iterator patterns for traversing the element tree

use crate::ElementId;
use crate::tree::ElementTree;

/// Iterator over ancestor elements
///
/// Iterates from parent to root.
///
/// # Examples
///
/// ```rust,ignore
/// let depth = context.ancestors().count();
/// let container = context.ancestors().find(|&id| /* check */);
/// ```
#[derive(Debug)]
pub struct Ancestors<'a> {
    pub(super) tree: parking_lot::RwLockReadGuard<'a, ElementTree>,
    pub(super) current: Option<ElementId>,
}

impl<'a> Ancestors<'a> {
    /// Returns the remaining elements to iterate
    #[must_use]
    pub fn remaining(&self) -> Option<ElementId> {
        self.current
    }
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = ElementId;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let current_id = self.current?;

        if let Some(element) = self.tree.get(current_id) {
            let parent_id = element.parent();
            self.current = parent_id;
            Some(current_id)
        } else {
            self.current = None;
            None
        }
    }
}

/// Iterator over child elements
///
/// Iterates over direct children of an element.
/// Collects children into a Vec to avoid lifetime issues.
///
/// # Examples
///
/// ```rust,ignore
/// let child_count = context.children().count();
/// for child_id in context.children() {
///     println!("Child: {:?}", child_id);
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Children {
    pub(super) children: Vec<ElementId>,
    pub(super) index: usize,
}

impl Children {
    /// Returns the total number of children (including already iterated)
    #[must_use]
    #[inline]
    pub fn total(&self) -> usize {
        self.children.len()
    }
}

impl Iterator for Children {
    type Item = ElementId;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.children.len() {
            let id = self.children[self.index];
            self.index += 1;
            Some(id)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.children.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Children {
    #[inline]
    fn len(&self) -> usize {
        self.children.len() - self.index
    }
}

impl DoubleEndedIterator for Children {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index < self.children.len() {
            self.children.pop()
        } else {
            None
        }
    }
}

/// Iterator over descendant elements (depth-first)
///
/// Iterates over all descendants in depth-first order.
///
/// # Examples
///
/// ```rust,ignore
/// // Count all descendants
/// let total = context.descendants().count();
///
/// // Find first dirty descendant
/// let dirty = context.descendants()
///     .find(|&id| {
///         context.tree().get(id)
///             .map(|e| e.is_dirty())
///             .unwrap_or(false)
///     });
/// ```
#[derive(Debug)]
pub struct Descendants<'a> {
    pub(super) tree: parking_lot::RwLockReadGuard<'a, ElementTree>,
    pub(super) stack: Vec<ElementId>,
}

impl<'a> Descendants<'a> {
    /// Returns the number of elements remaining in the stack
    #[must_use]
    #[inline]
    pub fn stack_size(&self) -> usize {
        self.stack.len()
    }
}

impl<'a> Iterator for Descendants<'a> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        let current_id = self.stack.pop()?;

        if let Some(element) = self.tree.get(current_id) {
            let children: Vec<_> = element.children_iter().collect();
            for child_id in children.into_iter().rev() {
                self.stack.push(child_id);
            }
        }

        Some(current_id)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let min = self.stack.len();
        (min, None) // Can't know max without traversing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_children_iterator_debug() {
        let children = Children {
            children: vec![ElementId::new(), ElementId::new()],
            index: 0,
        };
        let debug = format!("{:?}", children);
        assert!(debug.contains("Children"));
    }

    #[test]
    fn test_children_clone() {
        let children = Children {
            children: vec![ElementId::new()],
            index: 0,
        };
        let cloned = children.clone();
        assert_eq!(cloned.len(), children.len());
    }

    #[test]
    fn test_children_exact_size() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        let mut children = Children {
            children: vec![id1, id2],
            index: 0,
        };

        assert_eq!(children.len(), 2);
        assert_eq!(children.total(), 2);

        children.next();
        assert_eq!(children.len(), 1);
        assert_eq!(children.total(), 2); // Total doesn't change
    }

    #[test]
    fn test_children_double_ended() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        let mut children = Children {
            children: vec![id1, id2, id3],
            index: 0,
        };

        let first = children.next().unwrap();
        assert_eq!(first, id1);

        let last = children.next_back().unwrap();
        assert_eq!(last, id3);

        assert_eq!(children.len(), 1);
    }
}