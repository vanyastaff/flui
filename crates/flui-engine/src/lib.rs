#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// `missing_debug_implementations` is suppressed crate-wide for the public
// types that hold `lyon`/atlas state or boxed closures with no `Debug`:
// `GpuContext` (its painter, its device-loss wake) and, under `testing`, the
// bench re-exports (`WgpuPainter`, `OffscreenRenderer`, `PathCache`). wgpu 30's
// own handles (Device, Queue, Texture, Buffer, …) all derive `Debug`, so they
// are not the reason. It is an `expect`, not an `allow`: if the last such
// type gains `Debug`, the attribute must go with it.
#![expect(
    missing_debug_implementations,
    reason = "`GpuContext` holds a painter with lyon and atlas state and a device-loss wake, a boxed closure, with no `Debug`"
)]
// No hand-written production `unsafe` in this crate but two: surfaces come from wgpu's safe
// `create_surface` over an owned `WindowTarget` (ADR-0063), and every GPU
// handle is a wgpu value type. The exceptions are the read of DXGI frame
// statistics (`surface::present::dxgi_frame_statistics`), whose swap chain wgpu hands
// out only through its hal escape, and the creation of the pipeline cache
// from stored data (`pipelines::store::create_cache`), which wgpu offers only as
// an `unsafe fn`; each carries its own `expect`. `deny` rather than `forbid` because
// wgsl_bindgen's generated wrappers (`*/generated.rs`) declare an
// `unsafe fn from_raw` this crate never calls and `allow` it locally.
// `surface::fake_target` uses test-only raw-handle borrows for synthetic window
// and display handles, gated on `cfg(test)`.
#![cfg_attr(not(test), deny(unsafe_code))]
// `unreachable_pub` is a warning, not an error, because the workspace lint set
// does not carry it: it fires for a `pub` item in a private or `pub(crate)`
// module, which is a real finding (it claims reachability it does not have)
// but not a correctness one. Every `pub` in this crate either reaches the crate
// root or carries a scoped `#[cfg_attr(..., expect(unreachable_pub))]` naming
// the feature that makes it reachable. Keep it on: the next one is a warning,
// not a discovery.
#![warn(unreachable_pub)]
// The next-generation trait solver counts auto-trait proof depth honestly,
// and proving that the raster thread's closure is `Send` (its lifecycle
// channel carries a `ProbedDevice`, a wgpu device and queue) descends
// through wgpu-core (`wgpu::Queue -> Global -> Hub -> Registry ->
// RwLock<Storage<..>>`) past the default limit of 128. On stable that is
// invisible; on nightly it is the future-incompat lint
// `recursion_depth_exceeding_limit` (rust-lang/rust#159228), which becomes a
// hard error once the solver stabilises. Auto-trait proofs are structural and
// re-run in every crate that needs a wgpu handle to be `Send`, so flui-app
// carries the same attribute. A manual `impl Send` on an intermediate type
// would short-circuit the proof, but reopening a hand-written `Send`
// assertion is exactly what ADR-0045 decision 1 removed; the limit is the
// honest fix until wgpu adds the impls upstream (same class as
// rust-lang/rust#160036).
#![recursion_limit = "256"]

//! GPU compositor for FLUI: it turns a [`flui_layer::Scene`] into wgpu draw
//! calls (Vulkan / Metal / DX12 / WebGPU).
//!
//! wgpu is the engine, not a backend behind one: the crate re-exports the
//! exact [`wgpu`] it links against, so an embedder that hands over a device
//! or reads a surface format names the same version without a second
//! dependency line.
//!
//! # Which entry point do you want?
//!
//! | You are | Use |
//! |---|---|
//! | presenting every window of an application | [`RasterExecutor`] over one [`GpuContext`] ([`GpuContext::bootstrap`]) — each window a slot of one device, its surface made by the [`SurfaceFactory`] (ADR-0305) |
//! | rendering without a window (tests, screenshots, thumbnails, CI) | [`HeadlessRenderer::new`] — rasterizes a layer tree to RGBA8 bytes |
//!
//! What either draws is a [`Scene`](flui_layer::Scene): record draw calls on a
//! `flui_painting::Canvas`, put the display list it finishes into a picture
//! layer, and hand the scene to the executor as a frame. There is no public
//! painter below the scene.
//!
//! An application does not drive these itself: it draws through a
//! `CustomPaint` (as `examples/painting_demo` does), and `flui-app`'s runners
//! bootstrap the context, attach each window's surface as a slot and send the
//! executor its frames. An embedder that wants a scene's whole path reads
//! those runners, or starts from [`HeadlessRenderer`], which rasterizes a
//! scene without a window.
//!
//! The context and headless constructors are `async`, because wgpu's adapter
//! and device requests are. The library does not choose a blocking strategy
//! for you: an embedder with an async runtime awaits, and one without wraps
//! the call in its own `block_on`.
//!
//! # Architecture
//!
//! Everything below [`Scene`](flui_layer::Scene) is this crate's internals
//! (none of `LayerDispatcher`, `WgpuPainter` or `GpuReplay` is public),
//! reached through the entry point you picked above:
//!
//! ```text
//! Scene (flui-layer)          built by the widget tree's Canvas
//!     │
//!     ▼
//! RasterExecutor              renders each slot of one GpuContext
//!     │ walks the scene's version
//!     ▼
//! LayerDispatcher             routes each DrawCommand to the painter
//!     │
//!     ▼
//! WgpuPainter                 record: batched Command IR
//!     │
//!     ▼
//! GpuReplay                   replay: Command IR → wgpu draw calls
//!     │
//!     ▼
//! GPU (wgpu)
//! ```
//!
//! # Math-backend policy
//!
//! This engine layer uses `glam` (`Vec2`/`Mat4`/`vec4`, with `bytemuck` Pod)
//! **directly** for GPU and paint hot-path math. That is intentional and
//! sanctioned: `glam` is FLUI's chosen linear-algebra backend (it also backs
//! `flui_foundation::geometry::Matrix4` underneath), and the SIMD/Pod-friendly types belong
//! at the GPU boundary. Typed `flui_foundation::geometry` values are converted to `glam`
//! *here, at the engine edge* (`offset.dx`, `point.x`, …) — the typed unit
//! barrier lives in the layout/widget layers above, not in pixel-pushing code.
//! New direct `glam` use in this crate is expected, not a smell.
//!
//! # Cargo features
//!
//! - `vulkan` / `metal` / `dx12` / `webgpu` / `gles` — add a wgpu backend API
//!   explicitly. These are additive pass-throughs to wgpu's own features, not
//!   selectors: the right one for the target OS is already enabled by this
//!   crate's per-target dependency entries, and enabling two compiles both.
//! - `testing` — test support only; not part of the public API.

// Every public item is documented; keep it that way.
#![deny(missing_docs)]
// Hash tables on the frame path are FxHash (clippy.toml `disallowed-types`).
#![warn(clippy::disallowed_types)]

// Compile-time guard: the `fragile-send-sync-non-atomic-wasm` wgpu feature
// marks !Send types as Send+Sync, which is only sound when the wasm target has
// no threads (no atomics target feature). Enabling atomics with this feature
// is UB — catch it at compile time instead of silently producing data races.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
compile_error!(
    "fragile-send-sync-non-atomic-wasm is unsound with threads/atomics enabled \
     — see flui-engine Cargo.toml [target.wasm32] note"
);

/// The wgpu this crate links against, re-exported so embedders name the same
/// version (device handoff, surface formats, `wgpu::Features` queries).
pub use ::wgpu;

mod device;
mod error;
mod filters;
mod frame;
mod layers;
mod offscreen;
mod painter;
mod pipelines;
mod raster;
mod raster_executor;
mod record;
mod replay;
mod resources;
mod shaders;
mod surface;

mod gpu_context;
mod headless;

#[cfg(test)]
mod superellipse;

#[cfg(feature = "testing")]
pub mod testing;

#[cfg(all(test, feature = "testing"))]
mod readback_dump;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod blend_oracle;

#[cfg(test)]
mod clip_layer_readback_tests;
#[cfg(test)]
mod coverage_blend_readback_tests;
#[cfg(test)]
mod damage_readback_tests;
#[cfg(test)]
mod gradient_blend_readback_tests;
#[cfg(test)]
mod layer_blend_tests;
#[cfg(test)]
#[path = "layer_effect_tests.rs"]
mod layer_effect_readback_tests;
#[cfg(test)]
mod paragraph_readback_tests;
#[cfg(test)]
mod version_readback_tests;

#[cfg(all(test, feature = "testing"))]
mod aa_oracle_tests;
#[cfg(all(test, feature = "testing"))]
mod blur_filter_tests;
#[cfg(all(test, feature = "testing"))]
mod color_matrix_filter_tests;
#[cfg(all(test, feature = "testing"))]
mod compositing_readback_tests;
#[cfg(all(test, feature = "testing"))]
mod morphology_filter_tests;

// ============================================================================
// RE-EXPORTS (convenience)
// ============================================================================

// Each public item has this one path; the modules that hold them are private.
// `ExternalTextureError` is public as the payload of `EngineError::ExternalTexture`,
// which a scene naming an unregistered texture returns.
pub use error::{EngineError, EngineResult, ExternalTextureError, GeometryError, Recoverability};
pub use raster::PresentDisposition;
// The raster executor and its commands (ADR-0305).
// The threaded placement's handle (ADR-0305 §10); the web turns inline.
#[cfg(not(target_arch = "wasm32"))]
pub use raster_executor::ExecutorHandle;
pub use raster_executor::{
    DETACH_DEADLINE, DeviceAnswered, DeviceHealth, DeviceRetries, DeviceTicket, ExecutorFailure,
    ExecutorHealth, LIFECYCLE_CAPACITY, PainterResets, RasterBackend, RasterCommand,
    RasterExecutor, RasterPlacement, RasterSendError, RasterSender, RasterVerdict, SendReceipt,
    SlotChange, SlotConfig, SlotDomain, SlotFailure, SlotId, SlotPhase, SlotRecord, SlotSettlement,
    SlotSize, SlotSurface, SlotTarget, SlotVisibility, SurfaceHold, TurnOutcome,
};
// Windowless capture (golden-image / screenshot tooling).
pub use headless::HeadlessRenderer;
// The one GPU context every slot renders through (ADR-0305 §1).
pub use device::probe::{GpuProbe, ProbedDevice};
pub use gpu_context::{GpuBootstrap, GpuContext};
pub use surface::factory::{SurfaceFactory, UnconfiguredSurface};
// The swapchain latency a slot asks for, and the backend a context draws with.
pub use device::backend::{BackendFailure, GpuBackend};
pub use surface::color::SurfaceColor;
pub use surface::latency::FrameLatency;

// The painter below the scene and its external-texture registry — re-exported
// ONLY under the `testing` feature, for the engine's benches and its
// warm-path allocation test, which drive draw calls directly. An application
// draws through `flui_painting::Canvas` into a `flui_layer::Scene`.
#[cfg(feature = "testing")]
pub use painter::WgpuPainter;
#[cfg(feature = "testing")]
pub use resources::external_textures::{
    ExternalAlpha, ExternalColorEncoding, ExternalSampling, ExternalTextureDescriptor,
    ExternalTextureEntry, ExternalTextureRegistry,
};
// Presentation blit helper — re-exported ONLY under the
// `testing` feature for the retained-presentation blit benchmark.
// Gated so benching does not widen the public surface.
#[cfg(feature = "testing")]
pub use offscreen::OffscreenRenderer;
// The tessellation cache, for the `render_throughput` bench's warm-hit case.
#[cfg(feature = "testing")]
pub use record::path_cache::{PathCache, TessellationKey};
