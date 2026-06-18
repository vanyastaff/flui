//! wgpu rendering backend for FLUI
//!
//! This module provides GPU-accelerated rendering using wgpu
//! (Vulkan/Metal/DX12/WebGPU).
//!
//! # Math-backend policy (N-geom PR 2, Option D — §U16)
//!
//! This engine layer uses `glam` (`Vec2`/`Mat4`/`vec4`, with `bytemuck` Pod)
//! **directly** for GPU and paint hot-path math. That is intentional and
//! sanctioned: `glam` is FLUI's chosen linear-algebra backend (it also backs
//! `flui_geometry::Matrix4` underneath), and the SIMD/Pod-friendly types belong
//! at the GPU boundary. Typed `flui_geometry` values are converted to `glam`
//! *here, at the engine edge* (`offset.dx.0`, `point.x.0`, …) — the typed unit
//! barrier lives in the layout/widget layers above, not in pixel-pushing code.
//! New direct `glam` use in this module is expected, not a smell.
//!
//! # Architecture
//!
//! ```text
//! Scene (flui-layer)
//!     │
//!     ▼
//! SceneRenderer (scene.rs)
//!     │ renders LayerTree
//!     ▼
//! Layer + LayerRender trait
//!     │ dispatch commands
//!     ▼
//! CommandRenderer trait (crate::traits)
//!     │
//!     ▼
//! Backend (backend.rs) → WgpuPainter (painter.rs)
//!     │                      │
//!     │                      ├── Tessellator
//!     │                      ├── TextRenderer
//!     │                      └── Effects
//!     ▼
//! wgpu (GPU)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_engine::wgpu::{SceneRenderer, WgpuPainter};
//! use flui_engine::Painter;  // Abstract trait from crate root
//! use flui_layer::Scene;
//!
//! // Create renderer
//! let mut renderer = SceneRenderer::new(surface, 800, 600);
//!
//! // Render a scene
//! renderer.render(&scene)?;
//! ```

// ============================================================================
// CORE MODULES
// ============================================================================

/// Advanced dst-read blend composite driver: backdrop copy, pipeline, and
/// `flush_advanced_layer`.  No production caller exists yet (wired in PR-3);
/// the synthetic-op GPU gate in this module is the authoritative WGSL gate.
pub(crate) mod advanced_blend;
mod atlas;
mod backend;
/// Record-side draw accumulation helpers: `DrawBatcher` owns the tessellator,
/// path cache, and superellipse cache so they can be borrowed independently
/// from the flush-side state during draw recording.
mod batches;
mod buffer_pool;
/// Command IR data types: `DrawSegment`, `DrawItem`, `SavedLayer`,
/// `PendingOpacityLayer`, `PendingOffscreenTexture`, and their helpers
/// (`ScissorRect`, `ScissorRegion`, `TessellatedBatch`). Moved here from
/// `painter.rs` so the future batcher/compositor modules share one type home.
pub(crate) mod command_ir;
// Cycle 4 wave 5 E-10: `buffers.rs` deleted. Module hosted
// `DynamicBuffer` (auto-growing vertex/instance buffer) and
// `BufferManager` (5-buffer GPU resource bag). Workspace grep
// returned zero non-self consumers in any crate; the wgpu module
// graph went through `buffer_pool.rs` (live, distinct logic --
// reusable per-frame allocations) instead. The whole module was
// dead code masked by the `pub use buffers::{...}` re-export at
// `wgpu/mod.rs:148`, which itself had zero consumers (item-level
// audit revealed both layers were dead).
#[cfg(debug_assertions)]
mod debug;
/// Gradient + shadow + blur instance descriptors consumed by `painter.rs`'s
/// instanced-batch pipelines. The previous module-level `#[allow(dead_code)]`
/// reflex was removed in cycle 4 E-4 alongside the forward-looking helpers
/// (`ShadowParams::elevation_*`, `BlurIntensity`, `LinearGradientBuilder`,
/// the parallel `effects::BlurParams`); any zombie that returns lands as an
/// item-level lint, not a broad module suppression.
pub mod effects;
mod effects_pipeline;
mod external_texture_registry;
pub mod font_loader;
/// GPU instance-buffer types: `RectInstance`, `CircleInstance`,
/// `ArcInstance`, `TextureInstance`, gradient instances. All
/// surviving items are consumed by `painter.rs`. Cycle 4 E-5
/// deleted the 6 forward-looking shortcuts the previous
/// module-level `#[allow(dead_code)]` was masking (`RectInstance::rounded_rect`,
/// `RectInstance::with_clip_rsuperellipse`, `RectInstance::with_transform`,
/// `CircleInstance::ellipse`, `ArcInstance::ellipse`,
/// `TextureInstance::with_rotation`); any zombie that returns now
/// surfaces as an item-level lint, not a broad module suppression.
mod instancing;
// NOTE: integration_tests.rs removed - needs rewrite for new
// Pixels/DevicePixels API
mod multi_draw;
pub mod occlusion;
mod offscreen;
mod painter;
pub mod path_cache;
/// Pipeline key types and cache consumed by `painter.rs`. Live items:
/// `PipelineKey` (opaque/alpha-blend factory methods + bitfield queries),
/// `PipelineCache` (get_or_create, viewport_bind_group_layout), and
/// `pipeline_key_from_paint`. Unused constants/methods/cache helpers deleted.
mod pipeline;
// Cycle 4 E-6: the parallel `pipelines.rs` (with its own `PipelineCache` +
// `PipelineBuilder` structs, name-colliding with `pipeline.rs`) was deleted.
// The `pipelines.rs` introduced by T3 is distinct: it defines `PipelineSet`,
// which *composes* the live `PipelineCache` from `pipeline.rs` (singular) and
// adds the nine named pipelines previously scattered as painter fields.
/// Opacity/layer save-state machine: `opacity_stack`, `current_opacity`, and
/// `layer_stack` extracted from `WgpuPainter`.  Owns the book-keeping half of
/// `save_layer`/`restore_layer`; GPU emission lives in `GpuReplay`.
pub(super) mod layer_compositor;
/// Offscreen-layer render helper: extracted from `flush_opacity_layer` so the
/// renderer driver can reuse the same routine for backdrop-read compositing.
pub(super) mod opacity_layer;
pub(crate) mod pipelines;
mod profiler;
/// Frame render-target descriptor: `view` + optional back-reference `texture`
/// for dst-read blend passes.  Frame-scoped borrow, never stored in IR types.
pub(crate) mod render_target;
mod renderer;
/// Replay/submit component: owns GPU plumbing fields, the per-frame
/// `texture_batch` scratch, all six segment-flush phases, the top-level
/// `submit` dispatch loop, and `flush_opacity_layer` recursion.
pub(super) mod replay;
pub(crate) mod resources;
/// Shader cache for offscreen pipelines (`OffscreenRenderer` mask /
/// blur / morph). Cycle 4 E-7 dropped the module-level
/// `#[allow(dead_code)]` mask: the only forward-looking helper
/// (`ShaderCache::clear`) is now gated behind
/// `#[cfg(feature = "devtools")]`, so default-build dead-code
/// surfaces as an item-level lint rather than a broad module
/// suppression. The audit also mentioned `cached_count` but no such
/// method existed -- only `clear`.
mod shader_compiler;
mod shaders;
/// GPU draw-state stack: the four paired transform/scissor/SDF-clip stacks
/// and their cached current values, extracted from `WgpuPainter` so they can
/// be owned and delegated as a unit. Owned by `WgpuPainter` via the `state`
/// field.
pub(super) mod state_stack;
pub mod superellipse_cache;
mod tessellator;
mod text;
pub mod texture_cache;
mod texture_pool;
mod vertex;

// ============================================================================
// LAYER RENDERING
// ============================================================================

pub(crate) mod layer_render;

#[cfg(test)]
mod sdf_smoke_test;

#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod deterministic_replay_tests;

// ============================================================================
// PUBLIC API
// ============================================================================

// ----------------------------------------------------------------------------
// Cycle 4 wave 5 E-10: surface trim
//
// Workspace ripgrep of `flui_engine::wgpu::<Name>` returned consumers
// ONLY for `Renderer` (4 callsites in flui-app); every other
// `pub use` re-export here had zero external (non-flui-engine)
// consumers AND zero in-crate consumers either (sibling modules
// reach internal types via their module paths directly, not via
// the re-export at this module level).
//
// The previously-`pub` re-exports were paying public-API
// monomorphization + discoverability + stability cost for nothing.
// They are deleted outright, NOT demoted to `pub(crate)`: a
// `pub(crate) use` line with zero in-crate consumers is itself
// dead code (rustc emits `unused_imports` for it under
// `-D warnings`). The 30+ "dead-code" warnings the previous
// wave-4 attempt produced were masking the real signal: the
// re-exports were never wired into the module graph at this
// level.
//
// `Backend`, `FontLoader`, `LayerRender`, `WgpuPainter`, `DebugBackend`,
// `Renderer`, and the `commands::` / `traits::` re-exports stay
// because they have verified consumers (either flui-app callsites
// or lib.rs's crate-root re-export chain).
// ----------------------------------------------------------------------------

// Backend (external via lib.rs re-export at crate root)
pub use backend::Backend;
// Command rendering (re-exported from crate root)
pub use crate::{
    commands::{dispatch_command, dispatch_commands},
    traits::CommandRenderer,
};
#[cfg(debug_assertions)]
pub use debug::DebugBackend;
// Layer rendering (external via lib.rs re-export at crate root)
pub use layer_render::LayerRender;
pub use painter::WgpuPainter;

// Renderer (the one and only externally-consumed wgpu/* type)
pub use renderer::Renderer;
// Font loading utilities (external via lib.rs re-export at crate root)
pub use font_loader::FontLoader;
// GPU frame profile — feature-independent type, always available so callers
// can store/display profiling results without gating on `gpu-profiler`.
pub use profiler::{GpuFrameProfile, PassTiming};

// Painter (WgpuPainter is the concrete implementation; Painter trait deleted in Mythos U5)
