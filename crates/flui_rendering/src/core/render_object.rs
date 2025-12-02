//! Core render object trait.

use std::any::Any;
use std::fmt;

use flui_types::{Rect, Size};

/// Base trait for all render objects.
///
/// All implementors must be `Send + Sync + Debug + 'static`.
pub trait RenderObject: Send + Sync + fmt::Debug + 'static {
    /// Returns a reference to this render object as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Returns a human-readable debug name.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns the intrinsic size if this render object has one.
    fn intrinsic_size(&self) -> Option<Size> {
        None
    }

    /// Returns the bounding box in local coordinates.
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    /// Returns whether this render object handles pointer events.
    fn handles_pointer_events(&self) -> bool {
        false
    }
}

/// Extension trait for working with boxed render objects.
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
