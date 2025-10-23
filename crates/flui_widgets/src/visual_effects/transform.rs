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
use flui_core::{DynRenderObject, DynWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget, SingleChildRenderObjectElement};
use flui_rendering::RenderTransform;

// Use Matrix4 from rendering module
type Matrix4 = flui_rendering::objects::effects::transform::Matrix4;

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
#[derive(Debug, Clone, Builder)]
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
    pub child: Option<Box<dyn DynWidget>>,
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
        Self::new(Matrix4::translation(x, y))
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
        Self::new(Matrix4::rotation(radians))
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
        Self::new(Matrix4::scale(x, y))
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
    pub fn set_child<W: Widget + 'static>(&mut self, child: W) {
        self.child = Some(Box::new(child));
    }

    /// Validates Transform configuration.
    ///
    /// Returns an error if the transformation matrix is invalid (contains NaN or infinity).
    pub fn validate(&self) -> Result<(), String> {
        // Check if any matrix field is NaN or infinite
        let fields = [
            ("translate_x", self.transform.translate_x),
            ("translate_y", self.transform.translate_y),
            ("scale_x", self.transform.scale_x),
            ("scale_y", self.transform.scale_y),
            ("rotation", self.transform.rotation),
        ];

        for (name, value) in &fields {
            if value.is_nan() {
                return Err(format!(
                    "Invalid transform: field '{}' contains NaN",
                    name
                ));
            }
            if value.is_infinite() {
                return Err(format!(
                    "Invalid transform: field '{}' contains infinity",
                    name
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

// Implement Widget trait with associated type
impl Widget for Transform {
    type Element = SingleChildRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        SingleChildRenderObjectElement::new(self)
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for Transform {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        use flui_rendering::{SingleRenderBox, objects::effects::transform::TransformData};
        // Note: transform_hit_tests is ignored for now as RenderTransform doesn't support it yet
        Box::new(SingleRenderBox::new(TransformData::new(self.transform)))
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        if let Some(transform_render) = render_object.downcast_mut::<RenderTransform>() {
            transform_render.set_transform(self.transform);
            // Note: transform_hit_tests is ignored for now as RenderTransform doesn't support it yet
        }
    }
}

// Implement SingleChildRenderObjectWidget
impl SingleChildRenderObjectWidget for Transform {
    fn child(&self) -> &dyn DynWidget {
        self.child
            .as_ref()
            .map(|b| &**b as &dyn DynWidget)
            .unwrap_or_else(|| panic!("Transform requires a child"))
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
    pub fn child<W: Widget + 'static>(self, child: W) -> TransformBuilder<SetChild<S>> {
        self.child_internal(Box::new(child) as Box<dyn DynWidget>)
    }
}

// Public build() wrapper
impl<S: State> TransformBuilder<S> {
    /// Builds the Transform widget.
    ///
    /// Equivalent to calling the generated `build_transform()` finishing function.
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

#[cfg(disabled_test)] // TODO: Update tests to new Widget API
mod tests {
    use super::*;
    use std::f32::consts::PI;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    #[derive(Debug, Clone)]
    struct MockWidget;

    impl Widget for MockWidget {
        type Element = LeafRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            LeafRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

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
        matrix[0] = f32::NAN;

        let widget = Transform::new(matrix);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_transform_validate_infinite() {
        let mut matrix = Matrix4::identity();
        matrix[5] = f32::INFINITY;

        let widget = Transform::new(matrix);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_transform_render_object_creation() {
        let widget = Transform::rotate(PI / 4.0);
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderTransform>().is_some());
    }

    #[test]
    fn test_transform_render_object_update() {
        let widget1 = Transform::translate(10.0, 20.0);
        let mut render_object = widget1.create_render_object();

        let widget2 = Transform::rotate(PI / 2.0);
        widget2.update_render_object(&mut *render_object);

        let transform_render = render_object.downcast_ref::<RenderTransform>().unwrap();
        assert_eq!(transform_render.transform(), &Matrix4::rotation_z(PI / 2.0));
    }

    #[test]
    fn test_transform_render_object_hit_tests() {
        let widget = Transform::builder()
            .transform(Matrix4::translation(10.0, 20.0, 0.0))
            .transform_hit_tests(false)
            .build();

        let render_object = widget.create_render_object();
        let transform_render = render_object.downcast_ref::<RenderTransform>().unwrap();
        assert!(!transform_render.transform_hit_tests());
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

    #[test]
    fn test_transform_widget_trait() {
        let widget = Transform::builder()
            .transform(Matrix4::rotation_z(PI / 4.0))
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_transform_builder_with_child() {
        let widget = Transform::builder()
            .transform(Matrix4::scaling(2.0, 2.0, 1.0))
            .child(MockWidget)
            .build();

        assert!(widget.child.is_some());
        assert_eq!(widget.transform, Matrix4::scaling(2.0, 2.0, 1.0));
    }

    #[test]
    fn test_transform_set_child() {
        let mut widget = Transform::rotate(PI / 2.0);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }
}
