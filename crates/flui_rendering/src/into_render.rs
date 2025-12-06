//! IntoRender trait - Convert types into RenderObject
//!
//! # Overview
//!
//! The `IntoRender` trait provides a way to convert various render types
//! into boxed `RenderObject` instances for dynamic dispatch and storage
//! in the render tree.
//!
//! # Design
//!
//! This trait is the render-layer counterpart to `IntoElement` and `IntoView`.
//! While `IntoElement` converts to Element nodes and `IntoView` converts to
//! ViewObject trait objects, `IntoRender` converts to RenderObject trait objects.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_rendering::{IntoRender, RenderObject, RenderBox};
//!
//! #[derive(Debug)]
//! struct MyRenderBox { /* ... */ }
//!
//! impl RenderObject for MyRenderBox { /* ... */ }
//! impl RenderBox<Leaf> for MyRenderBox { /* ... */ }
//!
//! // Convert to RenderObject
//! let render_obj: Box<dyn RenderObject> = MyRenderBox {}.into_render();
//! ```

use std::any::Any;

use crate::core::RenderObject;

/// Converts a type into a boxed RenderObject.
///
/// This trait enables automatic conversion of render types into
/// trait objects for dynamic dispatch in the render tree.
///
/// # Implementations
///
/// This trait is implemented for:
/// - `Box<dyn RenderObject>` - Identity conversion
/// - All types implementing `RenderObject` (via blanket impl)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{IntoRender, RenderObject};
///
/// #[derive(Debug)]
/// struct MyRenderBox { size: f32 }
///
/// impl RenderObject for MyRenderBox {
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
/// }
///
/// // Convert to RenderObject
/// let render_obj = MyRenderBox { size: 100.0 }.into_render();
/// ```
pub trait IntoRender: Send + 'static {
    /// Convert this value into a boxed RenderObject.
    fn into_render(self) -> Box<dyn RenderObject>;
}

// ============================================================================
// IMPLEMENTATION FOR BOX<DYN RENDEROBJECT>
// ============================================================================

impl IntoRender for Box<dyn RenderObject> {
    /// Identity conversion - already a boxed RenderObject.
    #[inline]
    fn into_render(self) -> Box<dyn RenderObject> {
        self
    }
}

// ============================================================================
// BLANKET IMPLEMENTATION FOR ALL RENDEROBJECT TYPES
// ============================================================================

impl<R: RenderObject> IntoRender for R {
    /// Convert any RenderObject into boxed trait object.
    #[inline]
    fn into_render(self) -> Box<dyn RenderObject> {
        Box::new(self)
    }
}

// ============================================================================
// RENDER STATE CONVERSION
// ============================================================================

/// Trait for types that can be converted into a boxed render state.
///
/// This is used for type-erased storage of protocol-specific render state
/// in the element tree.
pub trait IntoRenderState: Send + Sync + 'static {
    /// Convert this value into a boxed Any for type-erased storage.
    fn into_render_state(self) -> Box<dyn Any + Send + Sync>;
}

impl<T: Any + Send + Sync + 'static> IntoRenderState for T {
    #[inline]
    fn into_render_state(self) -> Box<dyn Any + Send + Sync> {
        Box::new(self)
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

    impl RenderObject for TestRenderObject {}

    #[test]
    fn test_render_object_into_render() {
        let obj = TestRenderObject { value: 42 };
        let boxed = obj.into_render();

        // Can downcast back to concrete type
        let downcasted = boxed.as_any().downcast_ref::<TestRenderObject>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }

    #[test]
    fn test_boxed_render_object_into_render() {
        let obj = TestRenderObject { value: 42 };
        let boxed: Box<dyn RenderObject> = Box::new(obj);
        let boxed_again = boxed.into_render();

        // Can downcast back to concrete type
        let downcasted = boxed_again.as_any().downcast_ref::<TestRenderObject>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }

    #[test]
    fn test_into_render_state() {
        #[derive(Debug)]
        struct TestState {
            offset: f32,
        }

        let state = TestState { offset: 10.0 };
        let boxed = state.into_render_state();

        // Can downcast back
        let downcasted = boxed.downcast_ref::<TestState>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().offset, 10.0);
    }
}
