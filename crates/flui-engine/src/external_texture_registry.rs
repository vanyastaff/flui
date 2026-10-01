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
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

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
    textures: HashMap<u64, ExternalTextureEntry>,
    owner: Arc<()>,
    generation: u64,
    domain: Weak<DeviceDomain>,
    max_dimension: u32,
}

impl ExternalTextureRegistry {
    pub(crate) fn new(domain: &Arc<DeviceDomain>) -> Self {
        Self {
            textures: HashMap::new(),
            owner: Arc::new(()),
            generation: 0,
            domain: Arc::downgrade(domain),
            max_dimension: domain.device().limits().max_texture_dimension_2d,
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
