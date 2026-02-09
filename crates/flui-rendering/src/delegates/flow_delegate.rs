//! Flow delegate for custom flow layout algorithms.
//!
//! [`FlowDelegate`] allows users to implement custom flow layout behavior
//! with custom constraints and painting transforms.

use std::any::Any;
use std::fmt::Debug;

use flui_types::{Matrix4, Size};

use crate::constraints::BoxConstraints;

/// A delegate that provides custom flow layout behavior.
///
/// Flow layout is a powerful layout algorithm that allows positioning
/// children with arbitrary transforms. Unlike other layout delegates,
/// flow delegates can also control painting with custom transforms.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::{FlowDelegate, FlowPaintingContext};
/// use flui_types::{BoxConstraints, Matrix4, Size};
///
/// #[derive(Debug)]
/// struct CircularFlowDelegate {
///     radius: f32,
/// }
///
/// impl FlowDelegate for CircularFlowDelegate {
///     fn get_size(&self, constraints: BoxConstraints) -> Size {
///         let diameter = self.radius * 2.0;
///         constraints.constrain(Size::new(diameter, diameter))
///     }
///
///     fn get_constraints_for_child(&self, _index: usize, _constraints: BoxConstraints) -> BoxConstraints {
///         BoxConstraints::loose(Size::new(100.0, 100.0))
///     }
///
///     fn paint_children(&self, context: &mut FlowPaintingContext) {
///         let center_x = self.radius;
///         let center_y = self.radius;
///
///         for i in 0..context.child_count() {
///             let angle = 2.0 * std::f32::consts::PI * (i as f32) / (context.child_count() as f32);
///             let child_size = context.child_size(i);
///
///             let x = center_x + self.radius * angle.cos() - child_size.width / 2.0;
///             let y = center_y + self.radius * angle.sin() - child_size.height / 2.0;
///
///             let transform = Matrix4::from_translation(glam::vec3(x, y, 0.0));
///             context.paint_child(i, transform);
///         }
///     }
///
///     fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.radius != old.radius
///         } else {
///             true
///         }
///     }
///
///     fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool {
///         self.should_relayout(old_delegate)
///     }
/// }
/// ```
pub trait FlowDelegate: Send + Sync + Debug {
    /// Get the size of the flow layout for the given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The size of this render object.
    fn get_size(&self, constraints: BoxConstraints) -> Size;

    /// Get the constraints for a child at the given index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the child
    /// * `constraints` - The constraints from the parent
    ///
    /// # Returns
    ///
    /// The constraints to pass to the child.
    fn get_constraints_for_child(
        &self,
        index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints;

    /// Paint children with custom transforms.
    ///
    /// Use the context to paint each child with a specific transform matrix.
    ///
    /// # Arguments
    ///
    /// * `context` - The painting context providing child operations
    fn paint_children(&self, context: &mut FlowPaintingContext);

    /// Whether to relayout when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous delegate
    ///
    /// # Returns
    ///
    /// `true` if layout should be recalculated, `false` otherwise.
    fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool;

    /// Whether to repaint when the delegate changes.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous delegate
    ///
    /// # Returns
    ///
    /// `true` if painting should be redone, `false` otherwise.
    fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool;

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Context for flow painting operations.
///
/// This struct provides methods to query child sizes and paint children
/// with custom transforms.
pub struct FlowPaintingContext<'a> {
    /// The size of the flow layout.
    pub size: Size,
    child_sizes: &'a [Size],
    painted: Vec<bool>,
    // In real implementation, this would hold painting context
}

impl<'a> FlowPaintingContext<'a> {
    /// Creates a new flow painting context.
    pub fn new(size: Size, child_sizes: &'a [Size]) -> Self {
        let child_count = child_sizes.len();
        Self {
            size,
            child_sizes,
            painted: vec![false; child_count],
        }
    }

    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.child_sizes.len()
    }

    /// Returns the size of the child at the given index.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    pub fn child_size(&self, index: usize) -> Size {
        self.child_sizes[index]
    }

    /// Paint a child with the given transform.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the child to paint
    /// * `transform` - The transform matrix to apply
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    pub fn paint_child(&mut self, index: usize, _transform: Matrix4) {
        assert!(index < self.child_sizes.len(), "Child index out of bounds");
        self.painted[index] = true;
        // In real implementation, this would paint the child with the transform
    }

    /// Returns whether all children have been painted.
    pub fn all_children_painted(&self) -> bool {
        self.painted.iter().all(|&p| p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct LinearFlowDelegate {
        spacing: f32,
    }

    impl FlowDelegate for LinearFlowDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }

        fn get_constraints_for_child(
            &self,
            _index: usize,
            _constraints: BoxConstraints,
        ) -> BoxConstraints {
            BoxConstraints::loose(Size::new(100.0, 50.0))
        }

        fn paint_children(&self, context: &mut FlowPaintingContext) {
            let mut x = 0.0;
            for i in 0..context.child_count() {
                let transform = Matrix4::translation(x, 0.0, 0.0);
                context.paint_child(i, transform);
                x += context.child_size(i).width + self.spacing;
            }
        }

        fn should_relayout(&self, old_delegate: &dyn FlowDelegate) -> bool {
            if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
                (self.spacing - old.spacing).abs() > f32::EPSILON
            } else {
                true
            }
        }

        fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool {
            self.should_relayout(old_delegate)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_flow_painting_context() {
        let child_sizes = vec![
            Size::new(50.0, 30.0),
            Size::new(60.0, 40.0),
            Size::new(70.0, 50.0),
        ];
        let mut context = FlowPaintingContext::new(Size::new(300.0, 100.0), &child_sizes);

        assert_eq!(context.child_count(), 3);
        assert_eq!(context.child_size(0), Size::new(50.0, 30.0));
        assert_eq!(context.child_size(1), Size::new(60.0, 40.0));
        assert_eq!(context.child_size(2), Size::new(70.0, 50.0));

        assert!(!context.all_children_painted());

        context.paint_child(0, Matrix4::IDENTITY);
        context.paint_child(1, Matrix4::IDENTITY);
        assert!(!context.all_children_painted());

        context.paint_child(2, Matrix4::IDENTITY);
        assert!(context.all_children_painted());
    }

    #[test]
    fn test_linear_flow_delegate() {
        let delegate = LinearFlowDelegate { spacing: 10.0 };
        let constraints = BoxConstraints::new(0.0, 500.0, 0.0, 200.0);

        let size = delegate.get_size(constraints);
        assert_eq!(size, Size::new(500.0, 200.0));

        let child_constraints = delegate.get_constraints_for_child(0, constraints);
        assert_eq!(child_constraints.max_width, 100.0);
        assert_eq!(child_constraints.max_height, 50.0);
    }

    #[test]
    fn test_should_relayout() {
        let delegate1 = LinearFlowDelegate { spacing: 10.0 };
        let delegate2 = LinearFlowDelegate { spacing: 10.0 };
        let delegate3 = LinearFlowDelegate { spacing: 20.0 };

        assert!(!delegate1.should_relayout(&delegate2));
        assert!(delegate1.should_relayout(&delegate3));
    }
}
