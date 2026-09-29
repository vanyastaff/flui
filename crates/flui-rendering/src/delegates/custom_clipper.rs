//! Custom clipper delegate for custom clipping shapes.
//!
//! [`CustomClipper`] allows users to define owner-local custom clipping
//! shapes. Current render objects do not store these delegates directly;
//! path-based render effects use owner-lane targets instead.

use std::{any::Any, fmt::Debug};

use flui_foundation::geometry::{Point, Rect, Size};

/// A delegate that defines a custom clipping shape.
///
/// The type parameter `T` represents the clip shape type:
/// - `Rect` for rectangular clips
/// - `RRect` for rounded rectangle clips
/// - `Path` for arbitrary path clips
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::CustomClipper;
/// use flui_foundation::geometry::{Rect, Size};
///
/// #[derive(Debug)]
/// struct InsetClipper {
///     inset: f64,
/// }
///
/// impl CustomClipper<Rect> for InsetClipper {
///     fn get_clip(&self, size: Size) -> Rect {
///         Rect::from_ltrb(
///             self.inset,
///             self.inset,
///             size.width - self.inset,
///             size.height - self.inset,
///         )
///     }
///
///     fn should_reclip(&self, old_clipper: &dyn CustomClipper<Rect>) -> bool {
///         if let Some(old) = old_clipper.as_any().downcast_ref::<Self>() {
///             self.inset != old.inset
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait CustomClipper<T: Clone>: Debug {
    /// Get the clip shape for the given size.
    ///
    /// Called whenever the size changes or when `should_reclip` returns true.
    ///
    /// # Arguments
    ///
    /// * `size` - The size of the render object being clipped
    ///
    /// # Returns
    ///
    /// The clip shape to apply.
    fn get_clip(&self, size: Size) -> T;

    /// Get an approximate bounding rectangle for the clip shape.
    ///
    /// This is used for optimization - the bounding rect helps determine
    /// if objects might intersect with the clip region.
    ///
    /// The default implementation returns a rect covering the entire size.
    fn get_approximate_clip_rect(&self, size: Size) -> Rect {
        Rect::from_origin_size(Point::ZERO, size)
    }

    /// Whether to recompute the clip when the delegate changes.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the clip shape needs to be recalculated.
    ///
    /// # Arguments
    ///
    /// * `old_clipper` - The previous clipper delegate
    ///
    /// # Returns
    ///
    /// `true` if the clip should be recalculated, `false` otherwise.
    fn should_reclip(&self, old_clipper: &dyn CustomClipper<T>) -> bool;

    /// Returns self as `Any` for downcasting.
    ///
    /// This enables comparing clippers of the same concrete type in
    /// `should_reclip`.
    fn as_any(&self) -> &dyn Any;
}

/// Default clipper that clips to a rectangle covering the entire size.
#[derive(Debug, Clone, Copy, Default)]
pub struct RectClipper;

impl CustomClipper<Rect> for RectClipper {
    fn get_clip(&self, size: Size) -> Rect {
        Rect::from_origin_size(Point::ZERO, size)
    }

    fn should_reclip(&self, _old_clipper: &dyn CustomClipper<Rect>) -> bool {
        false // Shape never changes
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[derive(Debug)]
    struct InsetClipper {
        inset: f64,
    }

    impl CustomClipper<Rect> for InsetClipper {
        fn get_clip(&self, size: Size) -> Rect {
            Rect::from_ltrb(
                self.inset,
                self.inset,
                size.width - self.inset,
                size.height - self.inset,
            )
        }

        fn should_reclip(&self, old_clipper: &dyn CustomClipper<Rect>) -> bool {
            if let Some(old) = old_clipper.as_any().downcast_ref::<Self>() {
                self.inset != old.inset
            } else {
                true
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}
