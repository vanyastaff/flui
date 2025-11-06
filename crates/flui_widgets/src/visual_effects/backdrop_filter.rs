//! BackdropFilter widget - applies image filter to backdrop
//!
//! A widget that applies an image filter (like blur) to the content
//! behind it, creating effects like frosted glass.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, SingleRenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderBackdropFilter;
use flui_types::painting::{BlendMode, ImageFilter};

/// A widget that applies an image filter to the backdrop.
///
/// BackdropFilter applies filters (most commonly blur) to the content that
/// was painted before this widget. This is commonly used for frosted glass effects.
///
/// ## Key Properties
///
/// - **filter**: The image filter to apply (blur, brightness, saturation, invert)
/// - **blend_mode**: How to composite the filtered result (default: SrcOver)
/// - **child**: The child widget (optional)
///
/// ## Common Use Cases
///
/// ### Frosted glass effect
/// ```rust,ignore
/// BackdropFilter::blur(10.0, child)
/// ```
///
/// ### Custom filter
/// ```rust,ignore
/// BackdropFilter::new(
///     ImageFilter::brightness(1.2),
///     child
/// )
/// ```
///
/// ### With blend mode
/// ```rust,ignore
/// BackdropFilter::builder()
///     .filter(ImageFilter::blur(5.0))
///     .blend_mode(BlendMode::Multiply)
///     .child(content)
///     .build()
/// ```
///
/// ## Performance Notes
///
/// - This is an expensive operation (requires copying and filtering the backdrop)
/// - Consider using RepaintBoundary around filtered areas for better performance
/// - The filter is applied to the rectangular region covered by this widget
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple blur
/// BackdropFilter::blur(10.0, child)
///
/// // Brightness adjustment
/// BackdropFilter::new(ImageFilter::brightness(1.5), child)
///
/// // Multiple properties
/// BackdropFilter::builder()
///     .filter(ImageFilter::saturation(0.5))
///     .blend_mode(BlendMode::Screen)
///     .child(widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn = build_backdrop_filter)]
pub struct BackdropFilter {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Image filter to apply to backdrop
    /// Default: Blur with 0.0 radius
    #[builder(default = ImageFilter::blur(0.0))]
    pub filter: ImageFilter,

    /// Blend mode for compositing filtered result
    /// Default: BlendMode::SrcOver
    #[builder(default = BlendMode::SrcOver)]
    pub blend_mode: BlendMode,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

// Manual Debug implementation since AnyView doesn't implement Debug
impl std::fmt::Debug for BackdropFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackdropFilter")
            .field("key", &self.key)
            .field("filter", &self.filter)
            .field("blend_mode", &self.blend_mode)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl BackdropFilter {
    /// Creates a new BackdropFilter with blur effect.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let filter = BackdropFilter::blur(10.0, Box::new(child));
    /// ```
    pub fn blur(radius: f32, child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            filter: ImageFilter::blur(radius),
            blend_mode: BlendMode::SrcOver,
            child: Some(child),
        }
    }

    /// Creates a new BackdropFilter with custom filter.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let filter = BackdropFilter::new(
    ///     ImageFilter::brightness(1.2),
    ///     Box::new(child)
    /// );
    /// ```
    pub fn new(filter: ImageFilter, child: Box<dyn AnyView>) -> Self {
        Self {
            key: None,
            filter,
            blend_mode: BlendMode::SrcOver,
            child: Some(child),
        }
    }

    /// Creates a BackdropFilter with blur and blend mode.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let filter = BackdropFilter::blur_with_blend_mode(
    ///     10.0,
    ///     BlendMode::Multiply,
    ///     Box::new(child)
    /// );
    /// ```
    pub fn blur_with_blend_mode(
        radius: f32,
        blend_mode: BlendMode,
        child: Box<dyn AnyView>,
    ) -> Self {
        Self {
            key: None,
            filter: ImageFilter::blur(radius),
            blend_mode,
            child: Some(child),
        }
    }
}

impl Clone for BackdropFilter {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            filter: self.filter.clone(),
            blend_mode: self.blend_mode,
            child: self.child.clone(), // Widgets aren't cloned deeply
        }
    }
}

impl Default for BackdropFilter {
    fn default() -> Self {
        Self {
            key: None,
            filter: ImageFilter::blur(0.0),
            blend_mode: BlendMode::SrcOver,
            child: None,
        }
    }
}

// bon Builder Extensions
use backdrop_filter_builder::{IsUnset, SetChild, State};

impl<S: State> BackdropFilterBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> BackdropFilterBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Implement View trait
impl View for BackdropFilter {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Create RenderBackdropFilter with custom properties
        let mut render = RenderBackdropFilter::new(self.filter.clone());
        render.blend_mode = self.blend_mode;

        SingleRenderBuilder::new(render).with_optional_child(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backdrop_filter_blur() {
        let child = Box::new(crate::SizedBox::new());
        let filter = BackdropFilter::blur(10.0, child);
        assert!(matches!(filter.filter, ImageFilter::Blur { radius } if radius == 10.0));
    }

    #[test]
    fn test_backdrop_filter_new() {
        let child = Box::new(crate::SizedBox::new());
        let filter = BackdropFilter::new(ImageFilter::brightness(1.5), child);
        assert!(matches!(filter.filter, ImageFilter::Brightness { factor } if factor == 1.5));
    }

    #[test]
    fn test_backdrop_filter_builder() {
        let filter = BackdropFilter::builder()
            .filter(ImageFilter::saturation(0.8))
            .blend_mode(BlendMode::Screen)
            .build_backdrop_filter();
        assert!(matches!(filter.filter, ImageFilter::Saturation { factor } if factor == 0.8));
        assert_eq!(filter.blend_mode, BlendMode::Screen);
    }

    #[test]
    fn test_backdrop_filter_default() {
        let filter = BackdropFilter::default();
        assert!(filter.child.is_none());
    }
}
