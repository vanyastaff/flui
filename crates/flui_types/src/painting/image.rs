//! Image handling types for painting.

use crate::geometry::Size;
use crate::painting::BlendMode;
use crate::styling::Color;

/// How an image should be inscribed into a box.
///
/// Similar to Flutter's `BoxFit`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::BoxFit;
///
/// let fit = BoxFit::Cover;
/// assert_eq!(fit, BoxFit::Cover);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxFit {
    /// Fill the target box by distorting the source's aspect ratio.
    Fill,

    /// As large as possible while still containing the source entirely within the target box.
    ///
    /// This is the default behavior.
    #[default]
    Contain,

    /// As small as possible while still covering the entire target box.
    ///
    /// The source will be clipped to fit the box if necessary.
    Cover,

    /// Make sure the full width of the source is shown, regardless of
    /// whether this means the source overflows the target box vertically.
    FitWidth,

    /// Make sure the full height of the source is shown, regardless of
    /// whether this means the source overflows the target box horizontally.
    FitHeight,

    /// Align the source within the target box (by default, centering) and discard any
    /// portions of the source that lie outside the box.
    ///
    /// The source image is not resized.
    None,

    /// Align the source within the target box (by default, centering) and, if necessary,
    /// scale down the source to ensure that it fits within the box.
    ///
    /// This is the same as `Contain` if that would shrink the image, otherwise it is the same as `None`.
    ScaleDown,
}

impl BoxFit {
    /// Returns the size that results from applying this fit to the given source and destination sizes.
    ///
    /// Returns `(fitted_size, source_size)` where:
    /// - `fitted_size` is the size the image should be rendered at
    /// - `source_size` is the portion of the source image to use
    #[must_use]
    pub fn apply(self, input_size: Size, output_size: Size) -> FittedSizes {
        let input_aspect_ratio = if input_size.height != 0.0 {
            input_size.width / input_size.height
        } else {
            0.0
        };

        let output_aspect_ratio = if output_size.height != 0.0 {
            output_size.width / output_size.height
        } else {
            0.0
        };

        match self {
            BoxFit::Fill => FittedSizes {
                source: input_size,
                destination: output_size,
            },

            BoxFit::Contain => {
                if output_aspect_ratio > input_aspect_ratio && input_aspect_ratio != 0.0 {
                    let width = output_size.height * input_aspect_ratio;
                    FittedSizes {
                        source: input_size,
                        destination: Size::new(width, output_size.height),
                    }
                } else if output_aspect_ratio != 0.0 {
                    let height = output_size.width / input_aspect_ratio;
                    FittedSizes {
                        source: input_size,
                        destination: Size::new(output_size.width, height),
                    }
                } else {
                    FittedSizes {
                        source: input_size,
                        destination: output_size,
                    }
                }
            }

            BoxFit::Cover => {
                // Cover needs to fill the entire output, scaling to the smallest dimension
                if output_aspect_ratio < input_aspect_ratio && input_aspect_ratio != 0.0 {
                    // Output is taller, fit to height
                    let width = output_size.height * input_aspect_ratio;
                    FittedSizes {
                        source: input_size,
                        destination: Size::new(width, output_size.height),
                    }
                } else if output_aspect_ratio != 0.0 {
                    // Output is wider, fit to width
                    let height = output_size.width / input_aspect_ratio;
                    FittedSizes {
                        source: input_size,
                        destination: Size::new(output_size.width, height),
                    }
                } else {
                    FittedSizes {
                        source: input_size,
                        destination: output_size,
                    }
                }
            }

            BoxFit::FitWidth => {
                let height = output_size.width / input_aspect_ratio;
                FittedSizes {
                    source: input_size,
                    destination: Size::new(output_size.width, height),
                }
            }

            BoxFit::FitHeight => {
                let width = output_size.height * input_aspect_ratio;
                FittedSizes {
                    source: input_size,
                    destination: Size::new(width, output_size.height),
                }
            }

            BoxFit::None => FittedSizes {
                source: input_size,
                destination: input_size,
            },

            BoxFit::ScaleDown => {
                if input_size.width > output_size.width || input_size.height > output_size.height {
                    BoxFit::Contain.apply(input_size, output_size)
                } else {
                    BoxFit::None.apply(input_size, output_size)
                }
            }
        }
    }
}

/// How to repeat an image to fill its layout bounds.
///
/// Similar to Flutter's `ImageRepeat`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::ImageRepeat;
///
/// let repeat = ImageRepeat::RepeatX;
/// assert_eq!(repeat, ImageRepeat::RepeatX);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ImageRepeat {
    /// Repeat the image in both the x and y directions until the box is filled.
    Repeat,

    /// Repeat the image in the x direction until the box is filled horizontally.
    RepeatX,

    /// Repeat the image in the y direction until the box is filled vertically.
    RepeatY,

    /// Do not repeat the image.
    #[default]
    NoRepeat,
}

/// Configuration information for an image.
///
/// Similar to Flutter's `ImageConfiguration`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::ImageConfiguration;
/// use flui_types::geometry::Size;
///
/// let config = ImageConfiguration::new()
///     .with_size(Size::new(100.0, 100.0))
///     .with_device_pixel_ratio(2.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageConfiguration {
    /// The size at which the image will be rendered.
    pub size: Option<Size>,

    /// The device pixel ratio where the image will be shown.
    pub device_pixel_ratio: Option<f32>,

    /// The platform the image is being rendered on.
    pub platform: Option<String>,
}

impl ImageConfiguration {
    /// Creates a new empty image configuration.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            size: None,
            device_pixel_ratio: None,
            platform: None,
        }
    }

    /// Creates a configuration with the given size.
    #[inline]
    #[must_use]
    pub const fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// Creates a configuration with the given device pixel ratio.
    #[inline]
    #[must_use]
    pub const fn with_device_pixel_ratio(mut self, ratio: f32) -> Self {
        self.device_pixel_ratio = Some(ratio);
        self
    }

    /// Creates a configuration with the given platform.
    #[inline]
    #[must_use]
    pub fn with_platform(mut self, platform: String) -> Self {
        self.platform = Some(platform);
        self
    }

    /// Returns the effective device pixel ratio (defaults to 1.0).
    #[inline]
    #[must_use]
    pub const fn effective_device_pixel_ratio(&self) -> f32 {
        match self.device_pixel_ratio {
            Some(ratio) => ratio,
            None => 1.0,
        }
    }

    /// Returns the logical size in physical pixels.
    #[inline]
    #[must_use]
    pub fn physical_size(&self) -> Option<Size> {
        self.size.map(|s| Size::new(
            s.width * self.effective_device_pixel_ratio(),
            s.height * self.effective_device_pixel_ratio(),
        ))
    }
}

impl Default for ImageConfiguration {
    fn default() -> Self {
        Self::new()
    }
}

/// The result of fitting a source size into a destination size.
///
/// Similar to Flutter's `FittedSizes`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{BoxFit, FittedSizes};
/// use flui_types::geometry::Size;
///
/// let input = Size::new(200.0, 100.0);
/// let output = Size::new(100.0, 100.0);
/// let fitted = BoxFit::Contain.apply(input, output);
///
/// assert_eq!(fitted.destination.width, 100.0);
/// assert_eq!(fitted.destination.height, 50.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FittedSizes {
    /// The size of the part of the input to show on the output.
    pub source: Size,

    /// The size of the part of the output on which to show the input.
    pub destination: Size,
}

impl FittedSizes {
    /// Creates a new fitted sizes struct.
    #[inline]
    #[must_use]
    pub const fn new(source: Size, destination: Size) -> Self {
        Self {
            source,
            destination,
        }
    }

    /// Returns the scale factor from source to destination.
    #[inline]
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        if self.source.width > 0.0 {
            self.destination.width / self.source.width
        } else {
            1.0
        }
    }

    /// Returns true if the image needs to be scaled.
    #[inline]
    #[must_use]
    pub fn needs_scaling(&self) -> bool {
        self.source != self.destination
    }

    /// Returns true if the image will be clipped.
    #[inline]
    #[must_use]
    pub fn will_clip(&self) -> bool {
        self.destination.width > self.source.width || self.destination.height > self.source.height
    }
}

/// A color filter to apply to an image.
///
/// Similar to Flutter's `ColorFilter`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::{ColorFilter, BlendMode};
/// use flui_types::styling::Color;
///
/// let filter = ColorFilter::mode(Color::RED, BlendMode::Multiply);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ColorFilter {
    /// Apply a color blend mode.
    Mode {
        /// The color to blend with the image.
        color: Color,
        /// The blend mode to use.
        blend_mode: BlendMode,
    },

    /// Apply a 5x4 matrix transformation in the RGBA color space.
    ///
    /// The matrix is applied as follows:
    /// ```text
    /// | R' |   | a00 a01 a02 a03 a04 |   | R |
    /// | G' |   | a10 a11 a12 a13 a14 |   | G |
    /// | B' | = | a20 a21 a22 a23 a24 | * | B |
    /// | A' |   | a30 a31 a32 a33 a34 |   | A |
    /// | 1  |   |  0   0   0   0   1  |   | 1 |
    /// ```
    Matrix([f32; 20]),

    /// Apply a gamma curve when converting from linear to sRGB.
    LinearToSrgbGamma,

    /// Apply a gamma curve when converting from sRGB to linear.
    SrgbToLinearGamma,
}

impl ColorFilter {
    /// Creates a color filter that applies a color blend mode.
    #[inline]
    #[must_use]
    pub const fn mode(color: Color, blend_mode: BlendMode) -> Self {
        ColorFilter::Mode { color, blend_mode }
    }

    /// Creates a color filter that applies a matrix transformation.
    #[inline]
    #[must_use]
    pub const fn matrix(matrix: [f32; 20]) -> Self {
        ColorFilter::Matrix(matrix)
    }

    /// Creates a color filter that converts from linear to sRGB gamma.
    #[inline]
    #[must_use]
    pub const fn linear_to_srgb_gamma() -> Self {
        ColorFilter::LinearToSrgbGamma
    }

    /// Creates a color filter that converts from sRGB to linear gamma.
    #[inline]
    #[must_use]
    pub const fn srgb_to_linear_gamma() -> Self {
        ColorFilter::SrgbToLinearGamma
    }

    /// Creates a grayscale color filter using luminance.
    #[inline]
    #[must_use]
    pub const fn grayscale() -> Self {
        #[allow(clippy::excessive_precision)]
        ColorFilter::Matrix([
            0.2126, 0.7152, 0.0722, 0.0, 0.0, // R = luminance
            0.2126, 0.7152, 0.0722, 0.0, 0.0, // G = luminance
            0.2126, 0.7152, 0.0722, 0.0, 0.0, // B = luminance
            0.0, 0.0, 0.0, 1.0, 0.0,          // A = unchanged
        ])
    }

    /// Creates a sepia tone color filter.
    #[inline]
    #[must_use]
    pub const fn sepia() -> Self {
        ColorFilter::Matrix([
            0.393, 0.769, 0.189, 0.0, 0.0,
            0.349, 0.686, 0.168, 0.0, 0.0,
            0.272, 0.534, 0.131, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }

    /// Creates an inverted color filter.
    #[inline]
    #[must_use]
    pub const fn invert() -> Self {
        ColorFilter::Matrix([
            -1.0, 0.0, 0.0, 0.0, 255.0,
            0.0, -1.0, 0.0, 0.0, 255.0,
            0.0, 0.0, -1.0, 0.0, 255.0,
            0.0, 0.0, 0.0, 1.0, 0.0,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_fit_default() {
        assert_eq!(BoxFit::default(), BoxFit::Contain);
    }

    #[test]
    fn test_box_fit_fill() {
        let input = Size::new(200.0, 100.0);
        let output = Size::new(100.0, 200.0);
        let fitted = BoxFit::Fill.apply(input, output);

        assert_eq!(fitted.source, input);
        assert_eq!(fitted.destination, output);
    }

    #[test]
    fn test_box_fit_contain() {
        let input = Size::new(200.0, 100.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::Contain.apply(input, output);

        assert_eq!(fitted.source, input);
        assert_eq!(fitted.destination.width, 100.0);
        assert_eq!(fitted.destination.height, 50.0);
    }

    #[test]
    fn test_box_fit_cover() {
        let input = Size::new(100.0, 200.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::Cover.apply(input, output);

        assert_eq!(fitted.source, input);
        // Cover должен покрывать весь output, поэтому масштабируем по width
        // height = 100 / 0.5 = 200 (изображение будет обрезано по высоте)
        assert_eq!(fitted.destination.width, 100.0);
        assert_eq!(fitted.destination.height, 200.0);
    }

    #[test]
    fn test_box_fit_fit_width() {
        let input = Size::new(200.0, 100.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::FitWidth.apply(input, output);

        assert_eq!(fitted.destination.width, 100.0);
        assert_eq!(fitted.destination.height, 50.0);
    }

    #[test]
    fn test_box_fit_fit_height() {
        let input = Size::new(100.0, 200.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::FitHeight.apply(input, output);

        assert_eq!(fitted.destination.width, 50.0);
        assert_eq!(fitted.destination.height, 100.0);
    }

    #[test]
    fn test_box_fit_none() {
        let input = Size::new(200.0, 100.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::None.apply(input, output);

        assert_eq!(fitted.source, input);
        assert_eq!(fitted.destination, input);
    }

    #[test]
    fn test_box_fit_scale_down_shrinks() {
        let input = Size::new(200.0, 200.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::ScaleDown.apply(input, output);

        assert_eq!(fitted.destination.width, 100.0);
        assert_eq!(fitted.destination.height, 100.0);
    }

    #[test]
    fn test_box_fit_scale_down_no_shrink() {
        let input = Size::new(50.0, 50.0);
        let output = Size::new(100.0, 100.0);
        let fitted = BoxFit::ScaleDown.apply(input, output);

        assert_eq!(fitted.destination, input);
    }

    #[test]
    fn test_image_repeat_default() {
        assert_eq!(ImageRepeat::default(), ImageRepeat::NoRepeat);
    }

    #[test]
    fn test_image_repeat_variants() {
        assert_ne!(ImageRepeat::Repeat, ImageRepeat::RepeatX);
        assert_ne!(ImageRepeat::RepeatY, ImageRepeat::NoRepeat);
    }

    #[test]
    fn test_image_configuration_new() {
        let config = ImageConfiguration::new();
        assert!(config.size.is_none());
        assert!(config.device_pixel_ratio.is_none());
        assert!(config.platform.is_none());
    }

    #[test]
    fn test_image_configuration_builder() {
        let config = ImageConfiguration::new()
            .with_size(Size::new(100.0, 100.0))
            .with_device_pixel_ratio(2.0);

        assert_eq!(config.size, Some(Size::new(100.0, 100.0)));
        assert_eq!(config.device_pixel_ratio, Some(2.0));
    }

    #[test]
    fn test_fitted_sizes_new() {
        let source = Size::new(100.0, 100.0);
        let destination = Size::new(50.0, 50.0);
        let fitted = FittedSizes::new(source, destination);

        assert_eq!(fitted.source, source);
        assert_eq!(fitted.destination, destination);
    }

    #[test]
    fn test_color_filter_mode() {
        let filter = ColorFilter::mode(Color::RED, BlendMode::Multiply);

        match filter {
            ColorFilter::Mode { color, blend_mode } => {
                assert_eq!(color, Color::RED);
                assert_eq!(blend_mode, BlendMode::Multiply);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_color_filter_matrix() {
        let matrix = [0.0; 20];
        let filter = ColorFilter::matrix(matrix);

        match filter {
            ColorFilter::Matrix(m) => assert_eq!(m, matrix),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_color_filter_gamma() {
        let linear_to_srgb = ColorFilter::linear_to_srgb_gamma();
        assert!(matches!(linear_to_srgb, ColorFilter::LinearToSrgbGamma));

        let srgb_to_linear = ColorFilter::srgb_to_linear_gamma();
        assert!(matches!(srgb_to_linear, ColorFilter::SrgbToLinearGamma));
    }
}
