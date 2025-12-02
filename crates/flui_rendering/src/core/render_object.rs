//! Core render object trait.
//!
//! - [`RenderObject`] - Base trait for all render objects (protocol-agnostic)
//! - [`RenderObjectExt`] - Extension trait for downcasting

use std::any::Any;
use std::fmt;

use flui_types::{Rect, Size};

// ============================================================================
// RENDER OBJECT TRAIT
// ============================================================================

/// Base trait for all render objects (protocol-agnostic).
///
/// State is managed externally in `RenderState<P>`, not in the render object itself.
pub trait RenderObject: Send + Sync + fmt::Debug + 'static {
    /// For downcasting to concrete type.
    fn as_any(&self) -> &dyn Any;
    /// For mutable downcasting to concrete type.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Human-readable debug name (defaults to type name).
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Natural size independent of constraints (e.g., image dimensions).
    fn intrinsic_size(&self) -> Option<Size> {
        None
    }

    /// Bounding box in local coordinates.
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    /// Whether this object participates in hit testing.
    fn handles_pointer_events(&self) -> bool {
        false
    }
}

// ============================================================================
// RENDER OBJECT EXTENSION TRAIT
// ============================================================================

/// Extension trait for downcasting `dyn RenderObject`.
pub trait RenderObjectExt {
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T>;
    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T>;
    fn is_type<T: RenderObject>(&self) -> bool;
}

impl RenderObjectExt for dyn RenderObject {
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }

    fn is_type<T: RenderObject>(&self) -> bool {
        self.as_any().is::<T>()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderObject {
        value: i32,
    }

    impl RenderObject for TestRenderObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }

        fn intrinsic_size(&self) -> Option<Size> {
            Some(Size::new(100.0, 50.0))
        }

        fn handles_pointer_events(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_downcast_ref() {
        let obj = TestRenderObject { value: 42 };
        let trait_obj: &dyn RenderObject = &obj;

        // Downcast to correct type
        let downcasted = trait_obj.downcast_ref::<TestRenderObject>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);

        // Downcast to wrong type
        #[derive(Debug)]
        struct OtherType;
        impl RenderObject for OtherType {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        let wrong = trait_obj.downcast_ref::<OtherType>();
        assert!(wrong.is_none());
    }

    #[test]
    fn test_downcast_mut() {
        let mut obj = TestRenderObject { value: 42 };
        let trait_obj: &mut dyn RenderObject = &mut obj;

        // Downcast and mutate
        if let Some(downcasted) = trait_obj.downcast_mut::<TestRenderObject>() {
            downcasted.value = 100;
        }

        assert_eq!(obj.value, 100);
    }

    #[test]
    fn test_is_type() {
        let obj = TestRenderObject { value: 42 };
        let trait_obj: &dyn RenderObject = &obj;

        assert!(trait_obj.is_type::<TestRenderObject>());
    }

    #[test]
    fn test_debug_name() {
        let obj = TestRenderObject { value: 42 };
        assert_eq!(obj.debug_name(), "TestRenderObject");
    }

    #[test]
    fn test_intrinsic_size() {
        let obj = TestRenderObject { value: 42 };
        assert_eq!(obj.intrinsic_size(), Some(Size::new(100.0, 50.0)));
    }

    #[test]
    fn test_local_bounds() {
        let obj = TestRenderObject { value: 42 };
        assert_eq!(obj.local_bounds(), Rect::ZERO);
    }

    #[test]
    fn test_handles_pointer_events() {
        let obj = TestRenderObject { value: 42 };
        assert!(obj.handles_pointer_events());
    }

    #[test]
    fn test_default_implementations() {
        #[derive(Debug)]
        struct MinimalRenderObject;

        impl RenderObject for MinimalRenderObject {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        let obj = MinimalRenderObject;

        // Test default implementations
        assert_eq!(obj.intrinsic_size(), None);
        assert_eq!(obj.local_bounds(), Rect::ZERO);
        assert!(!obj.handles_pointer_events());
        assert!(obj.debug_name().contains("MinimalRenderObject"));
    }

    #[test]
    fn test_trait_object_safety() {
        let obj = TestRenderObject { value: 42 };
        let trait_obj: &dyn RenderObject = &obj;

        // Can call all methods through trait object
        let _ = trait_obj.as_any();
        let _ = trait_obj.debug_name();
        let _ = trait_obj.intrinsic_size();
        let _ = trait_obj.local_bounds();
        let _ = trait_obj.handles_pointer_events();
    }
}
