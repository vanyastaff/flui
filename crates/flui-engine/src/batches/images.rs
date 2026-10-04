//! Image recording uses one source-region/affine-quad contract for decoded
//! images, repeats, slices and sprites. Quota admission precedes publication.
use super::{
    super::{
        command_ir::{AdvancedShapeOp, DrawItem, DrawRun, DrawSegment},
        state_stack::GpuStateStack,
        texture_cache::{TextureCache, TextureKey},
    },
    DrawBatcher,
};
use flui_foundation::geometry::{Matrix4, Rect};
use flui_painting::{
    BlendMode, Paint,
    paint::{ColorFilter, Image, ImageRepeat},
    styling::Color,
};

#[derive(Clone)]
struct TileAxis {
    current: f64,
    end: f64,
    step: f64,
    repeat: bool,
}
impl TileAxis {
    fn new(start: f64, end: f64, origin: f64, step: f64, repeat: bool) -> Option<Self> {
        if ![start, end, origin, step, end - start]
            .into_iter()
            .all(f64::is_finite)
            || end <= start
            || step <= 0.0
        {
            return None;
        }
        let current = if repeat {
            origin + ((start - origin) / step).floor() * step
        } else {
            origin
        };
        if !current.is_finite() || (repeat && (current > start || start - current >= step)) {
            return None;
        }
        Some(Self {
            current,
            end,
            step,
            repeat,
        })
    }
}
impl Iterator for TileAxis {
    type Item = Result<(f64, f64), ()>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }
        let start = self.current;
        let end = (start + self.step).min(self.end);
        if end <= start || !end.is_finite() {
            self.current = self.end;
            return Some(Err(()));
        }
        self.current = if self.repeat { end } else { self.end };
        Some(Ok((start, end)))
    }
}

struct ImageQuad {
    origin: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
    bounds: Rect<f64>,
}
impl ImageQuad {
    fn new(matrix: Matrix4, dst: Rect<f64>) -> Option<Self> {
        if !valid_affine(matrix) || !valid_rect(dst) {
            return None;
        }
        let map = |x, y| {
            let (x, y) = matrix.transform_point(x, y);
            [x, y]
        };
        let corners = [
            map(dst.left(), dst.top()),
            map(dst.right(), dst.top()),
            map(dst.right(), dst.bottom()),
            map(dst.left(), dst.bottom()),
        ];
        let origin = corners[0];
        let x = [corners[1][0] - origin[0], corners[1][1] - origin[1]];
        let y = [corners[3][0] - origin[0], corners[3][1] - origin[1]];
        if !corners
            .into_iter()
            .flatten()
            .chain(x)
            .chain(y)
            .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX))
        {
            return None;
        }
        let origin = origin.map(|v| v as f32);
        let x = x.map(|v| v as f32);
        let y = y.map(|v| v as f32);
        if f64::from(x[0]) * f64::from(y[1]) - f64::from(x[1]) * f64::from(y[0]) == 0.0 {
            return None;
        }
        // The replay crop follows the narrowed quad sent to the GPU.
        let narrowed = [
            origin,
            [origin[0] + x[0], origin[1] + x[1]],
            [origin[0] + x[0] + y[0], origin[1] + x[1] + y[1]],
            [origin[0] + y[0], origin[1] + y[1]],
        ];
        if !narrowed.into_iter().flatten().all(f32::is_finite) {
            return None;
        }
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for [x, y] in narrowed {
            let x = f64::from(x);
            let y = f64::from(y);
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        }
        Some(Self {
            origin,
            x,
            y,
            bounds: Rect::from_ltrb(bounds[0], bounds[1], bounds[2], bounds[3]),
        })
    }
    fn instance(
        &self,
        uv: [f32; 4],
        original_image_uv: [f32; 4],
        tint: Color,
        state: &GpuStateStack,
    ) -> crate::instancing::TextureInstance {
        state
            .apply_active_clip(
                crate::instancing::TextureInstance::with_uv(self.bounds, uv, tint)
                    .with_quad(self.origin, self.x, self.y)
                    .with_original_image_uv(original_image_uv),
            )
            .with_linear_straight_source()
    }
}
fn valid_affine(matrix: Matrix4) -> bool {
    matrix.m.into_iter().all(f64::is_finite)
        && matrix.m[3] == 0.0
        && matrix.m[7] == 0.0
        && matrix.m[11] == 0.0
        && matrix.m[15] == 1.0
}
fn valid_rect(rect: Rect<f64>) -> bool {
    [
        rect.left(),
        rect.top(),
        rect.right(),
        rect.bottom(),
        rect.width(),
        rect.height(),
    ]
    .into_iter()
    .all(f64::is_finite)
        && rect.width() > 0.0
        && rect.height() > 0.0
}
fn full_source(image: &Image) -> Rect<f64> {
    Rect::from_xywh(
        0.0,
        0.0,
        f64::from(image.width()),
        f64::from(image.height()),
    )
}
fn union(bounds: &mut Option<Rect<f64>>, next: Rect<f64>) {
    *bounds = Some(bounds.map_or(next, |old| {
        Rect::from_ltrb(
            old.left().min(next.left()),
            old.top().min(next.top()),
            old.right().max(next.right()),
            old.bottom().max(next.bottom()),
        )
    }));
}
fn source_uv(src: Rect<f64>, image: &Image, atlas: [f32; 4]) -> Option<[f32; 4]> {
    if !valid_rect(src)
        || src.left() < 0.0
        || src.top() < 0.0
        || src.right() > f64::from(image.width())
        || src.bottom() > f64::from(image.height())
    {
        return None;
    }
    let [u0, v0, u1, v1] = atlas;
    Some([
        u0 + (src.left() / f64::from(image.width())) as f32 * (u1 - u0),
        v0 + (src.top() / f64::from(image.height())) as f32 * (v1 - v0),
        u0 + (src.right() / f64::from(image.width())) as f32 * (u1 - u0),
        v0 + (src.bottom() / f64::from(image.height())) as f32 * (v1 - v0),
    ])
}
fn append_image(
    segment: &mut DrawSegment,
    key: TextureKey,
    instance: crate::instancing::TextureInstance,
    state: &GpuStateStack,
    mode: BlendMode,
) -> bool {
    segment.cached_images.push((
        key,
        instance,
        state.current_scissor(),
        if mode.is_advanced() {
            BlendMode::SrcOver
        } else {
            mode
        },
    ));
    segment.record_run(
        DrawRun::CachedImage(
            segment.cached_images.len().saturating_sub(1)..segment.cached_images.len(),
        ),
        state.clip_chain(),
    );
    segment.recording_result().is_ok()
}
fn publish(
    parent: &mut DrawSegment,
    order: &mut Vec<DrawItem>,
    content: DrawSegment,
    bounds: Option<Rect<f64>>,
    mode: BlendMode,
) {
    let Some(device_bounds) = bounds else {
        return;
    };
    DrawBatcher::finish_current_segment(parent, order);
    if mode.is_advanced() {
        order.push(DrawItem::AdvancedShape(AdvancedShapeOp {
            segment: content.seal(),
            mode,
            device_bounds,
        }));
    } else {
        order.push(DrawItem::Segment(content.seal()));
    }
}

impl DrawBatcher {
    #[expect(
        clippy::too_many_arguments,
        reason = "Image recording borrows disjoint frame resources and a complete region operation"
    )]
    pub(in super::super) fn draw_image_region(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        src: Rect<f64>,
        dst: Rect<f64>,
        tile: Option<Rect<f64>>,
        repeat: ImageRepeat,
        filter: Option<ColorFilter>,
        paint: Option<&Paint>,
    ) {
        if segment.recording_result().is_err() {
            return;
        }
        let matrix = state.current_transform_matrix();
        // Validate the region before uploading or admitting any prefix.
        if source_uv(src, image, [0.0, 0.0, 1.0, 1.0]).is_none()
            || !valid_affine(matrix)
            || !valid_rect(dst)
        {
            return;
        }
        let filtered = filter.map(|filter| filtered_image(image, filter));
        let image = filtered.as_ref().unwrap_or(image);
        let key = if filtered.is_some() {
            TextureKey::from_data(image.width(), image.height(), image.data())
        } else {
            TextureKey::from_image(image)
        };
        let mode = paint.map_or(BlendMode::SrcOver, |paint| paint.blend_mode);
        let tint = paint.map_or(Color::WHITE, |paint| paint.color);
        let atlas =
            match cache.load_from_rgba(key.clone(), image.width(), image.height(), image.data()) {
                Ok(cached) => cached.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]),
                Err(error) => {
                    tracing::error!("Failed to load image texture: {error}");
                    return;
                }
            };
        if repeat == ImageRepeat::NoRepeat && !mode.is_advanced() {
            let Some(quad) = ImageQuad::new(matrix, dst) else {
                return;
            };
            let Some(uv) = source_uv(src, image, atlas) else {
                return;
            };
            // A single admitted quad has no unpublished prefix. Keep compatible
            // consecutive images in the current segment's texture batch.
            append_image(
                segment,
                key,
                quad.instance(uv, atlas, tint, state),
                state,
                mode,
            );
            return;
        }
        let mut content = segment.empty_sibling();
        let mut bounds = None;
        if repeat == ImageRepeat::NoRepeat {
            let Some(quad) = ImageQuad::new(matrix, dst) else {
                return;
            };
            let Some(uv) = source_uv(src, image, atlas) else {
                return;
            };
            if !append_image(
                &mut content,
                key,
                quad.instance(uv, atlas, tint, state),
                state,
                mode,
            ) {
                return;
            }
            union(&mut bounds, quad.bounds);
        } else {
            let fitted = tile.unwrap_or_else(|| {
                Rect::from_xywh(dst.left(), dst.top(), src.width(), src.height())
            });
            if !valid_rect(fitted) {
                return;
            }
            let Some(columns) = TileAxis::new(
                dst.left(),
                dst.right(),
                fitted.left(),
                fitted.width(),
                repeat != ImageRepeat::RepeatY,
            ) else {
                return;
            };
            let Some(rows) = TileAxis::new(
                dst.top(),
                dst.bottom(),
                fitted.top(),
                fitted.height(),
                repeat != ImageRepeat::RepeatX,
            ) else {
                return;
            };
            for row in rows {
                let Ok((top, bottom)) = row else {
                    return;
                };
                for column in columns.clone() {
                    let Ok((left, right)) = column else {
                        return;
                    };
                    let clipped = Rect::from_ltrb(
                        left.max(dst.left()),
                        top.max(dst.top()),
                        right.min(dst.right()),
                        bottom.min(dst.bottom()),
                    );
                    if !valid_rect(clipped) {
                        continue;
                    }
                    let Some(quad) = ImageQuad::new(matrix, clipped) else {
                        return;
                    };
                    // First and last tiles retain their natural source phase.
                    let map_x = |value: f64| {
                        (src.left() + (value - left) / fitted.width() * src.width())
                            .clamp(src.left(), src.right())
                    };
                    let map_y = |value: f64| {
                        (src.top() + (value - top) / fitted.height() * src.height())
                            .clamp(src.top(), src.bottom())
                    };
                    let tile_src = Rect::from_ltrb(
                        map_x(clipped.left()),
                        map_y(clipped.top()),
                        map_x(clipped.right()),
                        map_y(clipped.bottom()),
                    );
                    let Some(uv) = source_uv(tile_src, image, atlas) else {
                        return;
                    };
                    if !append_image(
                        &mut content,
                        key.clone(),
                        quad.instance(uv, atlas, tint, state),
                        state,
                        mode,
                    ) {
                        return;
                    }
                    union(&mut bounds, quad.bounds);
                }
            }
        }
        publish(segment, order, content, bounds, mode);
    }
    pub(in super::super) fn draw_image(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        dst: Rect<f64>,
        mode: BlendMode,
    ) {
        let paint = Paint::fill(Color::WHITE).with_blend_mode(mode);
        Self::draw_image_region(
            segment,
            order,
            state,
            cache,
            image,
            full_source(image),
            dst,
            None,
            ImageRepeat::NoRepeat,
            None,
            Some(&paint),
        );
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Legacy repeat convenience delegates to the complete region operation"
    )]
    pub(in super::super) fn draw_image_repeat(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        dst: Rect<f64>,
        repeat: ImageRepeat,
        mode: BlendMode,
    ) {
        let paint = Paint::fill(Color::WHITE).with_blend_mode(mode);
        Self::draw_image_region(
            segment,
            order,
            state,
            cache,
            image,
            full_source(image),
            dst,
            None,
            repeat,
            None,
            Some(&paint),
        );
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Legacy filtered convenience delegates to the complete region operation"
    )]
    pub(in super::super) fn draw_image_filtered(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        dst: Rect<f64>,
        filter: ColorFilter,
        mode: BlendMode,
    ) {
        let paint = Paint::fill(Color::WHITE).with_blend_mode(mode);
        Self::draw_image_region(
            segment,
            order,
            state,
            cache,
            image,
            full_source(image),
            dst,
            None,
            ImageRepeat::NoRepeat,
            Some(filter),
            Some(&paint),
        );
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Slices share one source cache and unpublished operation"
    )]
    pub(in super::super) fn draw_image_nine_slice(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        centre: Rect<f64>,
        dst: Rect<f64>,
        mode: BlendMode,
    ) {
        Self::draw_image_nine_slice_painted(
            segment,
            order,
            state,
            cache,
            image,
            centre,
            dst,
            Some(&Paint::fill(Color::WHITE).with_blend_mode(mode)),
        );
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Slices share one source cache and unpublished operation"
    )]
    pub(in super::super) fn draw_image_nine_slice_painted(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        centre: Rect<f64>,
        dst: Rect<f64>,
        paint: Option<&Paint>,
    ) {
        if segment.recording_result().is_err()
            || !valid_rect(dst)
            || source_uv(centre, image, [0.0, 0.0, 1.0, 1.0]).is_none()
        {
            return;
        }
        let mode = paint.map_or(BlendMode::SrcOver, |p| p.blend_mode);
        let tint = paint.map_or(Color::WHITE, |p| p.color);
        let key = TextureKey::from_image(image);
        let atlas =
            match cache.load_from_rgba(key.clone(), image.width(), image.height(), image.data()) {
                Ok(c) => c.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]),
                Err(error) => {
                    tracing::error!("Failed to load slice texture: {error}");
                    return;
                }
            };
        let xs = [0.0, centre.left(), centre.right(), f64::from(image.width())];
        let ys = [
            0.0,
            centre.top(),
            centre.bottom(),
            f64::from(image.height()),
        ];
        let inner_left = (dst.left() + centre.left()).min(dst.right());
        let inner_top = (dst.top() + centre.top()).min(dst.bottom());
        let dx = [
            dst.left(),
            inner_left,
            (dst.right() - (f64::from(image.width()) - centre.right())).max(inner_left),
            dst.right(),
        ];
        let dy = [
            dst.top(),
            inner_top,
            (dst.bottom() - (f64::from(image.height()) - centre.bottom())).max(inner_top),
            dst.bottom(),
        ];
        let mut content = segment.empty_sibling();
        let mut bounds = None;
        for row in 0..3 {
            for col in 0..3 {
                let tile = Rect::from_ltrb(dx[col], dy[row], dx[col + 1], dy[row + 1]);
                let src = Rect::from_ltrb(xs[col], ys[row], xs[col + 1], ys[row + 1]);
                if !valid_rect(tile) || !valid_rect(src) {
                    continue;
                }
                let Some(quad) = ImageQuad::new(state.current_transform_matrix(), tile) else {
                    return;
                };
                let Some(uv) = source_uv(src, image, atlas) else {
                    return;
                };
                if !append_image(
                    &mut content,
                    key.clone(),
                    quad.instance(uv, atlas, tint, state),
                    state,
                    mode,
                ) {
                    return;
                }
                union(&mut bounds, quad.bounds);
            }
        }
        publish(segment, order, content, bounds, mode);
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Sprites share one source cache and complete per-sprite affine transforms"
    )]
    pub(in super::super) fn draw_atlas(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        sprites: &[Rect<f64>],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        mode: BlendMode,
    ) {
        Self::draw_atlas_painted(
            segment, order, state, cache, image, sprites, transforms, colors, mode, None,
        );
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "Sprites share one source cache and complete per-sprite affine transforms"
    )]
    pub(in super::super) fn draw_atlas_painted(
        segment: &mut DrawSegment,
        order: &mut Vec<DrawItem>,
        state: &GpuStateStack,
        cache: &mut TextureCache,
        image: &Image,
        sprites: &[Rect<f64>],
        transforms: &[Matrix4],
        colors: Option<&[Color]>,
        mode: BlendMode,
        paint: Option<&Paint>,
    ) {
        if segment.recording_result().is_err()
            || sprites.len() != transforms.len()
            || colors.is_some_and(|c| c.len() != sprites.len())
        {
            return;
        }
        let key = TextureKey::from_image(image);
        let atlas =
            match cache.load_from_rgba(key.clone(), image.width(), image.height(), image.data()) {
                Ok(c) => c.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]),
                Err(error) => {
                    tracing::error!("Failed to load atlas image: {error}");
                    return;
                }
            };
        let mut content = segment.empty_sibling();
        let mut bounds = None;
        for (index, (src, transform)) in sprites.iter().zip(transforms).enumerate() {
            let matrix = state.current_transform_matrix() * *transform;
            let Some(quad) =
                ImageQuad::new(matrix, Rect::from_xywh(0.0, 0.0, src.width(), src.height()))
            else {
                return;
            };
            let Some(uv) = source_uv(*src, image, atlas) else {
                return;
            };
            let tint = colors
                .and_then(|c| c.get(index))
                .copied()
                .unwrap_or(Color::WHITE);
            let tint = paint.map_or(tint, |p| {
                Color::rgba(
                    ((u16::from(tint.r) * u16::from(p.color.r) + 127) / 255) as u8,
                    ((u16::from(tint.g) * u16::from(p.color.g) + 127) / 255) as u8,
                    ((u16::from(tint.b) * u16::from(p.color.b) + 127) / 255) as u8,
                    ((u16::from(tint.a) * u16::from(p.color.a) + 127) / 255) as u8,
                )
            });
            if !append_image(
                &mut content,
                key.clone(),
                quad.instance(uv, atlas, tint, state),
                state,
                mode,
            ) {
                return;
            }
            union(&mut bounds, quad.bounds);
        }
        publish(segment, order, content, bounds, mode);
    }
    /// Record an already resolved allocation and effective sampling policy.
    pub(in super::super) fn draw_texture(
        segment: &mut DrawSegment,
        state: &GpuStateStack,
        lease: crate::external_texture_registry::ExternalAllocationLease,
        sampling: crate::external_texture_registry::ExternalSampling,
        dst: Rect<f64>,
        src: Option<Rect<f64>>,
        opacity: f32,
    ) {
        if segment.recording_result().is_err() {
            return;
        }
        let (width, height) = lease.size();
        let src_uv = src.map_or([0.0, 0.0, 1.0, 1.0], |rect| {
            [
                rect.left() / f64::from(width),
                rect.top() / f64::from(height),
                rect.right() / f64::from(width),
                rect.bottom() / f64::from(height),
            ]
        });
        let tint = match lease.descriptor().alpha {
            crate::external_texture_registry::ExternalAlpha::Premultiplied => [opacity; 4],
            _ => [1.0, 1.0, 1.0, opacity],
        };
        let Some(quad) = ImageQuad::new(state.current_transform_matrix(), dst) else {
            segment.record_external_error(crate::error::ExternalTextureError::InvalidDestination);
            return;
        };
        let instance = state.apply_active_clip(
            crate::instancing::TextureInstance::with_uv_tint_f32(
                quad.bounds,
                src_uv.map(|v| v as f32),
                tint,
            )
            .with_quad(quad.origin, quad.x, quad.y),
        );
        let instance = match (lease.descriptor().alpha, sampling) {
            (crate::external_texture_registry::ExternalAlpha::Opaque, _) => {
                instance.with_opaque_source()
            }
            (
                crate::external_texture_registry::ExternalAlpha::Straight,
                crate::external_texture_registry::ExternalSampling::Linear,
            ) => instance.with_linear_straight_source(),
            _ => instance,
        };
        segment
            .external_images
            .push((lease, instance, state.current_scissor(), sampling));
        segment.record_run(
            DrawRun::ExternalImage(
                segment.external_images.len().saturating_sub(1)..segment.external_images.len(),
            ),
            state.clip_chain(),
        );
    }
}

/// Filters decoded straight channels before tinting and premultiplication.
fn filtered_image(image: &Image, filter: ColorFilter) -> Image {
    let mut data = Vec::with_capacity(image.data().len());
    for pixel in image.data().as_chunks::<4>().0 {
        match filter {
            ColorFilter::Mode { color, blend_mode } => {
                let value = color.blend(
                    Color::rgba(pixel[0], pixel[1], pixel[2], pixel[3]),
                    blend_mode,
                );
                data.extend_from_slice(&[value.r, value.g, value.b, value.a]);
            }
            ColorFilter::Matrix(matrix) => {
                let channels = pixel.map(|v| f32::from(v) / 255.0);
                for row in matrix.values.as_chunks::<5>().0 {
                    let value = (row[0] * channels[0]
                        + row[1] * channels[1]
                        + row[2] * channels[2]
                        + row[3] * channels[3]
                        + row[4])
                        .clamp(0.0, 1.0);
                    data.push((value * 255.0) as u8);
                }
            }
            ColorFilter::LinearToSrgbGamma | ColorFilter::SrgbToLinearGamma => {
                for channel in &pixel[..3] {
                    let value = f32::from(*channel) / 255.0;
                    let value = match filter {
                        ColorFilter::LinearToSrgbGamma if value <= 0.003_130_8 => value * 12.92,
                        ColorFilter::LinearToSrgbGamma => 1.055 * value.powf(1.0 / 2.4) - 0.055,
                        _ if value <= 0.04045 => value / 12.92,
                        _ => ((value + 0.055) / 1.055).powf(2.4),
                    };
                    data.push((value.clamp(0.0, 1.0) * 255.0) as u8);
                }
                data.push(pixel[3]);
            }
            _ => return image.clone(),
        }
    }
    Image::from_rgba8(image.width(), image.height(), data)
}
