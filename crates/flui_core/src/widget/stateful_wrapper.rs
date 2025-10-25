//! Zero-cost wrapper for StatefulWidget
//!
//! # DEPRECATED
//!
//! This wrapper is no longer necessary with the derive macro approach.
//! Use `#[derive(StatefulWidget)]` instead.
//!
//! This module is kept for backward compatibility but will be removed in a future version.
//!
//! # Migration
//!
//! **Old approach (wrapper):**
//! ```rust,ignore
//! let widget = Stateful(Counter { initial: 0 });
//! ```
//!
//! **New approach (derive macro):**
//! ```rust,ignore
//! #[derive(StatefulWidget, Clone, Debug)]
//! struct Counter { initial: i32 }
//! // No wrapper needed!
//! let widget = Counter { initial: 0 };
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::{StatefulWidget, Stateful, State};
//!
//! #[derive(Clone)]
//! struct Counter {
//!     initial: i32,
//! }
//!
//! struct CounterState {
//!     count: i32,
//! }
//!
//! impl State for CounterState {
//!     type Widget = Counter;
//!     fn init(widget: &Counter) -> Self {
//!         CounterState { count: widget.initial }
//!     }
//! }
//!
//! impl StatefulWidget for Counter {
//!     type State = CounterState;
//!     fn create_state(&self) -> CounterState {
//!         CounterState { count: self.initial }
//!     }
//! }
//!
//! // Wrap in Stateful to get Widget impl
//! let widget = Stateful(Counter { initial: 0 });
//! let element = widget.into_element(); // ✅ Works!
//! ```

use std::ops::{Deref, DerefMut};

use super::{StatefulWidget, Widget, DynWidget};
use crate::element::StatefulElement;

/// Zero-cost transparent wrapper for `StatefulWidget`
///
/// This wrapper enables `StatefulWidget` types to implement the `Widget` trait
/// without conflicting with the blanket impl for `StatelessWidget`.
///
/// # Zero-Cost Abstraction
///
/// Thanks to `#[repr(transparent)]`, this wrapper has **no runtime cost**.
/// The compiler treats `Stateful<W>` identically to `W` in terms of memory
/// layout and ABI.
///
/// # Usage
///
/// Simply wrap your `StatefulWidget` in `Stateful(...)`:
///
/// ```rust,ignore
/// let widget = Stateful(MyStatefulWidget { /* ... */ });
/// let element = widget.into_element();
/// ```
///
/// # Deref Coercion
///
/// `Stateful<W>` implements `Deref<Target = W>`, so you can call methods
/// on the wrapped widget directly:
///
/// ```rust,ignore
/// let widget = Stateful(Counter { count: 0 });
/// println!("Count: {}", widget.count); // ✅ Deref to Counter.count
/// ```
#[repr(transparent)]
#[derive(Debug, Clone)]
pub struct Stateful<W: StatefulWidget>(pub W);

impl<W: StatefulWidget> Stateful<W> {
    /// Create a new `Stateful` wrapper around a `StatefulWidget`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let widget = Stateful::new(Counter { initial: 0 });
    /// ```
    pub fn new(widget: W) -> Self {
        Stateful(widget)
    }

    /// Extract the inner widget from the wrapper
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let wrapper = Stateful(Counter { initial: 0 });
    /// let inner: Counter = wrapper.into_inner();
    /// ```
    pub fn into_inner(self) -> W {
        self.0
    }

    /// Get a reference to the inner widget
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let wrapper = Stateful(Counter { initial: 0 });
    /// let inner: &Counter = wrapper.inner();
    /// ```
    pub fn inner(&self) -> &W {
        &self.0
    }

    /// Get a mutable reference to the inner widget
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut wrapper = Stateful(Counter { initial: 0 });
    /// wrapper.inner_mut().initial = 10;
    /// ```
    pub fn inner_mut(&mut self) -> &mut W {
        &mut self.0
    }
}

// Deref implementations for convenience
impl<W: StatefulWidget> Deref for Stateful<W> {
    type Target = W;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<W: StatefulWidget> DerefMut for Stateful<W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// Widget impl for Stateful<W>
impl<W: StatefulWidget> Widget for Stateful<W> {
    type Element = StatefulElement<W>;

    fn key(&self) -> Option<&str> {
        None
    }

    fn into_element(self) -> StatefulElement<W> {
        StatefulElement::new(self.0)
    }
}

// DynWidget impl for Stateful<W>
impl<W: StatefulWidget> DynWidget for Stateful<W> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// PartialEq/Eq if inner widget supports it
impl<W: StatefulWidget + PartialEq> PartialEq for Stateful<W> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<W: StatefulWidget + Eq> Eq for Stateful<W> {}

// Hash if inner widget supports it
impl<W: StatefulWidget + std::hash::Hash> std::hash::Hash for Stateful<W> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, BoxedWidget};

    #[derive(Debug, Clone, PartialEq)]
    struct TestWidget {
        value: i32,
    }

    struct TestState;

    impl State for TestState {
        type Widget = TestWidget;

        fn init(_widget: &TestWidget) -> Self {
            TestState
        }
    }

    impl StatefulWidget for TestWidget {
        type State = TestState;

        fn create_state(&self) -> TestState {
            TestState
        }
    }

    #[test]
    fn test_stateful_wrapper_creation() {
        let widget = Stateful(TestWidget { value: 42 });
        assert_eq!(widget.value, 42);
    }

    #[test]
    fn test_stateful_new() {
        let widget = Stateful::new(TestWidget { value: 42 });
        assert_eq!(widget.value, 42);
    }

    #[test]
    fn test_stateful_into_inner() {
        let wrapper = Stateful(TestWidget { value: 42 });
        let inner = wrapper.into_inner();
        assert_eq!(inner.value, 42);
    }

    #[test]
    fn test_stateful_inner() {
        let wrapper = Stateful(TestWidget { value: 42 });
        assert_eq!(wrapper.inner().value, 42);
    }

    #[test]
    fn test_stateful_inner_mut() {
        let mut wrapper = Stateful(TestWidget { value: 42 });
        wrapper.inner_mut().value = 100;
        assert_eq!(wrapper.value, 100);
    }

    #[test]
    fn test_stateful_deref() {
        let wrapper = Stateful(TestWidget { value: 42 });
        // Deref allows direct access
        assert_eq!(wrapper.value, 42);
    }

    #[test]
    fn test_stateful_deref_mut() {
        let mut wrapper = Stateful(TestWidget { value: 42 });
        wrapper.value = 100;
        assert_eq!(wrapper.value, 100);
    }

    #[test]
    fn test_stateful_clone() {
        let wrapper = Stateful(TestWidget { value: 42 });
        let cloned = wrapper.clone();
        assert_eq!(wrapper.value, cloned.value);
    }

    #[test]
    fn test_stateful_partial_eq() {
        let w1 = Stateful(TestWidget { value: 42 });
        let w2 = Stateful(TestWidget { value: 42 });
        let w3 = Stateful(TestWidget { value: 100 });

        assert_eq!(w1, w2);
        assert_ne!(w1, w3);
    }

    #[test]
    fn test_stateful_size() {
        use std::mem::size_of;

        // Verify zero-cost: Stateful<W> should be same size as W
        assert_eq!(
            size_of::<Stateful<TestWidget>>(),
            size_of::<TestWidget>(),
            "Stateful wrapper should be zero-cost (same size as inner widget)"
        );
    }
}
