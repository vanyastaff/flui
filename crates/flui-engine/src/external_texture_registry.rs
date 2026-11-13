//! Validated, trusted imports for the current encoded-SDR texture pipeline.
//!
//! Raw wgpu imports are a trusted boundary: callers must supply textures from
//! the painter's device and must not destroy them while recorded/submitted work
//! uses them. wgpu exposes no device-provenance or destroyed-state query here.
//! The owner stamp checks engine lease routing; it does not prove raw provenance.
//! A lease freezes allocation and interpretation, not texels. Ordered writes to
//! the same allocation remain visible at submission; no pixel snapshot is made.

use crate::{
    device_domain::DeviceDomain,
    error::{EngineResult, ExternalTextureError},
};
use flui_painting::paint::TextureId;
use rustc_hash::FxHashMap;
use std::sync::{Arc, Weak};

/// Default sampling used by the resource-sampling draw entry point.
/// Explicit per-draw FilterQuality overrides this default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExternalSampling {
    /// Sample the nearest texel, without mipmaps.
    Nearest,
    /// Bilinear sampling of mip zero; no mipmapping or anisotropy is promised.
    Linear,
}

/// How the encoded source components represent coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "encoded coverage is opaque, straight or premultiplied"
)]
pub enum ExternalAlpha {
    /// Coverage is one; the stored alpha channel is ignored.
    Opaque,
    /// Encoded RGB is independent of alpha.
    Straight,
    /// Encoded RGB has already been multiplied by alpha.
    Premultiplied,
}

/// Color representation admitted by the current texture pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExternalColorEncoding {
    /// Nonlinear sRGB components in an unorm view, blended in encoded space.
    /// This does not advertise a linear-light, wide-gamut or HDR pipeline.
    EncodedSrgb,
}

/// Explicit interpretation of an imported allocation. Dimensions and format
/// come from the texture rather than caller-supplied duplicate metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalTextureDescriptor {
    /// Default sampling for draws that select resource sampling.
    pub sampling: ExternalSampling,
    /// Source alpha representation; no unknown/default assumption is accepted.
    pub alpha: ExternalAlpha,
    /// Source color representation.
    pub color: ExternalColorEncoding,
}

#[derive(Debug)]
struct ExternalAllocation {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    descriptor: ExternalTextureDescriptor,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    _usage: wgpu::TextureUsages,
    _registry_owner: Arc<()>,
    _generation: u64,
    domain: Weak<DeviceDomain>,
}

/// Engine-only strong allocation snapshot captured at lowering/recording.
/// Weak owner routing avoids a completion callback/queue/Domain ownership cycle.
#[derive(Clone, Debug)]
pub(crate) struct ExternalAllocationLease(Arc<ExternalAllocation>);

#[cfg(all(test, feature = "testing", not(target_arch = "wasm32")))]
#[derive(Clone, Debug)]
pub(crate) struct ExternalAllocationProbe(Weak<ExternalAllocation>);

#[cfg(all(test, feature = "testing", not(target_arch = "wasm32")))]
impl ExternalAllocationProbe {
    pub(crate) fn is_alive(&self) -> bool {
        self.0.strong_count() != 0
    }
}

impl ExternalAllocationLease {
    #[cfg(all(test, feature = "testing", not(target_arch = "wasm32")))]
    pub(crate) fn lifetime_probe(&self) -> ExternalAllocationProbe {
        ExternalAllocationProbe(Arc::downgrade(&self.0))
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        &self.0.view
    }
    pub(crate) fn descriptor(&self) -> ExternalTextureDescriptor {
        self.0.descriptor
    }
    pub(crate) fn size(&self) -> (u32, u32) {
        self.0.size
    }
    pub(crate) fn is_for_domain(&self, domain: &DeviceDomain) -> bool {
        self.0.domain.as_ptr() == std::ptr::from_ref(domain)
    }
    pub(crate) fn allocation_key(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    pub(crate) fn same_allocation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Immutable registered allocation information. No texture/view mutation or
/// destroy authority is exposed; recorded leases survive removal/replacement.
#[derive(Debug)]
pub struct ExternalTextureEntry {
    lease: ExternalAllocationLease,
}

impl ExternalTextureEntry {
    /// Interpretation preserved by update.
    #[must_use]
    pub fn descriptor(&self) -> ExternalTextureDescriptor {
        self.lease.descriptor()
    }
    /// Actual mip-zero dimensions in device pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.lease.size()
    }
    pub(crate) fn lease(&self) -> &ExternalAllocationLease {
        &self.lease
    }
}

/// Imported allocations owned by one painter/device domain.
/// Reach this registry through WgpuPainter's registry accessors.
#[derive(Debug)]
pub struct ExternalTextureRegistry {
    textures: FxHashMap<u64, ExternalTextureEntry>,
    owner: Arc<()>,
    generation: u64,
    domain: Weak<DeviceDomain>,
    max_dimension: u32,
}

impl ExternalTextureRegistry {
    pub(crate) fn new(domain: &Arc<DeviceDomain>) -> Self {
        Self {
            textures: FxHashMap::default(),
            owner: Arc::new(()),
            generation: 0,
            domain: Arc::downgrade(domain),
            max_dimension: domain.device().limits().max_texture_dimension_2d,
        }
    }

    /// Whether this registry's allocations are on `domain`'s device.
    pub(crate) fn is_on(&self, domain: &Arc<DeviceDomain>) -> bool {
        std::ptr::eq(self.domain.as_ptr(), Arc::as_ptr(domain))
    }

    /// Upload each frame of `updates` as its texture's current allocation
    /// and release the retired textures (see
    /// [`WgpuPainter::apply_texture_updates`](crate::painter::WgpuPainter::apply_texture_updates)).
    ///
    /// # Errors
    /// The first frame refused; every other frame and every retirement is
    /// still applied. [`EngineError::DeviceLost`](crate::error::EngineError::DeviceLost)
    /// for every frame once the registry's device is gone.
    pub(crate) fn apply(&mut self, updates: flui_layer::TextureUpdates) -> EngineResult<()> {
        let flui_layer::TextureUpdates {
            frames, retired, ..
        } = updates;
        for id in retired {
            self.unregister(id);
        }
        if frames.is_empty() {
            return Ok(());
        }
        let Some(domain) = self.domain.upgrade() else {
            return Err(crate::error::EngineError::DeviceLost);
        };
        let mut first_error = None;
        for (id, frame) in frames {
            if let Err(error) = self.upload(&domain, id, &frame) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Upload `frame` as `id`'s allocation: in place for one of the same
    /// size, registered anew for another.
    fn upload(
        &mut self,
        domain: &DeviceDomain,
        id: TextureId,
        frame: &flui_painting::paint::Image,
    ) -> EngineResult<()> {
        let size = wgpu::Extent3d {
            width: frame.width(),
            height: frame.height(),
            depth_or_array_layers: 1,
        };
        // Refuse before allocating: wgpu rejects these extents itself.
        check_frame_size(self.max_dimension, frame)?;
        let texture = domain.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("external texture frame"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        domain.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.data(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size.width),
                rows_per_image: Some(size.height),
            },
            size,
        );
        let same_size = self
            .get(id)
            .is_some_and(|entry| entry.size() == (size.width, size.height));
        if same_size {
            self.update(id, texture)
        } else {
            // A refused allocation leaves the previous image registered, as
            // the kept images hold it.
            let previous = self.textures.remove(&id.get());
            let registered = self.register(
                id,
                texture,
                ExternalTextureDescriptor {
                    sampling: ExternalSampling::Linear,
                    alpha: ExternalAlpha::Straight,
                    color: ExternalColorEncoding::EncodedSrgb,
                },
            );
            if registered.is_err()
                && let Some(previous) = previous
            {
                self.textures.insert(id.get(), previous);
            }
            registered
        }
    }

    /// Import an allocation under an unused logical ID.
    ///
    /// The caller guarantees device provenance, valid encoded-SDR texels and
    /// absence of raw destroy while leases are live. Texture clones can still
    /// change texels; registration does not copy them or issue revision wakes.
    ///
    /// # Errors
    /// Duplicate IDs, unsupported texture metadata or generation exhaustion.
    /// Validation precedes engine view allocation; rejection leaves the registry
    /// unchanged. Raw foreign/destroyed handles remain trusted misuse.
    pub fn register(
        &mut self,
        id: TextureId,
        texture: wgpu::Texture,
        descriptor: ExternalTextureDescriptor,
    ) -> EngineResult<()> {
        if self.textures.contains_key(&id.get()) {
            return Err(ExternalTextureError::DuplicateTexture { id: id.get() }.into());
        }
        self.validate(&texture, descriptor.color)?;
        let entry = self.allocate(texture, descriptor)?;
        self.textures.insert(id.get(), entry);
        Ok(())
    }

    /// Replace an allocation while preserving its size, format and descriptor.
    /// Existing recorded leases retain the old allocation. To change policy,
    /// explicitly unregister and register; no texel snapshot or wake is implied.
    /// The caller retains the raw-import provenance/destroy obligations.
    ///
    /// # Errors
    /// Unknown ID, unsupported metadata, incompatible size/format or generation
    /// exhaustion. Every returned error preserves the previous entry.
    pub fn update(&mut self, id: TextureId, texture: wgpu::Texture) -> EngineResult<()> {
        let previous = self
            .textures
            .get(&id.get())
            .ok_or(ExternalTextureError::UnknownTexture { id: id.get() })?;
        self.validate(&texture, previous.descriptor().color)?;
        let size = (texture.width(), texture.height());
        let format = texture.format();
        if size != previous.size() || format != previous.lease.0.format {
            return Err(ExternalTextureError::IncompatibleReplacement {
                expected_size: previous.size(),
                actual_size: size,
                expected_format: previous.lease.0.format,
                actual_format: format,
            }
            .into());
        }
        let descriptor = previous.descriptor();
        let entry = self.allocate(texture, descriptor)?;
        self.textures.insert(id.get(), entry);
        Ok(())
    }

    fn validate(&self, texture: &wgpu::Texture, color: ExternalColorEncoding) -> EngineResult<()> {
        let actual = texture.dimension();
        if actual != wgpu::TextureDimension::D2 {
            return Err(ExternalTextureError::InvalidTextureDimension { actual }.into());
        }
        let size = texture.size();
        if size.depth_or_array_layers != 1 {
            return Err(ExternalTextureError::InvalidTextureLayers {
                actual: size.depth_or_array_layers,
            }
            .into());
        }
        if texture.sample_count() != 1 {
            return Err(ExternalTextureError::InvalidTextureSamples {
                actual: texture.sample_count(),
            }
            .into());
        }
        if size.width == 0
            || size.height == 0
            || size.width > self.max_dimension
            || size.height > self.max_dimension
        {
            return Err(ExternalTextureError::InvalidTextureSize {
                width: size.width,
                height: size.height,
            }
            .into());
        }
        if !texture
            .usage()
            .contains(wgpu::TextureUsages::TEXTURE_BINDING)
        {
            return Err(ExternalTextureError::MissingTextureBindingUsage.into());
        }
        let format = texture.format();
        let supported = match color {
            ExternalColorEncoding::EncodedSrgb => matches!(
                format,
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
            ),
        };
        if !supported {
            return Err(ExternalTextureError::UnsupportedTextureFormat { format }.into());
        }
        Ok(())
    }

    // Called only after validation. No default sRGB reinterpretation, layer-array
    // view or mip selection can silently contradict the admitted representation.
    fn allocate(
        &mut self,
        texture: wgpu::Texture,
        descriptor: ExternalTextureDescriptor,
    ) -> EngineResult<ExternalTextureEntry> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(ExternalTextureError::GenerationExhausted)?;
        let size = (texture.width(), texture.height());
        let format = texture.format();
        let usage = texture.usage();
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("FLUI validated external mip-zero view"),
            format: Some(format),
            dimension: Some(wgpu::TextureViewDimension::D2),
            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        });
        let lease = ExternalAllocationLease(Arc::new(ExternalAllocation {
            _texture: texture,
            view,
            descriptor,
            size,
            format,
            _usage: usage,
            _registry_owner: Arc::clone(&self.owner),
            _generation: generation,
            domain: self.domain.clone(),
        }));
        self.generation = generation;
        Ok(ExternalTextureEntry { lease })
    }

    /// Remove future lookups; recorded/submitted leases remain alive.
    pub fn unregister(&mut self, id: TextureId) -> bool {
        self.textures.remove(&id.get()).is_some()
    }
    /// Inspect immutable registered metadata.
    #[must_use]
    pub fn get(&self, id: TextureId) -> Option<&ExternalTextureEntry> {
        self.textures.get(&id.get())
    }
    /// Whether the logical ID is currently registered.
    #[must_use]
    pub fn contains(&self, id: TextureId) -> bool {
        self.textures.contains_key(&id.get())
    }
    /// Number of currently registered IDs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.textures.len()
    }
    /// Whether no IDs are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }
    /// Close all future lookups without revoking recorded leases.
    pub fn clear(&mut self) {
        self.textures.clear();
    }
    /// Iterate currently registered logical IDs.
    pub fn texture_ids(&self) -> impl Iterator<Item = TextureId> + '_ {
        self.textures.keys().map(|&id| TextureId::new(id))
    }
}

/// Refuse a frame no texture of a device whose largest 2D texture side is
/// `max_dimension` can hold.
fn check_frame_size(max_dimension: u32, frame: &flui_painting::paint::Image) -> EngineResult<()> {
    let (width, height) = (frame.width(), frame.height());
    if width == 0 || height == 0 || width > max_dimension || height > max_dimension {
        return Err(ExternalTextureError::InvalidTextureSize { width, height }.into());
    }
    Ok(())
}

/// One presentation's external textures in a context that shares its device
/// and painter between presentations.
///
/// A frame reaches the device only when the slot is about to draw
/// ([`Self::prepare`]): `write_texture` holds staging memory until a queue
/// submission, and a hidden slot submits nothing, so a producer streaming into
/// it replaces one waiting image per texture instead of piling up uploads.
#[derive(Debug, Default)]
pub(crate) struct KeptTextures {
    /// The allocations, `None` until the first draw and after the device
    /// they were on is replaced or a frame unwound with them lent.
    registry: Option<ExternalTextureRegistry>,
    /// The image each texture's allocation holds, kept so a new device gets
    /// it back.
    drawn: FxHashMap<TextureId, flui_painting::paint::Image>,
    /// The newest frame of each texture, not on the device yet.
    waiting: FxHashMap<TextureId, flui_painting::paint::Image>,
}

impl KeptTextures {
    /// Keep each frame of `updates` that a texture of `domain`'s device can
    /// hold as its texture's waiting image, and release the retired textures.
    ///
    /// # Errors
    /// The first frame refused; the texture keeps its previous image, and
    /// every other frame and retirement is still applied.
    pub(crate) fn apply(
        &mut self,
        domain: &Arc<DeviceDomain>,
        updates: flui_layer::TextureUpdates,
    ) -> EngineResult<()> {
        let flui_layer::TextureUpdates {
            frames, retired, ..
        } = updates;
        for id in retired {
            self.drawn.remove(&id);
            self.waiting.remove(&id);
            if let Some(registry) = &mut self.registry {
                registry.unregister(id);
            }
        }
        let max_dimension = domain.device().limits().max_texture_dimension_2d;
        let mut first_error = None;
        for (id, frame) in frames {
            match check_frame_size(max_dimension, &frame) {
                Ok(()) => {
                    self.waiting.insert(id, frame);
                }
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Put the textures on `domain`'s device for a draw, and answer whether
    /// it wrote to the queue, which the draw's submission, or a flush when
    /// the draw submits nothing, then carries.
    ///
    /// Allocations on no device or another one are made again from the drawn
    /// images; then each waiting image is uploaded. A frame the device
    /// refuses (one past a new device's texture limit) leaves its texture's
    /// previous image, reported as a warning.
    pub(crate) fn prepare(&mut self, domain: &Arc<DeviceDomain>) -> bool {
        let fresh = !self
            .registry
            .as_ref()
            .is_some_and(|registry| registry.is_on(domain));
        if !fresh && self.waiting.is_empty() {
            return false;
        }
        let Self {
            registry,
            drawn,
            waiting,
        } = self;
        let mut refused = Vec::new();
        if fresh {
            // Installed only once whole, so a panic part way leaves the next
            // draw to start again.
            let mut made = ExternalTextureRegistry::new(domain);
            drawn.retain(|id, frame| match made.upload(domain, *id, frame) {
                Ok(()) => true,
                Err(error) => {
                    refused.push(error);
                    false
                }
            });
            *registry = Some(made);
        }
        let registry = registry
            .as_mut()
            .expect("BUG: the registry is on this device, made above if it was not");
        let ids: Vec<TextureId> = waiting.keys().copied().collect();
        for id in ids {
            let Some(frame) = waiting.get(&id) else {
                continue;
            };
            // Taken off the waiting list only once uploaded or refused, so a
            // panic in the upload leaves it for the next draw.
            let uploaded = registry.upload(domain, id, frame);
            let frame = waiting
                .remove(&id)
                .expect("BUG: the waiting image was read above");
            match uploaded {
                Ok(()) => {
                    drawn.insert(id, frame);
                }
                Err(error) => refused.push(error),
            }
        }
        for error in refused {
            tracing::warn!(target: "flui.gpu", %error, "external texture upload refused a frame; the texture keeps its previous image");
        }
        true
    }

    /// Whether a draw would write to the queue: allocations on no device or
    /// another one, or an image waiting.
    pub(crate) fn needs_writes(&self, domain: &Arc<DeviceDomain>) -> bool {
        !self.waiting.is_empty()
            || !self
                .registry
                .as_ref()
                .is_some_and(|registry| registry.is_on(domain))
    }

    /// The allocations on `domain`'s device, taken out for one frame after
    /// [`Self::prepare`]: until [`Self::take_back`] returns them, the next
    /// prepare makes them again from the drawn images, so a frame that never
    /// returns them costs a re-upload, not the textures. `None` when none are
    /// on this device, a frame whose prepare was put off: it draws none of
    /// the slot's textures.
    pub(crate) fn lend(&mut self, domain: &Arc<DeviceDomain>) -> Option<ExternalTextureRegistry> {
        if !self
            .registry
            .as_ref()
            .is_some_and(|registry| registry.is_on(domain))
        {
            return None;
        }
        self.registry.take()
    }

    /// Return the allocations [`Self::lend`] took out.
    pub(crate) fn take_back(&mut self, registry: ExternalTextureRegistry) {
        self.registry = Some(registry);
    }

    /// Let go of the allocations, kept on a device being replaced; the
    /// images stay for the next device.
    pub(crate) fn leave_device(&mut self) {
        self.registry = None;
    }

    /// How many textures have an image waiting for the device.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use super::*;

    /// A frame of a new size whose allocation the registry refuses leaves the
    /// previous image registered, as the kept images still hold it.
    ///
    /// Red-check: unregister the previous allocation before registering the
    /// new one, without putting it back on a refusal, and the texture is gone.
    #[test]
    fn a_refused_resize_keeps_the_previous_image() {
        let Some((device, queue)) = crate::test_support::try_test_device_and_queue(
            "a_refused_resize_keeps_the_previous_image",
        ) else {
            return;
        };
        let domain = DeviceDomain::new(device, queue);
        let mut registry = ExternalTextureRegistry::new(&domain);
        let id = TextureId::new(1);
        let red = flui_painting::paint::Image::from_rgba8(2, 2, [255, 0, 0, 255].repeat(4));
        registry
            .upload(&domain, id, &red)
            .expect("the first frame is accepted");
        registry.generation = u64::MAX;

        let larger = flui_painting::paint::Image::from_rgba8(4, 4, [0, 0, 255, 255].repeat(16));
        assert!(
            registry.upload(&domain, id, &larger).is_err(),
            "no generation is left for the new allocation"
        );
        assert_eq!(
            registry.get(id).map(ExternalTextureEntry::size),
            Some((2, 2)),
            "the previous image stays registered"
        );
    }

    /// A waiting frame the device refuses at upload leaves its texture the
    /// image it drew before, in the allocation and in the image kept for the
    /// next device.
    ///
    /// Red-check: keep the refused frame as the drawn image and the next
    /// device draws an image this one never showed.
    #[test]
    fn a_waiting_frame_refused_at_upload_leaves_the_drawn_image() {
        let Some((device, queue)) = crate::test_support::try_test_device_and_queue(
            "a_waiting_frame_refused_at_upload_leaves_the_drawn_image",
        ) else {
            return;
        };
        let domain = DeviceDomain::new(device, queue);
        let id = TextureId::new(1);
        let mut kept = KeptTextures::default();
        let updates = |side: u32| {
            flui_layer::TextureUpdates::from_frames(vec![(
                id,
                flui_painting::paint::Image::from_rgba8(
                    side,
                    side,
                    [255, 0, 0, 255].repeat((side * side) as usize),
                ),
            )])
        };
        kept.apply(&domain, updates(2)).expect("the frame fits");
        assert!(kept.prepare(&domain), "the waiting image is uploaded");
        kept.registry
            .as_mut()
            .expect("prepared on the device")
            .generation = u64::MAX;

        kept.apply(&domain, updates(4)).expect("the frame fits");
        assert!(kept.prepare(&domain));
        assert_eq!(kept.waiting(), 0, "a refused frame does not wait again");
        assert_eq!(
            kept.registry
                .as_ref()
                .and_then(|registry| registry.get(id))
                .map(ExternalTextureEntry::size),
            Some((2, 2)),
            "the allocation keeps the drawn image"
        );
        assert_eq!(
            kept.drawn
                .get(&id)
                .map(|image| (image.width(), image.height())),
            Some((2, 2)),
            "the image kept for the next device is the drawn one"
        );
    }
}
