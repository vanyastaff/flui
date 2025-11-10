//! Assertion helpers for testing
//!
//! Provides convenient assertion functions for testing FLUI applications.

use crate::{
    element::{Element, ElementId, ElementTree},
    foundation::Key,
};
use flui_types::Size;

/// Assert that an element exists in the tree
///
/// # Panics
///
/// Panics if the element does not exist.
///
/// # Examples
///
/// ```rust,ignore
/// assert_element_exists(&tree, element_id);
/// ```
pub fn assert_element_exists(tree: &ElementTree, id: ElementId) {
    assert!(
        tree.get(id).is_some(),
        "Expected element {:?} to exist, but it was not found",
        id
    );
}

/// Assert that an element does not exist in the tree
///
/// # Panics
///
/// Panics if the element exists.
///
/// # Examples
///
/// ```rust,ignore
/// assert_element_not_exists(&tree, element_id);
/// ```
pub fn assert_element_not_exists(tree: &ElementTree, id: ElementId) {
    assert!(
        tree.get(id).is_none(),
        "Expected element {:?} to not exist, but it was found",
        id
    );
}

/// Assert that an element is a component
///
/// # Panics
///
/// Panics if the element is not a component.
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_component(&tree, element_id);
/// ```
pub fn assert_is_component(tree: &ElementTree, id: ElementId) {
    let element = tree
        .get(id)
        .unwrap_or_else(|| panic!("Element {:?} not found", id));

    assert!(
        element.as_component().is_some(),
        "Expected element {:?} to be a Component, but it was {:?}",
        id,
        element_type_name(element)
    );
}

/// Assert that an element is a render element
///
/// # Panics
///
/// Panics if the element is not a render element.
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_render(&tree, element_id);
/// ```
pub fn assert_is_render(tree: &ElementTree, id: ElementId) {
    let element = tree
        .get(id)
        .unwrap_or_else(|| panic!("Element {:?} not found", id));

    assert!(
        element.as_render().is_some(),
        "Expected element {:?} to be a Render, but it was {:?}",
        id,
        element_type_name(element)
    );
}

/// Assert that an element is a provider
///
/// # Panics
///
/// Panics if the element is not a provider.
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_provider(&tree, element_id);
/// ```
pub fn assert_is_provider(tree: &ElementTree, id: ElementId) {
    let element = tree
        .get(id)
        .unwrap_or_else(|| panic!("Element {:?} not found", id));

    assert!(
        element.as_provider().is_some(),
        "Expected element {:?} to be a Provider, but it was {:?}",
        id,
        element_type_name(element)
    );
}

/// Assert that an element has a specific size
///
/// # Panics
///
/// Panics if the element doesn't have the expected size.
///
/// # Examples
///
/// ```rust,ignore
/// assert_element_size(&tree, element_id, Size::new(100.0, 50.0));
/// ```
pub fn assert_element_size(tree: &ElementTree, id: ElementId, expected: Size) {
    let element = tree
        .get(id)
        .unwrap_or_else(|| panic!("Element {:?} not found", id));

    let render = element
        .as_render()
        .unwrap_or_else(|| panic!("Element {:?} is not a RenderElement", id));

    let actual = render
        .render_state()
        .read()
        .size()
        .unwrap_or_else(|| panic!("Element {:?} has not been laid out", id));

    assert_eq!(
        actual, expected,
        "Expected element {:?} to have size {:?}, but it has {:?}",
        id, expected, actual
    );
}

/// Assert that a key exists in the tree
///
/// # Panics
///
/// Panics if no element with the key is found.
///
/// # Examples
///
/// ```rust,ignore
/// assert_key_exists(&tree, Key::from_str("submit-button"));
/// ```
pub fn assert_key_exists(tree: &ElementTree, key: Key) {
    assert!(
        tree.find_by_key(key).is_some(),
        "Expected to find element with key {:?}, but it was not found",
        key
    );
}

/// Assert that a key does not exist in the tree
///
/// # Panics
///
/// Panics if an element with the key is found.
///
/// # Examples
///
/// ```rust,ignore
/// assert_key_not_exists(&tree, Key::from_str("removed-button"));
/// ```
pub fn assert_key_not_exists(tree: &ElementTree, key: Key) {
    assert!(
        tree.find_by_key(key).is_none(),
        "Expected not to find element with key {:?}, but it was found",
        key
    );
}

/// Assert that the tree has a specific number of elements
///
/// # Panics
///
/// Panics if the tree doesn't have the expected element count.
///
/// # Examples
///
/// ```rust,ignore
/// assert_element_count(&tree, 5);
/// ```
pub fn assert_element_count(tree: &ElementTree, expected: usize) {
    let actual = tree.len();
    assert_eq!(
        actual, expected,
        "Expected tree to have {} elements, but it has {}",
        expected, actual
    );
}

/// Assert that an element is dirty
///
/// # Panics
///
/// Panics if the element is not dirty.
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_dirty(&tree, element_id);
/// ```
pub fn assert_is_dirty(tree: &ElementTree, id: ElementId) {
    let element = tree
        .get(id)
        .unwrap_or_else(|| panic!("Element {:?} not found", id));

    assert!(
        element.is_dirty(),
        "Expected element {:?} to be dirty, but it is clean",
        id
    );
}

/// Assert that an element is clean (not dirty)
///
/// # Panics
///
/// Panics if the element is dirty.
///
/// # Examples
///
/// ```rust,ignore
/// assert_is_clean(&tree, element_id);
/// ```
pub fn assert_is_clean(tree: &ElementTree, id: ElementId) {
    let element = tree
        .get(id)
        .unwrap_or_else(|| panic!("Element {:?} not found", id));

    assert!(
        !element.is_dirty(),
        "Expected element {:?} to be clean, but it is dirty",
        id
    );
}

/// Get a human-readable element type name
fn element_type_name(element: &Element) -> &'static str {
    match element {
        Element::Component(_) => "Component",
        Element::Render(_) => "Render",
        Element::Provider(_) => "Provider",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_type_name() {
        // This is just a smoke test for the helper function
        // Real tests would require actual elements
    }
}
