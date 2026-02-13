//! View configuration for the root render object.

use flui_types::{Matrix4, Size};

use crate::constraints::BoxConstraints;

/// The layout constraints for the root render object.
///
/// This configuration defines the size constraints and device pixel ratio
/// for the root of the render tree.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `ViewConfiguration` class from `rendering/view.dart`.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewConfiguration {
    /// The constraints of the output surface in physical pixels.
    ///
    /// These constraints are enforced when translating the logical size
    /// of the root render object back to physical pixels.
    physical_constraints: BoxConstraints,

    /// The constraints of the output surface in logical pixels.
    ///
    /// These constraints are passed to the child of the root render object.
    logical_constraints: BoxConstraints,

    /// The pixel density of the output surface.
    ///
    /// This is the number of physical pixels per logical pixel.
    device_pixel_ratio: f32,
}

impl Default for ViewConfiguration {
    fn default() -> Self {
        Self {
            physical_constraints: BoxConstraints::tight(Size::ZERO),
            logical_constraints: BoxConstraints::tight(Size::ZERO),
            device_pixel_ratio: 1.0,
        }
    }
}

impl ViewConfiguration {
    /// Creates a new view configuration.
    ///
    /// By default, the view has constraints with all dimensions set to zero
    /// and a device pixel ratio of 1.0.
    pub fn new(
        physical_constraints: BoxConstraints,
        logical_constraints: BoxConstraints,
        device_pixel_ratio: f32,
    ) -> Self {
        Self {
            physical_constraints,
            logical_constraints,
            device_pixel_ratio,
        }
    }

    /// Creates a view configuration from a physical size and device pixel ratio.
    ///
    /// This is a convenience constructor that derives logical constraints
    /// from physical constraints and the device pixel ratio.
    pub fn from_size(physical_size: Size, device_pixel_ratio: f32) -> Self {
        let physical_constraints = BoxConstraints::tight(physical_size);
        let logical_size = Size::new(
            physical_size.width / device_pixel_ratio,
            physical_size.height / device_pixel_ratio,
        );
        let logical_constraints = BoxConstraints::tight(logical_size);

        Self {
            physical_constraints,
            logical_constraints,
            device_pixel_ratio,
        }
    }

    /// Creates a view configuration with flexible constraints.
    ///
    /// This allows the root render object to size itself within the given bounds.
    pub fn flexible(
        min_physical_size: Size,
        max_physical_size: Size,
        device_pixel_ratio: f32,
    ) -> Self {
        let physical_constraints = BoxConstraints::new(
            min_physical_size.width,
            max_physical_size.width,
            min_physical_size.height,
            max_physical_size.height,
        );
        let logical_constraints = BoxConstraints::new(
            min_physical_size.width / device_pixel_ratio,
            max_physical_size.width / device_pixel_ratio,
            min_physical_size.height / device_pixel_ratio,
            max_physical_size.height / device_pixel_ratio,
        );

        Self {
            physical_constraints,
            logical_constraints,
            device_pixel_ratio,
        }
    }

    /// Returns the constraints in logical pixels.
    ///
    /// These constraints are passed to the child of the root render object.
    #[inline]
    pub fn logical_constraints(&self) -> BoxConstraints {
        self.logical_constraints
    }

    /// Returns the constraints in physical pixels.
    ///
    /// These are enforced when translating back to physical pixels for rendering.
    #[inline]
    pub fn physical_constraints(&self) -> BoxConstraints {
        self.physical_constraints
    }

    /// Returns the device pixel ratio.
    ///
    /// This is the number of physical pixels per logical pixel.
    #[inline]
    pub fn device_pixel_ratio(&self) -> f32 {
        self.device_pixel_ratio
    }

    /// Creates a transformation matrix that applies the device pixel ratio.
    ///
    /// The matrix translates points from the local coordinate system of the
    /// app (in logical pixels) to the global coordinate system of the
    /// output surface (in physical pixels).
    pub fn to_matrix(&self) -> Matrix4 {
        Matrix4::scaling(self.device_pixel_ratio, self.device_pixel_ratio, 1.0)
    }

    /// Returns whether `to_matrix` would return a different value for this
    /// configuration than it would for the given `old_configuration`.
    pub fn should_update_matrix(&self, old_configuration: &ViewConfiguration) -> bool {
        self.device_pixel_ratio != old_configuration.device_pixel_ratio
    }

    /// Transforms the provided size in logical pixels to physical pixels.
    ///
    /// The result is constrained to the physical constraints.
    pub fn to_physical_size(&self, logical_size: Size) -> Size {
        let physical_size = Size::new(
            logical_size.width * self.device_pixel_ratio,
            logical_size.height * self.device_pixel_ratio,
        );
        self.physical_constraints.constrain(physical_size)
    }

    /// Transforms the provided size in physical pixels to logical pixels.
    pub fn to_logical_size(&self, physical_size: Size) -> Size {
        Size::new(
            physical_size.width / self.device_pixel_ratio,
            physical_size.height / self.device_pixel_ratio,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_view_configuration_default() {
        let config = ViewConfiguration::default();
        assert_eq!(config.device_pixel_ratio(), 1.0);
        assert_eq!(
            config.logical_constraints(),
            BoxConstraints::tight(Size::ZERO)
        );
        assert_eq!(
            config.physical_constraints(),
            BoxConstraints::tight(Size::ZERO)
        );
    }

    #[test]
    fn test_view_configuration_from_size() {
        let config = ViewConfiguration::from_size(Size::new(px(1920.0), px(1080.0)), 2.0);
        assert_eq!(config.device_pixel_ratio(), 2.0);
        assert_eq!(
            config.physical_constraints(),
            BoxConstraints::tight(Size::new(px(1920.0), px(1080.0)))
        );
        assert_eq!(
            config.logical_constraints(),
            BoxConstraints::tight(Size::new(px(960.0), px(540.0)))
        );
    }

    #[test]
    fn test_view_configuration_flexible() {
        let config = ViewConfiguration::flexible(Size::new(px(0.0), px(0.0)), Size::new(px(800.0), px(600.0)), 1.0);
        let logical = config.logical_constraints();
        assert_eq!(logical.min_width, 0.0);
        assert_eq!(logical.max_width, 800.0);
        assert_eq!(logical.min_height, 0.0);
        assert_eq!(logical.max_height, 600.0);
    }

    #[test]
    fn test_view_configuration_to_matrix() {
        let config = ViewConfiguration::from_size(Size::new(px(1920.0), px(1080.0)), 2.0);
        let matrix = config.to_matrix();
        assert!((matrix[0] - 2.0).abs() < 1e-6);
        assert!((matrix[5] - 2.0).abs() < 1e-6);
        assert!((matrix[10] - 1.0).abs() < 1e-6);
        assert!((matrix[15] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_view_configuration_should_update_matrix() {
        let config1 = ViewConfiguration::from_size(Size::new(px(800.0), px(600.0)), 1.0);
        let config2 = ViewConfiguration::from_size(Size::new(px(1600.0), px(1200.0)), 2.0);
        let config3 = ViewConfiguration::from_size(Size::new(px(1920.0), px(1080.0)), 1.0);

        assert!(config1.should_update_matrix(&config2)); // Different DPR
        assert!(!config1.should_update_matrix(&config3)); // Same DPR
    }

    #[test]
    fn test_view_configuration_to_physical_size() {
        let config = ViewConfiguration::from_size(Size::new(px(1920.0), px(1080.0)), 2.0);
        let logical = Size::new(px(960.0), px(540.0));
        let physical = config.to_physical_size(logical);
        assert!((physical.width - 1920.0).abs() < 1e-6);
        assert!((physical.height - 1080.0).abs() < 1e-6);
    }

    #[test]
    fn test_view_configuration_to_logical_size() {
        let config = ViewConfiguration::from_size(Size::new(px(1920.0), px(1080.0)), 2.0);
        let physical = Size::new(px(1920.0), px(1080.0));
        let logical = config.to_logical_size(physical);
        assert!((logical.width - 960.0).abs() < 1e-6);
        assert!((logical.height - 540.0).abs() < 1e-6);
    }
}
