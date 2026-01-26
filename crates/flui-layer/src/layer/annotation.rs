//! Annotation search system for layer trees
//!
//! This module provides the annotation search infrastructure used for
//! finding annotations (like hit test targets, semantics nodes, etc.)
//! in the layer tree at a given position.
//!
//! # Architecture
//!
//! The annotation system mirrors Flutter's `findAnnotations` pattern:
//!
//! 1. **AnnotationEntry<T>** - A single annotation with its local position
//! 2. **AnnotationResult<T>** - A collection of found annotations
//! 3. **FindAnnotations trait** - Implemented by layers to participate in search
//!
//! # Example
//!
//! ```rust
//! use flui_layer::layer::annotation::{AnnotationEntry, AnnotationResult};
//! use flui_types::geometry::Offset;
//!
//! // Create a result collector
//! let mut result = AnnotationResult::<String>::new();
//!
//! // Add annotations found during search
//! result.add(AnnotationEntry::new("button".to_string(), Offset::new(px(10.0), px(20.0))));
//! result.add(AnnotationEntry::new("container".to_string(), Offset::new(px(0.0), px(0.0))));
//!
//! // Access annotations
//! assert_eq!(result.len(), 2);
//! assert_eq!(result.entries()[0].annotation(), &"button".to_string());
//! ```

use flui_types::geometry::{Offset, Pixels};
use std::fmt;

/// Information about a single annotation found in the layer tree.
///
/// This struct contains the annotation object and the position in the
/// annotation's local coordinate space where the search hit.
#[derive(Clone)]
pub struct AnnotationEntry<T> {
    /// The annotation object that was found.
    annotation: T,

    /// The position in the annotation's local coordinate space.
    local_position: Offset<Pixels>,
}

impl<T> AnnotationEntry<T> {
    /// Creates a new annotation entry.
    #[inline]
    pub fn new(annotation: T, local_position: Offset<Pixels>) -> Self {
        Self {
            annotation,
            local_position,
        }
    }

    /// Returns a reference to the annotation.
    #[inline]
    pub fn annotation(&self) -> &T {
        &self.annotation
    }

    /// Returns the annotation, consuming the entry.
    #[inline]
    pub fn into_annotation(self) -> T {
        self.annotation
    }

    /// Returns the local position where the annotation was found.
    #[inline]
    pub fn local_position(&self) -> Offset<Pixels> {
        self.local_position
    }

    /// Maps the annotation to a different type.
    #[inline]
    pub fn map<U, F>(self, f: F) -> AnnotationEntry<U>
    where
        F: FnOnce(T) -> U,
    {
        AnnotationEntry {
            annotation: f(self.annotation),
            local_position: self.local_position,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for AnnotationEntry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnnotationEntry")
            .field("annotation", &self.annotation)
            .field("local_position", &self.local_position)
            .finish()
    }
}

impl<T: PartialEq> PartialEq for AnnotationEntry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.annotation == other.annotation && self.local_position == other.local_position
    }
}

/// A collection of annotations found during a layer tree search.
///
/// Entries are added in order from most specific (leaf) to least specific (root),
/// typically during an upward walk of the tree.
#[derive(Clone)]
pub struct AnnotationResult<T> {
    /// The collected entries.
    entries: Vec<AnnotationEntry<T>>,
}

impl<T> AnnotationResult<T> {
    /// Creates a new empty result.
    #[inline]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Creates a new result with the given capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }

    /// Adds an entry to the result.
    ///
    /// Entries should be added in order from most specific to least specific.
    #[inline]
    pub fn add(&mut self, entry: AnnotationEntry<T>) {
        self.entries.push(entry);
    }

    /// Returns the number of entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns a slice of all entries.
    #[inline]
    pub fn entries(&self) -> &[AnnotationEntry<T>] {
        &self.entries
    }

    /// Returns an iterator over the entries.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &AnnotationEntry<T>> {
        self.entries.iter()
    }

    /// Returns an iterator over the annotations only.
    #[inline]
    pub fn annotations(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|e| &e.annotation)
    }

    /// Returns the first entry, if any.
    #[inline]
    pub fn first(&self) -> Option<&AnnotationEntry<T>> {
        self.entries.first()
    }

    /// Returns the first annotation, if any.
    #[inline]
    pub fn first_annotation(&self) -> Option<&T> {
        self.entries.first().map(|e| &e.annotation)
    }

    /// Clears all entries.
    #[inline]
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Extends this result with entries from another result.
    #[inline]
    pub fn extend(&mut self, other: AnnotationResult<T>) {
        self.entries.extend(other.entries);
    }

    /// Consumes the result and returns the entries.
    #[inline]
    pub fn into_entries(self) -> Vec<AnnotationEntry<T>> {
        self.entries
    }

    /// Consumes the result and returns only the annotations.
    #[inline]
    pub fn into_annotations(self) -> Vec<T> {
        self.entries.into_iter().map(|e| e.annotation).collect()
    }
}

impl<T> Default for AnnotationResult<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: fmt::Debug> fmt::Debug for AnnotationResult<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnnotationResult")
            .field("entries", &self.entries)
            .finish()
    }
}

impl<T> IntoIterator for AnnotationResult<T> {
    type Item = AnnotationEntry<T>;
    type IntoIter = std::vec::IntoIter<AnnotationEntry<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a AnnotationResult<T> {
    type Item = &'a AnnotationEntry<T>;
    type IntoIter = std::slice::Iter<'a, AnnotationEntry<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

/// Options for annotation search behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnnotationSearchOptions {
    /// If true, stop after finding the first annotation.
    pub only_first: bool,
}

impl AnnotationSearchOptions {
    /// Creates options for finding only the first annotation.
    #[inline]
    pub fn first_only() -> Self {
        Self { only_first: true }
    }

    /// Creates options for finding all annotations.
    #[inline]
    pub fn find_all() -> Self {
        Self { only_first: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_annotation_entry_new() {
        let entry = AnnotationEntry::new("test", Offset::new(px(10.0), px(20.0)));
        assert_eq!(entry.annotation(), &"test");
        assert_eq!(entry.local_position(), Offset::new(px(10.0), px(20.0)));
    }

    #[test]
    fn test_annotation_entry_into_annotation() {
        let entry = AnnotationEntry::new(String::from("test"), Offset::ZERO);
        let annotation = entry.into_annotation();
        assert_eq!(annotation, "test");
    }

    #[test]
    fn test_annotation_entry_map() {
        let entry = AnnotationEntry::new(42, Offset::new(px(5.0), px(5.0)));
        let mapped = entry.map(|n| n.to_string());
        assert_eq!(mapped.annotation(), &"42".to_string());
        assert_eq!(mapped.local_position(), Offset::new(px(5.0), px(5.0)));
    }

    #[test]
    fn test_annotation_result_new() {
        let result = AnnotationResult::<i32>::new();
        assert!(result.is_empty());
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_annotation_result_add() {
        let mut result = AnnotationResult::new();
        result.add(AnnotationEntry::new("first", Offset::ZERO));
        result.add(AnnotationEntry::new(
            "second",
            Offset::new(px(10.0), px(10.0)),
        ));

        assert_eq!(result.len(), 2);
        assert_eq!(result.first_annotation(), Some(&"first"));
    }

    #[test]
    fn test_annotation_result_iter() {
        let mut result = AnnotationResult::new();
        result.add(AnnotationEntry::new(1, Offset::ZERO));
        result.add(AnnotationEntry::new(2, Offset::ZERO));
        result.add(AnnotationEntry::new(3, Offset::ZERO));

        let annotations: Vec<_> = result.annotations().copied().collect();
        assert_eq!(annotations, vec![1, 2, 3]);
    }

    #[test]
    fn test_annotation_result_into_annotations() {
        let mut result = AnnotationResult::new();
        result.add(AnnotationEntry::new("a".to_string(), Offset::ZERO));
        result.add(AnnotationEntry::new("b".to_string(), Offset::ZERO));

        let annotations = result.into_annotations();
        assert_eq!(annotations, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn test_annotation_result_clear() {
        let mut result = AnnotationResult::new();
        result.add(AnnotationEntry::new(1, Offset::ZERO));
        result.add(AnnotationEntry::new(2, Offset::ZERO));
        assert_eq!(result.len(), 2);

        result.clear();
        assert!(result.is_empty());
    }

    #[test]
    fn test_annotation_result_extend() {
        let mut result1 = AnnotationResult::new();
        result1.add(AnnotationEntry::new(1, Offset::ZERO));

        let mut result2 = AnnotationResult::new();
        result2.add(AnnotationEntry::new(2, Offset::ZERO));
        result2.add(AnnotationEntry::new(3, Offset::ZERO));

        result1.extend(result2);
        assert_eq!(result1.len(), 3);
    }

    #[test]
    fn test_annotation_search_options() {
        let first_only = AnnotationSearchOptions::first_only();
        assert!(first_only.only_first);

        let find_all = AnnotationSearchOptions::find_all();
        assert!(!find_all.only_first);
    }
}
