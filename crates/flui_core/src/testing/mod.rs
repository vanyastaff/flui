//! Testing utilities
//!
//! This module provides utilities for testing Flui applications and components.
//!
//! # Overview
//!
//! Testing utilities make it easy to:
//! - Create test elements and widgets
//! - Mock render objects
//! - Assert on element tree state
//! - Simulate user interactions
//!
//! # Test Helpers
//!
//! - `TestElement`: Helper for creating test elements
//! - `TestWidget`: Helper for creating test widgets
//! - `MockRenderObject`: Mock render object for testing
//! - Assertions: Custom assertions for element tree
//!
//! # Example
//!
//! ```rust,ignore
//! #[test]
//! fn test_counter_increment() {
//!     let mut tester = TestElement::new();
//!
//!     // Build counter widget
//!     tester.pump(Counter::new());
//!
//!     // Find and tap increment button
//!     tester.tap("increment-button");
//!     tester.pump();
//!
//!     // Assert count updated
//!     assert_text(&tester, "Count: 1");
//! }
//! ```
//!
//! # Implementation Status
//!
//! This module is reserved for future testing utilities.
//! Testing is currently done through integration tests in flui_widgets and examples.

// Note: Testing utilities are currently disabled pending API updates
// pub mod assertions;
// pub mod mock_render;
// pub mod test_harness;
// pub mod view_tester;






