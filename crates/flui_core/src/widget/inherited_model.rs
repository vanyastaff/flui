//! InheritedModel - Aspect-based inherited widgets
//!
//! InheritedModel is a more advanced version of InheritedWidget that allows
//! widgets to depend on specific "aspects" of the data. Only widgets that
//! depend on changed aspects will be rebuilt.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::widget::InheritedModel;
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
//! enum ThemeAspect {
//!     PrimaryColor,
//!     TextStyle,
//! }
//!
//! #[derive(Debug, Clone)]
//! struct AppTheme {
//!     primary_color: Color,
//!     text_style: TextStyle,
//! }
//!
//! impl InheritedModel for AppTheme {
//!     type Aspect = ThemeAspect;
//!
//!     fn update_should_notify(&self, old: &Self) -> bool {
//!         self.primary_color != old.primary_color
//!             || self.text_style != old.text_style
//!     }
//!
//!     fn update_should_notify_dependent(
//!         &self,
//!         old: &Self,
//!         aspects: &[Self::Aspect],
//!     ) -> bool {
//!         for aspect in aspects {
//!             match aspect {
//!                 ThemeAspect::PrimaryColor if self.primary_color != old.primary_color => {
//!                     return true;
//!                 }
//!                 ThemeAspect::TextStyle if self.text_style != old.text_style => {
//!                     return true;
//!                 }
//!                 _ => {}
//!             }
//!         }
//!         false
//!     }
//! }
//! ```

use std::any::Any;
use std::fmt;

use crate::widget::InheritedWidget;

/// InheritedModel - aspect-based inherited widgets
///
/// Extends InheritedWidget with aspect-based dependency tracking.
/// Widgets can depend on specific aspects, and only rebuild when
/// those aspects change.
///
/// # Type Parameters
///
/// - `Aspect`: Type representing different aspects of the data.
///   Must be `Clone + PartialEq + Eq + Hash + Any`.
pub trait InheritedModel: InheritedWidget {
    /// Aspect type for this model
    ///
    /// Usually an enum representing different parts of the data.
    /// Must be `Clone + PartialEq + Eq + std::hash::Hash + Any`.
    type Aspect: Clone + PartialEq + Eq + std::hash::Hash + Any + fmt::Debug + Send + Sync;

    /// Check if dependents should be notified based on aspects
    ///
    /// This is called with the list of aspects that a dependent registered.
    /// Return true if any of those aspects changed.
    ///
    /// # Arguments
    ///
    /// - `old`: Previous widget state
    /// - `aspects`: List of aspects the dependent cares about
    ///
    /// # Returns
    ///
    /// `true` if the dependent should rebuild, `false` otherwise.
    fn update_should_notify_dependent(&self, old: &Self, aspects: &[Self::Aspect]) -> bool;

    /// Inherit from ancestor with specific aspect
    ///
    /// This is a convenience method for accessing InheritedModel with an aspect.
    /// The default implementation finds the ancestor and registers the aspect dependency.
    fn inherit_from_aspect(
        context: &crate::Context,
        aspect: Self::Aspect,
    ) -> Option<Self>
    where
        Self: Clone + 'static,
    {
        // Register dependency with aspect
        let aspect_boxed: Box<dyn Any + Send + Sync> = Box::new(aspect);
        context.depend_on_inherited_widget_of_exact_type_with_aspect::<Self>(Some(aspect_boxed))
    }
}

/// Helper to extract aspect from dependency info
///
/// TODO: Used by InheritedElement when aspect dependencies are implemented
#[allow(dead_code)]
pub(crate) fn extract_aspect<A: Clone + Any>(aspect: &Option<Box<dyn Any + Send + Sync>>) -> Option<A> {
    aspect
        .as_ref()
        .and_then(|boxed| boxed.downcast_ref::<A>())
        .cloned()
}

/// Helper to check if widget should notify dependent based on aspects
///
/// TODO: Used by InheritedElement when aspect dependencies are implemented
#[allow(dead_code)]
pub(crate) fn should_notify_dependent<W: InheritedModel>(
    new_widget: &W,
    old_widget: &W,
    aspects: &[W::Aspect],
) -> bool {
    if aspects.is_empty() {
        // No specific aspects - use default notification
        new_widget.update_should_notify(old_widget)
    } else {
        // Check specific aspects
        new_widget.update_should_notify_dependent(old_widget, aspects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAspect {
        ValueA,
        ValueB,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct TestModel {
        value_a: i32,
        value_b: String,
    }

    // ProxyWidget stub for testing
    impl crate::ProxyWidget for TestModel {
        fn child(&self) -> &dyn crate::AnyWidget {
            panic!("TestModel is a test stub")
        }
    }

    impl InheritedWidget for TestModel {
        fn update_should_notify(&self, old: &Self) -> bool {
            self.value_a != old.value_a || self.value_b != old.value_b
        }
    }

    impl InheritedModel for TestModel {
        type Aspect = TestAspect;

        fn update_should_notify_dependent(&self, old: &Self, aspects: &[Self::Aspect]) -> bool {
            for aspect in aspects {
                match aspect {
                    TestAspect::ValueA if self.value_a != old.value_a => return true,
                    TestAspect::ValueB if self.value_b != old.value_b => return true,
                    _ => {}
                }
            }
            false
        }
    }

    #[test]
    fn test_should_notify_dependent_no_change() {
        let old = TestModel {
            value_a: 1,
            value_b: "test".to_string(),
        };
        let new = old.clone();

        let aspects = vec![TestAspect::ValueA];
        assert!(!should_notify_dependent(&new, &old, &aspects));
    }

    #[test]
    fn test_should_notify_dependent_aspect_a_changed() {
        let old = TestModel {
            value_a: 1,
            value_b: "test".to_string(),
        };
        let new = TestModel {
            value_a: 2,
            value_b: "test".to_string(),
        };

        // Depends on ValueA - should notify
        let aspects = vec![TestAspect::ValueA];
        assert!(should_notify_dependent(&new, &old, &aspects));

        // Depends on ValueB - should NOT notify
        let aspects = vec![TestAspect::ValueB];
        assert!(!should_notify_dependent(&new, &old, &aspects));
    }

    #[test]
    fn test_should_notify_dependent_aspect_b_changed() {
        let old = TestModel {
            value_a: 1,
            value_b: "test".to_string(),
        };
        let new = TestModel {
            value_a: 1,
            value_b: "changed".to_string(),
        };

        // Depends on ValueA - should NOT notify
        let aspects = vec![TestAspect::ValueA];
        assert!(!should_notify_dependent(&new, &old, &aspects));

        // Depends on ValueB - should notify
        let aspects = vec![TestAspect::ValueB];
        assert!(should_notify_dependent(&new, &old, &aspects));
    }

    #[test]
    fn test_should_notify_dependent_multiple_aspects() {
        let old = TestModel {
            value_a: 1,
            value_b: "test".to_string(),
        };
        let new = TestModel {
            value_a: 2,
            value_b: "test".to_string(),
        };

        // Depends on both - should notify because A changed
        let aspects = vec![TestAspect::ValueA, TestAspect::ValueB];
        assert!(should_notify_dependent(&new, &old, &aspects));
    }

    #[test]
    fn test_extract_aspect() {
        let aspect = TestAspect::ValueA;
        let boxed: Option<Box<dyn Any + Send + Sync>> = Some(Box::new(aspect));

        let extracted: Option<TestAspect> = extract_aspect(&boxed);
        assert_eq!(extracted, Some(TestAspect::ValueA));
    }

    #[test]
    fn test_extract_aspect_wrong_type() {
        let boxed: Option<Box<dyn Any + Send + Sync>> = Some(Box::new(42i32));

        let extracted: Option<TestAspect> = extract_aspect(&boxed);
        assert_eq!(extracted, None);
    }
}
