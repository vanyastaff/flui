//! Shared per-owner-thread GPU services (ADR-0045 decision 2).
//!
//! Today, every [`Renderer`] builds its own private `Instance → Adapter →
//! Device → Queue` stack — once in [`Renderer::new`]'s windowed path, once
//! in [`Renderer::new_offscreen`], and a third time (offscreen branch only;
//! the windowed branch reuses the first site) inside [`Renderer::recover`].
//! That is real duplication when more than one `Renderer` lives on the same
//! owner thread: three GPU devices, three shader caches, three independent
//! `set_device_lost_callback` installs where wgpu's own contract is
//! last-writer-wins on that call — so the second install silently clobbers
//! the first, and whichever `Renderer` is *not* the last to construct never
//! observes device loss again.
//!
//! [`GpuServices`] is the fix: resolve the stack **once per owner thread**,
//! share it, and let every `Renderer` built from it borrow rather than
//! rebuild. Scope is deliberately **one owner thread, not one process** —
//! two realms on two owner threads legitimately get two devices, and this
//! type polices that boundary at runtime (`created_on`, checked in debug
//! builds) rather than merely documenting it.
//!
//! # What is NOT here
//!
//! This slice does not wire `GpuServices` into `flui-app`'s `AppRuntime`,
//! does not migrate `Renderer::new`'s five existing call sites, and does not
//! build the windowed (surface-owning) construction path's raw-handle
//! plumbing — those are later slices. What ships here is the value type
//! itself, its two owner-thread-scoped resolution paths (mirroring
//! `Renderer::new`'s adapter-selection-from-a-surface policy and
//! `Renderer::new_offscreen`'s no-surface policy), and the sharing seam
//! ([`Renderer::from_offscreen_services`]) that proves the device is
//! actually shared rather than merely shareable in principle.
//!
//! # Honesty note
//!
//! "One device per owner thread" is proven here only for the **offscreen**
//! path — the windowed path's adapter-selection-from-the-first-surface
//! policy (ADR-0045 decision 2: "later windows inherit that adapter") has
//! no `Renderer`-level sharing constructor yet, because building one
//! requires the raw-handle/surface-lifecycle plumbing ADR-0045 decision 1
//! already ties to the (not-yet-threaded) raster lane. This type's
//! `resolve_for_surface` constructor exists and is tested for adapter
//! selection, but nothing in this slice builds a *windowed* `Renderer` that
//! shares it.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(debug_assertions)]
use std::thread::ThreadId;

use super::renderer::{GpuCapabilities, Renderer};
use super::shader_compiler::ShaderCache;
use crate::error::{EngineError, EngineResult};

/// Monotonic stamp minted exactly once per [`GpuServices`] construction.
///
/// # Named `GpuResourceGeneration`, not `ResourceGeneration`
///
/// ADR-0045 decision 2's own prose calls this "an immutable
/// `ResourceGeneration`". It is NOT named that here: `flui-foundation`
/// already ships a `pub struct ResourceGeneration`
/// (`crates/flui-foundation/src/epoch.rs`) for a different domain —
/// "generational identity for worker-produced cache resources (image
/// decode, tessellation, glyph shaping)" — and `scripts/port-check.sh`
/// trigger 10 (SP-3, "parallel cross-crate type definitions") mechanically
/// forbids two distinct framework crates from each declaring a `pub struct`
/// with the same identifier; its own prescribed remedy is "consolidate, or
/// rename one" (not an allowlist marker — SP-3 reserves that for
/// unavoidable historical collisions, and this is a brand-new type with no
/// existing callers to break). These are genuinely different concepts —
/// GPU-device/resource freshness (this type, minted by a `GpuServices`
/// construction) vs. worker-cache freshness (that type, incremented by a
/// cache owner's `bump()`) — consolidating them into one type would let a
/// future call site compare a GPU generation against an asset-cache
/// generation and have it silently type-check, which is exactly the
/// footgun `flui_foundation::epoch`'s own doc says its three-type split
/// exists to prevent ("mixing them up ... is a compile error, not a bug
/// waiting to happen"). So this type is renamed instead, per SP-3's other
/// remedy. Also consistent with ADR-0045's own file scope, which never
/// names `crates/flui-foundation/src/epoch.rs`.
///
/// Two `GpuServices` values (even sequential ones on the same thread, e.g.
/// across a device-loss recovery that builds a brand-new value rather than
/// mutating the old one) never compare equal. There is exactly one mint
/// site in the crate — the private [`GpuResourceGeneration::mint`], called only
/// from [`GpuServices::resolve_from_adapter`] — and no public constructor:
/// the tuple field is private, so nothing outside this module can fabricate
/// a `GpuResourceGeneration` that did not come from a real `GpuServices`
/// construction. See the `compile_fail` doctest below.
///
/// ADR-0045 decision 2 names this immutable and scoped to the owner thread
/// that minted it: a frame gates on comparing its stamped generation against
/// the *current* `GpuServices::generation()`, and that comparison is always
/// made by the same thread that minted both sides. Nothing in this slice
/// sends a `GpuResourceGeneration` — or the `GpuServices` that minted it — to
/// another thread; that crossing has no consumer yet (the raster lane this
/// gates is a later slice), so it is not exercised by a test here. What IS
/// pinned below is the mechanism that would prevent it from happening by
/// accident: reading `GpuServices::generation()` from a thread other than
/// the one that constructed the value panics in debug builds (see
/// [`GpuServices::assert_owner_thread`]), so a stray cross-thread read is
/// caught rather than silently returning a generation minted somewhere
/// else.
///
/// ```compile_fail
/// // The tuple field is private outside this module — there is no way to
/// // construct a `GpuResourceGeneration` except via a real `GpuServices`.
/// let _ = flui_engine::GpuResourceGeneration(5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GpuResourceGeneration(u64);

impl GpuResourceGeneration {
    /// Mints the next generation from the crate-wide monotonic counter.
    ///
    /// Private — the only call site is [`GpuServices::resolve_from_adapter`].
    fn mint() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// Raw numeric value, for logging/diagnostics only.
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Shared GPU services for exactly one owner thread (ADR-0045 decision 2).
///
/// Holds the `Instance`/`Adapter`/`Device`/`Queue` stack, the shared
/// [`ShaderCache`] (the only cache this decision shares — `PipelineCache`,
/// `TextureCache`, `TexturePool`, `PathCache`, and the glyph atlas all stay
/// per-surface; see the module-level ADR reference), the one
/// `device_lost: Arc<AtomicBool>` behind exactly one
/// `set_device_lost_callback` install, and an immutable [`GpuResourceGeneration`].
///
/// Not `Send`-blocked at the type level — decision 2 explicitly designs the
/// `device_lost` flag to cross to a raster thread (`Arc::clone`, read there
/// via [`GpuServices::device_lost_handle`]/[`Renderer::is_device_lost`]), so
/// a blanket `!Send` would forbid the one crossing this type is built to
/// support. The owner-thread scope is enforced at runtime instead: every
/// accessor that hands out owner-affine state (`device`, `queue`,
/// `instance`, `adapter`, `capabilities`, `shader_cache`, `generation`)
/// asserts `created_on` matches the calling thread in debug builds.
/// `device_lost_handle`/`is_device_lost` deliberately do NOT assert — they
/// are the one fact designed to be read from a foreign thread.
#[derive(Debug)]
pub struct GpuServices {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    capabilities: GpuCapabilities,
    shader_cache: Arc<ShaderCache>,
    device_lost: Arc<AtomicBool>,
    generation: GpuResourceGeneration,
    /// The thread that constructed this value. Compared against
    /// `std::thread::current().id()` on every owner-affine accessor in
    /// debug builds; absent (and unchecked) in release builds, matching the
    /// rest of the crate's `#[cfg(debug_assertions)]` invariant-assertion
    /// convention (e.g. `Renderer`'s own `expect("BUG: ...")` sites).
    #[cfg(debug_assertions)]
    created_on: ThreadId,
}

impl GpuServices {
    /// Resolve GPU services for the windowed path, selecting the adapter
    /// against `surface`.
    ///
    /// Mirrors `Renderer::new`'s current adapter-selection policy exactly
    /// (`HighPerformance`, `compatible_surface: Some(surface)`,
    /// `force_fallback_adapter: false`) — per ADR-0045 decision 2, "selection
    /// for the first window is bit-identical to today; later windows inherit
    /// that adapter." Later windows on the same owner thread should reuse
    /// the already-resolved `GpuServices` rather than calling this again;
    /// re-selection on display-topology change (dock/undock, eGPU hotplug)
    /// is out of scope (ADR-0045 decision 2, "declared out of scope, not
    /// left implicit").
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::AdapterRequest`] or
    /// [`EngineError::DeviceCreation`] on the same conditions
    /// `Renderer::new` can fail under today (no compatible adapter, or the
    /// driver rejecting the requested device).
    pub async fn resolve_for_surface(surface: &wgpu::Surface<'_>) -> EngineResult<Self> {
        let backends = Renderer::select_backend();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(EngineError::adapter_request)?;

        Self::resolve_from_adapter(instance, adapter).await
    }

    /// Resolve GPU services for the offscreen path (no surface).
    ///
    /// Mirrors `Renderer::new_offscreen`'s current adapter-selection policy
    /// exactly (`HighPerformance`, `compatible_surface: None`,
    /// `force_fallback_adapter: false`).
    ///
    /// # Errors
    ///
    /// See [`Self::resolve_for_surface`].
    pub async fn resolve_offscreen() -> EngineResult<Self> {
        let backends = Renderer::select_backend();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(EngineError::adapter_request)?;

        Self::resolve_from_adapter(instance, adapter).await
    }

    /// Shared tail of both resolution paths: request the device, install
    /// diagnostics exactly once, and mint the generation.
    ///
    /// This is the ONLY call site in the crate that installs
    /// `set_device_lost_callback` against a device owned by `GpuServices` —
    /// `Renderer::from_offscreen_services` never requests a device of its
    /// own, so there is no second install to race or clobber the first
    /// (ADR-0045 decision 2's first hazard).
    async fn resolve_from_adapter(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
    ) -> EngineResult<Self> {
        let capabilities = GpuCapabilities::detect(&adapter);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FLUI Shared GPU Device"),
                required_features: Renderer::required_features(&capabilities),
                required_limits: Renderer::required_limits(&capabilities),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(EngineError::device_creation)?;

        let device_lost = Arc::new(AtomicBool::new(false));
        Renderer::install_device_diagnostics(&device, Arc::clone(&device_lost));

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            capabilities,
            shader_cache: Arc::new(ShaderCache::new()),
            device_lost,
            generation: GpuResourceGeneration::mint(),
            #[cfg(debug_assertions)]
            created_on: std::thread::current().id(),
        })
    }

    /// Panics (debug builds only) if called from a thread other than the one
    /// that constructed this value.
    #[cfg(debug_assertions)]
    fn assert_owner_thread(&self) {
        let current = std::thread::current().id();
        assert_eq!(
            current, self.created_on,
            "BUG: flui_engine::GpuServices accessed from a thread other than \
             its owner thread. ADR-0045 decision 2 scopes GPU services to \
             exactly one owner thread, never process-wide; two realms on two \
             owner threads must each resolve their own GpuServices rather \
             than share this one."
        );
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    fn assert_owner_thread(&self) {}

    /// The shared device. Every [`Renderer`] built from these services via
    /// [`Renderer::from_offscreen_services`] holds the SAME `Arc` — compare
    /// with `Arc::ptr_eq`, not by proxy (e.g. capability equality), to prove
    /// sharing.
    pub fn device(&self) -> &Arc<wgpu::Device> {
        self.assert_owner_thread();
        &self.device
    }

    /// The shared queue. Same sharing contract as [`Self::device`].
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        self.assert_owner_thread();
        &self.queue
    }

    /// The instance these services were resolved from.
    pub fn instance(&self) -> &wgpu::Instance {
        self.assert_owner_thread();
        &self.instance
    }

    /// The adapter these services were resolved from.
    pub fn adapter(&self) -> &wgpu::Adapter {
        self.assert_owner_thread();
        &self.adapter
    }

    /// Detected capabilities of the shared adapter.
    pub fn capabilities(&self) -> &GpuCapabilities {
        self.assert_owner_thread();
        &self.capabilities
    }

    /// The shared shader cache — the only cache ADR-0045 decision 2 shares
    /// across an owner thread's renderers; see the module doc.
    pub fn shader_cache(&self) -> &Arc<ShaderCache> {
        self.assert_owner_thread();
        &self.shader_cache
    }

    /// This value's immutable generation, minted once at construction.
    #[must_use]
    pub fn generation(&self) -> GpuResourceGeneration {
        self.assert_owner_thread();
        self.generation
    }

    /// Clone of the shared device-lost flag.
    ///
    /// Deliberately NOT thread-affinity checked: per ADR-0045 decision 2,
    /// "the device-lost *observation* crosses threads (an `Arc<AtomicBool>`
    /// the raster side sets and reads)". Handing out a clone is the
    /// sanctioned crossing, not a bypass of the owner-thread scope the other
    /// accessors enforce.
    #[must_use]
    pub fn device_lost_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.device_lost)
    }

    /// Whether the shared device has been lost. Same no-assert rationale as
    /// [`Self::device_lost_handle`].
    #[must_use]
    pub fn is_device_lost(&self) -> bool {
        self.device_lost.load(Ordering::Acquire)
    }
}

// Gated behind `enable-wgpu-tests` like every other test module in this
// crate's wgpu/* tree (renderer.rs, offscreen/mod.rs, shader_compiler.rs) —
// not because every test here needs a real adapter (each one gracefully
// skips without one, same as `Renderer::new_offscreen`'s own tests), but to
// match the file-wide convention so `cargo nextest run -p flui-engine`
// (default features) and `--all-features` both compile cleanly and a
// feature-gated file is never silently skipped by a marker sweep that only
// checks default features.
#[cfg(all(test, feature = "enable-wgpu-tests"))]
mod tests {
    use super::*;

    /// Builds a real `GpuServices` via the offscreen path, or returns `None`
    /// gracefully when no adapter is available (headless CI without a GPU).
    /// Same pattern as `renderer.rs`'s own `test_device_and_queue` /
    /// `Renderer::new_offscreen` test helpers.
    fn resolve_offscreen_or_skip() -> Option<GpuServices> {
        pollster::block_on(GpuServices::resolve_offscreen()).ok()
    }

    /// Pins "`GpuResourceGeneration` is minted only by `GpuServices`
    /// construction": two independently constructed `GpuServices` values
    /// (even sequential ones, on the same thread) never share a generation,
    /// and the second construction mints strictly after the first — proving
    /// the mint happens exactly once per construction and nowhere else (a
    /// leftover generation could not be "reused" for a second construction
    /// by accident, e.g. via a cached/default value).
    ///
    /// What this does NOT pin: "never crosses an owner thread" itself. There
    /// is no consumer in this slice that sends a `GpuResourceGeneration` (or
    /// the `GpuServices` that minted it) to another thread — the raster
    /// lane that would do so is a later slice — so that half of the property
    /// is enforced by `assert_owner_thread` (pinned below by
    /// `gpu_services_panics_on_cross_thread_access`) rather than exercised
    /// end-to-end here. Said explicitly rather than writing a test that
    /// would pass whether or not a real crossing were prevented.
    #[test]
    fn generation_is_minted_fresh_and_monotonic_per_construction() {
        let Some(first) = resolve_offscreen_or_skip() else {
            return;
        };
        let Some(second) = resolve_offscreen_or_skip() else {
            return;
        };
        assert_ne!(
            first.generation(),
            second.generation(),
            "two independently constructed GpuServices must never share a generation"
        );
        assert!(
            second.generation().value() > first.generation().value(),
            "the generation counter must be monotonic across constructions"
        );
    }

    /// Pins the runtime thread-affinity check (ADR-0045 decision 2: "scoped
    /// to one owner thread ... and it is *checked*"): an owner-affine
    /// accessor called from a thread other than the one that constructed
    /// `GpuServices` must panic in debug builds. This is also the mechanism
    /// that keeps `GpuResourceGeneration` from crossing an owner thread by
    /// accident — `generation()` is gated behind the exact same check as
    /// `device()`.
    ///
    /// Release builds have no `created_on` field and no check to trip; the
    /// assertion below is symmetric so the test is meaningful either way
    /// rather than vacuously passing in one configuration.
    #[test]
    fn gpu_services_panics_on_cross_thread_access() {
        let Some(services) = resolve_offscreen_or_skip() else {
            return;
        };

        let outcome = std::thread::spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = services.device();
            }))
        })
        .join()
        .expect("the spawned thread itself must not panic outside the checked call");

        #[cfg(debug_assertions)]
        assert!(
            outcome.is_err(),
            "GpuServices::device() must panic when called from a thread other \
             than the one that constructed it"
        );
        #[cfg(not(debug_assertions))]
        assert!(
            outcome.is_ok(),
            "release builds carry no created_on field, so cross-thread access \
             must succeed rather than panic"
        );
    }
}
