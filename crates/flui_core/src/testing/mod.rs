//! Testing infrastructure for widgets (Phase 15)
//!
//! Provides utilities for testing widgets in isolation without a full application.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::testing::WidgetTester;
//!
//! #[test]
//! fn test_my_widget() {
//!     let mut tester = WidgetTester::new();
//!
//!     // Mount widget
//!     tester.pump_widget(Box::new(MyWidget::new()));
//!
//!     // Rebuild after changes
//!     tester.pump();
//!
//!     // Access the tree
//!     let root_id = tester.root_element_id().unwrap();
//!     assert!(tester.tree().read().get(root_id).is_some());
//! }
//! ```

use std::sync::Arc;
use parking_lot::RwLock;

use crate::foundation::Key;
use crate::{AnyWidget, BuildOwner, ElementId, ElementTree};

/// Widget testing harness
///
/// Provides a simple environment for testing widgets without a full application.
/// Manages BuildOwner and provides methods to mount, rebuild, and inspect widgets.
///
/// # Philosophy
///
/// WidgetTester focuses on **widget behavior** not rendering. It:
/// - Mounts widgets and builds the element tree
/// - Triggers rebuilds via `pump()`
/// - Provides access to the tree for assertions
/// - Does NOT render (no egui Painter, no layout/paint)
///
/// # Example
///
/// ```rust,ignore
/// #[test]
/// fn test_stateful_widget() {
///     let mut tester = WidgetTester::new();
///
///     // Mount widget
///     let counter = CounterWidget::new(0);
///     tester.pump_widget(Box::new(counter));
///
///     // Widget is now built
///     assert!(tester.root_element_id().is_some());
///
///     // Trigger setState() somehow, then:
///     tester.pump(); // Rebuilds dirty elements
///
///     // Inspect tree
///     let tree = tester.tree().read();
///     // ... assertions
/// }
/// ```
pub struct WidgetTester {
    /// Build owner managing the widget tree
    owner: BuildOwner,
}

impl WidgetTester {
    /// Create a new widget tester
    pub fn new() -> Self {
        Self {
            owner: BuildOwner::new(),
        }
    }

    /// Mount a widget as the root
    ///
    /// This creates the root element and builds the widget tree.
    /// Any previously mounted widget is replaced.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut tester = WidgetTester::new();
    /// tester.pump_widget(Box::new(MyWidget::new()));
    /// ```
    pub fn pump_widget(&mut self, widget: Box<dyn AnyWidget>) -> ElementId {
        let root_id = self.owner.set_root(widget);

        // Build the tree
        self.owner.build_scope(|owner| {
            owner.flush_build();
        });

        root_id
    }

    /// Rebuild dirty elements
    ///
    /// Triggers a build phase, rebuilding all dirty elements.
    /// Call this after state changes to update the tree.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // State changed somehow
    /// tester.pump(); // Rebuild
    /// ```
    pub fn pump(&mut self) {
        self.owner.build_scope(|owner| {
            owner.flush_build();
        });
    }

    /// Get the root element ID
    pub fn root_element_id(&self) -> Option<ElementId> {
        self.owner.root_element_id()
    }

    /// Get reference to the element tree
    ///
    /// Use this to inspect the tree structure and elements.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tree = tester.tree().read();
    /// if let Some(element) = tree.get(element_id) {
    ///     // Inspect element
    /// }
    /// ```
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        self.owner.tree()
    }

    /// Get the BuildOwner
    ///
    /// Provides access to the underlying BuildOwner for advanced use cases.
    pub fn owner(&self) -> &BuildOwner {
        &self.owner
    }

    /// Get mutable BuildOwner
    pub fn owner_mut(&mut self) -> &mut BuildOwner {
        &mut self.owner
    }

    /// Get count of dirty elements
    pub fn dirty_count(&self) -> usize {
        self.owner.dirty_count()
    }

    /// Check if tree is clean (no dirty elements)
    pub fn is_clean(&self) -> bool {
        self.owner.dirty_count() == 0
    }
}

impl Default for WidgetTester {
    fn default() -> Self {
        Self::new()
    }
}

// Finder utilities for locating widgets/elements

/// Find elements by type
///
/// Returns all element IDs whose widgets match the given type.
///
/// # Example
///
/// ```rust,ignore
/// let text_elements = find_by_type::<Text>(&tester);
/// assert_eq!(text_elements.len(), 3);
/// ```
pub fn find_by_type<W: 'static>(tester: &WidgetTester) -> Vec<ElementId> {
    let tree = tester.tree().read();
    let type_id = std::any::TypeId::of::<W>();
    let mut found = Vec::new();

    tree.visit_all_elements(&mut |element| {
        if element.widget_type_id() == type_id {
            found.push(element.id());
        }
    });

    found
}

/// Find first element by type
///
/// Returns the first element whose widget matches the given type.
///
/// # Example
///
/// ```rust,ignore
/// if let Some(button_id) = find_first_by_type::<Button>(&tester) {
///     // Found button
/// }
/// ```
pub fn find_first_by_type<W: 'static>(tester: &WidgetTester) -> Option<ElementId> {
    let tree = tester.tree().read();
    let type_id = std::any::TypeId::of::<W>();
    let mut found = None;

    tree.visit_all_elements(&mut |element| {
        if found.is_none() && element.widget_type_id() == type_id {
            found = Some(element.id());
        }
    });

    found
}

/// Count elements by type
///
/// Returns the number of elements whose widgets match the given type.
///
/// # Example
///
/// ```rust,ignore
/// let text_count = count_by_type::<Text>(&tester);
/// assert_eq!(text_count, 5);
/// ```
pub fn count_by_type<W: 'static>(tester: &WidgetTester) -> usize {
    find_by_type::<W>(tester).len()
}

/// Find element by key
///
/// Returns the first element whose widget has the given key.
/// Uses the `key()` method from AnyElement to check keys.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::ValueKey;
///
/// // Widget created with key: MyWidget::builder().key("submit-button").build()
/// if let Some(button_id) = find_by_key(&tester, &ValueKey::new("submit-button")) {
///     // Found button with key
/// }
/// ```
pub fn find_by_key(tester: &WidgetTester, key: &dyn Key) -> Option<ElementId> {
    let tree = tester.tree().read();
    let mut found = None;

    tree.visit_all_elements(&mut |element| {
        if found.is_none() {
            if let Some(element_key) = element.key() {
                if element_key.equals(key) {
                    found = Some(element.id());
                }
            }
        }
    });

    found
}

/// Find elements by text content
///
/// Searches for elements whose widgets contain the given text string.
/// This uses the widget's Debug implementation to search for text.
///
/// **Note:** This is a basic implementation that searches Debug output.
/// For production code, widgets should implement a dedicated `text_content()` method.
///
/// # Example
///
/// ```rust,ignore
/// // Find all widgets that contain "Hello"
/// let elements = find_by_text(&tester, "Hello");
/// assert!(!elements.is_empty());
/// ```
pub fn find_by_text(tester: &WidgetTester, text: &str) -> Vec<ElementId> {
    let tree = tester.tree().read();
    let mut found = Vec::new();

    tree.visit_all_elements(&mut |element| {
        // Use Debug output to search for text
        // This is a simple implementation - production code should use a dedicated trait
        let debug_string = format!("{:?}", element);
        if debug_string.contains(text) {
            found.push(element.id());
        }
    });

    found
}

/// Find first element by text content
///
/// Returns the first element whose widget contains the given text string.
///
/// # Example
///
/// ```rust,ignore
/// if let Some(id) = find_first_by_text(&tester, "Submit") {
///     // Found first widget with "Submit" text
/// }
/// ```
pub fn find_first_by_text(tester: &WidgetTester, text: &str) -> Option<ElementId> {
    find_by_text(tester, text).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, StatelessWidget};

    // Simple test widget
    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            Box::new(TestWidget { value: self.value })
        }
    }

    #[test]
    fn test_widget_tester_creation() {
        let tester = WidgetTester::new();
        assert!(tester.root_element_id().is_none());
        assert!(tester.is_clean());
    }

    #[test]
    fn test_pump_widget() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 42 };
        let root_id = tester.pump_widget(Box::new(widget));

        assert_eq!(tester.root_element_id(), Some(root_id));
        assert!(tester.is_clean()); // After build, should be clean
    }

    #[test]
    fn test_pump_rebuilds() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 1 };
        let root_id = tester.pump_widget(Box::new(widget));

        // Mark dirty
        tester.owner_mut().schedule_build_for(root_id, 0);
        assert!(!tester.is_clean());

        // Pump rebuilds
        tester.pump();
        assert!(tester.is_clean());
    }

    #[test]
    fn test_find_by_type() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 42 };
        tester.pump_widget(Box::new(widget));

        let found = find_by_type::<TestWidget>(&tester);
        assert!(!found.is_empty());
    }

    #[test]
    fn test_find_first_by_type() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 42 };
        tester.pump_widget(Box::new(widget));

        let found = find_first_by_type::<TestWidget>(&tester);
        assert!(found.is_some());
    }

    #[test]
    fn test_count_by_type() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 42 };
        tester.pump_widget(Box::new(widget));

        let count = count_by_type::<TestWidget>(&tester);
        assert!(count > 0);
    }

    #[test]
    fn test_tree_access() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 42 };
        let root_id = tester.pump_widget(Box::new(widget));

        let tree = tester.tree().read();
        assert!(tree.get(root_id).is_some());
    }

    #[test]
    fn test_default() {
        let tester = WidgetTester::default();
        assert!(tester.root_element_id().is_none());
    }

    #[test]
    fn test_find_by_text() {
        let mut tester = WidgetTester::new();

        // TestWidget Debug impl contains "value: 42"
        let widget = TestWidget { value: 42 };
        tester.pump_widget(Box::new(widget));

        // Find by text in Debug output
        let found = find_by_text(&tester, "value: 42");
        assert!(!found.is_empty(), "Should find widget with value 42 in Debug output");

        // Not found
        let not_found = find_by_text(&tester, "nonexistent text");
        assert!(not_found.is_empty(), "Should not find nonexistent text");
    }

    #[test]
    fn test_find_first_by_text() {
        let mut tester = WidgetTester::new();

        let widget = TestWidget { value: 123 };
        tester.pump_widget(Box::new(widget));

        let found = find_first_by_text(&tester, "123");
        assert!(found.is_some(), "Should find first widget with text 123");

        let not_found = find_first_by_text(&tester, "999");
        assert!(not_found.is_none(), "Should not find text 999");
    }

    #[test]
    fn test_find_by_key() {
        use crate::foundation::ValueKey;

        // TODO: Add test when widgets support keys
        // For now, just test that find_by_key doesn't crash
        let mut tester = WidgetTester::new();
        let widget = TestWidget { value: 1 };
        tester.pump_widget(Box::new(widget));

        let key = ValueKey::new("test-key");
        let found = find_by_key(&tester, &key);
        // Will be None because TestWidget doesn't have a key
        assert!(found.is_none());
    }
}
