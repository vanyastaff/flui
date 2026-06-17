//! Image, atlas, and external-texture record methods: texture, draw_image,
//! draw_image_repeat, draw_image_nine_slice, draw_image_filtered, draw_atlas,
//! draw_texture.
//!
//! # Borrow seam
//!
//! These methods receive disjoint `WgpuPainter` fields as plain borrowed
//! parameters; see `batches/mod.rs` for the seam contract.
//!
//! | Method(s)                                    | Resources borrow         |
//! |----------------------------------------------|--------------------------|
//! | `texture`, `draw_texture`                    | `&ExternalTextureRegistry` |
//! | `draw_image`, `draw_atlas`                   | `&mut TextureCache`      |
//! | `draw_image_repeat`, `draw_image_nine_slice` | `&mut TextureCache` (delegate to `draw_image`) |
//! | `draw_image_filtered`                        | `&mut TextureCache` + `&mut Vec<DrawItem>` + `opacity: f32` (inner `rect`/`draw_image` calls) |
//!
//! # Invariants preserved
//!
//! - `cached_images` entries are `(TextureId, TextureInstance, ScissorRect)`;
//!   `external_images` entries are `(wgpu::TextureView, TextureInstance,
//!   ScissorRect)`.  Tuple layout and scissor-at-draw-time capture are unchanged.
//! - The `draw_image_repeat`/`draw_image_nine_slice` → `draw_image` delegation
//!   produces identical per-tile/per-region calls; loop bounds and dst rects are
//!   byte-identical to the painter originals.
//! - `draw_image_filtered`'s branch selection (overlay/CPU-recolor vs rect path)
//!   is unchanged; the inner `rect` call receives the same `opacity` the shim
//!   reads once from `compositor.current_opacity()`.
//! - `texture_batch` is **not touched** by any method here — it remains a
//!   painter field for T10.

use flui_painting::Paint;
use flui_types::{
    Offset, Point, Rect,
    geometry::{Pixels, px},
    painting::Image,
};

use super::{
    super::{
        command_ir::{DrawItem, DrawSegment},
        external_texture_registry::ExternalTextureRegistry,
        state_stack::GpuStateStack,
        texture_cache::TextureCache,
    },
    DrawBatcher,
};

// GPU rendering routinely converts between f32/u8/u32 for pixel coordinates,
// color channels, and buffer indices. These truncations are intentional.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
impl DrawBatcher {
    /// Record an external texture draw by ID into `external_images`.
    ///
    /// Looks up the texture in `registry`; if not found, warns and returns
    /// without recording anything (preserving the original painter behavior).
    pub(in super::super) fn texture(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        registry: &ExternalTextureRegistry,
        texture_id: flui_types::painting::TextureId,
        dst_rect: Rect<Pixels>,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "DrawBatcher::texture: id={:?}, dst_rect={:?}",
            texture_id,
            dst_rect
        );

        // Look up texture in external texture registry and capture the view
        // BEFORE building the instance so we can move `view` into the segment.
        let view = if let Some(entry) = registry.get(texture_id) {
            #[cfg(debug_assertions)]
            tracing::trace!(
                "DrawBatcher::texture: found {:?} ({}x{}, frame={})",
                texture_id,
                entry.width,
                entry.height,
                entry.frame_count
            );
            entry.view.clone()
        } else {
            #[cfg(debug_assertions)]
            tracing::warn!(
                "DrawBatcher::texture: texture {:?} not found in registry",
                texture_id
            );
            return;
        };

        // Apply transform to rect.
        let top_left = state.apply_transform(Point::new(dst_rect.left(), dst_rect.top()));
        let bottom_right = state.apply_transform(Point::new(dst_rect.right(), dst_rect.bottom()));
        let transformed_rect =
            Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        // Create texture instance (full UV mapping, no rotation, white tint).
        let instance = super::super::instancing::TextureInstance::new(
            transformed_rect,
            flui_types::Color::WHITE,
        );

        // Route through per-segment external_images (flushed by flush_segment_external_images
        // which calls flush_texture_batch per entry with the correct view bound).
        segment
            .external_images
            .push((view, instance, state.current_scissor()));
    }

    /// Record an image draw by uploading (or retrieving from cache) its RGBA
    /// pixels into the texture cache, then pushing a `cached_images` entry.
    pub(in super::super) fn draw_image(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        texture_cache: &mut TextureCache,
        image: &Image,
        dst_rect: Rect<Pixels>,
    ) {
        let top_left = state.apply_transform(Point::new(dst_rect.left(), dst_rect.top()));
        let bottom_right = state.apply_transform(Point::new(dst_rect.right(), dst_rect.bottom()));
        let transformed_rect =
            Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

        // Use Arc pointer identity for O(1) cache lookup instead of hashing all pixels.
        let texture_id = super::super::texture_cache::TextureId::from_ptr(image.data_ptr());
        let data = image.data();

        // Load or get cached texture (small images are auto-packed into the atlas).
        match texture_cache.load_from_rgba(texture_id.clone(), image.width(), image.height(), data)
        {
            Ok(cached_texture) => {
                // Preserve atlas UVs when the image is packed into the shared atlas.
                let instance = if let Some(uv_rect) = cached_texture.uv_rect {
                    super::super::instancing::TextureInstance::with_uv(
                        transformed_rect,
                        uv_rect,
                        flui_types::styling::Color::WHITE,
                    )
                } else {
                    super::super::instancing::TextureInstance::new(
                        transformed_rect,
                        flui_types::styling::Color::WHITE,
                    )
                };

                // Keep cached image draws in segment order for correct layer compositing.
                // Capture the active scissor so flush_segment_cached_images can clip
                // images that live inside a clip_rect region.
                segment
                    .cached_images
                    .push((texture_id, instance, state.current_scissor()));
            }
            Err(e) => {
                tracing::error!("Failed to load image texture: {}", e);
            }
        }
    }

    /// Record a tiled image draw by delegating to `draw_image` for each tile.
    ///
    /// Loop bounds and dst rects are byte-identical to the original painter
    /// implementation.
    pub(in super::super) fn draw_image_repeat(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        texture_cache: &mut TextureCache,
        image: &Image,
        dst: Rect<Pixels>,
        repeat: flui_painting::display_list::ImageRepeat,
    ) {
        use flui_painting::display_list::ImageRepeat;

        let img_w = image.width() as f32;
        let img_h = image.height() as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return;
        }

        match repeat {
            ImageRepeat::NoRepeat => {
                // Single draw, no tiling.
                Self::draw_image(segment, state, texture_cache, image, dst);
            }
            ImageRepeat::Repeat => {
                // Tile in both directions.
                let mut y = dst.top().0;
                while y < dst.bottom().0 {
                    let mut x = dst.left().0;
                    while x < dst.right().0 {
                        let tile_w = img_w.min(dst.right().0 - x);
                        let tile_h = img_h.min(dst.bottom().0 - y);
                        let tile_dst = Rect::from_xywh(px(x), px(y), px(tile_w), px(tile_h));
                        Self::draw_image(segment, state, texture_cache, image, tile_dst);
                        x += img_w;
                    }
                    y += img_h;
                }
            }
            ImageRepeat::RepeatX => {
                // Tile only horizontally.
                let tile_h = img_h.min(dst.height().0);
                let mut x = dst.left().0;
                while x < dst.right().0 {
                    let tile_w = img_w.min(dst.right().0 - x);
                    let tile_dst = Rect::from_xywh(px(x), dst.top(), px(tile_w), px(tile_h));
                    Self::draw_image(segment, state, texture_cache, image, tile_dst);
                    x += img_w;
                }
            }
            ImageRepeat::RepeatY => {
                // Tile only vertically.
                let tile_w = img_w.min(dst.width().0);
                let mut y = dst.top().0;
                while y < dst.bottom().0 {
                    let tile_h = img_h.min(dst.bottom().0 - y);
                    let tile_dst = Rect::from_xywh(dst.left(), px(y), px(tile_w), px(tile_h));
                    Self::draw_image(segment, state, texture_cache, image, tile_dst);
                    y += img_h;
                }
            }
        }
    }

    /// Record a nine-slice image draw by extracting and delegating each of the
    /// nine sub-image regions to `draw_image`.
    ///
    /// Sub-image extraction and slice boundaries are byte-identical to the
    /// original painter implementation.
    #[allow(
        clippy::type_complexity,
        reason = "nine-slice src/dst tuple layout is local detail; refactoring into a named type adds no clarity"
    )]
    pub(in super::super) fn draw_image_nine_slice(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        texture_cache: &mut TextureCache,
        image: &Image,
        center_slice: Rect<Pixels>,
        dst: Rect<Pixels>,
    ) {
        let img_w = image.width() as f32;
        let img_h = image.height() as f32;
        if img_w <= 0.0 || img_h <= 0.0 {
            return;
        }

        // Slice boundaries in image space.
        let sl = center_slice.left().0;
        let st = center_slice.top().0;
        let sr = center_slice.right().0;
        let sb = center_slice.bottom().0;

        // Destination boundaries.
        let dl = dst.left().0;
        let dt = dst.top().0;
        let dr = dst.right().0;
        let db = dst.bottom().0;

        // Inner destination boundaries (corners keep their pixel size).
        let d_inner_left = dl + sl;
        let d_inner_top = dt + st;
        let d_inner_right = dr - (img_w - sr);
        let d_inner_bottom = db - (img_h - sb);

        // Clamp: if dst is too small, inner edges collapse.
        let d_inner_left = d_inner_left.min(dr);
        let d_inner_top = d_inner_top.min(db);
        let d_inner_right = d_inner_right.max(d_inner_left);
        let d_inner_bottom = d_inner_bottom.max(d_inner_top);

        // Helper: draw a sub-image region to a destination rect.
        // Since draw_image draws the full image into dst_rect, we use it per-slice.
        // For a proper 9-slice we'd need draw_image_src_dst (src rect -> dst rect).
        // As a pragmatic v1, we draw the full image scaled into each 9 region
        // using the existing draw_image, which stretches the whole image.
        //
        // For correct 9-slice, we create sub-images from the pixel data.
        let data = image.data();
        let stride = (img_w as u32) * 4;

        // Extract a sub-region of the image as a new Image.
        let extract = |sx: f32, sy: f32, sw: f32, sh: f32| -> Option<Image> {
            let sx = sx.max(0.0) as u32;
            let sy = sy.max(0.0) as u32;
            let sw = sw.max(0.0) as u32;
            let sh = sh.max(0.0) as u32;
            if sw == 0 || sh == 0 {
                return None;
            }
            let mut sub = Vec::with_capacity((sw * sh * 4) as usize);
            for row in sy..(sy + sh) {
                let start = (row * stride + sx * 4) as usize;
                let end = start + (sw * 4) as usize;
                if end <= data.len() {
                    sub.extend_from_slice(&data[start..end]);
                }
            }
            if sub.len() == (sw * sh * 4) as usize {
                Some(Image::from_rgba8(sw, sh, sub))
            } else {
                None
            }
        };

        // 9 slices: (src_x, src_y, src_w, src_h) -> dst rect.
        let slices: [(f32, f32, f32, f32, f32, f32, f32, f32); 9] = [
            // Top-left corner
            (
                0.0,
                0.0,
                sl,
                st,
                dl,
                dt,
                d_inner_left - dl,
                d_inner_top - dt,
            ),
            // Top center
            (
                sl,
                0.0,
                sr - sl,
                st,
                d_inner_left,
                dt,
                d_inner_right - d_inner_left,
                d_inner_top - dt,
            ),
            // Top-right corner
            (
                sr,
                0.0,
                img_w - sr,
                st,
                d_inner_right,
                dt,
                dr - d_inner_right,
                d_inner_top - dt,
            ),
            // Middle-left
            (
                0.0,
                st,
                sl,
                sb - st,
                dl,
                d_inner_top,
                d_inner_left - dl,
                d_inner_bottom - d_inner_top,
            ),
            // Center
            (
                sl,
                st,
                sr - sl,
                sb - st,
                d_inner_left,
                d_inner_top,
                d_inner_right - d_inner_left,
                d_inner_bottom - d_inner_top,
            ),
            // Middle-right
            (
                sr,
                st,
                img_w - sr,
                sb - st,
                d_inner_right,
                d_inner_top,
                dr - d_inner_right,
                d_inner_bottom - d_inner_top,
            ),
            // Bottom-left corner
            (
                0.0,
                sb,
                sl,
                img_h - sb,
                dl,
                d_inner_bottom,
                d_inner_left - dl,
                db - d_inner_bottom,
            ),
            // Bottom center
            (
                sl,
                sb,
                sr - sl,
                img_h - sb,
                d_inner_left,
                d_inner_bottom,
                d_inner_right - d_inner_left,
                db - d_inner_bottom,
            ),
            // Bottom-right corner
            (
                sr,
                sb,
                img_w - sr,
                img_h - sb,
                d_inner_right,
                d_inner_bottom,
                dr - d_inner_right,
                db - d_inner_bottom,
            ),
        ];

        for (sx, sy, sw, sh, dx, dy, dw, dh) in slices {
            if dw <= 0.0 || dh <= 0.0 || sw <= 0.0 || sh <= 0.0 {
                continue;
            }
            if let Some(sub_image) = extract(sx, sy, sw, sh) {
                let tile_dst = Rect::from_xywh(px(dx), px(dy), px(dw), px(dh));
                Self::draw_image(segment, state, texture_cache, &sub_image, tile_dst);
            }
        }
    }

    /// Record a color-filtered image draw.
    ///
    /// The `Mode` branch draws the image then overlays a tinted rect; the rect
    /// call requires `&mut draw_order` and `opacity` (same seam as
    /// `WgpuPainter::rect`). The CPU-recolor branches (`Matrix`,
    /// `LinearToSrgbGamma`, `SrgbToLinearGamma`) apply the filter on CPU and
    /// delegate to `draw_image`.
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/draw_order/state/texture_cache/opacity are disjoint \
                  WgpuPainter fields passed as separate borrows; merging them into a context struct \
                  defeats the T9a borrow split"
    )]
    #[allow(
        clippy::many_single_char_names,
        reason = "w/h/r/g/b/a are idiomatic in CPU-side color-matrix pixel loops"
    )]
    pub(in super::super) fn draw_image_filtered(
        &mut self,
        segment: &mut DrawSegment,
        draw_order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        texture_cache: &mut TextureCache,
        opacity: f32,
        image: &Image,
        dst: Rect<Pixels>,
        filter: flui_painting::display_list::ColorFilter,
    ) {
        use flui_painting::display_list::ColorFilter;

        match filter {
            ColorFilter::Mode {
                color,
                blend_mode: _,
            } => {
                // Pragmatic v1: draw image then overlay a tinted rect.
                // First draw the image normally.
                Self::draw_image(segment, state, texture_cache, image, dst);

                // Then overlay with the tint color using a semi-transparent rect.
                let tint_paint = Paint {
                    color: color.with_alpha(color.a / 2),
                    style: flui_painting::PaintStyle::Fill,
                    ..Default::default()
                };
                self.rect(segment, draw_order, state, opacity, dst, &tint_paint);

                tracing::debug!(
                    "draw_image_filtered: Mode filter applied as color overlay (color={:?})",
                    color
                );
            }
            ColorFilter::Matrix(matrix) => {
                // Apply color matrix to image pixel data on CPU.
                let data = image.data();
                let w = image.width();
                let h = image.height();
                let mut new_data = Vec::with_capacity(data.len());

                for pixel in data.chunks_exact(4) {
                    let r = f32::from(pixel[0]) / 255.0;
                    let g = f32::from(pixel[1]) / 255.0;
                    let b = f32::from(pixel[2]) / 255.0;
                    let a = f32::from(pixel[3]) / 255.0;

                    let nr =
                        (matrix[0] * r + matrix[1] * g + matrix[2] * b + matrix[3] * a + matrix[4])
                            .clamp(0.0, 1.0);
                    let ng =
                        (matrix[5] * r + matrix[6] * g + matrix[7] * b + matrix[8] * a + matrix[9])
                            .clamp(0.0, 1.0);
                    let nb = (matrix[10] * r
                        + matrix[11] * g
                        + matrix[12] * b
                        + matrix[13] * a
                        + matrix[14])
                        .clamp(0.0, 1.0);
                    let na = (matrix[15] * r
                        + matrix[16] * g
                        + matrix[17] * b
                        + matrix[18] * a
                        + matrix[19])
                        .clamp(0.0, 1.0);

                    new_data.push((nr * 255.0) as u8);
                    new_data.push((ng * 255.0) as u8);
                    new_data.push((nb * 255.0) as u8);
                    new_data.push((na * 255.0) as u8);
                }

                let filtered = Image::from_rgba8(w, h, new_data);
                Self::draw_image(segment, state, texture_cache, &filtered, dst);

                tracing::debug!("draw_image_filtered: Matrix filter applied via CPU");
            }
            ColorFilter::LinearToSrgbGamma => {
                // Apply linear-to-sRGB gamma correction on CPU.
                let data = image.data();
                let w = image.width();
                let h = image.height();
                let mut new_data = Vec::with_capacity(data.len());

                for pixel in data.chunks_exact(4) {
                    for &ch in &pixel[..3] {
                        let linear = f32::from(ch) / 255.0;
                        let srgb = if linear <= 0.003_130_8 {
                            linear * 12.92
                        } else {
                            1.055 * linear.powf(1.0 / 2.4) - 0.055
                        };
                        new_data.push((srgb.clamp(0.0, 1.0) * 255.0) as u8);
                    }
                    new_data.push(pixel[3]); // Alpha unchanged.
                }

                let filtered = Image::from_rgba8(w, h, new_data);
                Self::draw_image(segment, state, texture_cache, &filtered, dst);

                tracing::debug!("draw_image_filtered: LinearToSrgbGamma applied via CPU");
            }
            ColorFilter::SrgbToLinearGamma => {
                // Apply sRGB-to-linear gamma correction on CPU.
                let data = image.data();
                let w = image.width();
                let h = image.height();
                let mut new_data = Vec::with_capacity(data.len());

                for pixel in data.chunks_exact(4) {
                    for &ch in &pixel[..3] {
                        let srgb = f32::from(ch) / 255.0;
                        let linear = if srgb <= 0.04045 {
                            srgb / 12.92
                        } else {
                            ((srgb + 0.055) / 1.055).powf(2.4)
                        };
                        new_data.push((linear.clamp(0.0, 1.0) * 255.0) as u8);
                    }
                    new_data.push(pixel[3]); // Alpha unchanged.
                }

                let filtered = Image::from_rgba8(w, h, new_data);
                Self::draw_image(segment, state, texture_cache, &filtered, dst);

                tracing::debug!("draw_image_filtered: SrgbToLinearGamma applied via CPU");
            }
        }
    }

    /// Record a sprite atlas draw: loads the atlas image once, then pushes one
    /// `cached_images` entry per sprite with the sprite's UV sub-rect and tint.
    ///
    /// `sprite_origins` are the per-sprite translation offsets in pixel space,
    /// already extracted from any transform matrices at the trait-boundary caller.
    /// The batcher is glam-only; `Matrix4` must not appear on this side of the seam.
    pub(in super::super) fn draw_atlas(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        texture_cache: &mut TextureCache,
        image: &Image,
        sprites: &[Rect<Pixels>],
        sprite_origins: &[Offset<Pixels>],
        colors: Option<&[flui_types::styling::Color]>,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "DrawBatcher::draw_atlas: image={}x{}, sprites={}",
            image.width(),
            image.height(),
            sprites.len()
        );

        // Validate input.
        if sprites.len() != sprite_origins.len() {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawAtlas: sprite count ({}) doesn't match origin count ({})",
                sprites.len(),
                sprite_origins.len()
            );
            return;
        }

        if let Some(colors_arr) = colors
            && colors_arr.len() != sprites.len()
        {
            #[cfg(debug_assertions)]
            tracing::error!(
                "DrawAtlas: color count ({}) doesn't match sprite count ({})",
                colors_arr.len(),
                sprites.len()
            );
            return;
        }

        // Use Arc pointer identity for O(1) cache lookup instead of hashing all pixels.
        // Clone the id: `load_from_rgba` takes ownership, but we need the same key
        // for per-sprite `cached_images` pushes in the success branch below.
        let texture_id = super::super::texture_cache::TextureId::from_ptr(image.data_ptr());
        let cache_id = texture_id.clone();

        match texture_cache.load_from_rgba(texture_id, image.width(), image.height(), image.data())
        {
            Ok(_cached_texture) => {
                let image_width = image.width() as f32;
                let image_height = image.height() as f32;

                // Create texture instances for each sprite.
                for (i, (sprite_rect, origin)) in
                    sprites.iter().zip(sprite_origins.iter()).enumerate()
                {
                    // Get color tint for this sprite (default to white).
                    let tint = colors
                        .and_then(|c| c.get(i))
                        .copied()
                        .unwrap_or(flui_types::styling::Color::WHITE);

                    // Calculate UV coordinates from sprite rect.
                    let src_uv = [
                        (sprite_rect.left() / image_width).0,
                        (sprite_rect.top() / image_height).0,
                        (sprite_rect.right() / image_width).0,
                        (sprite_rect.bottom() / image_height).0,
                    ];

                    let dst_rect = Rect::from_xywh(
                        origin.dx,
                        origin.dy,
                        sprite_rect.width(),
                        sprite_rect.height(),
                    );

                    // Create texture instance and route through cached_images so it is
                    // flushed by flush_segment_cached_images (not the orphaned texture_batch).
                    let instance =
                        super::super::instancing::TextureInstance::with_uv(dst_rect, src_uv, tint);
                    segment.cached_images.push((
                        cache_id.clone(),
                        instance,
                        state.current_scissor(),
                    ));
                }
            }
            Err(e) => {
                tracing::error!("Failed to load atlas texture: {}", e);
            }
        }
    }

    /// Record a draw from an external (platform-registered) texture by ID, with
    /// optional source UV sub-rect, filter quality hint, and opacity.
    ///
    /// - `src` rect is normalized to the texture dimensions to produce UV
    ///   coordinates; `None` means full texture (`[0,0,1,1]`).
    /// - `opacity` is baked into the tint color alpha.
    /// - `_filter_quality` is accepted for API compatibility but currently unused
    ///   (the sampler is determined at pipeline level, not per-draw).
    #[allow(
        clippy::too_many_arguments,
        reason = "borrow-seam design: segment/state/registry are disjoint WgpuPainter fields; \
                  src/filter_quality/opacity are distinct per-draw parameters with no natural grouping"
    )]
    pub(in super::super) fn draw_texture(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        registry: &ExternalTextureRegistry,
        texture_id: flui_types::painting::TextureId,
        dst: Rect<Pixels>,
        src: Option<Rect<Pixels>>,
        _filter_quality: flui_types::painting::FilterQuality,
        opacity: f32,
    ) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "DrawBatcher::draw_texture: id={}, dst={:?}, src={:?}, opacity={}",
            texture_id.get(),
            dst,
            src,
            opacity
        );

        // Look up the external texture in the registry.
        if let Some(entry) = registry.get(texture_id) {
            // Calculate UV coordinates from source rect.
            let src_uv = if let Some(src_rect) = src {
                // Normalize to texture dimensions.
                let tex_width = entry.width as f32;
                let tex_height = entry.height as f32;
                [
                    (src_rect.left() / tex_width).0,
                    (src_rect.top() / tex_height).0,
                    (src_rect.right() / tex_width).0,
                    (src_rect.bottom() / tex_height).0,
                ]
            } else {
                // Full texture.
                [0.0, 0.0, 1.0, 1.0]
            };

            // Apply opacity via tint color alpha.
            let tint = flui_types::styling::Color::rgba(255, 255, 255, (opacity * 255.0) as u8);

            // Apply the current transform to dst corners (translation + scale; rotation
            // collapses to AABB — same accepted limitation as `texture()` and `draw_image`).
            let top_left = state.apply_transform(Point::new(dst.left(), dst.top()));
            let bottom_right = state.apply_transform(Point::new(dst.right(), dst.bottom()));
            let transformed_dst =
                Rect::from_ltrb(top_left.x, top_left.y, bottom_right.x, bottom_right.y);

            let instance =
                super::super::instancing::TextureInstance::with_uv(transformed_dst, src_uv, tint);
            // Clone the registry view so this segment owns it independently of
            // the registry lifetime.
            let view = entry.view.clone();

            #[cfg(debug_assertions)]
            tracing::trace!(
                "External texture {} found: {}x{}, frame={}",
                texture_id.get(),
                entry.width,
                entry.height,
                entry.frame_count
            );

            // Push into per-segment external_images; flushed in flush_segment
            // via flush_segment_external_images which binds each view via
            // flush_texture_batch — identical to the cached-image path.
            segment
                .external_images
                .push((view, instance, state.current_scissor()));
        } else {
            // Texture not registered — warn and skip.  Rendering an invisible
            // placeholder via `texture_batch` (the old code) would orphan the
            // instance because texture_batch is never drained by flush_segment.
            tracing::warn!(
                "External texture {} not registered — skipping draw_texture",
                texture_id.get()
            );
        }
    }
}
