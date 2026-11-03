//! Ordered backdrop reads belong to the current attachment, including nested groups.
use std::sync::Arc;

use flui_foundation::geometry::Rect;
use flui_painting::paint::{BlendMode, ImageFilter, ImageSampling, ImageTransform};

use super::WgpuPainter;
use crate::{
    clip_chain::ClipOp,
    clip_geometry::{ValidatedAffine, ValidatedClip},
    command_ir::{BackdropEffect, BackdropOp, DrawItem, DrawSegment, RecordError, ResampleFilter},
    device_domain::PreparedCost,
    error::{EngineError, EngineResult, GeometryError},
    pipeline_set::PipelineSet,
    render_target::RenderTarget,
    replay::GpuReplay,
    resources::GpuResources,
    texture_pool::PooledTexture,
};

impl WgpuPainter {
    /// Record a backdrop read at this position, before recording its children.
    pub(crate) fn record_backdrop_filter(
        &mut self,
        bounds: Rect<f64>,
        filter: &ImageFilter,
        blend: BlendMode,
        group: Option<&flui_layer::BackdropGroupKey>,
    ) -> EngineResult<()> {
        self.current_segment.recording_result()?;
        let result = self.record_backdrop_filter_impl(bounds, filter, blend, group);
        if let Err(error) = &result {
            let fault = match error {
                EngineError::InvalidGeometry(error) => Some(RecordError::Geometry(error.clone())),
                EngineError::UnsupportedBackdropFilter { reason } => {
                    Some(RecordError::Backdrop(reason))
                }
                EngineError::PreparedResourceLimit {
                    resource,
                    requested,
                    limit,
                } => Some(RecordError::Limit {
                    resource,
                    requested: *requested,
                    limit: *limit,
                }),
                _ => None,
            };
            if let Some(fault) = fault {
                self.current_segment.budget.record_error(fault);
            }
        }
        result
    }

    fn record_backdrop_filter_impl(
        &mut self,
        bounds: Rect<f64>,
        filter: &ImageFilter,
        blend: BlendMode,
        group: Option<&flui_layer::BackdropGroupKey>,
    ) -> EngineResult<()> {
        let matrix = self.current_transform_matrix().m;
        let affine = ValidatedAffine::try_new(glam::DMat4::from_cols_array(&matrix))?;
        let shape = ValidatedClip::rect(bounds)?;
        let Some(output_bounds) = affine.map_bounds(bounds)? else {
            return Ok(());
        };
        if output_bounds.is_empty() || !output_bounds.intersects(&self.viewport_bounds()) {
            return Ok(());
        }
        if !matches!(
            self.surface_format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Err(EngineError::UnsupportedBackdropFilter {
                reason: "a backdrop filter requires encoded SDR RGBA8/BGRA8 Unorm",
            });
        }
        let effect = match filter {
            ImageFilter::Blur { sigma_x, sigma_y } => blur_effect(*sigma_x, *sigma_y, &matrix)?,
            ImageFilter::Transform {
                transform,
                sampling,
            } => resample_effect(transform, *sampling, &matrix)?,
            _ => {
                return Err(EngineError::UnsupportedBackdropFilter {
                    reason: "a backdrop filter is a Gaussian blur or a transform",
                });
            }
        };
        let mut clip = self.captured_group_clip();
        clip.chain = match clip.chain.append(
            shape,
            affine,
            ClipOp::Intersect,
            true,
            &self.current_segment.budget,
        ) {
            Ok(chain) => chain,
            Err(error) => {
                self.current_segment.budget.record_error(error);
                return self.current_segment.recording_result();
            }
        };
        if !self
            .current_segment
            .budget
            .charge(std::mem::size_of::<BackdropOp>(), 1)
        {
            return self.current_segment.recording_result();
        }
        // Reserve the ordering slot before sealing/mutating the draw stream.
        if self.draw_order.try_reserve(2).is_err() {
            self.current_segment
                .budget
                .release(std::mem::size_of::<BackdropOp>(), 1);
            return Err(EngineError::PreparedResourceLimit {
                resource: "backdrop ordering allocation",
                requested: usize::MAX,
                limit: 128 * 1024 * 1024,
            });
        }
        let context = DrawSegment::with_budget(Arc::clone(&self.current_segment.budget)).seal();
        let op = BackdropOp {
            context,
            clip,
            scissor: self.state.current_scissor(),
            output_bounds,
            effect,
            blend,
            group: group.cloned(),
        };
        self.finish_current_segment();
        self.draw_order.push(DrawItem::Backdrop(op));
        Ok(())
    }
}

/// A Gaussian backdrop blur under the 4x4 column-major `matrix`, as device
/// sigmas.
fn blur_effect(sigma_x: f64, sigma_y: f64, matrix: &[f64; 16]) -> EngineResult<BackdropEffect> {
    if !sigma_x.is_finite() || !sigma_y.is_finite() || sigma_x < 0.0 || sigma_y < 0.0 {
        return Err(EngineError::UnsupportedBackdropFilter {
            reason: "Gaussian sigma must be finite and nonnegative",
        });
    }
    // Covariance remains diagonal only for these axis-preserving transforms.
    // General rotation/shear requires directional kernels, not AABB sigma.
    // Public rotation matrices retain sin/cos roundoff at quarter turns.
    // Compare within each column so an unrelated large scale cannot hide
    // meaningful shear in the other column.
    let sigma = if axis_roundoff(matrix[1], matrix[0]) && axis_roundoff(matrix[4], matrix[5]) {
        [sigma_x * matrix[0].abs(), sigma_y * matrix[5].abs()]
    } else if axis_roundoff(matrix[0], matrix[1]) && axis_roundoff(matrix[5], matrix[4]) {
        [sigma_y * matrix[4].abs(), sigma_x * matrix[1].abs()]
    } else {
        return Err(EngineError::UnsupportedBackdropFilter {
            reason: "Gaussian backdrop blur requires an axis-preserving transform",
        });
    };
    let packed_sigma = sigma.map(|value| value as f32);
    if sigma.iter().zip(packed_sigma).any(|(original, value)| {
        !value.is_finite() || value > 1_048_576.0 || (*original != 0.0 && value == 0.0)
    }) {
        return Err(GeometryError::Unrepresentable {
            context: "backdrop Gaussian sigma",
        }
        .into());
    }
    Ok(BackdropEffect::Blur(packed_sigma))
}

/// A backdrop seen through `transform` (in the layer's local space) under
/// the 4x4 column-major `matrix`, as a map from a device output pixel to
/// the device backdrop pixel it shows.
///
/// The device map is `matrix · transform⁻¹ · matrix⁻¹`, any affine map:
/// rotation and shear resample like scale. A sampling without a lowering
/// fails the frame rather than drawing the backdrop untransformed.
fn resample_effect(
    transform: &ImageTransform,
    sampling: ImageSampling,
    matrix: &[f64; 16],
) -> EngineResult<BackdropEffect> {
    let filter = match sampling {
        ImageSampling::Nearest => ResampleFilter::Nearest,
        ImageSampling::Linear => ResampleFilter::Linear,
        _ => {
            return Err(EngineError::UnsupportedBackdropFilter {
                reason: "a backdrop transform has no lowering for this sampling",
            });
        }
    };
    let device = glam::DAffine2::from_cols_array(&[
        matrix[0], matrix[1], matrix[4], matrix[5], matrix[12], matrix[13],
    ]);
    let shown_to_source = glam::DAffine2::from_cols_array(&transform.inverse_coefficients());
    let map = device * shown_to_source * device.inverse();
    let [xx, yx, xy, yy, x0, y0] = map.to_cols_array();
    if ![xx, yx, xy, yy, x0, y0]
        .iter()
        .all(|value| value.is_finite())
    {
        return Err(GeometryError::NonFinite {
            context: "backdrop transform",
        }
        .into());
    }
    Ok(BackdropEffect::Resample { map, filter })
}

/// The bounding box of `rect` mapped through `map`.
fn mapped_bounds(map: glam::DAffine2, rect: Rect<f64>) -> Rect<f64> {
    let corners = [
        (rect.left(), rect.top()),
        (rect.right(), rect.top()),
        (rect.left(), rect.bottom()),
        (rect.right(), rect.bottom()),
    ]
    .map(|(x, y)| map.transform_point2(glam::DVec2::new(x, y)));
    let low = corners
        .iter()
        .fold(glam::DVec2::INFINITY, |low, &corner| low.min(corner));
    let high = corners
        .iter()
        .fold(glam::DVec2::NEG_INFINITY, |high, &corner| high.max(corner));
    Rect::from_ltrb(low.x, low.y, high.x, high.y)
}

/// Whether `off_axis` is only rounding beside `on_axis`.
fn axis_roundoff(off_axis: f64, on_axis: f64) -> bool {
    off_axis.abs() <= 8.0 * f64::EPSILON * on_axis.abs()
}

impl ResampleFilter {
    /// How far past a sample point the texels it reads reach: half a texel
    /// for a bilinear read, none for a nearest one.
    fn reach(self) -> f64 {
        match self {
            Self::Nearest => 0.0,
            Self::Linear => 0.5,
        }
    }

    /// The texels one output pixel reads.
    fn taps(self) -> usize {
        match self {
            Self::Nearest => 1,
            Self::Linear => 4,
        }
    }

    fn filter_mode(self) -> wgpu::FilterMode {
        match self {
            Self::Nearest => wgpu::FilterMode::Nearest,
            Self::Linear => wgpu::FilterMode::Linear,
        }
    }
}

impl GpuReplay {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn replay_backdrop(
        &mut self,
        op: BackdropOp,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        snapshots: &mut BackdropSnapshots,
    ) -> EngineResult<()> {
        let (root_x, root_y) = (
            self.attachment_origin.0 as f64,
            self.attachment_origin.1 as f64,
        );
        let viewport = Rect::from_xywh(
            root_x,
            root_y,
            f64::from(viewport_size.0),
            f64::from(viewport_size.1),
        );
        let Some(mut output) = op
            .output_bounds
            .intersect(&viewport)
            .filter(|rect| !rect.is_empty())
        else {
            return Ok(());
        };
        if let Some(bound) = op.clip.chain.root_bounds()? {
            let Some(cropped) = output.intersect(&bound).filter(|rect| !rect.is_empty()) else {
                return Ok(());
            };
            output = cropped;
        }
        if let Some((x, y, width, height)) = op.scissor {
            let writes = Rect::from_xywh(x as f64, y as f64, f64::from(width), f64::from(height));
            let Some(cropped) = output.intersect(&writes).filter(|rect| !rect.is_empty()) else {
                return Ok(());
            };
            output = cropped;
        }
        let backing = target
            .texture
            .ok_or(EngineError::CompositeBackdropUnavailable)?;
        if backing.dimension() != wgpu::TextureDimension::D2
            || backing.depth_or_array_layers() != 1
            || backing.sample_count() != 1
            || backing.format() != surface_format
            || !backing.usage().contains(wgpu::TextureUsages::COPY_SRC)
        {
            return Err(EngineError::InvalidRenderTarget {
                reason: "backdrop requires a matching single-sample COPY_SRC attachment",
            });
        }
        let attachment_size = (backing.width(), backing.height());
        // During resize the acquired image can still have the preceding extent.
        // Copy and output use attachment texels; never stretch their correspondence
        // through the painter's newer viewport uniform.
        let readable = Rect::from_xywh(
            root_x,
            root_y,
            f64::from(viewport_size.0.min(attachment_size.0)),
            f64::from(viewport_size.1.min(attachment_size.1)),
        );
        // A grouped filter reads the backdrop its group captured on this
        // surface; the first member captures the whole readable attachment.
        // It captures before any crop below can skip it, so a member that
        // shows nothing still fixes the group's backdrop before its children
        // paint.
        let snapshot = match &op.group {
            Some(group) => Some(snapshots.capture(
                group,
                backing,
                (
                    viewport_size.0.min(attachment_size.0),
                    viewport_size.1.min(attachment_size.1),
                ),
                surface_format,
                resources,
                encoder,
            )?),
            None => None,
        };
        let Some(cropped) = output.intersect(&readable).filter(|rect| !rect.is_empty()) else {
            return Ok(());
        };
        output = cropped;
        // The backdrop rect each effect reads, its taps per pixel, and the
        // transparent texels its copy keeps past the attachment edge (left,
        // top, right, bottom). A blur reads its kernel halo even outside the
        // output/damage clip; a resample reads the box its map sends the
        // output to, and its pad makes a tap past the attachment read
        // transparency instead of repeating the edge.
        let (read, taps, pad) = match op.effect {
            BackdropEffect::Blur(sigma) => {
                let radius = sigma.map(|sigma| f64::from((sigma * 1.732_050_8).ceil()));
                let read = Rect::from_ltrb(
                    (output.left() - radius[0]).floor().max(readable.left()),
                    (output.top() - radius[1]).floor().max(readable.top()),
                    (output.right() + radius[0]).ceil().min(readable.right()),
                    (output.bottom() + radius[1]).ceil().min(readable.bottom()),
                );
                let taps = (radius[0] as usize)
                    .checked_add(radius[1] as usize)
                    .and_then(|radii| radii.checked_mul(2))
                    .and_then(|taps| taps.checked_add(2))
                    .ok_or(EngineError::PreparedResourceOverflow)?;
                (read, taps, [0_u32; 4])
            }
            BackdropEffect::Resample { map, filter } => {
                let reach = filter.reach();
                // Output whose source lies past the readable attachment shows
                // nothing: crop the output to the box the inverse sends the
                // source it can show to, keeping the `reach` past the edge
                // where a sample still reads the edge texel.
                let support = Rect::from_ltrb(
                    readable.left() - reach,
                    readable.top() - reach,
                    readable.right() + reach,
                    readable.bottom() + reach,
                );
                let Some(source) = mapped_bounds(map, output)
                    .intersect(&support)
                    .filter(|rect| !rect.is_empty())
                else {
                    return Ok(());
                };
                let back = mapped_bounds(map.inverse(), source);
                let Some(cropped) = output.intersect(&back).filter(|rect| !rect.is_empty()) else {
                    return Ok(());
                };
                output = cropped;
                let shown = mapped_bounds(map, output);
                // A sample at the shown box's edge reads texels `reach` past
                // it, so the copy takes that halo too. Past the attachment the
                // halo is one transparent texel: the resample clamps there, so
                // a sample further out reads transparency as well.
                let halo = Rect::from_ltrb(
                    (shown.left() - reach).floor().max(readable.left() - 1.0),
                    (shown.top() - reach).floor().max(readable.top() - 1.0),
                    (shown.right() + reach).ceil().min(readable.right() + 1.0),
                    (shown.bottom() + reach).ceil().min(readable.bottom() + 1.0),
                );
                let read = Rect::from_ltrb(
                    halo.left().max(readable.left()),
                    halo.top().max(readable.top()),
                    halo.right().min(readable.right()),
                    halo.bottom().min(readable.bottom()),
                );
                let pad = [
                    read.left() - halo.left(),
                    read.top() - halo.top(),
                    halo.right() - read.right(),
                    halo.bottom() - read.bottom(),
                ]
                .map(|texels| texels.max(0.0) as u32);
                // Each output pixel reads its filter's taps.
                (read, filter.taps(), pad)
            }
        };
        if read.is_empty() {
            return Ok(());
        }
        let left = (read.left() - root_x) as u32;
        let top = (read.top() - root_y) as u32;
        let right = (read.right() - root_x) as u32;
        let bottom = (read.bottom() - root_y) as u32;
        let dimensions = (right - left, bottom - top);
        if dimensions.0 == 0 || dimensions.1 == 0 {
            return Ok(());
        }
        // The copy, with a resample's transparent halo around it.
        let texture_size = (
            dimensions
                .0
                .checked_add(pad[0] + pad[2])
                .ok_or(EngineError::PreparedResourceOverflow)?,
            dimensions
                .1
                .checked_add(pad[1] + pad[3])
                .ok_or(EngineError::PreparedResourceOverflow)?,
        );
        // The halo can take a copy of a full-size attachment past the
        // device's texture limit.
        let limit = device.limits().max_texture_dimension_2d;
        let longest = texture_size.0.max(texture_size.1);
        if longest > limit {
            return Err(EngineError::PreparedResourceLimit {
                resource: "backdrop copy dimension",
                requested: longest as usize,
                limit: limit as usize,
            });
        }
        let pixels = (texture_size.0 as usize)
            .checked_mul(texture_size.1 as usize)
            .ok_or(EngineError::PreparedResourceOverflow)?;
        // A resample draws into a texture over the whole pixels the output
        // touches, which a magnifier has far more of than texels it copies;
        // its taps run per pixel of that texture.
        let out_box = Rect::from_ltrb(
            output.left().floor(),
            output.top().floor(),
            output.right().ceil(),
            output.bottom().ceil(),
        );
        let out_size = (out_box.width() as u32, out_box.height() as u32);
        if matches!(op.effect, BackdropEffect::Resample { .. }) {
            let longest = out_size.0.max(out_size.1);
            if longest > limit {
                return Err(EngineError::PreparedResourceLimit {
                    resource: "backdrop resample dimension",
                    requested: longest as usize,
                    limit: limit as usize,
                });
            }
        }
        let covered = (out_size.0 as usize)
            .checked_mul(out_size.1 as usize)
            .ok_or(EngineError::PreparedResourceOverflow)?;
        let (sampled, textures) = match op.effect {
            // Source, H and V textures.
            BackdropEffect::Blur(_) => (pixels, pixels.checked_mul(3)),
            // The copy and the resampled output.
            BackdropEffect::Resample { .. } => (covered, pixels.checked_add(covered)),
        };
        op.context.budget.admit_effect_work(
            sampled
                .checked_mul(taps)
                .ok_or(EngineError::PreparedResourceOverflow)?,
        )?;
        let bytes = textures
            .and_then(|texels| texels.checked_mul(4))
            .and_then(|bytes| bytes.checked_add(64))
            .ok_or(EngineError::PreparedResourceOverflow)?;
        // Textures and views, the immutable uniforms, samplers, bindings and
        // the generated per-pass bind-group layouts.
        // Pool reuse is charged conservatively per operation; the legacy idle pool is excluded.
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: 0,
            objects: 14,
        })?;
        let source = resources.layer_texture_pool_mut().acquire(
            texture_size.0,
            texture_size.1,
            surface_format,
        );
        if pad.iter().any(|&texels| texels > 0) {
            // A pooled texture holds an earlier frame's texels: clear the
            // halo to transparent before the copy fills the rest.
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("FLUI Backdrop Resample Halo Clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: source.view(),
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: snapshot.unwrap_or(backing),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: left,
                    y: top,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: source.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: pad[0],
                    y: pad[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: dimensions.0,
                height: dimensions.1,
                depth_or_array_layers: 1,
            },
        );
        let read_bounds = Rect::from_xywh(
            f64::from(left) + root_x,
            f64::from(top) + root_y,
            f64::from(dimensions.0),
            f64::from(dimensions.1),
        );
        // The copy's origin sits `pad` texels before the copied rect.
        let copy_origin = glam::DVec2::new(
            root_x + f64::from(left) - f64::from(pad[0]),
            root_y + f64::from(top) - f64::from(pad[1]),
        );
        // The filtered texture, its device origin and its size: the composite
        // shows it texel for texel over the output.
        let (filtered, origin, size) = match op.effect {
            BackdropEffect::Blur(sigma) => (
                crate::blur::apply_blur(
                    sigma[0],
                    sigma[1],
                    &source,
                    read_bounds,
                    (
                        i64::from(left) + self.attachment_origin.0,
                        i64::from(top) + self.attachment_origin.1,
                    ),
                    dimensions,
                    surface_format,
                    &pipelines.blur,
                    resources,
                    device,
                    encoder,
                ),
                copy_origin,
                texture_size,
            ),
            BackdropEffect::Resample { map, filter } => {
                // From a texel of the output box to the device pixel it
                // covers, through the map, to a texel of the copy.
                let out_origin = glam::DVec2::new(out_box.left(), out_box.top());
                let texel_map = glam::DAffine2::from_translation(-copy_origin)
                    * map
                    * glam::DAffine2::from_translation(out_origin);
                (
                    crate::resample::apply_resample(
                        texel_map,
                        filter.filter_mode(),
                        &source,
                        texture_size,
                        out_size,
                        surface_format,
                        &pipelines.resample,
                        resources,
                        device,
                        encoder,
                    ),
                    out_origin,
                    out_size,
                )
            }
        };
        let uv = [
            ((output.left() - origin.x) / f64::from(size.0)) as f32,
            ((output.top() - origin.y) / f64::from(size.1)) as f32,
            ((output.right() - origin.x) / f64::from(size.0)) as f32,
            ((output.bottom() - origin.y) / f64::from(size.1)) as f32,
        ];
        self.composite_group_texture(
            filtered,
            output,
            uv,
            wgpu::FilterMode::Nearest,
            1.0,
            [1.0; 3],
            op.blend,
            Some(&op.clip),
            &op.context,
            attachment_size,
            surface_format,
            device,
            queue,
            pipelines,
            resources,
            encoder,
            target,
            op.scissor,
        )
    }
}

/// Backdrops captured for grouped filters within one ordered item list, i.e.
/// one attachment. Dropped with the list, so a group never reads another
/// surface's pixels or a later reuse of the same pooled attachment.
#[derive(Default)]
pub(crate) struct BackdropSnapshots {
    captured: Vec<(flui_layer::BackdropGroupKey, PooledTexture)>,
}

impl BackdropSnapshots {
    /// The group's snapshot of `backing`, capturing the readable attachment
    /// (texels `0..extent`) on the group's first read.
    fn capture(
        &mut self,
        group: &flui_layer::BackdropGroupKey,
        backing: &wgpu::Texture,
        extent: (u32, u32),
        format: wgpu::TextureFormat,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<&wgpu::Texture> {
        if let Some(index) = self.captured.iter().position(|(key, _)| key == group) {
            return Ok(self.captured[index].1.texture());
        }
        let bytes = (extent.0 as usize)
            .checked_mul(extent.1 as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| bytes.checked_add(64))
            .ok_or(EngineError::PreparedResourceOverflow)?;
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: 0,
            objects: 2,
        })?;
        let snapshot = resources
            .layer_texture_pool_mut()
            .acquire(extent.0, extent.1, format);
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: backing,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: snapshot.texture(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
        );
        let index = self.captured.len();
        self.captured.push((group.clone(), snapshot));
        Ok(self.captured[index].1.texture())
    }
}
