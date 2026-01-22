//! Box layout types - fit and shape

use crate::geometry::Size;

/// Epsilon for safe float comparisons (Rust 1.91.0 strict arithmetic)
const EPSILON: f32 = 1e-6;

/// How a box should inscribe into another box.
///
/// This is similar to CSS `object-fit` property and Flutter's `BoxFit`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::BoxFit;
///
/// let fill = BoxFit::Fill;
/// let contain = BoxFit::Contain;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxFit {
    /// Fill the target box by distorting the source's aspect ratio.
    ///
    /// The entire source will be rendered to fill the destination box,
    /// distorting the aspect ratio if necessary.
    Fill,

    /// As large as possible while still containing the source entirely within the target box.
    ///
    /// Maintains aspect ratio. May leave empty space.
    ///
    /// Similar to CSS `object-fit: contain`.
    #[default]
    Contain,

    /// As small as possible while still covering the entire target box.
    ///
    /// Maintains aspect ratio. May clip the source.
    ///
    /// Similar to CSS `object-fit: cover`.
    Cover,

    /// Fit width, ignoring height. May overflow vertically.
    ///
    /// Maintains aspect ratio by scaling to match the width.
    FitWidth,

    /// Fit height, ignoring width. May overflow horizontally.
    ///
    /// Maintains aspect ratio by scaling to match the height.
    FitHeight,

    /// Center the source within the target box without scaling.
    ///
    /// If the source is larger than the target, it will be clipped.
    /// If smaller, there will be empty space.
    None,

    /// Center and scale down if needed to fit, but never scale up.
    ///
    /// Like `Contain`, but will not scale up if source is smaller.
    ScaleDown,
}

impl BoxFit {
    /// Returns true if this fit mode may clip content.
    #[inline]
    #[must_use]
    pub const fn may_clip(&self) -> bool {
        matches!(
            self,
            BoxFit::Cover | BoxFit::FitWidth | BoxFit::FitHeight | BoxFit::None
        )
    }

    /// Returns true if this fit mode always maintains aspect ratio.
    #[inline]
    #[must_use]
    pub const fn maintains_aspect_ratio(&self) -> bool {
        !matches!(self, BoxFit::Fill)
    }

    /// Returns true if this fit mode may scale content.
    #[inline]
    #[must_use]
    pub const fn may_scale(&self) -> bool {
        !matches!(self, BoxFit::None)
    }

    /// Returns true if this fit mode may scale up content.
    #[inline]
    #[must_use]
    pub const fn may_scale_up(&self) -> bool {
        !matches!(self, BoxFit::None | BoxFit::ScaleDown)
    }

    /// Returns true if this fit mode may scale down content.
    #[inline]
    #[must_use]
    pub const fn may_scale_down(&self) -> bool {
        matches!(
            self,
            BoxFit::Contain
                | BoxFit::Cover
                | BoxFit::FitWidth
                | BoxFit::FitHeight
                | BoxFit::ScaleDown
                | BoxFit::Fill
        )
    }

    /// Returns true if this fit mode fills the entire target area.
    #[inline]
    #[must_use]
    pub const fn fills_target(&self) -> bool {
        matches!(
            self,
            BoxFit::Fill | BoxFit::Cover | BoxFit::FitWidth | BoxFit::FitHeight
        )
    }

    /// Returns true if this fit mode leaves empty space.
    #[inline]
    #[must_use]
    pub const fn may_leave_space(&self) -> bool {
        matches!(
            self,
            BoxFit::Contain
                | BoxFit::None
                | BoxFit::ScaleDown
                | BoxFit::FitWidth
                | BoxFit::FitHeight
        )
    }

    /// Apply this fit mode to given input and output sizes.
    ///
    /// Returns `(fitted_size, source_size)` where:
    /// - `fitted_size` is the size the image should be rendered at
    /// - `source_size` is the portion of the source image to use
    #[must_use]
    pub fn apply(self, input_size: Size<f32>, output_size: Size<f32>) -> FittedSizes {
        let input_aspect_ratio = if input_size.height.abs() > EPSILON {
            input_size.width / input_size.height
        } else {
            0.0
        };

        let output_aspect_ratio = if output_size.height.abs() > EPSILON {
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
                if output_aspect_ratio > input_aspect_ratio && input_aspect_ratio.abs() > EPSILON {
                    let width = output_size.height * input_aspect_ratio;
                    FittedSizes {
                        source: input_size,
                        destination: Size::new(width, output_size.height),
                    }
                } else if output_aspect_ratio.abs() > EPSILON {
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
                if output_aspect_ratio < input_aspect_ratio && input_aspect_ratio.abs() > EPSILON {
                    // Output is taller, fit to height
                    let width = output_size.height * input_aspect_ratio;
                    FittedSizes {
                        source: input_size,
                        destination: Size::new(width, output_size.height),
                    }
                } else if output_aspect_ratio.abs() > EPSILON {
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
                let height = if input_aspect_ratio.abs() > EPSILON {
                    output_size.width / input_aspect_ratio
                } else {
                    output_size.height
                };
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

/// The shape of a box.
///
/// This is used to determine how a box should be clipped or rendered.
/// Similar to Flutter's `BoxShape`.
///
/// # Examples
///
/// ```
/// use flui_types::layout::BoxShape;
///
/// let rect = BoxShape::Rectangle;
/// let circle = BoxShape::Circle;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BoxShape {
    /// A rectangle, possibly with rounded corners.
    ///
    /// When rendering with a border radius, the box will have rounded corners.
    /// Without a border radius, it's a simple rectangle.
    #[default]
    Rectangle,

    /// A circle.
    ///
    /// The box will be clipped to a circle that fits within the box's bounds.
    /// If the box is not square, the circle will be inscribed in the shorter dimension.
    Circle,
}

impl BoxShape {
    /// Returns true if this shape is circular.
    #[inline]
    #[must_use]
    pub const fn is_circle(&self) -> bool {
        matches!(self, BoxShape::Circle)
    }

    /// Returns true if this shape is rectangular.
    #[inline]
    #[must_use]
    pub const fn is_rectangle(&self) -> bool {
        matches!(self, BoxShape::Rectangle)
    }

    /// Returns true if this shape requires clipping.
    ///
    /// Circle shapes always need clipping, rectangles may need it for rounded corners.
    #[inline]
    #[must_use]
    pub const fn requires_clipping(&self) -> bool {
        matches!(self, BoxShape::Circle)
    }
}

/// Result of applying a [`BoxFit`] mode to input and output sizes.
///
/// Contains both the portion of the source to show and the size
/// at which to render it on the destination.
///
/// # Example
///
/// ```
/// use flui_types::layout::{BoxFit, FittedSizes};
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
    pub source: Size<f32>,

    /// The size of the part of the output on which to show the input.
    pub destination: Size<f32>,
}

impl FittedSizes {
    /// Creates a new fitted sizes struct.
    #[inline]
    #[must_use]
    pub const fn new(source: Size<f32>, destination: Size<f32>) -> Self {
        Self {
            source,
            destination,
        }
    }

    /// Returns the scale factor from source to destination.
    #[inline]
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        if self.source.width.abs() > EPSILON {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_fit_properties() {
        assert!(BoxFit::Cover.may_clip());
        assert!(!BoxFit::Contain.may_clip());
        assert!(!BoxFit::Fill.may_clip());
        assert!(BoxFit::None.may_clip());

        assert!(BoxFit::Contain.maintains_aspect_ratio());
        assert!(BoxFit::Cover.maintains_aspect_ratio());
        assert!(!BoxFit::Fill.maintains_aspect_ratio());

        assert!(BoxFit::Contain.may_scale());
        assert!(!BoxFit::None.may_scale());
    }

    #[test]
    fn test_box_fit_default() {
        let default = BoxFit::default();
        assert_eq!(default, BoxFit::Contain);
    }

    #[test]
    fn test_box_shape_is_circle() {
        assert!(BoxShape::Circle.is_circle());
        assert!(!BoxShape::Rectangle.is_circle());
    }

    #[test]
    fn test_box_shape_is_rectangle() {
        assert!(BoxShape::Rectangle.is_rectangle());
        assert!(!BoxShape::Circle.is_rectangle());
    }

    #[test]
    fn test_box_shape_default() {
        let default = BoxShape::default();
        assert_eq!(default, BoxShape::Rectangle);
    }
}
