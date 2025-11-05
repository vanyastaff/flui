//! Transform widget - applies matrix transformations to child
//!
//! A widget that applies a transformation matrix before painting its child.
//! Similar to Flutter's Transform widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Transform {
//!     transform: Matrix4::translation(10.0, 20.0, 0.0),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Transform::builder()
//!     .transform(Matrix4::rotation_z(PI / 4.0))
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! transform! {
//!     transform: Matrix4::scaling(2.0, 2.0, 1.0),
//! }
//! ```

use bon::Builder;
use flui_core::{BuildContext, Element, RenderElement};
use flui_core::render::RenderNode;
use flui_core::view::{View, ChangeFlags, AnyView};
use flui_rendering::RenderTransform;
use flui_types::Matrix4;

/// A widget that applies a transformation matrix before painting its child.
///
/// The transformation affects painting and hit testing. By default, hit tests are
/// transformed to match the painted position.
///
/// ## Layout Behavior
///
/// - Passes constraints directly to child
/// - Takes the size of its child
/// - Transformation does not affect layout, only painting
///
/// ## Common Use Cases
///
/// ### Translation (Move)
/// ```rust,ignore
/// Transform::translate(10.0, 20.0)
///     .child(widget)
/// ```
///
/// ### Rotation
/// ```rust,ignore
/// use std::f32::consts::PI;
/// Transform::rotate(PI / 4.0)  // 45 degrees
///     .child(widget)
/// ```
///
/// ### Scaling
/// ```rust,ignore
/// Transform::scale(2.0, 2.0)  // 2x size
///     .child(widget)
/// ```
///
/// ### Combined Transformations
/// ```rust,ignore
/// let transform = Matrix4::translation(100.0, 100.0, 0.0)
///     * Matrix4::rotation_z(PI / 4.0)
///     * Matrix4::scaling(2.0, 2.0, 1.0);
///
/// Transform::new(transform)
///     .child(widget)
/// ```
///
/// ## Performance Considerations
///
/// - Transformations are applied during painting, not layout
/// - Complex transformations (rotation, skew) may be expensive
/// - For simple translations, consider using Positioned or Align instead
///
/// ## Examples
///
/// ```rust,ignore
/// // Rotate 45 degrees
/// Transform::rotate(std::f32::consts::PI / 4.0)
///     .child(Container::new().width(100.0).height(100.0))
///
/// // Scale 2x
/// Transform::scale(2.0, 2.0)
///     .child(Text::new("Big Text"))
///
/// // Move right 50px, down 30px
/// Transform::translate(50.0, 30.0)
///     .child(widget)
///
/// // Combined: scale then rotate
/// Transform::builder()
///     .transform(
///         Matrix4::rotation_z(PI / 6.0) * Matrix4::scaling(1.5, 1.5, 1.0)
///     )
///     .child(widget)
///     .build()
/// ```
///
/// ## See Also
///
/// - Positioned: For translating within Stack
/// - Align: For alignment-based positioning
/// - RotatedBox: For 90-degree rotations with layout changes
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Matrix4, into),
    finish_fn = build_transform
)]
pub struct Transform {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The transformation matrix to apply.
    ///
    /// Common transformations:
    /// - `Matrix4::translation(x, y, z)` - Move by offset
    /// - `Matrix4::rotation_z(radians)` - Rotate around Z axis
    /// - `Matrix4::scaling(x, y, z)` - Scale by factors
    /// - Combined: `translate * rotate * scale` (right-to-left application)
    #[builder(default = Matrix4::identity())]
    pub transform: Matrix4,

    /// Whether to transform hit tests (default: true).
    ///
    /// If true, hit tests are performed in the transformed coordinate space.
    /// If false, hit tests are performed in the child's original coordinate space.
    #[builder(default = true)]
    pub transform_hit_tests: bool,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transform")
            .field("key", &self.key)
            .field("transform", &self.transform)
            .field("transform_hit_tests", &self.transform_hit_tests)
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for Transform {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            transform: self.transform,
            transform_hit_tests: self.transform_hit_tests,
            child: self.child.clone(),
        }
    }
}

impl Transform {
    /// Creates a new Transform widget with the given transformation matrix.
    ///
    /// # Arguments
    ///
    /// * `transform` - The transformation matrix to apply
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Rotate 90 degrees
    /// let widget = Transform::new(Matrix4::rotation_z(PI / 2.0));
    ///
    /// // Scale 1.5x
    /// let widget = Transform::new(Matrix4::scaling(1.5, 1.5, 1.0));
    /// ```
    pub fn new(transform: Matrix4) -> Self {
        Self {
            key: None,
            transform,
            transform_hit_tests: true,
            child: None,
        }
    }

    /// Creates a Transform that translates (moves) its child.
    ///
    /// # Arguments
    ///
    /// * `x` - Horizontal offset
    /// * `y` - Vertical offset
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Move 50px right, 30px down
    /// let widget = Transform::translate(50.0, 30.0);
    /// ```
    pub fn translate(x: f32, y: f32) -> Self {
        Self::new(Matrix4::translation(x, y, 0.0))
    }

    /// Creates a Transform that rotates its child around the Z axis.
    ///
    /// # Arguments
    ///
    /// * `radians` - Rotation angle in radians
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use std::f32::consts::PI;
    ///
    /// // Rotate 45 degrees
    /// let widget = Transform::rotate(PI / 4.0);
    ///
    /// // Rotate 90 degrees
    /// let widget = Transform::rotate(PI / 2.0);
    /// ```
    pub fn rotate(radians: f32) -> Self {
        Self::new(Matrix4::rotation_z(radians))
    }

    /// Creates a Transform that scales its child.
    ///
    /// # Arguments
    ///
    /// * `x` - Horizontal scale factor
    /// * `y` - Vertical scale factor
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Double size
    /// let widget = Transform::scale(2.0, 2.0);
    ///
    /// // Flip horizontally
    /// let widget = Transform::scale(-1.0, 1.0);
    /// ```
    pub fn scale(x: f32, y: f32) -> Self {
        Self::new(Matrix4::scaling(x, y, 1.0))
    }

    /// Creates a Transform with identity matrix (no transformation).
    ///
    /// Equivalent to `Transform::new(Matrix4::identity())`.
    pub fn identity() -> Self {
        Self::new(Matrix4::identity())
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = Transform::rotate(PI / 4.0);
    /// widget.set_child(Container::new());
    /// ```
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates Transform configuration.
    ///
    /// Returns an error if the transformation matrix is invalid (contains NaN or infinity).
    pub fn validate(&self) -> Result<(), String> {
        // Check if any matrix element is NaN or infinite
        for (i, &value) in self.transform.m.iter().enumerate() {
            if value.is_nan() {
                return Err(format!(
                    "Invalid transform: matrix element [{}] contains NaN",
                    i
                ));
            }
            if value.is_infinite() {
                return Err(format!(
                    "Invalid transform: matrix element [{}] contains infinity",
                    i
                ));
            }
        }

        Ok(())
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

// Implement View for Transform - New architecture
impl View for Transform {
    type Element = Element;
    type State = Option<Box<dyn std::any::Any>>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child if present
        let (child_id, child_state) = if let Some(child) = self.child {
            let (elem, state) = child.build_any(ctx);
            let id = ctx.tree().write().insert(elem.into_element());
            (Some(id), Some(state))
        } else {
            (None, None)
        };

        // Convert Matrix4 to Transform::Matrix struct variant
        use flui_engine::layer::Transform as EngineTransform;
        let m = &self.transform.m;
        let transform = EngineTransform::Matrix {
            a: m[0],
            b: m[1],
            c: m[4],
            d: m[5],
            tx: m[12],
            ty: m[13],
        };

        // Create RenderNode (Single - child is Option<ElementId>)
        let render_node = RenderNode::Single {
            render: Box::new(RenderTransform::new(transform)),
            child: child_id,
        };

        // Create RenderElement using constructor
        let render_element = RenderElement::new(render_node);

        (Element::Render(render_element), child_state)
    }

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // TODO: Implement proper rebuild logic if needed
        // For now, return NONE as View architecture handles rebuilding
        ChangeFlags::NONE
    }
}

// bon Builder Extensions
use transform_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> TransformBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Transform::builder()
    ///     .transform(Matrix4::rotation_z(PI / 4.0))
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: impl View + 'static) -> TransformBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Public build() wrapper
impl<S: State> TransformBuilder<S> {
    /// Builds the Transform widget.
    pub fn build(self) -> Transform {
        self.build_transform()
    }
}

/// Macro for creating Transform with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// use std::f32::consts::PI;
///
/// // Rotate 45 degrees
/// transform! {
///     transform: Matrix4::rotation_z(PI / 4.0),
/// }
///
/// // Scale 2x
/// transform! {
///     transform: Matrix4::scaling(2.0, 2.0, 1.0),
/// }
///
/// // Disable hit test transformation
/// transform! {
///     transform: Matrix4::translation(10.0, 20.0, 0.0),
///     transform_hit_tests: false,
/// }
/// ```
#[macro_export]
macro_rules! transform {
    () => {
        $crate::Transform::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Transform {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_transform_new() {
        let matrix = Matrix4::translation(10.0, 20.0, 0.0);
        let widget = Transform::new(matrix);
        assert!(widget.key.is_none());
        assert_eq!(widget.transform, matrix);
        assert!(widget.transform_hit_tests);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_transform_identity() {
        let widget = Transform::identity();
        assert_eq!(widget.transform, Matrix4::identity());
    }

    #[test]
    fn test_transform_default() {
        let widget = Transform::default();
        assert_eq!(widget.transform, Matrix4::identity());
        assert!(widget.transform_hit_tests);
    }

    #[test]
    fn test_transform_translate() {
        let widget = Transform::translate(50.0, 30.0);
        assert_eq!(widget.transform, Matrix4::translation(50.0, 30.0, 0.0));
    }

    #[test]
    fn test_transform_rotate() {
        let angle = PI / 4.0;
        let widget = Transform::rotate(angle);
        assert_eq!(widget.transform, Matrix4::rotation_z(angle));
    }

    #[test]
    fn test_transform_scale() {
        let widget = Transform::scale(2.0, 3.0);
        assert_eq!(widget.transform, Matrix4::scaling(2.0, 3.0, 1.0));
    }

    #[test]
    fn test_transform_builder() {
        let matrix = Matrix4::rotation_z(PI / 2.0);
        let widget = Transform::builder()
            .transform(matrix)
            .transform_hit_tests(false)
            .build();

        assert_eq!(widget.transform, matrix);
        assert!(!widget.transform_hit_tests);
    }

    #[test]
    fn test_transform_struct_literal() {
        let matrix = Matrix4::scaling(1.5, 1.5, 1.0);
        let widget = Transform {
            transform: matrix,
            transform_hit_tests: false,
            ..Default::default()
        };

        assert_eq!(widget.transform, matrix);
        assert!(!widget.transform_hit_tests);
    }

    #[test]
    fn test_transform_validate_ok() {
        let widget = Transform::new(Matrix4::identity());
        assert!(widget.validate().is_ok());

        let widget = Transform::translate(100.0, 200.0);
        assert!(widget.validate().is_ok());

        let widget = Transform::rotate(PI);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_transform_validate_nan() {
        let mut matrix = Matrix4::identity();
        matrix.m[0] = f32::NAN;

        let widget = Transform::new(matrix);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_transform_validate_infinite() {
        let mut matrix = Matrix4::identity();
        matrix.m[5] = f32::INFINITY;

        let widget = Transform::new(matrix);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_transform_macro_empty() {
        let widget = transform!();
        assert_eq!(widget.transform, Matrix4::identity());
    }

    #[test]
    fn test_transform_macro_with_transform() {
        let matrix = Matrix4::scaling(2.0, 2.0, 1.0);
        let widget = transform! {
            transform: matrix,
        };
        assert_eq!(widget.transform, matrix);
    }

    #[test]
    fn test_transform_macro_with_hit_tests() {
        let widget = transform! {
            transform: Matrix4::identity(),
            transform_hit_tests: false,
        };
        assert!(!widget.transform_hit_tests);
    }

    #[test]
    fn test_transform_combined_transformations() {
        // Scale -> Rotate -> Translate (applied right to left)
        let transform = Matrix4::translation(100.0, 100.0, 0.0)
            * Matrix4::rotation_z(PI / 4.0)
            * Matrix4::scaling(2.0, 2.0, 1.0);

        let widget = Transform::new(transform);
        assert_eq!(widget.transform, transform);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_transform_flip_horizontal() {
        let widget = Transform::scale(-1.0, 1.0);
        assert_eq!(widget.transform, Matrix4::scaling(-1.0, 1.0, 1.0));
    }

    #[test]
    fn test_transform_flip_vertical() {
        let widget = Transform::scale(1.0, -1.0);
        assert_eq!(widget.transform, Matrix4::scaling(1.0, -1.0, 1.0));
    }

    #[test]
    fn test_transform_rotate_180() {
        let widget = Transform::rotate(PI);
        assert_eq!(widget.transform, Matrix4::rotation_z(PI));
    }

    #[test]
    fn test_transform_zero_translation() {
        let widget = Transform::translate(0.0, 0.0);
        assert_eq!(widget.transform, Matrix4::translation(0.0, 0.0, 0.0));
    }

    #[test]
    fn test_transform_zero_scale() {
        let widget = Transform::scale(0.0, 0.0);
        assert_eq!(widget.transform, Matrix4::scaling(0.0, 0.0, 1.0));
    }
}

// Transform now implements View trait directly
