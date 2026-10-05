//! Ordered backdrop reads belong to the current attachment, including nested groups.
use std::sync::Arc;

use flui_foundation::geometry::Rect;
use flui_painting::paint::{BlendMode, ImageFilter};

use super::WgpuPainter;
use crate::{
    clip_chain::ClipOp,
    clip_geometry::{ValidatedAffine, ValidatedClip},
    command_ir::{BackdropOp, DrawItem, DrawSegment, RecordError},
    device_domain::PreparedCost,
    error::{EngineError, EngineResult, GeometryError},
    pipeline_set::PipelineSet,
    render_target::RenderTarget,
    replay::GpuReplay,
    resources::GpuResources,
};

impl WgpuPainter {
    /// Record a backdrop read at this position, before recording its children.
    pub(crate) fn record_backdrop_filter(
        &mut self,
        bounds: Rect<f64>,
        filter: &ImageFilter,
        blend: BlendMode,
    ) -> EngineResult<()> {
        self.current_segment.recording_result()?;
        let result = self.record_backdrop_filter_impl(bounds, filter, blend);
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
    ) -> EngineResult<()> {
        let matrix = self.current_transform_matrix().m;
        let affine = ValidatedAffine::new(glam::DMat4::from_cols_array(&matrix))?;
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
                reason: "backdrop Gaussian blur requires encoded SDR RGBA8/BGRA8 Unorm",
            });
        }
        let ImageFilter::Blur { sigma_x, sigma_y } = filter else {
            return Err(EngineError::UnsupportedBackdropFilter {
                reason: "only Gaussian backdrop blur is supported",
            });
        };
        if !sigma_x.is_finite() || !sigma_y.is_finite() || *sigma_x < 0.0 || *sigma_y < 0.0 {
            return Err(EngineError::UnsupportedBackdropFilter {
                reason: "Gaussian sigma must be finite and nonnegative",
            });
        }
        // Covariance remains diagonal only for these axis-preserving transforms.
        // General rotation/shear requires directional kernels, not AABB sigma.
        // Public rotation matrices retain sin/cos roundoff at quarter turns.
        // Compare within each column so an unrelated large scale cannot hide
        // meaningful shear in the other column.
        let axis_roundoff =
            |off_axis: f64, on_axis: f64| off_axis.abs() <= 8.0 * f64::EPSILON * on_axis.abs();
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
        let sigma = packed_sigma;
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
            sigma,
            blend,
        };
        self.finish_current_segment();
        self.draw_order.push(DrawItem::Backdrop(op));
        Ok(())
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
        let Some(cropped) = output.intersect(&readable).filter(|rect| !rect.is_empty()) else {
            return Ok(());
        };
        output = cropped;
        // Read extent includes the kernel halo even outside the output/damage clip.
        // The attachment edge is a transparent decal boundary.
        let radius = op
            .sigma
            .map(|sigma| f64::from((sigma * 1.732_050_8).ceil()));
        let left = ((output.left() - radius[0]).floor().max(readable.left()) - root_x) as u32;
        let top = ((output.top() - radius[1]).floor().max(readable.top()) - root_y) as u32;
        let right = ((output.right() + radius[0]).ceil().min(readable.right()) - root_x) as u32;
        let bottom = ((output.bottom() + radius[1]).ceil().min(readable.bottom()) - root_y) as u32;
        let dimensions = (right - left, bottom - top);
        let pixels = (dimensions.0 as usize)
            .checked_mul(dimensions.1 as usize)
            .ok_or(EngineError::PreparedResourceOverflow)?;
        let taps = (radius[0] as usize)
            .checked_add(radius[1] as usize)
            .and_then(|radii| radii.checked_mul(2))
            .and_then(|taps| taps.checked_add(2))
            .ok_or(EngineError::PreparedResourceOverflow)?;
        op.context.budget.admit_effect_work(
            pixels
                .checked_mul(taps)
                .ok_or(EngineError::PreparedResourceOverflow)?,
        )?;
        let bytes = pixels
            .checked_mul(12)
            .and_then(|bytes| bytes.checked_add(64))
            .ok_or(EngineError::PreparedResourceOverflow)?;
        // Source/H/V textures+views, two immutable uniforms, samplers, bindings
        // and the generated per-pass bind-group layouts.
        // Pool reuse is charged conservatively per operation; the legacy idle pool is excluded.
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: 0,
            objects: 14,
        })?;
        let source =
            resources
                .layer_texture_pool_mut()
                .acquire(dimensions.0, dimensions.1, surface_format);
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: backing,
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
                origin: wgpu::Origin3d::ZERO,
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
        let filtered = crate::blur::apply_blur(
            op.sigma[0],
            op.sigma[1],
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
        );
        let uv = [
            ((output.left() - root_x - f64::from(left)) / f64::from(dimensions.0)) as f32,
            ((output.top() - root_y - f64::from(top)) / f64::from(dimensions.1)) as f32,
            ((output.right() - root_x - f64::from(left)) / f64::from(dimensions.0)) as f32,
            ((output.bottom() - root_y - f64::from(top)) / f64::from(dimensions.1)) as f32,
        ];
        self.composite_group_texture(
            filtered,
            output,
            uv,
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
