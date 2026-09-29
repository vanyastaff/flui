//! The render target a partial frame repaints into (ADR-0087 §4).
//!
//! wgpu does not expose a swapchain image's age (gfx-rs/wgpu#682), so a
//! scissored repaint straight into a freshly acquired swapchain image would
//! leave the pixels outside the scissor showing whatever older frame last
//! used that image. A [`RetainedTarget`] holds the last rendered frame
//! instead: a partial frame repaints only its damage into it, and the whole
//! target is then blitted to the swapchain.
//!
//! The target is allocated lazily, by the first frame that renders through
//! it, so a renderer whose frames are all full (damage switched off, or
//! every frame over the threshold) never pays its memory. It costs
//! `width × height × 4` bytes per window while held.

/// A persistent texture in the surface format, and whether it holds the
/// last frame this renderer presented.
///
/// Validity is the whole protocol: [`Self::begin`] clears it before a frame
/// renders into the target and [`Self::commit`] sets it only once that frame
/// was submitted, so a frame that errors or unwinds in between leaves the
/// target invalid and the next partial frame renders in full instead of
/// trusting half-written pixels.
#[derive(Default)]
pub(crate) struct RetainedTarget {
    slot: Option<Slot>,
    valid: bool,
}

struct Slot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    format: wgpu::TextureFormat,
}

impl std::fmt::Debug for RetainedTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedTarget")
            .field("size", &self.slot.as_ref().map(|slot| slot.size))
            .field("valid", &self.valid)
            .finish()
    }
}

impl RetainedTarget {
    /// Whether the target holds the last presented frame, so a partial
    /// frame may repaint only its damage into it.
    #[must_use]
    pub(crate) fn is_valid(&self) -> bool {
        self.valid
    }

    /// Stops trusting the target's pixels (a frame rendered elsewhere, a
    /// surface reconfigured). The texture is kept for reuse.
    pub(crate) fn invalidate(&mut self) {
        self.valid = false;
    }

    /// Drops the texture as well: its device is gone, or the surface was
    /// released and the memory should go with it.
    pub(crate) fn release(&mut self) {
        self.slot = None;
        self.valid = false;
    }

    /// Prepares the target for a frame at `size` in `format` and returns the
    /// texture and view to render into.
    ///
    /// Reallocates when the size or format changed, and always leaves the
    /// target invalid until [`Self::commit`].
    pub(crate) fn begin(
        &mut self,
        device: &wgpu::Device,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        self.valid = false;
        let reusable = self
            .slot
            .as_ref()
            .is_some_and(|slot| slot.size == size && slot.format == format);
        if !reusable {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("FLUI Retained Frame Target"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                // Rendered into, sampled by the blit and by backdrop filters,
                // copied from by dst-reading blends, copied into by tests.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            tracing::debug!(
                target: "flui.gpu",
                event = "retained_target_allocated",
                width = size.0,
                height = size.1,
                // Every surface format this engine selects is 4 bytes a pixel.
                bytes = u64::from(size.0) * u64::from(size.1) * 4,
                ?format,
                "retained frame target allocated"
            );
            self.slot = Some(Slot {
                texture,
                view,
                size,
                format,
            });
        }
        let slot = self
            .slot
            .as_ref()
            .expect("BUG: the retained target slot was filled just above");
        (slot.texture.clone(), slot.view.clone())
    }

    /// Records that the frame begun by [`Self::begin`] was submitted in full:
    /// the target now holds it.
    pub(crate) fn commit(&mut self) {
        debug_assert!(
            self.slot.is_some(),
            "BUG: RetainedTarget::commit without a begun frame"
        );
        self.valid = self.slot.is_some();
    }

    /// The target's texture, when one is allocated.
    #[cfg(test)]
    pub(crate) fn texture(&self) -> Option<&wgpu::Texture> {
        self.slot.as_ref().map(|slot| &slot.texture)
    }
}
