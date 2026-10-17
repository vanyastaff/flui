//! RenderImage — paints a bitmap image placed by an [`ImagePlacement`].
//!
//! The box size preserves the image's logical aspect ratio; the placement
//! (fit, alignment and repetition, or a stretchable centre slice) decides
//! only where the image is painted inside that box.

use flui_foundation::Diagnosticable;
use flui_foundation::Leaf;
use flui_foundation::geometry::{Rect, Size};
use flui_painting::ImagePlacement;
use flui_painting::paint::Image;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// Render object for displaying images.
///
/// RenderImage displays a bitmap image within a rectangular area with support for:
/// - Aspect-ratio preserving layout from the image's logical size
/// - Placement inside the box: any [`BoxFit`](flui_painting::BoxFit) at a
///   fractional alignment, optional repetition, or a nine-slice centre
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
    /// Where the image is painted inside the laid-out box. Paint-only:
    /// sizing never reads it.
    placement: ImagePlacement,
}

impl RenderImage {
    /// Creates a new RenderImage with the given intrinsic size and no image
    /// source yet (placeholder layout).
    pub fn new(intrinsic_size: Size, placement: ImagePlacement) -> Self {
        Self {
            image: None,
            intrinsic_size,
            width: None,
            height: None,
            scale: 1.0,
            placement,
        }
    }

    /// Creates a RenderImage backed by an actual [`Image`].
    ///
    /// The intrinsic size is derived from the image's pixel dimensions.
    pub fn from_image(image: Image, placement: ImagePlacement) -> Self {
        let intrinsic_size = image.size();
        Self {
            image: Some(image),
            intrinsic_size,
            width: None,
            height: None,
            scale: 1.0,
            placement,
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

    /// Returns where the image is painted inside the box.
    pub fn placement(&self) -> ImagePlacement {
        self.placement
    }

    /// Sets where the image is painted inside the box. Paint-only.
    pub fn set_placement(
        &mut self,
        placement: ImagePlacement,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.placement == placement {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.placement = placement;
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

    /// Where one copy of the image lands inside a box of `box_size`, before
    /// any repetition, as [`Self::paint`] places it.
    ///
    /// The public, pipeline-independent form of the placement used by paint,
    /// so callers can reproduce where the content lands without a paint pass.
    /// `None` when the intrinsic size or the box is degenerate, or a centre
    /// slice lies outside the image: nothing would be painted.
    pub fn paint_rect_in(&self, box_size: Size) -> Option<Rect> {
        let pixels = self.image.as_ref().map_or(self.intrinsic_size, Image::size);
        self.placement.destination(
            Rect::from_xywh(0.0, 0.0, box_size.width, box_size.height),
            pixels,
            self.natural_size(),
        )
    }

    /// The image's logical size: intrinsic pixels divided by the scale. May
    /// overflow to infinity for a tiny scale; the placement bounds it.
    fn natural_size(&self) -> Size {
        Size::new(
            self.intrinsic_size.width / self.scale,
            self.intrinsic_size.height / self.scale,
        )
    }

    /// Computes the box size for the given constraints.
    ///
    /// The box size is **independent of the [`ImagePlacement`]**: the
    /// explicit `width`/`height` are folded into the constraints, then the box
    /// takes the largest size that fits while preserving the image's aspect
    /// ratio (intrinsic size divided by `scale`). The placement only governs
    /// where the image is drawn inside this box, in [`Self::paint_rect_in`].
    ///
    /// The logical aspect source is `intrinsic_size / scale`; when it is
    /// degenerate (zero in either dimension) the box falls back to the
    /// constraints' smallest size.
    /// Natural extents larger than `f64::MAX` are bounded together, preserving
    /// the source ratio before applying constraints.
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

        let mut aspect = Size::new(
            self.intrinsic_size.width / self.scale,
            self.intrinsic_size.height / self.scale,
        );
        if aspect.width.is_infinite() || aspect.height.is_infinite() {
            // Cap the magnitude, not each axis independently: the source
            // ratio remains meaningful when its natural extent exceeds f64.
            let longest = self.intrinsic_size.width.max(self.intrinsic_size.height);
            aspect = Size::new(
                (self.intrinsic_size.width / longest) * f64::MAX,
                (self.intrinsic_size.height / longest) * f64::MAX,
            );
        }
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
        properties.add("placement", format!("{:?}", self.placement));
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
        // The recorder pre-translates to this node's origin, so the box is
        // local; its laid-out size comes from `ctx.size()`.
        let size = ctx.size();
        self.placement.paint(
            ctx.canvas(),
            Rect::from_xywh(0.0, 0.0, size.width, size.height),
            image,
            self.natural_size(),
            None,
            None,
        );
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
