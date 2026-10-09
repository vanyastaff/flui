//! Single-child layout delegate for custom layout algorithms.
//!
//! [`SingleChildLayoutDelegate`] allows users to implement custom layout
//! behavior for render objects with a single child.

use std::{any::Any, fmt::Debug};

use flui_foundation::geometry::{Offset, Size};

use crate::constraints::BoxConstraints;

/// A delegate that provides custom layout behavior for a single child.
///
/// Implement this trait to define custom layout algorithms for render objects
/// that have exactly one child. The delegate controls:
/// - The size of the parent
/// - The constraints passed to the child
/// - The position of the child within the parent
///
/// # Example
///
/// ```ignore
/// use flui_rendering::constraints::BoxConstraints;
/// use flui_rendering::delegates::SingleChildLayoutDelegate;
/// use flui_foundation::geometry::{Offset, Size};
///
/// #[derive(Debug)]
/// struct CenteringDelegate;
///
/// impl SingleChildLayoutDelegate for CenteringDelegate {
///     fn get_size(&self, constraints: BoxConstraints) -> Size {
///         constraints.biggest()
///     }
///
///     fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
///         constraints.loosen()
///     }
///
///     fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
///         Offset::new(
///             (size.width - child_size.width) / 2.0,
///             (size.height - child_size.height) / 2.0,
///         )
///     }
///
///     fn should_relayout(&self, _old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
///         false
///     }
/// }
/// ```
pub trait SingleChildLayoutDelegate: Debug {
    /// Get the size of the parent for the given constraints.
    ///
    /// Called during layout to determine the parent's size.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The size of this render object.
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    /// Get the constraints to pass to the child.
    ///
    /// Called during layout to determine what constraints the child
    /// should receive.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The constraints to pass to the child.
    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints
    }

    /// Get the position of the child within the parent.
    ///
    /// Called after the child has been laid out to determine where
    /// to position it.
    ///
    /// # Arguments
    ///
    /// * `size` - The size of the parent
    /// * `child_size` - The size of the child after layout
    ///
    /// # Returns
    ///
    /// The offset of the child from the parent's origin.
    fn get_position_for_child(&self, _size: Size, _child_size: Size) -> Offset {
        Offset::ZERO
    }

    /// Whether to relayout when the delegate changes.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the layout needs to be recalculated.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous layout delegate
    ///
    /// # Returns
    ///
    /// `true` if layout should be recalculated, `false` otherwise.
    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool;

    /// Returns self as `Any` for downcasting.
    ///
    /// This enables comparing delegates of the same concrete type in
    /// `should_relayout`.
    fn as_any(&self) -> &dyn Any;
}

/// A layout delegate that centers the child.
#[derive(Debug, Clone, Copy, Default)]
pub struct CenterLayoutDelegate;

impl SingleChildLayoutDelegate for CenterLayoutDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.loosen()
    }

    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
        Offset::new(
            (size.width - child_size.width) / 2.0,
            (size.height - child_size.height) / 2.0,
        )
    }

    fn should_relayout(&self, _old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        false
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A layout delegate that maintains a specific aspect ratio.
#[derive(Debug, Clone, Copy)]
pub struct AspectRatioDelegate {
    /// The desired aspect ratio (width / height).
    pub aspect_ratio: f64,
}

impl AspectRatioDelegate {
    /// Creates a new aspect ratio delegate.
    pub fn new(aspect_ratio: f64) -> Self {
        Self { aspect_ratio }
    }
}

impl SingleChildLayoutDelegate for AspectRatioDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        let width = constraints.max_width;
        let height = width / self.aspect_ratio;

        if height <= constraints.max_height {
            Size::new(width, height)
        } else {
            let height = constraints.max_height;
            let width = height * self.aspect_ratio;
            Size::new(width, height)
        }
    }

    fn get_constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        let size = self.get_size(constraints);
        BoxConstraints::tight(size)
    }

    fn get_position_for_child(&self, size: Size, child_size: Size) -> Offset {
        Offset::new(
            (size.width - child_size.width) / 2.0,
            (size.height - child_size.height) / 2.0,
        )
    }

    fn should_relayout(&self, old_delegate: &dyn SingleChildLayoutDelegate) -> bool {
        if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
            (self.aspect_ratio - old.aspect_ratio).abs() > f64::EPSILON
        } else {
            true
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
