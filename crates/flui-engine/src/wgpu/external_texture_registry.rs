//! External Texture Registry for GPU texture management
//!
//! Provides a registry for external GPU textures (video frames, camera previews,
//! platform views) that can be rendered by FLUI.
//!
//! # Architecture
//!
//! External textures are identified by `flui_types::painting::TextureId` (u64).
//! This registry maps those IDs to actual GPU textures that can be rendered.
//!
//! ```text
//! Platform Code (Video Decoder, Camera, etc.)
//!     ↓
//! ExternalTextureRegistry::register(TextureId, wgpu::Texture)
//!     ↓
//! Canvas::draw_texture(TextureId, dst_rect, ...)
//!     ↓
//! WgpuPainter::draw_texture() → lookup in registry → render
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_engine::painter::ExternalTextureRegistry;
//! use flui_types::painting::TextureId;
//!
//! // Platform code registers a video frame texture
//! let texture_id = TextureId::new(42);
//! registry.register(texture_id, gpu_texture, 1920, 1080);
//!
//! // Later, in rendering code:
//! if let Some(entry) = registry.get(texture_id) {
//!     // Render the texture
//! }
//!
//! // When done, unregister
//! registry.unregister(texture_id);
//! ```

use flui_types::painting::TextureId;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::{
    AddressMode, BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout,
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType, Device,
    FilterMode, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, Texture,
    TextureSampleType, TextureView, TextureViewDimension,
};

/// Entry for an external texture in the registry
#[derive(Debug)]
pub struct ExternalTextureEntry {
    /// The GPU texture
    pub texture: Texture,
    /// Texture view for rendering
    pub view: TextureView,
    /// Bind group for this texture (texture + sampler)
    pub bind_group: BindGroup,
    /// Texture width in pixels
    pub width: u32,
    /// Texture height in pixels
    pub height: u32,
    /// Whether this texture needs to be updated each frame (e.g., video)
    pub is_dynamic: bool,
    /// Frame counter for tracking updates
    pub frame_count: u64,
}

/// Registry for external GPU textures
///
/// Maps `flui_types::painting::TextureId` to GPU textures that can be rendered.
#[allow(missing_debug_implementations)]
pub struct ExternalTextureRegistry {
    /// Registered textures by ID
    textures: HashMap<u64, ExternalTextureEntry>,
    /// Bind group layout for texture + sampler
    bind_group_layout: BindGroupLayout,
    /// Linear sampler (bilinear filtering)
    linear_sampler: Sampler,
    /// Nearest sampler (no filtering, for pixel art)
    nearest_sampler: Sampler,
    /// Device reference for creating bind groups
    device: Arc<Device>,
}

impl ExternalTextureRegistry {
    /// Create a new external texture registry
    pub fn new(device: Arc<Device>) -> Self {
        // Create bind group layout for texture + sampler
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("External Texture Bind Group Layout"),
            entries: &[
                // Texture
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Sampler
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // Create samplers
        let linear_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("External Texture Linear Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            ..Default::default()
        });

        let nearest_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("External Texture Nearest Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            textures: HashMap::new(),
            bind_group_layout,
            linear_sampler,
            nearest_sampler,
            device,
        }
    }

    /// Get the bind group layout for external textures
    pub fn bind_group_layout(&self) -> &BindGroupLayout {
        &self.bind_group_layout
    }

    /// Register an external texture
    ///
    /// # Arguments
    ///
    /// * `texture_id` - The public texture ID from `flui_types::painting::TextureId`
    /// * `texture` - The GPU texture
    /// * `width` - Texture width in pixels
    /// * `height` - Texture height in pixels
    /// * `is_dynamic` - Whether this texture changes frequently (e.g., video frames)
    /// * `use_linear_filter` - Use linear filtering (true) or nearest neighbor (false)
    pub fn register(
        &mut self,
        texture_id: TextureId,
        texture: Texture,
        width: u32,
        height: u32,
        is_dynamic: bool,
        use_linear_filter: bool,
    ) {
        let view = texture.create_view(&Default::default());

        let sampler = if use_linear_filter {
            &self.linear_sampler
        } else {
            &self.nearest_sampler
        };

        let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some(&format!("External Texture {} Bind Group", texture_id.get())),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        });

        let entry = ExternalTextureEntry {
            texture,
            view,
            bind_group,
            width,
            height,
            is_dynamic,
            frame_count: 0,
        };

        self.textures.insert(texture_id.get(), entry);

        tracing::trace!(
            "Registered external texture {}: {}x{}, dynamic={}",
            texture_id.get(),
            width,
            height,
            is_dynamic
        );
    }

    /// Update an existing texture's GPU data
    ///
    /// Use this for dynamic textures like video frames where the texture
    /// dimensions stay the same but the content changes.
    ///
    /// Returns `true` if the texture was updated, `false` if not found.
    pub fn update(&mut self, texture_id: TextureId, new_texture: Texture) -> bool {
        if let Some(entry) = self.textures.get_mut(&texture_id.get()) {
            // Create new view for the new texture
            let view = new_texture.create_view(&Default::default());

            // Recreate bind group with new texture view
            let sampler = &self.linear_sampler; // Use linear for dynamic textures

            let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
                label: Some(&format!("External Texture {} Bind Group", texture_id.get())),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::TextureView(&view),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::Sampler(sampler),
                    },
                ],
            });

            entry.texture = new_texture;
            entry.view = view;
            entry.bind_group = bind_group;
            entry.frame_count += 1;

            true
        } else {
            false
        }
    }

    /// Unregister an external texture
    ///
    /// Returns `true` if the texture was removed, `false` if not found.
    pub fn unregister(&mut self, texture_id: TextureId) -> bool {
        let removed = self.textures.remove(&texture_id.get()).is_some();
        if removed {
            tracing::trace!("Unregistered external texture {}", texture_id.get());
        }
        removed
    }

    /// Get an external texture entry by ID
    pub fn get(&self, texture_id: TextureId) -> Option<&ExternalTextureEntry> {
        self.textures.get(&texture_id.get())
    }

    /// Check if a texture is registered
    pub fn contains(&self, texture_id: TextureId) -> bool {
        self.textures.contains_key(&texture_id.get())
    }

    /// Get the number of registered textures
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    /// Clear all registered textures
    pub fn clear(&mut self) {
        self.textures.clear();
        tracing::trace!("Cleared all external textures");
    }

    /// Get iterator over all texture IDs
    pub fn texture_ids(&self) -> impl Iterator<Item = TextureId> + '_ {
        self.textures.keys().map(|&id| TextureId::new(id))
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    #[test]
    fn test_texture_id_mapping() {
        let id = TextureId::new(42);
        assert_eq!(id.get(), 42);

        let id2 = TextureId::new(42);
        assert_eq!(id, id2);

        let id3 = TextureId::new(43);
        assert_ne!(id, id3);
    }
}
