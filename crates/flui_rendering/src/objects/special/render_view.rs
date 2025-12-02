//! RenderView - Root of the render tree
//!
//! RenderView is the root RenderObject that connects the render tree to the
//! compositor/window. It handles the initial frame setup and coordinates
//! the output surface configuration.

use crate::core::{BoxLayoutCtx, RenderBox, Single};
use flui_types::{BoxConstraints, Size};

/// Configuration for the RenderView's layout constraints
///
/// ViewConfiguration specifies the size and constraints for the root
/// render object, typically matching the window/screen dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewConfiguration {
    /// The size of the output surface (e.g., window size)
    pub size: Size,

    /// Device pixel ratio (for high-DPI displays)
    pub device_pixel_ratio: f32,
}

impl ViewConfiguration {
    /// Create a new ViewConfiguration
    pub fn new(size: Size, device_pixel_ratio: f32) -> Self {
        Self {
            size,
            device_pixel_ratio,
        }
    }

    /// Create ViewConfiguration for a standard display (device_pixel_ratio = 1.0)
    pub fn standard(size: Size) -> Self {
        Self::new(size, 1.0)
    }

    /// Convert to BoxConstraints that tightly constrain to the view size
    pub fn to_constraints(&self) -> BoxConstraints {
        BoxConstraints::tight(self.size)
    }
}

impl Default for ViewConfiguration {
    fn default() -> Self {
        Self::new(Size::new(800.0, 600.0), 1.0)
    }
}

/// Root RenderObject that connects the render tree to the output surface
///
/// RenderView is the root of the render tree in FLUI. It represents the
/// total output surface (window, canvas, etc.) and bootstraps the rendering
/// pipeline.
///
/// # Responsibilities
///
/// - Provides root-level constraints based on output surface size
/// - Manages the single child that represents the entire UI
/// - Handles initial frame setup and configuration
/// - Acts as repaint boundary for the entire tree
///
/// # Usage
///
/// RenderView is typically created and managed by the framework, not
/// directly by application code. It receives ViewConfiguration from
/// the platform layer (window size, DPI, etc.) and propagates tight
/// constraints to its child.
///
/// ```rust,ignore
/// use flui_rendering::{RenderView, ViewConfiguration};
/// use flui_types::Size;
///
/// // Create root render object
/// let config = ViewConfiguration::new(
///     Size::new(1920.0, 1080.0),  // Window size
///     2.0,  // Retina display
/// );
/// let view = RenderView::new(config);
/// ```
///
/// # Layout Behavior
///
/// RenderView constrains its child to exactly match the output surface size
/// (tight constraints). This means the root widget will always be sized to
/// fill the entire window/canvas.
///
/// # Paint Behavior
///
/// RenderView acts as an automatic repaint boundary, meaning it always
/// has its own layer. This is because it's the root and must composite
/// separately from any potential parent (there is none).
#[derive(Debug)]
pub struct RenderView {
    /// Configuration for the view (size, DPI, etc.)
    pub configuration: ViewConfiguration,
}

impl RenderView {
    /// Create a new RenderView with the given configuration
    pub fn new(configuration: ViewConfiguration) -> Self {
        Self { configuration }
    }

    /// Create RenderView with standard configuration (800x600 @ 1.0 DPI)
    pub fn with_default_config() -> Self {
        Self::new(ViewConfiguration::default())
    }

    /// Update the view configuration (e.g., window resize)
    pub fn set_configuration(&mut self, configuration: ViewConfiguration) {
        self.configuration = configuration;
    }

    /// Get the current configuration
    pub fn get_configuration(&self) -> ViewConfiguration {
        self.configuration
    }

    /// Get the current view size
    pub fn size(&self) -> Size {
        self.configuration.size
    }

    /// Get the device pixel ratio
    pub fn device_pixel_ratio(&self) -> f32 {
        self.configuration.device_pixel_ratio
    }
}

impl RenderBox<Single> for RenderView {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Single>) -> Size {
        // Get the single child
        let child_id = ctx.children.single();

        // Create tight constraints that exactly match the view size
        let constraints = self.configuration.to_constraints();

        // Layout child with tight constraints (fills entire surface)
        let child_size = ctx.layout_child(child_id, constraints);

        // RenderView always returns the configuration size, regardless of child
        // (child must conform to our constraints)
        debug_assert_eq!(
            child_size, self.configuration.size,
            "RenderView child must fill the entire surface"
        );

        self.configuration.size
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Simply paint the child at origin (0, 0)
        // RenderView doesn't apply any transformations or effects
        let child_id = ctx.children.single();
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_configuration_new() {
        let config = ViewConfiguration::new(Size::new(1920.0, 1080.0), 2.0);
        assert_eq!(config.size, Size::new(1920.0, 1080.0));
        assert_eq!(config.device_pixel_ratio, 2.0);
    }

    #[test]
    fn test_view_configuration_standard() {
        let config = ViewConfiguration::standard(Size::new(800.0, 600.0));
        assert_eq!(config.size, Size::new(800.0, 600.0));
        assert_eq!(config.device_pixel_ratio, 1.0);
    }

    #[test]
    fn test_view_configuration_default() {
        let config = ViewConfiguration::default();
        assert_eq!(config.size, Size::new(800.0, 600.0));
        assert_eq!(config.device_pixel_ratio, 1.0);
    }

    #[test]
    fn test_view_configuration_to_constraints() {
        let config = ViewConfiguration::new(Size::new(1920.0, 1080.0), 2.0);
        let constraints = config.to_constraints();

        assert_eq!(constraints.min_width, 1920.0);
        assert_eq!(constraints.max_width, 1920.0);
        assert_eq!(constraints.min_height, 1080.0);
        assert_eq!(constraints.max_height, 1080.0);
    }

    #[test]
    fn test_render_view_new() {
        let config = ViewConfiguration::new(Size::new(1920.0, 1080.0), 2.0);
        let view = RenderView::new(config);

        assert_eq!(view.configuration, config);
        assert_eq!(view.size(), Size::new(1920.0, 1080.0));
        assert_eq!(view.device_pixel_ratio(), 2.0);
    }

    #[test]
    fn test_render_view_with_default_config() {
        let view = RenderView::with_default_config();
        assert_eq!(view.size(), Size::new(800.0, 600.0));
        assert_eq!(view.device_pixel_ratio(), 1.0);
    }

    #[test]
    fn test_render_view_set_configuration() {
        let mut view = RenderView::with_default_config();

        let new_config = ViewConfiguration::new(Size::new(2560.0, 1440.0), 3.0);
        view.set_configuration(new_config);

        assert_eq!(view.size(), Size::new(2560.0, 1440.0));
        assert_eq!(view.device_pixel_ratio(), 3.0);
    }

    #[test]
    fn test_get_configuration() {
        let config = ViewConfiguration::new(Size::new(1024.0, 768.0), 1.5);
        let view = RenderView::new(config);

        let retrieved_config = view.get_configuration();
        assert_eq!(retrieved_config, config);
    }
}
