//! Pipeline specialization for optimal GPU rendering
//!
//! Based on Bevy/Iced patterns, this module provides:
//! - Pipeline variants for different rendering requirements
//! - Automatic pipeline selection based on Paint properties
//! - Pipeline caching to avoid recreation overhead
//!
//! Performance benefits:
//! - Opaque draws skip blending (faster)
//! - Specialized pipelines avoid unnecessary GPU work
//! - Cache eliminates pipeline recreation overhead

use std::collections::HashMap;
use wgpu::RenderPipeline;

use flui_painting::Paint;

/// Pipeline key identifying a specific pipeline variant
///
/// Uses bitflags for compact representation and fast hashing.
/// Each bit represents a different pipeline feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineKey {
    bits: u32,
}

impl PipelineKey {
    // Feature flags
    const ALPHA_BLEND: u32 = 1 << 0; // Requires alpha blending
    const TEXTURED: u32 = 1 << 1; // Uses textures
    const MSAA_4X: u32 = 1 << 2; // 4x MSAA enabled
    const MSAA_8X: u32 = 1 << 3; // 8x MSAA enabled
    const HDR: u32 = 1 << 4; // HDR color space
    const PREMUL_ALPHA: u32 = 1 << 5; // Premultiplied alpha

    /// Create opaque pipeline key (no blending, fastest)
    pub fn opaque() -> Self {
        Self { bits: 0 }
    }

    /// Create alpha blending pipeline key
    pub fn alpha_blend() -> Self {
        Self {
            bits: Self::ALPHA_BLEND,
        }
    }

    /// Create textured pipeline key
    pub fn textured() -> Self {
        Self {
            bits: Self::TEXTURED,
        }
    }

    /// Enable alpha blending
    pub fn with_alpha_blend(mut self) -> Self {
        self.bits |= Self::ALPHA_BLEND;
        self
    }

    /// Enable texturing
    pub fn with_textured(mut self) -> Self {
        self.bits |= Self::TEXTURED;
        self
    }

    /// Enable 4x MSAA
    pub fn with_msaa_4x(mut self) -> Self {
        self.bits |= Self::MSAA_4X;
        self
    }

    /// Enable 8x MSAA
    pub fn with_msaa_8x(mut self) -> Self {
        self.bits |= Self::MSAA_8X;
        self
    }

    /// Enable HDR
    pub fn with_hdr(mut self) -> Self {
        self.bits |= Self::HDR;
        self
    }

    /// Enable premultiplied alpha
    pub fn with_premul_alpha(mut self) -> Self {
        self.bits |= Self::PREMUL_ALPHA;
        self
    }

    /// Check if pipeline requires alpha blending
    pub fn is_alpha_blended(&self) -> bool {
        self.bits & Self::ALPHA_BLEND != 0
    }

    /// Check if pipeline uses textures
    pub fn is_textured(&self) -> bool {
        self.bits & Self::TEXTURED != 0
    }

    /// Get MSAA sample count
    pub fn msaa_samples(&self) -> u32 {
        if self.bits & Self::MSAA_8X != 0 {
            8
        } else if self.bits & Self::MSAA_4X != 0 {
            4
        } else {
            1
        }
    }
}

/// Pipeline cache managing specialized pipeline variants
///
/// Automatically creates and caches pipelines on-demand based on PipelineKey.
/// Avoids expensive pipeline recreation by reusing cached variants.
pub struct PipelineCache {
    /// Cached pipelines indexed by key
    cache: HashMap<PipelineKey, RenderPipeline>,

    /// Shader module (shared across all pipelines)
    shader: wgpu::ShaderModule,

    /// Surface format
    format: wgpu::TextureFormat,
}

impl PipelineCache {
    /// Create a new pipeline cache
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `shader_source` - WGSL shader source code
    /// * `format` - Surface texture format
    pub fn new(device: &wgpu::Device, shader_source: &str, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        Self {
            cache: HashMap::new(),
            shader,
            format,
        }
    }

    /// Get or create a pipeline for the given key
    ///
    /// Returns cached pipeline if available, otherwise creates and caches new one.
    pub fn get_or_create(&mut self, device: &wgpu::Device, key: PipelineKey) -> &RenderPipeline {
        // Check if pipeline exists
        if !self.cache.contains_key(&key) {
            // Create and insert new pipeline
            let pipeline = self.create_pipeline(device, key);
            self.cache.insert(key, pipeline);
        }

        // Return cached pipeline (guaranteed to exist now)
        &self.cache[&key]
    }

    /// Create a new specialized pipeline
    fn create_pipeline(&self, device: &wgpu::Device, key: PipelineKey) -> RenderPipeline {
        #[cfg(debug_assertions)]
        tracing::trace!("PipelineCache::create_pipeline: key={:?}", key);

        // Create layout
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shape Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        // Configure blend state based on key
        let blend_state = if key.is_alpha_blended() {
            Some(wgpu::BlendState::ALPHA_BLENDING)
        } else {
            None // Opaque - no blending (faster!)
        };

        // Configure MSAA
        let msaa_samples = key.msaa_samples();

        // Create specialized pipeline
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Specialized Shape Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_main"),
                buffers: &[super::vertex::Vertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: blend_state,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: msaa_samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }

    /// Get number of cached pipelines
    pub fn cached_count(&self) -> usize {
        self.cache.len()
    }

    /// Clear the cache (useful for resource cleanup)
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

/// Helper to determine pipeline key from paint properties
pub fn pipeline_key_from_paint(paint: &Paint) -> PipelineKey {
    let color = paint.color;

    // Check if we need alpha blending
    if color.a < 255 {
        PipelineKey::alpha_blend()
    } else {
        PipelineKey::opaque()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_key_opaque() {
        let key = PipelineKey::opaque();
        assert!(!key.is_alpha_blended());
        assert!(!key.is_textured());
        assert_eq!(key.msaa_samples(), 1);
    }

    #[test]
    fn test_pipeline_key_alpha_blend() {
        let key = PipelineKey::alpha_blend();
        assert!(key.is_alpha_blended());
        assert!(!key.is_textured());
        assert_eq!(key.msaa_samples(), 1);
    }

    #[test]
    fn test_pipeline_key_builder() {
        let key = PipelineKey::opaque().with_alpha_blend().with_msaa_4x();

        assert!(key.is_alpha_blended());
        assert_eq!(key.msaa_samples(), 4);
    }

    #[test]
    fn test_pipeline_key_msaa() {
        let key_4x = PipelineKey::opaque().with_msaa_4x();
        assert_eq!(key_4x.msaa_samples(), 4);

        let key_8x = PipelineKey::opaque().with_msaa_8x();
        assert_eq!(key_8x.msaa_samples(), 8);

        // 8x overrides 4x
        let key_both = PipelineKey::opaque().with_msaa_4x().with_msaa_8x();
        assert_eq!(key_both.msaa_samples(), 8);
    }

    #[test]
    fn test_pipeline_key_equality() {
        let key1 = PipelineKey::alpha_blend().with_msaa_4x();
        let key2 = PipelineKey::opaque().with_alpha_blend().with_msaa_4x();

        assert_eq!(key1, key2);
    }
}
