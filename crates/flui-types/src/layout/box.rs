//! Box layout types - fit and shape

use crate::geometry::{Pixels, Size};

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

    /// As large as possible while still containing the source entirely within
    /// the target box.
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
    /// Behavior-faithful port of Flutter's
    /// [`applyBoxFit`](https://api.flutter.dev/flutter/painting/applyBoxFit.html)
    /// (`packages/flutter/lib/src/painting/box_fit.dart`, 3.44.0) — every
    /// branch below mirrors that function's `switch` arm exactly, including
    /// which variants crop the source (`Cover`, and the "like cover" half
    /// of `FitWidth`/`FitHeight`/`None`) versus which never do (`Contain`,
    /// `ScaleDown`, `Fill`, and the "like contain" half of `FitWidth`/
    /// `FitHeight`).
    ///
    /// Returns a [`FittedSizes`] where:
    /// - [`FittedSizes::source`] is the portion of `input_size` to show —
    ///   equal to `input_size` unless this fit mode crops (see above).
    /// - [`FittedSizes::destination`] is the portion of `output_size` to
    ///   paint into — smaller than `output_size` when the fit letterboxes/
    ///   pillarboxes (`Contain`, `ScaleDown`, the non-cropping halves of
    ///   `FitWidth`/`FitHeight`), otherwise exactly `output_size`.
    ///
    /// A non-positive width or height on either input answers the
    /// degenerate `(Size::ZERO, Size::ZERO)` pair, matching Flutter's own
    /// leading guard — there is no meaningful fit to compute.
    #[must_use]
    #[inline]
    pub fn apply(self, input_size: Size<Pixels>, output_size: Size<Pixels>) -> FittedSizes {
        if input_size.width <= Pixels::ZERO
            || input_size.height <= Pixels::ZERO
            || output_size.width <= Pixels::ZERO
            || output_size.height <= Pixels::ZERO
        {
            return FittedSizes {
                source: Size::ZERO,
                destination: Size::ZERO,
            };
        }

        let input_aspect_ratio = input_size.width / input_size.height;
        let output_aspect_ratio = output_size.width / output_size.height;

        match self {
            BoxFit::Fill => FittedSizes {
                source: input_size,
                destination: output_size,
            },

            BoxFit::Contain => {
                let destination = if output_aspect_ratio > input_aspect_ratio {
                    Size::new(
                        input_size.width * (output_size.height / input_size.height),
                        output_size.height,
                    )
                } else {
                    Size::new(
                        output_size.width,
                        input_size.height * (output_size.width / input_size.width),
                    )
                };
                FittedSizes {
                    source: input_size,
                    destination,
                }
            }

            BoxFit::Cover => {
                let source = Self::cover_source(
                    input_size,
                    output_size,
                    input_aspect_ratio,
                    output_aspect_ratio,
                );
                FittedSizes {
                    source,
                    destination: output_size,
                }
            }

            BoxFit::FitWidth => {
                if output_aspect_ratio > input_aspect_ratio {
                    // Like Cover: crop the source height to match the output's aspect.
                    let source = Self::cover_source(
                        input_size,
                        output_size,
                        input_aspect_ratio,
                        output_aspect_ratio,
                    );
                    FittedSizes {
                        source,
                        destination: output_size,
                    }
                } else {
                    // Like Contain: no crop, letterbox vertically.
                    let destination = Size::new(
                        output_size.width,
                        input_size.height * (output_size.width / input_size.width),
                    );
                    FittedSizes {
                        source: input_size,
                        destination,
                    }
                }
            }

            BoxFit::FitHeight => {
                if output_aspect_ratio > input_aspect_ratio {
                    // Like Contain: no crop, letterbox horizontally.
                    let destination = Size::new(
                        input_size.width * (output_size.height / input_size.height),
                        output_size.height,
                    );
                    FittedSizes {
                        source: input_size,
                        destination,
                    }
                } else {
                    // Like Cover: crop the source width to match the output's aspect.
                    let source = Self::cover_source(
                        input_size,
                        output_size,
                        input_aspect_ratio,
                        output_aspect_ratio,
                    );
                    FittedSizes {
                        source,
                        destination: output_size,
                    }
                }
            }

            BoxFit::None => {
                // The visible region is capped to output on whichever axis
                // input overflows it; destination always equals that same
                // (possibly cropped) source — `None` never scales.
                let source = Size::new(
                    input_size.width.min(output_size.width),
                    input_size.height.min(output_size.height),
                );
                FittedSizes {
                    source,
                    destination: source,
                }
            }

            BoxFit::ScaleDown => {
                // Flutter's own two-step sequential shrink (NOT a delegation
                // to Contain/None): source is always the full input; the
                // destination starts at the input's own size and is
                // rescaled down an axis at a time, height first, using the
                // ORIGINAL input's aspect ratio throughout.
                let mut destination = input_size;
                if destination.height > output_size.height {
                    destination =
                        Size::new(output_size.height * input_aspect_ratio, output_size.height);
                }
                if destination.width > output_size.width {
                    destination =
                        Size::new(output_size.width, output_size.width / input_aspect_ratio);
                }
                FittedSizes {
                    source: input_size,
                    destination,
                }
            }
        }
    }

    /// Shared `Cover`-style source crop: the largest sub-rect of `input_size`
    /// whose aspect ratio matches `output_size`'s, keeping the FULL extent
    /// of whichever axis is the tighter constraint. `Cover` uses this
    /// directly; `FitWidth`/`FitHeight` fall into it on the half of their
    /// own branch that behaves like `Cover` (see `applyBoxFit`'s "like
    /// cover" comments).
    #[inline]
    fn cover_source(
        input_size: Size<Pixels>,
        output_size: Size<Pixels>,
        input_aspect_ratio: f32,
        output_aspect_ratio: f32,
    ) -> Size<Pixels> {
        if output_aspect_ratio > input_aspect_ratio {
            Size::new(
                input_size.width,
                input_size.width * (output_size.height / output_size.width),
            )
        } else {
            Size::new(
                input_size.height * (output_size.width / output_size.height),
                input_size.height,
            )
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
    /// If the box is not square, the circle will be inscribed in the shorter
    /// dimension.
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
    /// Circle shapes always need clipping, rectangles may need it for rounded
    /// corners.
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
/// use flui_types::{
///     geometry::{Size, px},
///     layout::{BoxFit, FittedSizes},
/// };
///
/// let input = Size::new(px(200.0), px(100.0));
/// let output = Size::new(px(100.0), px(100.0));
/// let fitted = BoxFit::Contain.apply(input, output);
///
/// assert_eq!(fitted.destination.width, px(100.0));
/// assert_eq!(fitted.destination.height, px(50.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FittedSizes {
    /// The size of the part of the input to show on the output.
    pub source: Size<Pixels>,

    /// The size of the part of the output on which to show the input.
    pub destination: Size<Pixels>,
}

impl FittedSizes {
    /// Creates a new fitted sizes struct.
    #[inline]
    #[must_use]
    pub const fn new(source: Size<Pixels>, destination: Size<Pixels>) -> Self {
        Self {
            source,
            destination,
        }
    }

    /// Returns the scale factor from source to destination.
    #[inline]
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        if self.source.width.abs() > Pixels(EPSILON) {
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
    use crate::geometry::px;

    fn size(w: f32, h: f32) -> Size<Pixels> {
        Size::new(px(w), px(h))
    }

    /// Every predicate for every variant:
    /// `(clip, keeps aspect, scale, scale up, scale down, fills, leaves space)`.
    #[test]
    fn box_fit_predicates() {
        use BoxFit::*;
        let table = [
            (Fill, [false, false, true, true, true, true, false]),
            (Contain, [false, true, true, true, true, false, true]),
            (Cover, [true, true, true, true, true, true, false]),
            (FitWidth, [true, true, true, true, true, true, true]),
            (FitHeight, [true, true, true, true, true, true, true]),
            (None, [true, true, false, false, false, false, true]),
            (ScaleDown, [false, true, true, false, true, false, true]),
        ];
        for (fit, expected) in table {
            let got = [
                fit.may_clip(),
                fit.maintains_aspect_ratio(),
                fit.may_scale(),
                fit.may_scale_up(),
                fit.may_scale_down(),
                fit.fills_target(),
                fit.may_leave_space(),
            ];
            assert_eq!(got, expected, "{fit:?}");
        }
        assert_eq!(BoxFit::default(), BoxFit::Contain);
    }

    #[test]
    fn box_shape_predicates() {
        let c = BoxShape::Circle;
        let r = BoxShape::Rectangle;
        assert_eq!(
            (c.is_circle(), c.is_rectangle(), c.requires_clipping()),
            (true, false, true)
        );
        assert_eq!(
            (r.is_circle(), r.is_rectangle(), r.requires_clipping()),
            (false, true, false)
        );
        assert_eq!(BoxShape::default(), r);
    }

    /// Any one non-positive dimension, on either side, is degenerate.
    #[test]
    fn apply_is_degenerate_on_any_empty_axis() {
        let good = size(100.0, 50.0);
        for (input, output) in [
            (size(0.0, 50.0), good),
            (size(100.0, 0.0), good),
            (good, size(0.0, 50.0)),
            (good, size(100.0, -1.0)),
            (good, size(100.0, 0.0)),
        ] {
            let fitted = BoxFit::Fill.apply(input, output);
            assert_eq!(
                fitted,
                FittedSizes::new(Size::ZERO, Size::ZERO),
                "{input:?} -> {output:?}"
            );
        }
    }

    /// The branches the square-output cases in `painting::image` can't reach:
    /// `Contain` with a tall input, `Cover`'s crop against a non-square
    /// output (where `out.h / out.w != 1`), and `ScaleDown` shrinking a tall
    /// input through its height step.
    #[test]
    fn apply_with_non_square_output_and_tall_input() {
        let contain = BoxFit::Contain.apply(size(100.0, 200.0), size(300.0, 100.0));
        assert_eq!(
            contain,
            FittedSizes::new(size(100.0, 200.0), size(50.0, 100.0))
        );

        let cover_wide = BoxFit::Cover.apply(size(400.0, 400.0), size(200.0, 100.0));
        assert_eq!(
            cover_wide,
            FittedSizes::new(size(400.0, 200.0), size(200.0, 100.0))
        );

        let cover_tall = BoxFit::Cover.apply(size(400.0, 400.0), size(100.0, 200.0));
        assert_eq!(
            cover_tall,
            FittedSizes::new(size(200.0, 400.0), size(100.0, 200.0))
        );

        let scale_down = BoxFit::ScaleDown.apply(size(100.0, 400.0), size(100.0, 100.0));
        assert_eq!(
            scale_down,
            FittedSizes::new(size(100.0, 400.0), size(25.0, 100.0))
        );
    }

    #[test]
    fn fitted_sizes_queries() {
        let shrunk = FittedSizes::new(size(200.0, 100.0), size(50.0, 25.0));
        assert_eq!(shrunk.scale_factor(), 0.25);
        assert!(shrunk.needs_scaling());
        assert!(!shrunk.will_clip());

        let unchanged = FittedSizes::new(size(40.0, 30.0), size(40.0, 30.0));
        assert!(!unchanged.needs_scaling());
        assert!(!unchanged.will_clip());
        // A source no wider than epsilon has no meaningful scale.
        for degenerate in [0.0, EPSILON] {
            let sizes = FittedSizes::new(size(degenerate, 1.0), size(9.0, 9.0));
            assert_eq!(sizes.scale_factor(), 1.0, "source width {degenerate}");
        }

        for grown in [size(41.0, 30.0), size(40.0, 31.0)] {
            assert!(
                FittedSizes::new(size(40.0, 30.0), grown).will_clip(),
                "{grown:?}"
            );
        }
    }
}
