//! Candidate/committed retained images (ADR-0100, superseding ADR-0087 §4).
//! Preparation never writes the previous committed image. A partial candidate
//! is seeded by an ordered copy and promoted only after the whole frame submits.

use std::sync::Arc;

use crate::device_domain::{DeviceDomain, PreparedCost, PreparedPermit};
use crate::error::{EngineError, EngineResult};

#[derive(Default)]
pub(crate) struct RetainedTarget {
    slot: Option<Slot>,
    spare: Option<Slot>,
    valid: bool,
}

struct Slot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    domain: Arc<DeviceDomain>,
    permit: Arc<PreparedPermit>,
    bytes: usize,
}

impl Drop for Slot {
    fn drop(&mut self) {
        // Earlier clear/content/copy/blit submissions may still use this image.
        // Only the charge is captured; no Domain/Device ownership cycle.
        self.domain
            .retire_after_previous_submissions(vec![Arc::clone(&self.permit)]);
    }
}

pub(crate) struct CandidateTarget(Slot);

impl CandidateTarget {
    pub(crate) fn texture(&self) -> &wgpu::Texture {
        &self.0.texture
    }
    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.0.view
    }
}

impl std::fmt::Debug for RetainedTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedTarget")
            .field("size", &self.slot.as_ref().map(|slot| slot.size))
            .field("spare_size", &self.spare.as_ref().map(|slot| slot.size))
            .field("valid", &self.valid)
            .finish()
    }
}

impl RetainedTarget {
    pub(crate) fn is_valid(&self) -> bool {
        self.valid
    }
    pub(crate) fn invalidate(&mut self) {
        self.valid = false;
    }
    pub(crate) fn release(&mut self) {
        self.slot = None;
        self.spare = None;
        self.valid = false;
    }

    pub(crate) fn begin(
        &mut self,
        domain: &Arc<DeviceDomain>,
        size: (u32, u32),
        format: wgpu::TextureFormat,
        partial: bool,
    ) -> EngineResult<CandidateTarget> {
        domain.poll()?;
        let limit = domain.device().limits().max_texture_dimension_2d;
        for dimension in [size.0, size.1] {
            if dimension == 0 || dimension > limit {
                return Err(EngineError::PreparedResourceLimit {
                    resource: "retained target dimension",
                    requested: dimension as usize,
                    limit: limit as usize,
                });
            }
        }
        let (block_width, block_height) = format.block_dimensions();
        let block_bytes = format
            .block_copy_size(None)
            .ok_or(EngineError::PreparedResourceOverflow)? as usize;
        let bytes = (size.0.div_ceil(block_width) as usize)
            .checked_mul(size.1.div_ceil(block_height) as usize)
            .and_then(|blocks| blocks.checked_mul(block_bytes))
            .ok_or(EngineError::PreparedResourceOverflow)?;
        // Only a submitted previous candidate becomes spare. All subsequent
        // writes are ordered on the same queue, never to the committed image.
        let reusable = self.spare.take().filter(|slot| {
            slot.size == size && slot.format == format && Arc::ptr_eq(&slot.domain, domain)
        });
        // Discarded incompatible spare may have registered an already-ready
        // retirement callback; offer nonblocking progress before fresh admission.
        if reusable.is_none() {
            domain.poll()?;
        }
        let source = if partial {
            Some(
                self.slot
                    .as_ref()
                    .filter(|slot| {
                        self.valid
                            && slot.size == size
                            && slot.format == format
                            && Arc::ptr_eq(&slot.domain, domain)
                    })
                    .ok_or(EngineError::DeviceDomainMismatch)?,
            )
        } else {
            None
        };
        let previous_bytes = self
            .slot
            .as_ref()
            .filter(|slot| Arc::ptr_eq(&slot.domain, domain))
            .map_or(0, |slot| slot.bytes);
        domain.check_footprint(PreparedCost {
            gpu_bytes: bytes
                .checked_add(previous_bytes)
                .ok_or(EngineError::PreparedResourceOverflow)?,
            cpu_bytes: 0,
            objects: if previous_bytes == 0 { 2 } else { 4 },
        })?;
        let candidate = CandidateTarget(if let Some(slot) = reusable {
            slot
        } else {
            let permit = domain.reserve(PreparedCost {
                gpu_bytes: bytes,
                cpu_bytes: 0,
                objects: 2,
            })?;
            let texture = domain.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("FLUI Candidate Frame Target"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            Slot {
                texture,
                view,
                size,
                format,
                domain: Arc::clone(domain),
                permit,
                bytes,
            }
        });
        if let Some(source) = source {
            let mut encoder =
                domain
                    .device()
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Seed Partial Candidate"),
                    });
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &source.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: candidate.texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
            );
            domain.submit(domain.prepare(
                vec![encoder.finish()],
                vec![Arc::clone(&source.permit), Arc::clone(&candidate.0.permit)],
            )?)?;
        }
        Ok(candidate)
    }

    pub(crate) fn commit(&mut self, candidate: CandidateTarget) {
        self.spare = self.slot.replace(candidate.0);
        self.valid = true;
    }

    #[cfg(test)]
    pub(crate) fn texture(&self) -> Option<&wgpu::Texture> {
        self.slot.as_ref().map(|slot| &slot.texture)
    }
}
