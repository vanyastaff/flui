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

use flui_painting::{BlendMode, Paint};
use wgpu::RenderPipeline;

/// Pipeline key identifying a specific pipeline variant
///
/// Uses bitflags for compact representation of MSAA / blend-enable state, plus a
/// [`BlendMode`] dimension so the tessellated path produces (and caches) one
/// pipeline per fixed-function Porter-Duff blend mode.
///
/// The `blend_mode` is only meaningful when blending is enabled (the
/// [`Self::ALPHA_BLEND`] bit). Opaque keys carry `BlendMode::SrcOver` purely as
/// a canonical value so equal opaque keys hash equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipelineKey {
    bits: u32,
    /// Fixed-function blend mode for the color target. Only consulted when
    /// [`Self::is_alpha_blended`] is true.
    blend_mode: BlendMode,
}

impl PipelineKey {
    // Feature flags
    const ALPHA_BLEND: u32 = 1 << 0; // Requires alpha blending
    const MSAA_4X: u32 = 1 << 2; // 4x MSAA enabled
    const MSAA_8X: u32 = 1 << 3; // 8x MSAA enabled

    /// Create opaque pipeline key (no blending, fastest)
    pub fn opaque() -> Self {
        Self {
            bits: 0,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create an alpha-blending pipeline key for the default `SrcOver` mode.
    pub fn alpha_blend() -> Self {
        Self {
            bits: Self::ALPHA_BLEND,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create an alpha-blending pipeline key for a specific fixed-function
    /// [`BlendMode`].
    ///
    /// The caller is responsible for passing a Porter-Duff mode; advanced
    /// (dst-read) modes are mapped to `SrcOver` upstream in
    /// [`pipeline_key_from_paint`].
    pub fn with_blend(mode: BlendMode) -> Self {
        Self {
            bits: Self::ALPHA_BLEND,
            blend_mode: mode,
        }
    }

    /// Check if pipeline requires alpha blending
    pub fn is_alpha_blended(self) -> bool {
        self.bits & Self::ALPHA_BLEND != 0
    }

    /// The fixed-function blend mode this key selects (only meaningful when
    /// [`Self::is_alpha_blended`] is true).
    pub fn blend_mode(self) -> BlendMode {
        self.blend_mode
    }

    /// Get MSAA sample count
    pub fn msaa_samples(self) -> u32 {
        if self.bits & Self::MSAA_8X != 0 {
            8
        } else if self.bits & Self::MSAA_4X != 0 {
            4
        } else {
            1
        }
    }
}

/// Map a fixed-function Porter-Duff [`BlendMode`] to its premultiplied-alpha
/// [`wgpu::BlendState`].
///
/// These factors assume PREMULTIPLIED source and destination color (the
/// tessellated `shape.wgsl` fragment emits `rgb * a`), which is the only form in
/// which fixed-function Porter-Duff blending is correct. Color and alpha
/// components use identical factors unless a mode requires otherwise.
///
/// `SrcOver` is exactly [`wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING`].
///
/// Advanced (separable/non-separable, dst-reading) modes are *not* handled here
/// — they are mapped to `SrcOver` in [`pipeline_key_from_paint`] before a key is
/// built, so this function only ever receives Porter-Duff modes. Any advanced
/// mode that reaches it (a logic error) falls back to `SrcOver` rather than
/// panicking.
pub fn blend_state_for(mode: BlendMode) -> wgpu::BlendState {
    use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState};

    // Helper: build a BlendState whose color and alpha components share the
    // same (src, dst) factors with the Add operation.
    let same = |src: BlendFactor, dst: BlendFactor| BlendState {
        color: BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: BlendOperation::Add,
        },
    };

    match mode {
        BlendMode::Clear => same(BlendFactor::Zero, BlendFactor::Zero),
        BlendMode::Src => same(BlendFactor::One, BlendFactor::Zero),
        BlendMode::Dst => same(BlendFactor::Zero, BlendFactor::One),
        BlendMode::SrcOver => same(BlendFactor::One, BlendFactor::OneMinusSrcAlpha),
        BlendMode::DstOver => same(BlendFactor::OneMinusDstAlpha, BlendFactor::One),
        BlendMode::SrcIn => same(BlendFactor::DstAlpha, BlendFactor::Zero),
        BlendMode::DstIn => same(BlendFactor::Zero, BlendFactor::SrcAlpha),
        BlendMode::SrcOut => same(BlendFactor::OneMinusDstAlpha, BlendFactor::Zero),
        BlendMode::DstOut => same(BlendFactor::Zero, BlendFactor::OneMinusSrcAlpha),
        BlendMode::SrcATop => same(BlendFactor::DstAlpha, BlendFactor::OneMinusSrcAlpha),
        BlendMode::DstATop => same(BlendFactor::OneMinusDstAlpha, BlendFactor::SrcAlpha),
        BlendMode::Xor => same(BlendFactor::OneMinusDstAlpha, BlendFactor::OneMinusSrcAlpha),
        // Plus / Lighter: additive.
        BlendMode::Plus => same(BlendFactor::One, BlendFactor::One),
        // Modulate: src * dst. The color channels multiply by the destination
        // color; alpha multiplies by destination alpha.
        BlendMode::Modulate => BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::Dst,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::DstAlpha,
                dst_factor: BlendFactor::Zero,
                operation: BlendOperation::Add,
            },
        },
        // Advanced modes never reach here (mapped to SrcOver upstream). Fall
        // back defensively rather than panicking.
        _ => BlendState::PREMULTIPLIED_ALPHA_BLENDING,
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

    /// Viewport bind group layout (for coordinate transformation)
    viewport_bind_group_layout: wgpu::BindGroupLayout,
}

impl PipelineCache {
    /// Create a new pipeline cache
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `shader_source` - WGSL shader source code
    /// * `format` - Surface texture format
    /// * `viewport_bind_group_layout` - Bind group layout for viewport uniform
    pub fn new(
        device: &wgpu::Device,
        shader_source: &str,
        format: wgpu::TextureFormat,
        viewport_bind_group_layout: wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        Self {
            cache: HashMap::new(),
            shader,
            format,
            viewport_bind_group_layout,
        }
    }

    /// Get or create a pipeline for the given key
    ///
    /// Returns cached pipeline if available, otherwise creates and caches new
    /// one.
    pub fn get_or_create(&mut self, device: &wgpu::Device, key: PipelineKey) -> &RenderPipeline {
        // `entry` needs `&mut self.cache`; `create_pipeline` needs `&self.shader` /
        // `self.format` / `self.viewport_bind_group_layout` — disjoint fields.
        // We pre-create on miss, then insert, to keep one logical lookup on hit.
        if !self.cache.contains_key(&key) {
            let pipeline = self.create_pipeline(device, key);
            self.cache.insert(key, pipeline);
        }
        // Safety: just inserted above on miss path.
        &self.cache[&key]
    }

    /// Create a new specialized pipeline
    fn create_pipeline(&self, device: &wgpu::Device, key: PipelineKey) -> RenderPipeline {
        #[cfg(debug_assertions)]
        tracing::trace!("PipelineCache::create_pipeline: key={:?}", key);

        // Create layout with viewport bind group
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shape Pipeline Layout"),
            bind_group_layouts: &[Some(&self.viewport_bind_group_layout)],
            immediate_size: 0,
        });

        // Configure blend state based on key. The tessellated fragment shader
        // emits PREMULTIPLIED alpha, so blended pipelines use the premultiplied
        // Porter-Duff factors for `key.blend_mode()`. SrcOver maps to
        // PREMULTIPLIED_ALPHA_BLENDING — visually identical to the previous
        // straight-alpha output now that the shader premultiplies.
        let blend_state = if key.is_alpha_blended() {
            Some(blend_state_for(key.blend_mode()))
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
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: blend_state,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
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
            multiview_mask: None,
            cache: None,
        })
    }

    /// Get a reference to the viewport bind group layout
    ///
    /// This is needed to create bind groups that are compatible with pipelines
    /// created by this cache. In wgpu, bind groups must be created with the
    /// exact same layout object that the pipeline expects.
    pub fn viewport_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.viewport_bind_group_layout
    }
}

/// Helper to determine pipeline key from paint properties.
///
/// Blend-mode routing (Phase A — fixed-function Porter-Duff):
/// - A non-`SrcOver` Porter-Duff mode always selects a blended pipeline keyed by
///   that mode (the blend stage is required even for fully opaque source, e.g.
///   `Clear`/`DstOut` punch-outs and `Plus` additive).
/// - An advanced (dst-reading) mode — `Screen`, `Multiply`, `Overlay`, the HSL
///   modes, etc. — is *not* expressible as a fixed-function blend. It is mapped
///   to `SrcOver` here with a one-shot `tracing::warn!` so the fallback is
///   observable rather than silent. These land in Phase B.
/// - `SrcOver` keeps the legacy fast heuristic: opaque source (`a == 255`) skips
///   the blend stage entirely; translucent source uses the SrcOver blend.
pub fn pipeline_key_from_paint(paint: &Paint) -> PipelineKey {
    let mode = paint.blend_mode;

    if mode == BlendMode::SrcOver {
        // Legacy fast path: opaque SrcOver skips blending.
        return if paint.color.a < 255 {
            PipelineKey::alpha_blend()
        } else {
            PipelineKey::opaque()
        };
    }

    if mode.is_porter_duff() {
        // Fixed-function Porter-Duff: dedicated blended pipeline for this mode.
        PipelineKey::with_blend(mode)
    } else {
        // Advanced / dst-read mode: not representable with fixed-function
        // blending. Fall back to SrcOver and warn once per process so the gap
        // is honest and visible (Phase B will implement these via a dst-read
        // shader pass).
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            tracing::warn!(
                blend_mode = ?mode,
                "shape blend mode {:?} is an advanced (dst-read) mode not supported \
                 by the Phase A fixed-function path; falling back to SrcOver \
                 (Phase B) (logged once per process)",
                mode
            );
        }
        // Preserve the opaque/alpha distinction for the SrcOver fallback.
        if paint.color.a < 255 {
            PipelineKey::alpha_blend()
        } else {
            PipelineKey::opaque()
        }
    }
}

/// Pure-logic tests for the blend-mode routing and Porter-Duff factor table.
/// Not gated behind `enable-wgpu-tests` because they need no GPU device, so they
/// run in the default `cargo test --lib` gate.
#[cfg(test)]
mod blend_logic {
    use flui_painting::BlendMode;
    use wgpu::{BlendFactor, BlendOperation};

    use super::*;

    #[test]
    fn srcover_opaque_skips_blending() {
        let paint = Paint::fill(flui_types::Color::rgb(10, 20, 30)); // a == 255, SrcOver
        let key = pipeline_key_from_paint(&paint);
        assert!(
            !key.is_alpha_blended(),
            "opaque SrcOver must skip the blend stage"
        );
    }

    #[test]
    fn srcover_translucent_uses_blend() {
        let paint = Paint::fill(flui_types::Color::rgba(10, 20, 30, 128));
        let key = pipeline_key_from_paint(&paint);
        assert!(key.is_alpha_blended());
        assert_eq!(key.blend_mode(), BlendMode::SrcOver);
    }

    #[test]
    fn porter_duff_modes_select_their_own_pipeline() {
        // Even an opaque source must take the blend stage for non-SrcOver modes
        // (Clear punches out, Plus adds, etc.).
        for mode in [
            BlendMode::Clear,
            BlendMode::Src,
            BlendMode::Dst,
            BlendMode::DstOver,
            BlendMode::SrcIn,
            BlendMode::DstIn,
            BlendMode::SrcOut,
            BlendMode::DstOut,
            BlendMode::SrcATop,
            BlendMode::DstATop,
            BlendMode::Xor,
            BlendMode::Plus,
            BlendMode::Modulate,
        ] {
            let paint = Paint::fill(flui_types::Color::rgb(255, 0, 0)).with_blend_mode(mode);
            let key = pipeline_key_from_paint(&paint);
            assert!(key.is_alpha_blended(), "{mode:?} must enable blending");
            assert_eq!(key.blend_mode(), mode, "{mode:?} must key its own pipeline");
        }
    }

    #[test]
    fn advanced_modes_fall_back_to_srcover() {
        for mode in [
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Multiply,
            BlendMode::Darken,
            BlendMode::Hue,
            BlendMode::Luminosity,
        ] {
            let paint = Paint::fill(flui_types::Color::rgb(255, 0, 0)).with_blend_mode(mode);
            let key = pipeline_key_from_paint(&paint);
            // Opaque source under the fallback keeps the opaque key (SrcOver).
            assert!(
                !key.is_alpha_blended(),
                "{mode:?} fallback should reuse the SrcOver heuristic"
            );
            assert_eq!(key.blend_mode(), BlendMode::SrcOver);
        }
    }

    #[test]
    fn srcover_blend_state_matches_premultiplied() {
        // SrcOver must equal wgpu's PREMULTIPLIED_ALPHA_BLENDING so the shader's
        // premultiply switch is a no-op visually.
        assert_eq!(
            blend_state_for(BlendMode::SrcOver),
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
        );
    }

    #[test]
    fn blend_state_factor_table_is_correct() {
        let c = |m: BlendMode| blend_state_for(m).color;
        let a = |m: BlendMode| blend_state_for(m).alpha;

        // Clear: zero everything.
        assert_eq!(c(BlendMode::Clear).src_factor, BlendFactor::Zero);
        assert_eq!(c(BlendMode::Clear).dst_factor, BlendFactor::Zero);

        // Src: keep source, drop dest.
        assert_eq!(c(BlendMode::Src).src_factor, BlendFactor::One);
        assert_eq!(c(BlendMode::Src).dst_factor, BlendFactor::Zero);

        // Plus: additive.
        assert_eq!(c(BlendMode::Plus).src_factor, BlendFactor::One);
        assert_eq!(c(BlendMode::Plus).dst_factor, BlendFactor::One);

        // DstOver: dst wins where it covers.
        assert_eq!(
            c(BlendMode::DstOver).src_factor,
            BlendFactor::OneMinusDstAlpha
        );
        assert_eq!(c(BlendMode::DstOver).dst_factor, BlendFactor::One);

        // Modulate: color uses Dst (src*dst), alpha uses DstAlpha.
        assert_eq!(c(BlendMode::Modulate).src_factor, BlendFactor::Dst);
        assert_eq!(c(BlendMode::Modulate).dst_factor, BlendFactor::Zero);
        assert_eq!(a(BlendMode::Modulate).src_factor, BlendFactor::DstAlpha);

        // All Porter-Duff modes use the Add operation.
        for mode in [
            BlendMode::Clear,
            BlendMode::SrcOver,
            BlendMode::Xor,
            BlendMode::Modulate,
            BlendMode::Plus,
        ] {
            assert_eq!(c(mode).operation, BlendOperation::Add);
            assert_eq!(a(mode).operation, BlendOperation::Add);
        }
    }

    #[test]
    fn distinct_blend_modes_produce_distinct_keys() {
        let red = flui_types::Color::rgb(255, 0, 0);
        let k_plus = pipeline_key_from_paint(&Paint::fill(red).with_blend_mode(BlendMode::Plus));
        let k_clear = pipeline_key_from_paint(&Paint::fill(red).with_blend_mode(BlendMode::Clear));
        assert_ne!(
            k_plus, k_clear,
            "different blend modes must hash to different pipeline keys"
        );
    }

    /// Golden lock for the current routing of `pipeline_key_from_paint`.
    ///
    /// Asserts the exact key produced for each blend mode so any change to the
    /// routing is forced to produce a diff here — accidental regressions surface
    /// as a test failure rather than a silent render change.
    ///
    /// ## SrcOver / Porter-Duff record
    ///
    /// - `SrcOver` + opaque source → opaque key (no blend stage).
    /// - `SrcOver` + translucent source → alpha-blend key (`SrcOver` mode).
    /// - Every other Porter-Duff mode → alpha-blend key keyed to that mode.
    ///
    /// ## Advanced-mode record (current: warn-fallback to SrcOver)
    ///
    /// All 15 advanced modes currently fall back to the SrcOver heuristic
    /// (opaque → opaque key, translucent → alpha-blend SrcOver key).  When
    /// the advanced-blend key is introduced, the routing changes and this
    /// test must be updated to match.
    #[test]
    fn pipeline_key_routing_golden() {
        let opaque = flui_types::Color::rgb(200, 100, 50); // a == 255
        let translucent = flui_types::Color::rgba(200, 100, 50, 128);

        // ── SrcOver ─────────────────────────────────────────────────────────
        let k = pipeline_key_from_paint(&Paint::fill(opaque).with_blend_mode(BlendMode::SrcOver));
        assert!(!k.is_alpha_blended(), "SrcOver + opaque → opaque key");
        assert_eq!(k.blend_mode(), BlendMode::SrcOver);

        let k =
            pipeline_key_from_paint(&Paint::fill(translucent).with_blend_mode(BlendMode::SrcOver));
        assert!(k.is_alpha_blended(), "SrcOver + translucent → blend key");
        assert_eq!(k.blend_mode(), BlendMode::SrcOver);

        // ── Porter-Duff modes (all 13 non-SrcOver) ──────────────────────────
        for mode in [
            BlendMode::Clear,
            BlendMode::Src,
            BlendMode::Dst,
            BlendMode::DstOver,
            BlendMode::SrcIn,
            BlendMode::DstIn,
            BlendMode::SrcOut,
            BlendMode::DstOut,
            BlendMode::SrcATop,
            BlendMode::DstATop,
            BlendMode::Xor,
            BlendMode::Plus,
            BlendMode::Modulate,
        ] {
            let k = pipeline_key_from_paint(&Paint::fill(opaque).with_blend_mode(mode));
            assert!(
                k.is_alpha_blended(),
                "{mode:?}: Porter-Duff must always use the blend stage"
            );
            assert_eq!(
                k.blend_mode(),
                mode,
                "{mode:?}: key must encode the exact mode"
            );
        }

        // ── Advanced modes (current fallback: SrcOver-heuristic) ────────────
        // Opaque source → opaque key; translucent source → alpha-blend SrcOver.
        // When the advanced-blend key is introduced these assertions must be
        // updated to match the new dedicated key.
        for mode in [
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Multiply,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ] {
            let k_opaque = pipeline_key_from_paint(&Paint::fill(opaque).with_blend_mode(mode));
            assert!(
                !k_opaque.is_alpha_blended(),
                "{mode:?} opaque fallback: current routing produces opaque key"
            );
            assert_eq!(
                k_opaque.blend_mode(),
                BlendMode::SrcOver,
                "{mode:?} opaque fallback: mode must be SrcOver"
            );

            let k_trans = pipeline_key_from_paint(&Paint::fill(translucent).with_blend_mode(mode));
            assert!(
                k_trans.is_alpha_blended(),
                "{mode:?} translucent fallback: current routing produces blend key"
            );
            assert_eq!(
                k_trans.blend_mode(),
                BlendMode::SrcOver,
                "{mode:?} translucent fallback: mode must be SrcOver"
            );
        }
    }
}

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_key_opaque() {
        let key = PipelineKey::opaque();
        assert!(!key.is_alpha_blended());
        assert_eq!(key.msaa_samples(), 1);
    }

    #[test]
    fn test_pipeline_key_alpha_blend() {
        let key = PipelineKey::alpha_blend();
        assert!(key.is_alpha_blended());
        assert_eq!(key.msaa_samples(), 1);
    }

    #[test]
    fn test_pipeline_key_msaa_samples_default() {
        // opaque() has no MSAA bits set → 1 sample
        let key = PipelineKey::opaque();
        assert_eq!(key.msaa_samples(), 1);
    }
}
