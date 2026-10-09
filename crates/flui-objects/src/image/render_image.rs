//! RenderImage — renders bitmap images with aspect preservation and alignment.
//!
//! Supports aspect-ratio preservation, fit modes (Fill/Contain/Cover/ScaleDown/None),
//! and alignment.

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_painting::paint::Image;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// How to inscribe an image into a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFit {
    /// Fill the entire box, distorting the image if necessary.
    Fill,
    /// Contain the image within the box, maintaining aspect ratio.
    /// Image may be smaller than the box.
    Contain,
    /// Cover the entire box, maintaining aspect ratio.
    /// Image may be cropped.
    Cover,
    /// Contain the image and scale to fit, but only shrink (never enlarge).
    ScaleDown,
    /// Keep natural scale, cropping oversized content to the allocated box.
    None,
}

/// How to align an image within a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageAlignment {
    /// Align to the top-left corner.
    TopLeft,
    /// Align to the top-center edge.
    Top,
    /// Align to the top-right corner.
    TopRight,
    /// Align to the left-center edge.
    Left,
    /// Align to the center.
    Center,
    /// Align to the right-center edge.
    Right,
    /// Align to the bottom-left corner.
    BottomLeft,
    /// Align to the bottom-center edge.
    Bottom,
    /// Align to the bottom-right corner.
    BottomRight,
}

impl ImageAlignment {
    /// Calculates the offset for the given image and container size.
    fn offset(&self, image_size: Size, container_size: Size) -> Offset {
        let x = match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => 0.0,
            Self::Top | Self::Center | Self::Bottom => {
                (container_size.width - image_size.width) * 0.5
            }
            Self::TopRight | Self::Right | Self::BottomRight => {
                container_size.width - image_size.width
            }
        };

        let y = match self {
            Self::TopLeft | Self::Top | Self::TopRight => 0.0,
            Self::Left | Self::Center | Self::Right => {
                (container_size.height - image_size.height) * 0.5
            }
            Self::BottomLeft | Self::Bottom | Self::BottomRight => {
                container_size.height - image_size.height
            }
        };

        Offset::new(x, y)
    }
}

/// Render object for displaying images.
///
/// RenderImage displays a bitmap image within a rectangular area with support for:
/// - Aspect-ratio preservation via `ImageFit` mode
/// - Alignment within the containing box
/// - Intrinsic size queries based on image dimensions
#[derive(Debug, Clone)]
pub struct RenderImage {
    /// The image to display. `None` until a source is provided (e.g. async
    /// load). When `None`, the object still lays out via `intrinsic_size`
    /// but paints nothing.
    image: Option<Image>,
    /// Natural (intrinsic) size of the image, in image pixels. Divided by
    /// [`scale`](Self::scale) to obtain the logical aspect source. It is kept
    /// stored rather than derived live from the image, so a not-yet-loaded
    /// image can still reserve layout space instead of collapsing to
    /// `constraints.smallest`.
    intrinsic_size: Size,
    /// Optional forced logical width. Folded into the constraints during
    /// sizing; `None` means derive from the image aspect.
    width: Option<f64>,
    /// Optional forced logical height.
    height: Option<f64>,
    /// Number of image pixels per logical pixel.
    /// The intrinsic size is divided by this to get the logical aspect source,
    /// so a 2x asset renders at half its pixel dimensions.
    scale: f64,
    /// How to fit the image into available space. Affects paint only (where
    /// the image is fitted into the laid-out box), not the box size: sizing
    /// never reads `fit`.
    fit: ImageFit,
    /// How to align the image within the box. Paint-only, like `fit`.
    alignment: ImageAlignment,
}

impl RenderImage {
    /// Creates a new RenderImage with the given intrinsic size and no image
    /// source yet (placeholder layout).
    pub fn new(intrinsic_size: Size, fit: ImageFit, alignment: ImageAlignment) -> Self {
        Self {
            image: None,
            intrinsic_size,
            width: None,
            height: None,
            scale: 1.0,
            fit,
            alignment,
        }
    }

    /// Creates a RenderImage backed by an actual [`Image`].
    ///
    /// The intrinsic size is derived from the image's pixel dimensions.
    pub fn from_image(image: Image, fit: ImageFit, alignment: ImageAlignment) -> Self {
        let intrinsic_size = image.size();
        Self {
            image: Some(image),
            intrinsic_size,
            width: None,
            height: None,
            scale: 1.0,
            fit,
            alignment,
        }
    }

    /// Returns the current image source, if any.
    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    /// Returns the forced logical width, if set.
    pub fn width(&self) -> Option<f64> {
        self.width
    }

    /// Returns the forced logical height, if set.
    pub fn height(&self) -> Option<f64> {
        self.height
    }

    /// Returns the image-pixels-per-logical-pixel scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Sets the image source and updates the intrinsic size from its
    /// dimensions.
    ///
    pub fn set_image(&mut self, image: Option<Image>) -> flui_rendering::RenderUpdateImpact {
        if self.image == image {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let old_intrinsic_size = self.intrinsic_size;
        if let Some(ref img) = image {
            self.intrinsic_size = img.size();
        }
        self.image = image;
        if self.intrinsic_size == old_intrinsic_size {
            flui_rendering::RenderUpdateImpact::PAINT
        } else {
            flui_rendering::RenderUpdateImpact::LAYOUT
        }
    }

    /// Sets the intrinsic (natural) size of the image.
    pub fn set_intrinsic_size(&mut self, size: Size) -> flui_rendering::RenderUpdateImpact {
        if self.intrinsic_size == size {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.intrinsic_size = size;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the fit mode for the image.
    pub fn set_fit(&mut self, fit: ImageFit) -> flui_rendering::RenderUpdateImpact {
        if self.fit == fit {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.fit = fit;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Sets the alignment of the image within the box.
    pub fn set_alignment(
        &mut self,
        alignment: ImageAlignment,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.alignment == alignment {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.alignment = alignment;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Sets the forced logical width (`None` to derive from the image aspect).
    pub fn set_width(&mut self, width: Option<f64>) -> flui_rendering::RenderUpdateImpact {
        if self.width == width {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.width = width;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the forced logical height (`None` to derive from the image aspect).
    pub fn set_height(&mut self, height: Option<f64>) -> flui_rendering::RenderUpdateImpact {
        if self.height == height {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.height = height;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the image-pixels-per-logical-pixel scale. A non-finite or
    /// non-positive value is rejected (the previous scale is kept) because it
    /// would make the logical aspect source NaN or zero.
    pub fn set_scale(&mut self, scale: f64) -> flui_rendering::RenderUpdateImpact {
        if !scale.is_finite() || scale <= 0.0 || self.scale == scale {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.scale = scale;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Computes the destination rectangle for the image content within a box
    /// of the given size, applying the fit mode (scaling) and alignment
    /// (positioning).
    ///
    /// This is the public, pipeline-independent form of the fit + alignment
    /// math used by [`Self::paint`]; it lets callers (tests, demos, custom
    /// compositors) reproduce exactly where the image content lands inside a
    /// box of `box_size` without driving a full paint pass.
    ///
    /// Returns `None` when the intrinsic size is degenerate (zero in either
    /// dimension), in which case there is nothing to paint.
    pub fn paint_rect_in(&self, box_size: Size) -> Option<Rect> {
        self.fitted_rects(box_size).map(|(_, dst)| dst)
    }

    fn fitted_rects(&self, box_size: Size) -> Option<(Rect, Rect)> {
        let input = Size::new(
            self.intrinsic_size.width / self.scale,
            self.intrinsic_size.height / self.scale,
        );
        if ![input.width, input.height, box_size.width, box_size.height]
            .into_iter()
            .all(|v| v.is_finite() && v > 0.0)
        {
            return None;
        }
        let fit = match self.fit {
            ImageFit::Fill => flui_painting::BoxFit::Fill,
            ImageFit::Contain => flui_painting::BoxFit::Contain,
            ImageFit::Cover => flui_painting::BoxFit::Cover,
            ImageFit::ScaleDown => flui_painting::BoxFit::ScaleDown,
            ImageFit::None => flui_painting::BoxFit::None,
        };
        let fitted = fit.apply(input, box_size);
        let src_origin = self.alignment.offset(fitted.source, input);
        let dst_origin = self.alignment.offset(fitted.destination, box_size);
        Some((
            Rect::from_origin_size(Point::new(src_origin.dx, src_origin.dy), fitted.source),
            Rect::from_origin_size(Point::new(dst_origin.dx, dst_origin.dy), fitted.destination),
        ))
    }

    /// Computes the box size for the given constraints.
    ///
    /// The box size is **independent of [`fit`](ImageFit)**: the explicit
    /// `width`/`height` are folded into the constraints, then the box takes the
    /// largest size that fits while preserving the image's aspect ratio
    /// (intrinsic size divided by `scale`). `fit` only governs where the image
    /// is drawn inside this box, in [`Self::paint_rect_in`].
    ///
    /// The logical aspect source is `intrinsic_size / scale`; when it is
    /// degenerate (zero in either dimension) the box falls back to the
    /// constraints' smallest size.
    ///
    /// Public so that callers can reproduce layout sizing without driving a
    /// full layout pass (tests, demos).
    pub fn compute_size(&self, constraints: &BoxConstraints) -> Size {
        // Fold the explicit width/height into the constraints so all three are
        // treated uniformly.
        // `tighten` clamps each forced dimension INTO the parent's range, so a
        // forced size outside the parent's min/max can never commit a size that
        // violates the incoming constraints.
        let folded = constraints.tighten(self.width, self.height);

        let aspect = Size::new(
            self.intrinsic_size.width / self.scale,
            self.intrinsic_size.height / self.scale,
        );
        if aspect.width <= 0.0 || aspect.height <= 0.0 {
            return folded.smallest();
        }
        folded.constrain_size_and_attempt_to_preserve_aspect_ratio(aspect)
    }
}

impl Diagnosticable for RenderImage {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add(
            "image",
            if self.image.is_some() {
                "loaded"
            } else {
                "none"
            },
        );
        properties.add("intrinsic_size", format!("{:?}", self.intrinsic_size));
        properties.add(
            "width",
            self.width
                .map_or_else(|| "unset".to_string(), |w| format!("{w:?}")),
        );
        properties.add(
            "height",
            self.height
                .map_or_else(|| "unset".to_string(), |h| format!("{h:?}")),
        );
        properties.add_default_double("scale", self.scale, 1.0, None);
        properties.add_enum("fit", self.fit);
        properties.add_enum("alignment", self.alignment);
    }
}

impl RenderBox for RenderImage {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(self.compute_size(ctx.constraints()))
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        // Nothing to draw without a source image.
        let Some(image) = self.image.as_ref() else {
            return;
        };
        // Apply fit + alignment to obtain the destination rect in local
        // coordinates (the recorder pre-translates to this node's origin).
        // The laid-out box size comes from RenderState via `ctx.size()`.
        if let Some((source, dst)) = self.fitted_rects(ctx.size()) {
            let logical = Size::new(
                self.intrinsic_size.width / self.scale,
                self.intrinsic_size.height / self.scale,
            );
            let sx = f64::from(image.width()) / logical.width;
            let sy = f64::from(image.height()) / logical.height;
            let src = Rect::from_ltrb(
                source.left() * sx,
                source.top() * sy,
                source.right() * sx,
                source.bottom() * sy,
            );
            ctx.canvas()
                .draw_image_region(image.clone(), src, dst, None);
        }
    }

    // Intrinsics report the size the box
    // would take with the cross-axis extent tightened. With no forced
    // width/height the min-intrinsics report 0 (the image can scale to nothing),
    // while max-intrinsics always report the aspect-preserved extent. A leaf,
    // so the `_ctx` child channel is unused.

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if self.width.is_none() && self.height.is_none() {
            return Ok(0.0);
        }
        Ok(self
            .compute_size(&BoxConstraints::tight_for_finite(f64::INFINITY, height))
            .width)
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        Ok(self
            .compute_size(&BoxConstraints::tight_for_finite(f64::INFINITY, height))
            .width)
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if self.width.is_none() && self.height.is_none() {
            return Ok(0.0);
        }
        Ok(self
            .compute_size(&BoxConstraints::tight_for_finite(width, f64::INFINITY))
            .height)
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        _ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        Ok(self
            .compute_size(&BoxConstraints::tight_for_finite(width, f64::INFINITY))
            .height)
    }

    /// Dry layout is the exact box size `perform_layout` commits — both go
    /// through `compute_size`.
    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(self.compute_size(&constraints))
    }
}
