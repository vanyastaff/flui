//! RenderImage — renders bitmap images with aspect preservation and alignment.
//!
//! Implements the RenderImage protocol object following Flutter's image.dart (22-404).
//! Supports aspect-ratio preservation, fit modes (Fill/Contain/Cover/ScaleDown/None),
//! and alignment.

use flui_foundation::Diagnosticable;
use flui_tree::Leaf;
use flui_types::{Offset, Pixels, Point, Rect, Size, painting::Image};

use crate::{
    constraints::BoxConstraints,
    context::{BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
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
    /// Do not scale the image; show at natural size.
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
            Self::TopLeft | Self::Left | Self::BottomLeft => Pixels::ZERO,
            Self::Top | Self::Center | Self::Bottom => {
                (container_size.width - image_size.width) * 0.5
            }
            Self::TopRight | Self::Right | Self::BottomRight => {
                container_size.width - image_size.width
            }
        };

        let y = match self {
            Self::TopLeft | Self::Top | Self::TopRight => Pixels::ZERO,
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
    /// [`scale`](Self::scale) to obtain the logical aspect source. flui keeps
    /// this stored (Flutter derives it live from the image) so a not-yet-loaded
    /// image can still reserve layout space — a superset of Flutter, which
    /// returns `constraints.smallest` while the image is null.
    intrinsic_size: Size,
    /// Optional forced logical width. Folded into the constraints during
    /// sizing (Flutter `RenderImage.width`); `None` means derive from the
    /// image aspect.
    width: Option<Pixels>,
    /// Optional forced logical height (Flutter `RenderImage.height`).
    height: Option<Pixels>,
    /// Number of image pixels per logical pixel (Flutter `RenderImage.scale`).
    /// The intrinsic size is divided by this to get the logical aspect source,
    /// so a 2x asset renders at half its pixel dimensions.
    scale: f32,
    /// How to fit the image into available space. Affects paint only (where
    /// the image is fitted into the laid-out box), not the box size — matching
    /// Flutter, whose `_sizeForConstraints` never reads `fit`.
    fit: ImageFit,
    /// How to align the image within the box. Paint-only, like `fit`.
    alignment: ImageAlignment,
    /// Cached layout size (set by perform_layout).
    size: Size,
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
            size: Size::ZERO,
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
            size: Size::ZERO,
        }
    }

    /// Returns the current image source, if any.
    pub fn image(&self) -> Option<&Image> {
        self.image.as_ref()
    }

    /// Returns the forced logical width, if set.
    pub fn width(&self) -> Option<Pixels> {
        self.width
    }

    /// Returns the forced logical height, if set.
    pub fn height(&self) -> Option<Pixels> {
        self.height
    }

    /// Returns the image-pixels-per-logical-pixel scale.
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Sets the image source and updates the intrinsic size from its
    /// dimensions.
    ///
    /// The caller is responsible for marking the node layout-dirty.
    pub fn set_image(&mut self, image: Option<Image>) {
        if let Some(ref img) = image {
            self.intrinsic_size = img.size();
        }
        self.image = image;
    }

    /// Sets the intrinsic (natural) size of the image.
    pub fn set_intrinsic_size(&mut self, size: Size) {
        self.intrinsic_size = size;
        // Caller responsible for marking layout dirty
    }

    /// Sets the fit mode for the image.
    pub fn set_fit(&mut self, fit: ImageFit) {
        self.fit = fit;
        // Caller responsible for marking layout dirty
    }

    /// Sets the alignment of the image within the box.
    pub fn set_alignment(&mut self, alignment: ImageAlignment) {
        self.alignment = alignment;
        // Caller responsible for marking repaint dirty
    }

    /// Sets the forced logical width (`None` to derive from the image aspect).
    pub fn set_width(&mut self, width: Option<Pixels>) {
        self.width = width;
        // Caller responsible for marking the node layout-dirty.
    }

    /// Sets the forced logical height (`None` to derive from the image aspect).
    pub fn set_height(&mut self, height: Option<Pixels>) {
        self.height = height;
        // Caller responsible for marking the node layout-dirty.
    }

    /// Sets the image-pixels-per-logical-pixel scale. A non-finite or
    /// non-positive value is rejected (the previous scale is kept) because it
    /// would make the logical aspect source NaN or zero.
    pub fn set_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
        // Caller responsible for marking the node layout-dirty.
    }

    /// Computes the destination rectangle for the image content within the
    /// laid-out box, applying the fit mode (scaling) and alignment
    /// (positioning).
    ///
    /// Returns `None` when the intrinsic size is degenerate (zero in either
    /// dimension), in which case there is nothing to paint.
    fn compute_paint_rect(&self) -> Option<Rect> {
        self.paint_rect_in(self.size)
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
        // Logical image size (Flutter `ImageInfo.scale`): the fit math operates
        // on the same `intrinsic / scale` dimensions the box was laid out
        // against, so a high-DPI asset paints at its logical size — without the
        // divide, `ImageFit::None`/`ScaleDown` would draw a 2x asset at its full
        // pixel size and overflow its laid-out box.
        let iw = self.intrinsic_size.width.get() / self.scale;
        let ih = self.intrinsic_size.height.get() / self.scale;
        if iw <= 0.0 || ih <= 0.0 {
            return None;
        }

        let bw = box_size.width.get();
        let bh = box_size.height.get();

        // Determine the painted (scaled) size of the image content.
        let (pw, ph) = match self.fit {
            ImageFit::Fill => (bw, bh),
            ImageFit::Contain => {
                let scale = (bw / iw).min(bh / ih);
                (iw * scale, ih * scale)
            }
            ImageFit::Cover => {
                let scale = (bw / iw).max(bh / ih);
                (iw * scale, ih * scale)
            }
            ImageFit::ScaleDown => {
                // Like Contain but never enlarge.
                let scale = (bw / iw).min(bh / ih).min(1.0);
                (iw * scale, ih * scale)
            }
            ImageFit::None => (iw, ih),
        };

        let painted = Size::new(Pixels::new(pw), Pixels::new(ph));
        let origin = self.alignment.offset(painted, box_size);
        Some(Rect::from_origin_size(
            Point::new(origin.dx, origin.dy),
            painted,
        ))
    }

    /// Computes the box size for the given constraints — a direct port of
    /// Flutter's `RenderImage._sizeForConstraints` (`image.dart`).
    ///
    /// The box size is **independent of [`fit`](ImageFit)**: the explicit
    /// `width`/`height` are folded into the constraints, then the box takes the
    /// largest size that fits while preserving the image's aspect ratio
    /// (intrinsic size divided by `scale`). `fit` only governs where the image
    /// is drawn inside this box, in [`Self::paint_rect_in`].
    ///
    /// The logical aspect source is `intrinsic_size / scale`; when it is
    /// degenerate (zero in either dimension) the box falls back to the
    /// constraints' smallest size, mirroring Flutter's null-image branch.
    ///
    /// Public so that callers can reproduce layout sizing without driving a
    /// full layout pass (tests, demos).
    pub fn compute_size(&self, constraints: &BoxConstraints) -> Size {
        // Fold the explicit width/height into the constraints so all three are
        // treated uniformly (Flutter `tightFor(w, h).enforce(constraints)`).
        // `tighten` clamps each forced dimension INTO the parent's range, so a
        // forced size outside the parent's min/max can never commit a size that
        // violates the incoming constraints.
        let folded = constraints.tighten(self.width, self.height);

        let aspect = Size::new(
            Pixels::new(self.intrinsic_size.width.get() / self.scale),
            Pixels::new(self.intrinsic_size.height.get() / self.scale),
        );
        if aspect.width.get() <= 0.0 || aspect.height.get() <= 0.0 {
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
                .map(|w| format!("{w:?}"))
                .unwrap_or_else(|| "unset".to_string()),
        );
        properties.add(
            "height",
            self.height
                .map(|h| format!("{h:?}"))
                .unwrap_or_else(|| "unset".to_string()),
        );
        properties.add_default_double("scale", self.scale, 1.0, None);
        properties.add_enum("fit", self.fit);
        properties.add_enum("alignment", self.alignment);
    }
}

impl RenderBox for RenderImage {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let size = self.compute_size(ctx.constraints());
        self.size = size;
        size
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Leaf>) {
        // Nothing to draw without a source image.
        let Some(image) = self.image.as_ref() else {
            return;
        };
        // Apply fit + alignment to obtain the destination rect in local
        // coordinates (the recorder pre-translates to this node's origin).
        if let Some(dst) = self.compute_paint_rect() {
            ctx.canvas().draw_image(image.clone(), dst, None);
        }
    }

    // Intrinsics mirror Flutter `RenderImage` (`image.dart`): the size the box
    // would take with the cross-axis extent tightened. With no forced
    // width/height the min-intrinsics report 0 (the image can scale to nothing),
    // while max-intrinsics always report the aspect-preserved extent. A leaf,
    // so the `_ctx` child channel is unused.

    fn compute_min_intrinsic_width(
        &self,
        height: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if self.width.is_none() && self.height.is_none() {
            return 0.0;
        }
        self.compute_size(&BoxConstraints::tight_for_finite(
            Pixels::INFINITY,
            Pixels::new(height),
        ))
        .width
        .get()
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        self.compute_size(&BoxConstraints::tight_for_finite(
            Pixels::INFINITY,
            Pixels::new(height),
        ))
        .width
        .get()
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if self.width.is_none() && self.height.is_none() {
            return 0.0;
        }
        self.compute_size(&BoxConstraints::tight_for_finite(
            Pixels::new(width),
            Pixels::INFINITY,
        ))
        .height
        .get()
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f32,
        _ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        self.compute_size(&BoxConstraints::tight_for_finite(
            Pixels::new(width),
            Pixels::INFINITY,
        ))
        .height
        .get()
    }

    /// Dry layout is the exact box size `perform_layout` commits — both go
    /// through `compute_size` (Flutter `computeDryLayout == _sizeForConstraints`).
    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.compute_size(&constraints)
    }
}

impl PaintEffectsCapability for RenderImage {}
impl SemanticsCapability for RenderImage {}
impl HotReloadCapability for RenderImage {}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_render_image_creation() {
        let intrinsic = Size::new(px(100.0), px(200.0));
        let image = RenderImage::new(intrinsic, ImageFit::Contain, ImageAlignment::Center);

        assert_eq!(image.intrinsic_size, intrinsic);
        assert_eq!(image.fit, ImageFit::Contain);
        assert_eq!(image.alignment, ImageAlignment::Center);
        assert_eq!(image.size(), &Size::ZERO);
    }

    #[test]
    fn test_image_fit_contain_shrinks_width() {
        let image = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        let constraints = BoxConstraints {
            min_width: Pixels::ZERO,
            max_width: px(100.0),
            min_height: Pixels::ZERO,
            max_height: px(100.0),
        };

        let computed = image.compute_size(&constraints);
        // Original 2:1 aspect ratio (200x100)
        // Max 100x100 → should shrink to 100x50 (maintains ratio, fits in height)
        assert!(computed.width <= constraints.max_width);
        assert!(computed.height <= constraints.max_height);
        // Check aspect ratio preserved: width/height = 200/100 = 2/1
        let expected_height = computed.width * 0.5; // height = width / 2
        assert!((computed.height.get() - expected_height.get()).abs() < 0.01);
    }

    #[test]
    fn test_image_fit_fill_stretches() {
        let image = RenderImage::new(
            Size::new(px(100.0), px(100.0)),
            ImageFit::Fill,
            ImageAlignment::Center,
        );
        let constraints = BoxConstraints {
            min_width: px(200.0),
            max_width: px(200.0),
            min_height: px(150.0),
            max_height: px(150.0),
        };

        let computed = image.compute_size(&constraints);
        assert_eq!(computed.width, px(200.0));
        assert_eq!(computed.height, px(150.0));
    }

    #[test]
    fn test_image_fit_none_constrains() {
        let image = RenderImage::new(
            Size::new(px(50.0), px(50.0)),
            ImageFit::None,
            ImageAlignment::Center,
        );
        let constraints = BoxConstraints {
            min_width: Pixels::ZERO,
            max_width: px(100.0),
            min_height: Pixels::ZERO,
            max_height: px(100.0),
        };

        let computed = image.compute_size(&constraints);
        // None fit: show at natural size (50x50), which fits in constraints
        assert_eq!(computed.width, px(50.0));
        assert_eq!(computed.height, px(50.0));
    }

    #[test]
    fn test_image_alignment_center_offset() {
        let alignment = ImageAlignment::Center;
        let image_size = Size::new(px(50.0), px(50.0));
        let container_size = Size::new(px(100.0), px(100.0));

        let offset = alignment.offset(image_size, container_size);
        // Center alignment should place image at (25, 25) in container
        assert_eq!(offset.dx, px(25.0));
        assert_eq!(offset.dy, px(25.0));
    }

    #[test]
    fn test_image_alignment_top_left() {
        let alignment = ImageAlignment::TopLeft;
        let image_size = Size::new(px(50.0), px(50.0));
        let container_size = Size::new(px(100.0), px(100.0));

        let offset = alignment.offset(image_size, container_size);
        assert_eq!(offset.dx, Pixels::ZERO);
        assert_eq!(offset.dy, Pixels::ZERO);
    }

    fn test_image_2x2() -> Image {
        // 2x2 RGBA image (16 bytes), all opaque white.
        Image::from_rgba8(2, 2, vec![255; 2 * 2 * 4])
    }

    #[test]
    fn test_from_image_derives_intrinsic_size() {
        let image =
            RenderImage::from_image(test_image_2x2(), ImageFit::Contain, ImageAlignment::Center);
        assert_eq!(image.intrinsic_size, Size::new(px(2.0), px(2.0)));
        assert!(image.image().is_some());
    }

    #[test]
    fn test_set_image_updates_intrinsic_size() {
        let mut image = RenderImage::new(
            Size::new(px(10.0), px(10.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        assert!(image.image().is_none());

        image.set_image(Some(test_image_2x2()));
        assert_eq!(image.intrinsic_size, Size::new(px(2.0), px(2.0)));
        assert!(image.image().is_some());

        image.set_image(None);
        assert!(image.image().is_none());
        // Clearing the image leaves the last intrinsic size unchanged.
        assert_eq!(image.intrinsic_size, Size::new(px(2.0), px(2.0)));
    }

    #[test]
    fn test_compute_paint_rect_contain_centers() {
        // Intrinsic 2:1 (200x100) in a 100x100 box → contain gives 100x50
        // centered vertically at y=25.
        let mut image = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        image.size = Size::new(px(100.0), px(100.0));

        let rect = image.compute_paint_rect().expect("paint rect");
        assert_eq!(rect.size().width, px(100.0));
        assert_eq!(rect.size().height, px(50.0));
        assert_eq!(rect.origin().x, Pixels::ZERO);
        assert_eq!(rect.origin().y, px(25.0));
    }

    #[test]
    fn test_compute_paint_rect_fill_matches_box() {
        let mut image = RenderImage::new(
            Size::new(px(50.0), px(50.0)),
            ImageFit::Fill,
            ImageAlignment::TopLeft,
        );
        image.size = Size::new(px(120.0), px(80.0));

        let rect = image.compute_paint_rect().expect("paint rect");
        assert_eq!(rect.size().width, px(120.0));
        assert_eq!(rect.size().height, px(80.0));
        assert_eq!(rect.origin().x, Pixels::ZERO);
        assert_eq!(rect.origin().y, Pixels::ZERO);
    }

    #[test]
    fn test_compute_paint_rect_cover_overflows_box() {
        // Intrinsic 1:2 (100x200) covered into 100x100 box → scale by max
        // (1.0 vs 0.5) = 1.0, painted 100x200, overflowing height (cropped).
        let mut image = RenderImage::new(
            Size::new(px(100.0), px(200.0)),
            ImageFit::Cover,
            ImageAlignment::Center,
        );
        image.size = Size::new(px(100.0), px(100.0));

        let rect = image.compute_paint_rect().expect("paint rect");
        assert_eq!(rect.size().width, px(100.0));
        assert_eq!(rect.size().height, px(200.0));
        // Centered vertically → origin y = (100 - 200)/2 = -50 (crop top/bottom)
        assert_eq!(rect.origin().y, px(-50.0));
    }

    #[test]
    fn test_compute_paint_rect_scale_down_never_enlarges() {
        // Small 10x10 image in a big 100x100 box → ScaleDown keeps 10x10.
        let mut image = RenderImage::new(
            Size::new(px(10.0), px(10.0)),
            ImageFit::ScaleDown,
            ImageAlignment::TopLeft,
        );
        image.size = Size::new(px(100.0), px(100.0));

        let rect = image.compute_paint_rect().expect("paint rect");
        assert_eq!(rect.size().width, px(10.0));
        assert_eq!(rect.size().height, px(10.0));
    }

    #[test]
    fn test_compute_paint_rect_zero_intrinsic_is_none() {
        let mut image = RenderImage::new(
            Size::new(px(0.0), px(50.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        image.size = Size::new(px(100.0), px(100.0));
        assert!(image.compute_paint_rect().is_none());
    }

    // ===== Paint pipeline integration (drives the real paint() method) =====

    use crate::context::{FragmentRecorder, PaintCx};
    use flui_painting::{DisplayListCore, DrawCommand};
    use flui_types::Offset;

    /// Runs `paint()` through a real FragmentRecorder and returns the
    /// recorded DrawImage commands (image byte_count, dst rect).
    fn capture_draw_images(image: &RenderImage) -> Vec<(usize, Rect)> {
        let mut rec = FragmentRecorder::new(Offset::ZERO, 1.0);
        {
            let mut cx = PaintCx::<Leaf>::new(&mut rec, 0);
            image.paint(&mut cx);
        }
        let frag = rec.finish();
        let mut out = Vec::new();
        for op in &frag.ops {
            if let crate::context::FragmentOp::Run(list) = op {
                for cmd in list.commands() {
                    if let DrawCommand::DrawImage { image, dst, .. } = cmd {
                        out.push((image.byte_count(), *dst));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn test_paint_without_image_records_nothing() {
        let mut image = RenderImage::new(
            Size::new(px(10.0), px(10.0)),
            ImageFit::Fill,
            ImageAlignment::Center,
        );
        image.size = Size::new(px(100.0), px(100.0));
        // No source image set → paint() should be a no-op.
        assert!(capture_draw_images(&image).is_empty());
    }

    #[test]
    fn test_paint_with_image_records_draw_image_with_fit_rect() {
        // 2x2 image, Contain into a 100x50 box (intrinsic 2:2 = 1:1).
        // Contain scale = min(100/2, 50/2) = 25 → painted 50x50.
        // Center alignment → origin x = (100-50)/2 = 25, y = (50-50)/2 = 0.
        let mut image =
            RenderImage::from_image(test_image_2x2(), ImageFit::Contain, ImageAlignment::Center);
        image.size = Size::new(px(100.0), px(50.0));

        let draws = capture_draw_images(&image);
        assert_eq!(draws.len(), 1, "expected exactly one DrawImage command");

        let (byte_count, dst) = draws[0];
        assert_eq!(byte_count, 2 * 2 * 4, "2x2 RGBA = 16 bytes");
        assert_eq!(dst.size().width, px(50.0));
        assert_eq!(dst.size().height, px(50.0));
        assert_eq!(dst.origin().x, px(25.0));
        assert_eq!(dst.origin().y, Pixels::ZERO);
    }

    #[test]
    fn test_paint_fill_covers_whole_box() {
        let mut image =
            RenderImage::from_image(test_image_2x2(), ImageFit::Fill, ImageAlignment::TopLeft);
        image.size = Size::new(px(120.0), px(80.0));

        let draws = capture_draw_images(&image);
        assert_eq!(draws.len(), 1);
        let (_, dst) = draws[0];
        assert_eq!(dst.origin().x, Pixels::ZERO);
        assert_eq!(dst.origin().y, Pixels::ZERO);
        assert_eq!(dst.size().width, px(120.0));
        assert_eq!(dst.size().height, px(80.0));
    }

    // ===== width / height / scale folding + intrinsics + dry layout =====

    use crate::context::intrinsics_test_support::{leaf_dry_layout, leaf_intrinsics};

    #[test]
    fn forced_width_tightens_box_and_preserves_aspect() {
        // 1:1 intrinsic, forced logical width 40, otherwise unconstrained → 40x40.
        let mut img = RenderImage::new(
            Size::new(px(4.0), px(4.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        img.set_width(Some(px(40.0)));
        assert_eq!(
            img.compute_size(&BoxConstraints::UNCONSTRAINED),
            Size::new(px(40.0), px(40.0)),
        );
    }

    #[test]
    fn scale_divides_the_aspect_source() {
        // 200x100 intrinsic at scale 2 → logical aspect source 100x50.
        let mut img = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::Fill,
            ImageAlignment::Center,
        );
        img.set_scale(2.0);
        assert_eq!(
            img.compute_size(&BoxConstraints::UNCONSTRAINED),
            Size::new(px(100.0), px(50.0)),
        );
    }

    #[test]
    fn box_size_is_independent_of_fit() {
        // Flutter `_sizeForConstraints` never reads `fit`: every fit mode under
        // the same constraints + intrinsic must produce the same box.
        let constraints = BoxConstraints {
            min_width: Pixels::ZERO,
            max_width: px(80.0),
            min_height: Pixels::ZERO,
            max_height: px(80.0),
        };
        let intrinsic = Size::new(px(200.0), px(100.0)); // 2:1 → 80x40 in an 80² box
        for fit in [
            ImageFit::Fill,
            ImageFit::Contain,
            ImageFit::Cover,
            ImageFit::ScaleDown,
            ImageFit::None,
        ] {
            let img = RenderImage::new(intrinsic, fit, ImageAlignment::Center);
            assert_eq!(
                img.compute_size(&constraints),
                Size::new(px(80.0), px(40.0)),
                "fit {fit:?} must not affect the box size",
            );
        }
    }

    #[test]
    fn intrinsics_report_aspect_extent() {
        let img = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        // Max-intrinsics under an unbounded cross extent = the aspect source.
        assert_eq!(
            leaf_intrinsics(|c| img.compute_max_intrinsic_width(f32::INFINITY, c)),
            200.0,
        );
        assert_eq!(
            leaf_intrinsics(|c| img.compute_max_intrinsic_height(f32::INFINITY, c)),
            100.0,
        );
        // Min-intrinsics are 0 with no forced width/height (the image can scale
        // down to nothing).
        assert_eq!(
            leaf_intrinsics(|c| img.compute_min_intrinsic_width(f32::INFINITY, c)),
            0.0,
        );
        assert_eq!(
            leaf_intrinsics(|c| img.compute_min_intrinsic_height(f32::INFINITY, c)),
            0.0,
        );
    }

    #[test]
    fn forced_width_drives_min_intrinsic_and_dry_layout_matches_layout() {
        let mut img = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        img.set_width(Some(px(80.0)));
        // A forced width makes the min-intrinsic-width report it (80).
        assert_eq!(
            leaf_intrinsics(|c| img.compute_min_intrinsic_width(f32::INFINITY, c)),
            80.0,
        );
        // Dry layout equals the size perform_layout commits for the same constraints.
        let constraints = BoxConstraints::UNCONSTRAINED;
        let dry = leaf_dry_layout(|c| img.compute_dry_layout(constraints, c));
        assert_eq!(dry, img.compute_size(&constraints));
        assert_eq!(dry, Size::new(px(80.0), px(40.0)));
    }

    #[test]
    fn forced_width_below_parent_minimum_respects_the_parent() {
        // Forced width 40, but the parent demands min_width 50: the committed
        // size must not drop below the parent's minimum.
        let mut img = RenderImage::new(
            Size::new(px(4.0), px(4.0)),
            ImageFit::Contain,
            ImageAlignment::Center,
        );
        img.set_width(Some(px(40.0)));
        let constraints = BoxConstraints {
            min_width: px(50.0),
            max_width: px(200.0),
            min_height: Pixels::ZERO,
            max_height: px(200.0),
        };
        let size = img.compute_size(&constraints);
        assert!(
            size.width >= px(50.0),
            "forced width {} must not violate the parent minimum 50",
            size.width.get(),
        );
        assert!(
            constraints.is_satisfied_by(size),
            "size {size:?} must satisfy parent"
        );
    }

    #[test]
    fn scale_shrinks_the_painted_size() {
        // 200x100 asset at scale 2 → logical 100x50. In an oversized box,
        // ImageFit::None paints at the logical size, not the raw pixel size.
        let mut img = RenderImage::new(
            Size::new(px(200.0), px(100.0)),
            ImageFit::None,
            ImageAlignment::TopLeft,
        );
        img.set_scale(2.0);
        let rect = img
            .paint_rect_in(Size::new(px(400.0), px(400.0)))
            .expect("paint rect");
        assert_eq!(rect.size().width, px(100.0));
        assert_eq!(rect.size().height, px(50.0));
    }
}
