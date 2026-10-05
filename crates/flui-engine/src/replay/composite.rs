//! Group coverage belongs to the finished effect, not its isolated input.
use super::GpuReplay;
use crate::{
    advanced_blend::{AdvancedBlendOp, flush_advanced_layer},
    clip_chain::ClipChain,
    command_ir::{DrawSegment, GroupClip},
    error::{EngineError, EngineResult},
    pipeline_set::PipelineSet,
    render_target::RenderTarget,
    resources::GpuResources,
    texture_pool::PooledTexture,
};
use flui_foundation::geometry::Rect;
use flui_painting::paint::BlendMode;
use std::sync::Arc;

impl GpuReplay {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn composite_group_texture(
        &mut self,
        texture: PooledTexture,
        bounds: Rect<f64>,
        uv: [f32; 4],
        opacity: f32,
        tint: [f32; 3],
        mode: BlendMode,
        clip: Option<&GroupClip>,
        context: &DrawSegment,
        viewport_size: (u32, u32),
        surface_format: wgpu::TextureFormat,
        device: &Arc<wgpu::Device>,
        queue: &Arc<wgpu::Queue>,
        pipelines: &mut PipelineSet,
        resources: &mut GpuResources,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        scissor: crate::command_ir::ScissorRect,
    ) -> EngineResult<()> {
        let (bounds, uv) = if let Some((x, y, w, h)) = scissor {
            let cut = Rect::from_xywh(x as f64, y as f64, f64::from(w), f64::from(h));
            let Some(cropped) = bounds.intersect(&cut).filter(|rect| !rect.is_empty()) else {
                return Ok(());
            };
            let fractions = [
                ((cropped.left() - bounds.left()) / bounds.width()) as f32,
                ((cropped.top() - bounds.top()) / bounds.height()) as f32,
                ((cropped.right() - bounds.left()) / bounds.width()) as f32,
                ((cropped.bottom() - bounds.top()) / bounds.height()) as f32,
            ];
            let du = uv[2] - uv[0];
            let dv = uv[3] - uv[1];
            (
                cropped,
                [
                    uv[0] + du * fractions[0],
                    uv[1] + dv * fractions[1],
                    uv[0] + du * fractions[2],
                    uv[1] + dv * fractions[3],
                ],
            )
        } else {
            (bounds, uv)
        };
        if bounds.is_empty() {
            return Ok(());
        }
        let unclipped = ClipChain::default();
        let chain = clip.map_or(&unclipped, |clip| &clip.chain);
        // Freeze the composite in its attachment coordinate system. A managed
        // resize can leave the recorder's viewport different from this target.
        let previous_uniform_size = std::mem::replace(&mut self.uniform_size, viewport_size);
        let binding = self.prepare_clip_binding(
            context,
            chain,
            viewport_size,
            device,
            pipelines,
            resources,
            encoder,
        );
        self.uniform_size = previous_uniform_size;
        self.viewport_bind_group = binding?;
        // With fractional coverage these modes cannot express
        // mix(destination, blend(source, destination), coverage) by folding
        // coverage into source alpha. Binary masks instead discard the outside
        // and use fixed-function blending, including on view-only targets.
        let reads_destination = mode.is_advanced()
            || (chain.has_antialias()
                && (mode == BlendMode::Plus
                    || crate::pipeline_cache::destination_alpha_scale_for(mode).is_some()));
        if reads_destination {
            let destination = target
                .texture
                .ok_or(EngineError::CompositeBackdropUnavailable)?;
            let op = AdvancedBlendOp {
                foreground: texture,
                mode,
                device_bounds: bounds,
                opacity,
                tint,
                src_uv_min: [uv[0], uv[1]],
                src_uv_max: [uv[2], uv[3]],
                clip: clip
                    .filter(|_| chain.is_unclipped())
                    .map(|clip| clip.legacy),
            };
            self.admit_filter_composite(viewport_size, surface_format, resources)?;
            flush_advanced_layer(
                op,
                destination,
                target.view,
                surface_format,
                viewport_size,
                &pipelines.advanced_blend,
                resources,
                device,
                encoder,
                Some(&self.viewport_bind_group),
                self.attachment_origin,
            );
            return Ok(());
        }
        let o = opacity.clamp(0.0, 1.0);
        let mut instance = crate::instancing::TextureInstance::with_uv_tint_f32(
            bounds,
            uv,
            [tint[0] * o, tint[1] * o, tint[2] * o, o],
        );
        if chain.is_unclipped()
            && let Some(clip) = clip
        {
            use crate::instancing::ClippableInstance as _;
            instance = instance.with_clip(clip.legacy);
        }
        let _ = self.texture_batch.add(instance);
        self.flush_texture_batch_premultiplied_with_mode(
            mode,
            device,
            queue,
            pipelines,
            resources,
            viewport_size,
            encoder,
            target.view,
            texture.view(),
            scissor,
        );
        Ok(())
    }
}
