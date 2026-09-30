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
pub(crate) struct PipelineKey {
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
    pub(crate) fn opaque() -> Self {
        Self {
            bits: 0,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create an alpha-blending pipeline key for the default `SrcOver` mode.
    pub(crate) fn alpha_blend() -> Self {
        Self {
            bits: Self::ALPHA_BLEND,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Create an alpha-blending pipeline key for a specific fixed-function
    /// [`BlendMode`].
    ///
    /// Intended for fixed-function Porter-Duff modes. Advanced (dst-read) modes
    /// may also be passed, but the tessellated record path intercepts them via
    /// [`BlendMode::is_advanced`] (see `DrawBatcher::add_tessellated_with_key`)
    /// before the key reaches [`PipelineCache`], so an advanced key never selects
    /// a fixed-function pipeline.
    pub(crate) fn with_blend(mode: BlendMode) -> Self {
        Self {
            bits: Self::ALPHA_BLEND,
            blend_mode: mode,
        }
    }

    /// Check if pipeline requires alpha blending
    pub(crate) fn is_alpha_blended(self) -> bool {
        self.bits & Self::ALPHA_BLEND != 0
    }

    /// The fixed-function blend mode this key selects (only meaningful when
    /// [`Self::is_alpha_blended`] is true).
    pub(crate) fn blend_mode(self) -> BlendMode {
        self.blend_mode
    }

    /// Get MSAA sample count
    pub(crate) fn msaa_samples(self) -> u32 {
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
/// Advanced (separable/non-separable, dst-reading) modes are *not* handled here:
/// shape records divert to `DrawItem::AdvancedShape` before a pipeline key is
/// built (see `DrawBatcher::add_tessellated_with_key`), and
/// [`PipelineCache::get_or_create`] debug-asserts that no advanced key reaches the
/// cache. The defensive `_` arm below maps any stray advanced mode to `SrcOver`
/// in release rather than panicking — but that path is a routing logic error.
pub(crate) fn blend_state_for(mode: BlendMode) -> wgpu::BlendState {
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

/// The `k` in a blend mode's destination factor `D = k * srcAlpha`, for the
/// modes where folding clip coverage into the source alpha is WRONG; `None`
/// for the modes where it is exact.
///
/// ## Why folding is not always exact
///
/// The shape shader reports partial clip coverage by scaling its premultiplied
/// output, so the blender computes
///
/// ```text
/// S * (src * coverage) + D(alpha * coverage) * dst
/// ```
///
/// where the coverage-correct answer is
///
/// ```text
/// mix(dst, S * src + D(alpha) * dst, coverage)
/// ```
///
/// The SOURCE halves always agree: no mode's source factor `S` reads source
/// alpha (they read `0`, `1`, `dstAlpha`, `1 - dstAlpha`, or `dst`), so the
/// `coverage` scale factors straight out. The DESTINATION halves agree iff
/// `D(alpha * coverage) == 1 - coverage * (1 - D(alpha))` for all inputs,
/// which holds exactly when `D` has the absorbing form `1 - k*srcAlpha` (or is
/// `One`). Those modes return `None` here and need no correction.
///
/// The rest have `D = k * srcAlpha`, and `k` is what this returns:
///
/// | `k`   | destination factor | modes                                     |
/// |-------|--------------------|-------------------------------------------|
/// | `0.0` | `Zero`             | `Clear`, `Src`, `SrcIn`, `SrcOut`, `Modulate` |
/// | `1.0` | `SrcAlpha`         | `DstIn`, `DstATop`                        |
///
/// Note `DstOut` is in neither list: it is the erase-by-alpha mode one would
/// expect to break alongside `Clear`, and its `(Zero, OneMinusSrcAlpha)` pair
/// has exactly the absorbing shape, so it is already correct.
///
/// The value is handed to `shape_fragment_second_source.wgsl` as the
/// `destination_alpha_scale` overridable constant, which turns it into the
/// second blend source `coverage * (1 - k*alpha)`;
/// [`coverage_blend_state_for`] pairs that with `dst_factor = OneMinusSrc1`.
///
/// Advanced (dst-reading) modes never reach a fixed-function blend state and
/// return `None`.
pub(crate) fn destination_alpha_scale_for(mode: BlendMode) -> Option<f32> {
    match mode {
        BlendMode::Clear
        | BlendMode::Src
        | BlendMode::SrcIn
        | BlendMode::SrcOut
        | BlendMode::Modulate => Some(0.0),
        BlendMode::DstIn | BlendMode::DstATop => Some(1.0),
        _ => None,
    }
}

/// [`blend_state_for`], corrected so partial coverage feathers the blend
/// instead of applying it at full strength.
///
/// Identical to [`blend_state_for`] except for the modes
/// [`destination_alpha_scale_for`] names: their destination factor becomes
/// `OneMinusSrc1`, which reads the coverage term
/// `shape_fragment_second_source.wgsl` emits as its second blend source.
///
/// Only valid on a device with [`wgpu::Features::DUAL_SOURCE_BLENDING`], and
/// only paired with that shader. `PipelineCache` owns both halves of that
/// pairing.
pub(crate) fn coverage_blend_state_for(mode: BlendMode) -> wgpu::BlendState {
    let folded = blend_state_for(mode);
    if destination_alpha_scale_for(mode).is_none() {
        return folded;
    }

    let feathered = |component: wgpu::BlendComponent| wgpu::BlendComponent {
        dst_factor: wgpu::BlendFactor::OneMinusSrc1,
        ..component
    };
    wgpu::BlendState {
        color: feathered(folded.color),
        alpha: feathered(folded.alpha),
    }
}

/// Classify a blend mode as "tile-safe" for SSAA compositing.
///
/// A mode is tile-safe iff compositing a **transparent-source** tile (src alpha=0,
/// src color=0) onto any destination leaves the destination unchanged:
///
///   `blend(transparent_src, dst, mode) == dst`   for ALL dst values.
///
/// Derivation from `blend_state_for` with src=(0,0,0,0):
///
/// | Mode      | src-factor × 0 + dst-factor × dst | tile-safe? |
/// |-----------|-----------------------------------|-----------|
/// | SrcOver   | 0 + (1-0)·dst = dst               | ✓         |
/// | Dst       | 0 + 1·dst = dst                   | ✓         |
/// | DstOver   | 0 + 1·dst = dst                   | ✓         |
/// | DstOut    | 0 + (1-0)·dst = dst               | ✓         |
/// | SrcATop   | 0 + (1-0)·dst = dst               | ✓         |
/// | Xor       | 0 + (1-0)·dst = dst               | ✓         |
/// | Plus      | 0 + 1·dst = dst                   | ✓         |
/// | Clear     | 0 + 0·dst = 0                     | ✗ (kills dst) |
/// | Src       | 0 + 0·dst = 0                     | ✗         |
/// | SrcIn     | dst_a·0 + 0·dst = 0               | ✗         |
/// | DstIn     | 0 + src_a(=0)·dst = 0             | ✗         |
/// | SrcOut    | (1-dst_a)·0 + 0·dst = 0           | ✗         |
/// | DstATop   | dst_a·0 + src_a(=0)·dst = 0       | ✗         |
/// | Modulate  | dst·0 + dst·src_a(=0) = 0         | ✗         |
///
/// (Rows where a dst-factor depends on `src_a` — DstIn, DstATop, Modulate —
/// vanish only *because* `src_a = 0` here; they are state-dependent factors, not
/// the constant `Zero`. No test ties the partition to `Color::blend`, so this
/// hand-derivation is the only record of it.)
///
/// Advanced (dst-read) modes are NOT tile-safe by this definition, but they are
/// handled separately via `flush_advanced_layer` (not fixed-function blend).
/// Use `blend.is_advanced()` to detect them before calling this function.
///
/// ## Coverage-destructive exception set
///
/// The following modes are NOT tile-safe and NOT advanced (Porter-Duff modes
/// that destroy the destination where the SSAA tile is transparent):
///
///   Clear, Src, SrcIn, DstIn, SrcOut, DstATop, Modulate
///
/// These modes KEEP the existing tessellated (aliased) render path for fills.
/// This is an explicit, justified exception: routing them through the SSAA tile
/// would apply the blend to the transparent padding, incorrectly writing zeros
/// to destination pixels outside the shape's geometric boundary.
/// The aliased result is COMPLETE and CORRECT in coverage region — only the
/// 1px edge band is aliased, which is the same quality as the pre-PR-3 engine.
pub(crate) fn is_tile_safe_for_ssaa(mode: BlendMode) -> bool {
    matches!(
        mode,
        BlendMode::SrcOver
            | BlendMode::Dst
            | BlendMode::DstOver
            | BlendMode::DstOut
            | BlendMode::SrcATop
            | BlendMode::Xor
            | BlendMode::Plus
    )
}

/// Minimum device-pixel² area a shape must have before SSAA is eligible.
///
/// A 16×16 px² shape (256 px²) is the crossover below which SSAA overhead
/// (2× texture allocation + downsample pass) is not worth the quality gain.
/// Shapes smaller than this fall back to the tessellated (SDF-AA) path.
///
/// Previously declared in `batches/paths.rs` (private to that module);
/// centralised here so all batch modules share one constant.
pub(crate) const SSAA_AREA_THRESHOLD_PX_SQ: f32 = 256.0;

/// Returns `true` when SSAA tiling is both blend-safe and large enough to
/// justify the 2× oversample overhead.
///
/// A shape is SSAA-eligible when:
/// 1. `is_tile_safe_for_ssaa(mode)` — transparent padding in the tile cannot
///    destroy destination pixels (no coverage-destructive Porter-Duff mode).
/// 2. OR `mode.is_advanced()` — advanced (dst-read) modes are handled via
///    the GPU compositor path which is inherently tile-aware.
/// 3. AND `device_area >= SSAA_AREA_THRESHOLD_PX_SQ` — shape is large enough
///    to amortise the tile's allocation and downsample cost.
///
/// ## Shape-specific prefix rules
///
/// Most shapes call this directly. Two shapes have additional prefix guards
/// that must be evaluated BY THE CALLER:
///
/// - **arc** (`batches/shapes.rs`): must KEEP an outer `mode != BlendMode::SrcOver &&`
///   guard — SrcOver arcs reach the non-SrcOver reflection-fallback branch and
///   must stay on the tessellated path.
/// - **drrect** (`batches/shapes.rs`): the `mode == BlendMode::SrcOver ||`
///   prefix was dropped (it is subsumed by `is_tile_safe_for_ssaa(SrcOver)` == `true`).
pub(crate) fn ssaa_eligible_for(mode: BlendMode, device_area: f32) -> bool {
    (is_tile_safe_for_ssaa(mode) || mode.is_advanced()) && device_area >= SSAA_AREA_THRESHOLD_PX_SQ
}

/// The name of the overridable constant `shape_fragment_second_source.wgsl`
/// declares, through which [`PipelineCache`] hands each pipeline the
/// `destination_alpha_scale` for its blend mode.
///
/// Shared with the WGSL by spelling, not by type, so
/// `override_constant_names_match_their_shaders` pins the two together.
const DESTINATION_ALPHA_SCALE_OVERRIDE: &str = "destination_alpha_scale";

/// The two assemblies of one coverage-correct shader, from which a pipeline
/// cache builds pipelines.
///
/// They differ only in the fragment entry point: one folds clip coverage into
/// the source alpha, the other emits it as a second blend source. Passed as
/// one value rather than two `&str` parameters because swapping them compiles
/// and then fails at pipeline creation with a message about `@blend_src`
/// rather than about the mix-up.
///
/// Not shape-specific: `shaders::coverage_correct_shader!` builds one of these
/// for the tessellated shape and for each of the three instanced gradients.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CoverageShaderSources {
    /// Coverage folded into the source alpha. Compiles on every device.
    pub folded: &'static str,
    /// Coverage emitted as `@blend_src(1)`. Requires
    /// [`wgpu::Features::DUAL_SOURCE_BLENDING`] on the device that compiles it.
    pub second_source: &'static str,
}

/// The shader assembly a blend mode must be drawn with, and the blend state and
/// override constants that must accompany it.
///
/// Generic over how the caller holds a shader — a [`wgpu::ShaderModule`] the
/// cache compiled up front, or the `&'static str` source a lazily-built
/// pipeline still has to compile — because the DECISION is the same for both
/// and only the representation differs.
pub(super) struct CoverageBlendSelection<S> {
    /// The assembly to build the pipeline from.
    pub(super) shader: S,
    /// The blend state that assembly's output is valid for.
    pub(super) blend_state: wgpu::BlendState,
    /// Pipeline-override constants the assembly reads, empty for the folded one.
    pub(super) constants: Vec<(&'static str, f64)>,
}

/// Pick the assembly, blend state, and constants for `mode`.
///
/// Passing `second_source` as an `Option` rather than a `bool` is what makes
/// the three parts inseparable: there is no way to select the second-source
/// blend state on a device that has no second-source assembly to pair it with,
/// because the value the caller would return is not there to return.
///
/// `None` for `second_source` therefore means the folded assembly and the
/// UNCORRECTED factors — the documented fallback of ADR-0057, not a silent
/// downgrade.
pub(super) fn select_coverage_blend<S>(
    mode: BlendMode,
    folded: S,
    second_source: Option<S>,
) -> CoverageBlendSelection<S> {
    match destination_alpha_scale_for(mode).zip(second_source) {
        Some((destination_alpha_scale, shader)) => CoverageBlendSelection {
            shader,
            blend_state: coverage_blend_state_for(mode),
            constants: vec![(
                DESTINATION_ALPHA_SCALE_OVERRIDE,
                f64::from(destination_alpha_scale),
            )],
        },
        None => CoverageBlendSelection {
            shader: folded,
            blend_state: blend_state_for(mode),
            constants: Vec::new(),
        },
    }
}

/// Pipeline cache managing specialized pipeline variants
///
/// Automatically creates and caches pipelines on-demand based on PipelineKey.
/// Avoids expensive pipeline recreation by reusing cached variants.
pub(crate) struct PipelineCache {
    /// Cached pipelines indexed by key
    cache: HashMap<PipelineKey, RenderPipeline>,

    /// Shader module folding clip coverage into the source alpha. Used by every
    /// pipeline whose blend mode absorbs `1 - coverage` anyway, and by every
    /// pipeline at all when [`Self::second_source_shader`] is `None`.
    shader: wgpu::ShaderModule,

    /// Shader module emitting clip coverage as a second blend source.
    ///
    /// `None` when the device lacks [`wgpu::Features::DUAL_SOURCE_BLENDING`],
    /// in which case the modes that need it fall back to `shader` and keep a
    /// hard clip edge — see [`Self::new`].
    second_source_shader: Option<wgpu::ShaderModule>,

    /// Surface format
    format: wgpu::TextureFormat,

    /// Viewport bind group layout (for coordinate transformation)
    viewport_bind_group_layout: wgpu::BindGroupLayout,

    /// Per-batch SDF clip bind group layout (group 1).
    ///
    /// Owned here rather than passed in, because every pipeline this cache
    /// builds uses it and there is exactly one shader module behind them all.
    clip_bind_group_layout: wgpu::BindGroupLayout,
}

impl PipelineCache {
    /// Create a new pipeline cache.
    ///
    /// # Arguments
    /// * `device` - wgpu device
    /// * `shader_sources` - the two assemblies of the shape shader
    /// * `format` - Surface texture format
    /// * `viewport_bind_group_layout` - Bind group layout for viewport uniform
    ///
    /// # Coverage fidelity depends on the device
    ///
    /// [`CoverageShaderSources::second_source`] is compiled only when `device`
    /// enabled [`wgpu::Features::DUAL_SOURCE_BLENDING`] — naga rejects its
    /// `@blend_src` outputs otherwise. Where it is absent, the modes
    /// [`destination_alpha_scale_for`] names keep a HARD clip edge instead of a
    /// feathered one, because coverage has nowhere to ride but the source
    /// alpha their destination factor ignores.
    ///
    /// That is a deliberate, documented backend divergence rather than a
    /// silent one: the feature is optional in WebGPU and absent on the wasm32
    /// target this workspace ships, and the alternative — spending a paint's
    /// alpha channel on coverage — would change what a translucent `Clear`
    /// paint means on every backend to fix an edge on one.
    pub(crate) fn new(
        device: &wgpu::Device,
        shader_sources: CoverageShaderSources,
        format: wgpu::TextureFormat,
        viewport_bind_group_layout: wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(shader_sources.folded.into()),
        });

        let second_source_shader = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING)
            .then(|| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Shape Shader (coverage as second blend source)"),
                    source: wgpu::ShaderSource::Wgsl(shader_sources.second_source.into()),
                })
            });

        let clip_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Tessellated Clip Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        Self {
            cache: HashMap::new(),
            shader,
            second_source_shader,
            format,
            viewport_bind_group_layout,
            clip_bind_group_layout,
        }
    }

    /// Get or create a pipeline for the given key
    ///
    /// Returns cached pipeline if available, otherwise creates and caches new
    /// one.
    pub(crate) fn get_or_create(
        &mut self,
        device: &wgpu::Device,
        key: PipelineKey,
    ) -> &RenderPipeline {
        // Invariant: advanced (dst-read) modes are NOT fixed-function and must never
        // build a `PipelineCache` entry — shape records divert to
        // `DrawItem::AdvancedShape` in `add_tessellated_with_key` before a key is
        // created. A stray advanced key here is a routing logic error; catch it
        // loudly in debug/tests (release degrades to the defensive SrcOver arm in
        // `blend_state_for`). This guards future producers (e.g. gradient/image
        // advanced blend) from silently rendering SrcOver.
        debug_assert!(
            !key.blend_mode().is_advanced(),
            "advanced blend key {:?} reached PipelineCache; advanced shapes must \
             divert to DrawItem::AdvancedShape via add_tessellated_with_key",
            key.blend_mode()
        );
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
            bind_group_layouts: &[
                Some(&self.viewport_bind_group_layout),
                Some(&self.clip_bind_group_layout),
            ],
            immediate_size: 0,
        });

        // Configure the shader module, blend state, and coverage constant
        // together — they are three parts of one decision, and a pipeline that
        // mixes them is silently wrong rather than invalid.
        //
        // The tessellated fragment shader emits PREMULTIPLIED alpha, so blended
        // pipelines use the premultiplied Porter-Duff factors for
        // `key.blend_mode()`. SrcOver maps to PREMULTIPLIED_ALPHA_BLENDING —
        // visually identical to the previous straight-alpha output now that the
        // shader premultiplies.
        //
        // A mode whose destination factor cannot absorb `1 - coverage` takes
        // the second-source shader and the `OneMinusSrc1` factors instead, so a
        // partially covered fragment feathers the blend. Where that shader does
        // not exist (no `DUAL_SOURCE_BLENDING`), the mode falls back to the
        // folded shader and the uncorrected factors — see `Self::new`.
        let (shader, blend_state, constants) = if key.is_alpha_blended() {
            let selection = select_coverage_blend(
                key.blend_mode(),
                &self.shader,
                self.second_source_shader.as_ref(),
            );
            (
                selection.shader,
                Some(selection.blend_state),
                selection.constants,
            )
        } else {
            // Opaque - no blending (faster!)
            (&self.shader, None, Vec::new())
        };

        // Configure MSAA
        let msaa_samples = key.msaa_samples();

        // Create specialized pipeline
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Specialized Shape Pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(crate::vertex::Vertex::desc())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    blend: blend_state,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &constants,
                    ..Default::default()
                },
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
    pub(crate) fn viewport_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.viewport_bind_group_layout
    }

    /// The per-batch SDF clip bind-group layout (group 1) every tessellated
    /// draw binds against.
    pub(crate) fn clip_bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.clip_bind_group_layout
    }
}

/// Helper to determine pipeline key from paint properties.
///
/// Blend-mode routing (Phase A — fixed-function Porter-Duff; Phase B — advanced):
/// - A non-`SrcOver` Porter-Duff mode always selects a blended pipeline keyed by
///   that mode (the blend stage is required even for fully opaque source, e.g.
///   `Clear`/`DstOut` punch-outs and `Plus` additive).
/// - An advanced (dst-reading) mode — `Screen`, `Multiply`, `Overlay`, the HSL
///   modes, etc. — is carried through in the key so that
///   `DrawBatcher::add_tessellated_with_key` can detect `is_advanced()` and divert
///   the shape into `DrawItem::AdvancedShape` before the key is used for a
///   pipeline-cache lookup. The advanced key must never reach the cache:
///   [`PipelineCache::get_or_create`] debug-asserts against it.
/// - `SrcOver` keeps the legacy fast heuristic: opaque source (`a == 255`) skips
///   the blend stage entirely; translucent source uses the SrcOver blend.
pub(crate) fn pipeline_key_from_paint(paint: &Paint) -> PipelineKey {
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
        // Advanced / dst-read mode: carry the original mode in the key so that
        // `DrawBatcher::add_tessellated_with_key` can detect `is_advanced()` and
        // divert the shape into `DrawItem::AdvancedShape` before the key is ever
        // used for pipeline-cache lookup.
        //
        // The advanced key MUST NOT reach `PipelineCache::get_or_create` — the
        // diversion in `add_tessellated_with_key` fires unconditionally for
        // `is_advanced()` keys, so the cache never sees them for tessellated shapes.
        //
        // Non-tessellated callers (gradients, images) that reach
        // `flush_tessellated_geometry` with an advanced key would hit a pipeline-cache
        // miss or produce incorrect output; they are guarded by their own routing —
        // `dispatch_shader_rect` (batches/gradients.rs) diverts advanced shader/gradient
        // rects, and the `is_advanced()` branches in the image/atlas draw entry points
        // (batches/images.rs) divert advanced image draws — each into isolated
        // `DrawItem::AdvancedShape` segments before the key reaches the cache.
        PipelineKey::with_blend(mode)
    }
}

/// Pure-logic tests for the blend-mode routing and Porter-Duff factor table.
/// Not gated behind `testing` because they need no GPU device, so they
/// run in the default `cargo test --lib` gate.
#[cfg(test)]
mod blend_logic {

    use super::*;

    /// The overridable constant is shared with the WGSL by spelling. A rename on
    /// one side alone is caught here rather than on a device: naga answers a
    /// name it cannot find with `PipelineConstantError::NotFound` and pipeline
    /// creation fails. What no error catches is the other direction — a
    /// pipeline that needs the constant passing none, leaving the shader on its
    /// `0.0` default and every `DstIn`/`DstATop` fringe subtly wrong.
    ///
    /// One declaration serves every assembly — they all end in the same
    /// `common/fragment_second_source.wgsl` — so checking the shape's checks
    /// the gradients' too.
    #[test]
    fn override_constant_names_match_their_shaders() {
        let declaration = format!("override {DESTINATION_ALPHA_SCALE_OVERRIDE}:");
        assert!(
            crate::shaders::SHAPE.second_source.contains(&declaration),
            "no `{declaration}` in the second-source shape shader"
        );
        crate::pipeline_set::premultiplied_source_override_matches_the_shader();
    }

    // =========================================================================
    // H1: ssaa_eligible_for() behaviour tests
    // =========================================================================
}
