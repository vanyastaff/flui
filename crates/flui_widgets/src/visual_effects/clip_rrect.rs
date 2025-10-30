//! ClipRRect widget - clips child to a rounded rectangle
//!
//! A widget that clips its child to a rounded rectangle.
//! Similar to Flutter's ClipRRect widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! ClipRRect {
//!     border_radius: BorderRadius::circular(10.0),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! ClipRRect::builder()
//!     .border_radius(BorderRadius::circular(10.0))
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! clip_rrect! {
//!     border_radius: BorderRadius::circular(10.0),
//! }
//! ```

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::{RenderClipRRect, RRectShape};
use flui_types::styling::BorderRadius;
use flui_types::painting::Clip;

/// A widget that clips its child to a rounded rectangle.
///
/// The clipping affects painting and hit testing. Points outside the rounded
/// rectangle are clipped and do not receive hits.
///
/// ## Layout Behavior
///
/// - Passes constraints directly to child
/// - Takes the size of its child
/// - Clipping does not affect layout, only painting and hit testing
///
/// ## Common Use Cases
///
/// ### Simple Circular Corners
/// ```rust,ignore
/// ClipRRect::circular(10.0)
///     .child(Image::asset("avatar.png"))
/// ```
///
/// ### Different Corner Radii
/// ```rust,ignore
/// ClipRRect::builder()
///     .border_radius(BorderRadius::only(
///         Radius::circular(10.0),  // top-left
///         Radius::circular(20.0),  // top-right
///         Radius::circular(10.0),  // bottom-left
///         Radius::circular(20.0),  // bottom-right
///     ))
///     .child(widget)
///     .build()
/// ```
///
/// ### Clip Behavior Control
/// ```rust,ignore
/// ClipRRect::builder()
///     .border_radius(BorderRadius::circular(10.0))
///     .clip_behavior(Clip::AntiAlias)  // Smooth edges
///     .child(widget)
///     .build()
/// ```
///
/// ## Performance Considerations
///
/// - Anti-aliased clipping is more expensive than hard-edge clipping
/// - Use `Clip::HardEdge` for better performance when edge quality is not critical
/// - Use `Clip::None` to disable clipping entirely (useful for debugging)
///
/// ## Examples
///
/// ```rust,ignore
/// // Avatar with rounded corners
/// ClipRRect::circular(8.0)
///     .child(Image::network("https://example.com/avatar.png"))
///
/// // Card with rounded top corners
/// ClipRRect::builder()
///     .border_radius(BorderRadius::vertical_top(Radius::circular(12.0)))
///     .child(Container::new()
///         .width(200.0)
///         .height(300.0)
///         .color(Color::BLUE))
///     .build()
///
/// // Pill-shaped button
/// ClipRRect::builder()
///     .border_radius(BorderRadius::circular(999.0))  // Large radius = pill shape
///     .child(Container::new()
///         .padding(EdgeInsets::symmetric(16.0, 8.0))
///         .color(Color::GREEN))
///     .build()
/// ```
///
/// ## See Also
///
/// - ClipRect: For rectangular (non-rounded) clipping
/// - ClipOval: For circular/elliptical clipping
/// - ClipPath: For arbitrary path clipping
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(BorderRadius, into),
    finish_fn = build_clip_rrect
)]
pub struct ClipRRect {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The border radius for the rounded rectangle.
    ///
    /// Common patterns:
    /// - `BorderRadius::circular(r)` - All corners with same radius
    /// - `BorderRadius::only(tl, tr, bl, br)` - Different radius per corner
    /// - `BorderRadius::vertical_top(r)` - Rounded top corners only
    /// - `BorderRadius::vertical_bottom(r)` - Rounded bottom corners only
    #[builder(default = BorderRadius::circular(0.0))]
    pub border_radius: BorderRadius,

    /// How to clip the child.
    ///
    /// - `Clip::None` - No clipping (for debugging)
    /// - `Clip::HardEdge` - Fast clipping with hard edges
    /// - `Clip::AntiAlias` - Smooth clipping with anti-aliasing (default)
    /// - `Clip::AntiAliasWithSaveLayer` - Highest quality, slowest
    #[builder(default = Clip::AntiAlias)]
    pub clip_behavior: Clip,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl ClipRRect {
    /// Creates a new ClipRRect widget with the given border radius.
    ///
    /// Uses `Clip::AntiAlias` by default.
    ///
    /// # Arguments
    ///
    /// * `border_radius` - The border radius for rounded corners
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // All corners with 10px radius
    /// let widget = ClipRRect::new(BorderRadius::circular(10.0));
    ///
    /// // Different radius per corner
    /// let widget = ClipRRect::new(BorderRadius::only(
    ///     Radius::circular(10.0),
    ///     Radius::circular(20.0),
    ///     Radius::circular(10.0),
    ///     Radius::circular(20.0),
    /// ));
    /// ```
    pub fn new(border_radius: BorderRadius) -> Self {
        Self {
            key: None,
            border_radius,
            clip_behavior: Clip::AntiAlias,
            child: None,
        }
    }

    /// Creates a ClipRRect with circular (equal) border radius on all corners.
    ///
    /// Convenience constructor for the common case of uniform corner rounding.
    ///
    /// # Arguments
    ///
    /// * `radius` - The radius to apply to all four corners
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Simple rounded corners
    /// let widget = ClipRRect::circular(10.0);
    ///
    /// // Pill shape (very large radius)
    /// let widget = ClipRRect::circular(999.0);
    /// ```
    pub fn circular(radius: f32) -> Self {
        Self::new(BorderRadius::circular(radius))
    }

    /// Creates a ClipRRect with no border radius (rectangular clipping).
    ///
    /// Equivalent to `ClipRRect::new(BorderRadius::circular(0.0))`.
    pub fn rectangular() -> Self {
        Self::new(BorderRadius::circular(0.0))
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = ClipRRect::circular(10.0);
    /// widget.set_child(Container::new());
    /// ```
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }

    /// Validates ClipRRect configuration.
    ///
    /// Returns an error if any border radius value is negative or invalid.
    pub fn validate(&self) -> Result<(), String> {
        // Check top-left
        if self.border_radius.top_left.x < 0.0 || self.border_radius.top_left.y < 0.0 {
            return Err(format!(
                "Invalid border_radius: top_left radius cannot be negative ({}, {})",
                self.border_radius.top_left.x, self.border_radius.top_left.y
            ));
        }

        // Check top-right
        if self.border_radius.top_right.x < 0.0 || self.border_radius.top_right.y < 0.0 {
            return Err(format!(
                "Invalid border_radius: top_right radius cannot be negative ({}, {})",
                self.border_radius.top_right.x, self.border_radius.top_right.y
            ));
        }

        // Check bottom-left
        if self.border_radius.bottom_left.x < 0.0 || self.border_radius.bottom_left.y < 0.0 {
            return Err(format!(
                "Invalid border_radius: bottom_left radius cannot be negative ({}, {})",
                self.border_radius.bottom_left.x, self.border_radius.bottom_left.y
            ));
        }

        // Check bottom-right
        if self.border_radius.bottom_right.x < 0.0 || self.border_radius.bottom_right.y < 0.0 {
            return Err(format!(
                "Invalid border_radius: bottom_right radius cannot be negative ({}, {})",
                self.border_radius.bottom_right.x, self.border_radius.bottom_right.y
            ));
        }

        // Check for NaN or infinity
        let radii = [
            self.border_radius.top_left.x,
            self.border_radius.top_left.y,
            self.border_radius.top_right.x,
            self.border_radius.top_right.y,
            self.border_radius.bottom_left.x,
            self.border_radius.bottom_left.y,
            self.border_radius.bottom_right.x,
            self.border_radius.bottom_right.y,
        ];

        for (i, &radius) in radii.iter().enumerate() {
            if radius.is_nan() {
                return Err(format!(
                    "Invalid border_radius: contains NaN at position {}",
                    i
                ));
            }
            if radius.is_infinite() {
                return Err(format!(
                    "Invalid border_radius: contains infinity at position {}",
                    i
                ));
            }
        }

        Ok(())
    }
}

impl Default for ClipRRect {
    fn default() -> Self {
        Self::rectangular()
    }
}

// Implement Widget trait with associated type


// Implement RenderObjectWidget
impl RenderWidget for ClipRRect {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderClipRRect::new(
            RRectShape::new(self.border_radius),
            self.clip_behavior,
        )))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(obj) = render.downcast_mut::<RenderClipRRect>() {
                obj.set_border_radius(self.border_radius);
                obj.set_clip_behavior(self.clip_behavior);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// bon Builder Extensions
use clip_r_rect_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> ClipRRectBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// ClipRRect::builder()
    ///     .border_radius(BorderRadius::circular(10.0))
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: Widget) -> ClipRRectBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

// Public build() wrapper
impl<S: State> ClipRRectBuilder<S> {
    /// Builds the ClipRRect widget.
    ///
    /// Equivalent to calling the generated `build_clip_rrect()` finishing function.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_clip_rrect())
    }
}

/// Macro for creating ClipRRect with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Circular corners
/// clip_rrect! {
///     border_radius: BorderRadius::circular(10.0),
/// }
///
/// // Hard edge clipping
/// clip_rrect! {
///     border_radius: BorderRadius::circular(10.0),
///     clip_behavior: Clip::HardEdge,
/// }
///
/// // No clipping (debugging)
/// clip_rrect! {
///     clip_behavior: Clip::None,
/// }
/// ```
#[macro_export]
macro_rules! clip_rrect {
    () => {
        $crate::ClipRRect::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::ClipRRect {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::Radius;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    #[derive(Debug, Clone)]
    struct MockWidget;

    

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

    #[test]
    fn test_clip_rrect_new() {
        let border_radius = BorderRadius::circular(10.0);
        let widget = ClipRRect::new(border_radius);
        assert!(widget.key.is_none());
        assert_eq!(widget.border_radius, border_radius);
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_clip_rrect_circular() {
        let widget = ClipRRect::circular(15.0);
        assert_eq!(widget.border_radius, BorderRadius::circular(15.0));
    }

    #[test]
    fn test_clip_rrect_rectangular() {
        let widget = ClipRRect::rectangular();
        assert_eq!(widget.border_radius, BorderRadius::circular(0.0));
    }

    #[test]
    fn test_clip_rrect_default() {
        let widget = ClipRRect::default();
        assert_eq!(widget.border_radius, BorderRadius::circular(0.0));
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rrect_builder() {
        let border_radius = BorderRadius::circular(20.0);
        let widget = ClipRRect::builder()
            .border_radius(border_radius)
            .clip_behavior(Clip::HardEdge)
            .build();

        assert_eq!(widget.border_radius, border_radius);
        assert_eq!(widget.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_clip_rrect_struct_literal() {
        let border_radius = BorderRadius::circular(25.0);
        let widget = ClipRRect {
            border_radius,
            clip_behavior: Clip::None,
            ..Default::default()
        };

        assert_eq!(widget.border_radius, border_radius);
        assert_eq!(widget.clip_behavior, Clip::None);
    }

    #[test]
    fn test_clip_rrect_different_corner_radii() {
        let border_radius = BorderRadius::only(
            Radius::circular(10.0),
            Radius::circular(20.0),
            Radius::circular(5.0),
            Radius::circular(15.0),
        );
        let widget = ClipRRect::new(border_radius);
        assert_eq!(widget.border_radius, border_radius);
    }

    #[test]
    fn test_clip_rrect_validate_ok() {
        let widget = ClipRRect::circular(10.0);
        assert!(widget.validate().is_ok());

        let widget = ClipRRect::rectangular();
        assert!(widget.validate().is_ok());

        let widget = ClipRRect::new(BorderRadius::only(
            Radius::circular(5.0),
            Radius::circular(10.0),
            Radius::circular(15.0),
            Radius::circular(20.0),
        ));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_clip_rrect_validate_negative() {
        let border_radius = BorderRadius::only(
            Radius::circular(-10.0),
            Radius::circular(10.0),
            Radius::circular(10.0),
            Radius::circular(10.0),
        );
        let widget = ClipRRect::new(border_radius);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_clip_rrect_validate_nan() {
        let border_radius = BorderRadius::only(
            Radius::circular(f32::NAN),
            Radius::circular(10.0),
            Radius::circular(10.0),
            Radius::circular(10.0),
        );
        let widget = ClipRRect::new(border_radius);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_clip_rrect_validate_infinite() {
        let border_radius = BorderRadius::only(
            Radius::circular(10.0),
            Radius::circular(f32::INFINITY),
            Radius::circular(10.0),
            Radius::circular(10.0),
        );
        let widget = ClipRRect::new(border_radius);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_clip_rrect_render_object_creation() {
        let widget = ClipRRect::circular(10.0);
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderClipRRect>().is_some());
    }

    #[test]
    fn test_clip_rrect_render_object_update() {
        let widget1 = ClipRRect::circular(10.0);
        let mut render_object = widget1.create_render_object();

        let widget2 = ClipRRect::builder()
            .border_radius(BorderRadius::circular(20.0))
            .clip_behavior(Clip::HardEdge)
            .build();
        widget2.update_render_object(&mut *render_object);

        let clip_render = render_object.downcast_ref::<RenderClipRRect>().unwrap();
        assert_eq!(clip_render.border_radius(), BorderRadius::circular(20.0));
        assert_eq!(clip_render.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_rrect_render_object_clip_behavior() {
        let widget = ClipRRect::builder()
            .border_radius(BorderRadius::circular(10.0))
            .clip_behavior(Clip::None)
            .build();

        let render_object = widget.create_render_object();
        let clip_render = render_object.downcast_ref::<RenderClipRRect>().unwrap();
        assert_eq!(clip_render.clip_behavior(), Clip::None);
    }

    #[test]
    fn test_clip_rrect_macro_empty() {
        let widget = clip_rrect!();
        assert_eq!(widget.border_radius, BorderRadius::circular(0.0));
    }

    #[test]
    fn test_clip_rrect_macro_with_border_radius() {
        let border_radius = BorderRadius::circular(10.0);
        let widget = clip_rrect! {
            border_radius: border_radius,
        };
        assert_eq!(widget.border_radius, border_radius);
    }

    #[test]
    fn test_clip_rrect_macro_with_clip_behavior() {
        let widget = clip_rrect! {
            border_radius: BorderRadius::circular(10.0),
            clip_behavior: Clip::HardEdge,
        };
        assert_eq!(widget.clip_behavior, Clip::HardEdge);
    }

    #[test]
    fn test_clip_rrect_zero_radius() {
        let widget = ClipRRect::circular(0.0);
        assert_eq!(widget.border_radius, BorderRadius::circular(0.0));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_clip_rrect_large_radius() {
        // Large radius for pill shapes
        let widget = ClipRRect::circular(999.0);
        assert_eq!(widget.border_radius, BorderRadius::circular(999.0));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_clip_rrect_anti_alias_default() {
        let widget = ClipRRect::circular(10.0);
        assert_eq!(widget.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_clip_rrect_all_clip_behaviors() {
        let behaviors = [
            Clip::None,
            Clip::HardEdge,
            Clip::AntiAlias,
            Clip::AntiAliasWithSaveLayer,
        ];

        for behavior in behaviors {
            let widget = ClipRRect::builder()
                .border_radius(BorderRadius::circular(10.0))
                .clip_behavior(behavior)
                .build();
            assert_eq!(widget.clip_behavior, behavior);
        }
    }

    #[test]
    fn test_clip_rrect_widget_trait() {
        let widget = ClipRRect::builder()
            .border_radius(BorderRadius::circular(10.0))
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_clip_rrect_builder_with_child() {
        let widget = ClipRRect::builder()
            .border_radius(BorderRadius::circular(10.0))
            .child(MockWidget)
            .build();

        assert!(widget.child.is_some());
        assert_eq!(widget.border_radius, BorderRadius::circular(10.0));
    }

    #[test]
    fn test_clip_rrect_set_child() {
        let mut widget = ClipRRect::circular(5.0);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(ClipRRect, render);
