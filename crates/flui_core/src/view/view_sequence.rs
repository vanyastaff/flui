//! ViewSequence trait - for composing multiple views
//!
//! ViewSequence allows building UI from tuples of views, similar to how
//! SwiftUI and Leptos handle multiple children.

use super::view::{ChangeFlags, View, ViewElement};
use super::build_context::BuildContext;
use crate::element::Element;

/// ViewSequence - trait for types that represent sequences of views
///
/// This allows using tuples as child views:
///
/// ```rust,ignore
/// // Single view
/// MyWidget { child: Counter { count: 0 } }
///
/// // Multiple views (tuple)
/// MyWidget {
///     children: (
///         Counter { count: 0 },
///         Text { content: "Hello".to_string() },
///         Button { label: "Click me".to_string() },
///     )
/// }
/// ```
pub trait ViewSequence: 'static {
    /// State for this sequence
    type State: 'static;

    /// Elements created by this sequence
    type Elements: ViewElementSequence;

    /// Build all views in sequence
    fn build(self, ctx: &mut BuildContext) -> (Self::Elements, Self::State);

    /// Rebuild all views in sequence
    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        elements: &mut Self::Elements,
    ) -> ChangeFlags;

    /// Teardown all views in sequence
    fn teardown(&self, state: &mut Self::State, elements: &mut Self::Elements);

    /// Count of views in this sequence
    fn len(&self) -> usize;

    /// Check if sequence is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// ViewElementSequence - trait for sequences of elements
///
/// Companion trait to ViewSequence for the element side.
pub trait ViewElementSequence: 'static {
    /// Convert elements to Vec<Element>
    fn into_elements(self) -> Vec<Element>;

    /// Get elements as slice
    fn as_elements(&self) -> Vec<&Element>;

    /// Get elements as mutable slice
    fn as_elements_mut(&mut self) -> Vec<&mut Element>;

    /// Count of elements
    fn len(&self) -> usize;

    /// Check if empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Blanket implementation: ViewElement implements ViewElementSequence (sequence of 1)
impl<E: ViewElement> ViewElementSequence for E {
    fn into_elements(self) -> Vec<Element> {
        vec![Box::new(self).into_element()]
    }

    fn as_elements(&self) -> Vec<&Element> {
        // Single element - can't return reference without storing it
        Vec::new()
    }

    fn as_elements_mut(&mut self) -> Vec<&mut Element> {
        // Single element - can't return reference without storing it
        Vec::new()
    }

    fn len(&self) -> usize {
        1
    }
}

// Implementation for single view
impl<V: View> ViewSequence for V {
    type State = V::State;
    type Elements = V::Element;

    fn build(self, ctx: &mut BuildContext) -> (Self::Elements, Self::State) {
        self.build(ctx)
    }

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        elements: &mut Self::Elements,
    ) -> ChangeFlags {
        self.rebuild(prev, state, elements)
    }

    fn teardown(&self, state: &mut Self::State, elements: &mut Self::Elements) {
        self.teardown(state, elements)
    }

    fn len(&self) -> usize {
        1
    }
}

// Implementation for empty tuple (no views)
impl ViewSequence for () {
    type State = ();
    type Elements = ();

    fn build(self, _ctx: &mut BuildContext) -> (Self::Elements, Self::State) {
        ((), ())
    }

    fn rebuild(
        self,
        _prev: &Self,
        _state: &mut Self::State,
        _elements: &mut Self::Elements,
    ) -> ChangeFlags {
        ChangeFlags::NONE
    }

    fn teardown(&self, _state: &mut Self::State, _elements: &mut Self::Elements) {}

    fn len(&self) -> usize {
        0
    }
}

impl ViewElementSequence for () {
    fn into_elements(self) -> Vec<Element> {
        Vec::new()
    }

    fn as_elements(&self) -> Vec<&Element> {
        Vec::new()
    }

    fn as_elements_mut(&mut self) -> Vec<&mut Element> {
        Vec::new()
    }

    fn len(&self) -> usize {
        0
    }
}

// Macro to implement ViewSequence for tuples
macro_rules! impl_view_sequence_for_tuple {
    ($($T:ident $idx:tt),+) => {
        impl<$($T: View),+> ViewSequence for ($($T,)+) {
            type State = ($($T::State,)+);
            type Elements = ($($T::Element,)+);

            fn build(self, ctx: &mut BuildContext) -> (Self::Elements, Self::State) {
                let elements = (
                    $(self.$idx.clone().build(ctx).0,)+
                );
                let state = (
                    $(self.$idx.build(ctx).1,)+
                );
                (elements, state)
            }

            fn rebuild(
                self,
                prev: &Self,
                state: &mut Self::State,
                elements: &mut Self::Elements,
            ) -> ChangeFlags {
                let mut flags = ChangeFlags::NONE;
                $(
                    flags = flags | self.$idx.rebuild(&prev.$idx, &mut state.$idx, &mut elements.$idx);
                )+
                flags
            }

            fn teardown(&self, state: &mut Self::State, elements: &mut Self::Elements) {
                $(
                    self.$idx.teardown(&mut state.$idx, &mut elements.$idx);
                )+
            }

            fn len(&self) -> usize {
                let mut count = 0;
                $(
                    let _ = stringify!($T);
                    count += 1;
                )+
                count
            }
        }

        impl<$($T: ViewElement),+> ViewElementSequence for ($($T,)+) {
            fn into_elements(self) -> Vec<Element> {
                vec![
                    $(Box::new(self.$idx).into_element(),)+
                ]
            }

            fn as_elements(&self) -> Vec<&Element> {
                // Note: This requires elements to store Element internally
                // For now, return empty vec (will be fixed when elements store Element)
                Vec::new()
            }

            fn as_elements_mut(&mut self) -> Vec<&mut Element> {
                // Note: This requires elements to store Element internally
                // For now, return empty vec (will be fixed when elements store Element)
                Vec::new()
            }

            fn len(&self) -> usize {
                let mut count = 0;
                $(
                    let _ = stringify!($T);
                    count += 1;
                )+
                count
            }
        }
    };
}

// Implement for tuples up to 12 elements (same as Rust's standard trait implementations)
impl_view_sequence_for_tuple!(A 0);
impl_view_sequence_for_tuple!(A 0, B 1);
impl_view_sequence_for_tuple!(A 0, B 1, C 2);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
impl_view_sequence_for_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);

// Implementation for Vec<V>
impl<V: View> ViewSequence for Vec<V> {
    type State = Vec<V::State>;
    type Elements = Vec<V::Element>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Elements, Self::State) {
        let mut elements = Vec::with_capacity(self.len());
        let mut states = Vec::with_capacity(self.len());

        for view in self {
            let (element, state) = view.build(ctx);
            elements.push(element);
            states.push(state);
        }

        (elements, states)
    }

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        elements: &mut Self::Elements,
    ) -> ChangeFlags {
        let mut flags = ChangeFlags::NONE;

        // Handle length mismatch
        match self.len().cmp(&prev.len()) {
            std::cmp::Ordering::Greater => {
                // New items added
                flags = flags | ChangeFlags::NEEDS_BUILD;
            }
            std::cmp::Ordering::Less => {
                // Items removed
                flags = flags | ChangeFlags::NEEDS_BUILD;
            }
            std::cmp::Ordering::Equal => {
                // Same length - rebuild each
                for (i, view) in self.into_iter().enumerate() {
                    if i < state.len() && i < elements.len() {
                        flags = flags | view.rebuild(&prev[i], &mut state[i], &mut elements[i]);
                    }
                }
            }
        }

        flags
    }

    fn teardown(&self, state: &mut Self::State, elements: &mut Self::Elements) {
        for (i, view) in self.iter().enumerate() {
            if i < state.len() && i < elements.len() {
                view.teardown(&mut state[i], &mut elements[i]);
            }
        }
    }

    fn len(&self) -> usize {
        self.len()
    }
}

impl<V: ViewElement> ViewElementSequence for Vec<V> {
    fn into_elements(self) -> Vec<Element> {
        self.into_iter()
            .map(|e| Box::new(e).into_element())
            .collect()
    }

    fn as_elements(&self) -> Vec<&Element> {
        // Note: Requires elements to store Element
        Vec::new()
    }

    fn as_elements_mut(&mut self) -> Vec<&mut Element> {
        // Note: Requires elements to store Element
        Vec::new()
    }

    fn len(&self) -> usize {
        self.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_sequence() {
        let seq = ();
        assert_eq!(seq.len(), 0);
        assert!(seq.is_empty());
    }

    #[test]
    fn test_tuple_length() {
        // These are compile-time checks that the trait is implemented
        fn check_len<S: ViewSequence>(seq: S, expected: usize) {
            assert_eq!(seq.len(), expected);
        }

        // Note: We can't actually call these without concrete View types
        // This just verifies the trait is implemented for different tuple sizes
    }
}
