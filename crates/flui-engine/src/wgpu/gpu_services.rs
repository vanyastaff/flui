//! Shared per-owner-thread GPU services (ADR-0045 decision 2).
//!
//! Today, [`Renderer::new`] and [`Renderer::new_offscreen`] each build
//! their own private `Instance → Adapter → Device → Queue` stack. That is
//! real duplication once more than one `Renderer` lives on the same owner
//! thread: N GPU devices, N shader caches, and — per ADR-0045 decision 2's
//! first named hazard — a naive per-renderer `Arc`-share of ONE device would
//! also mean N independent `set_device_lost_callback` installs racing to
//! overwrite each other, since that call is last-writer-wins. Today's
//! actual per-renderer-own-device code has no such race (each renderer
//! installs on its own distinct device), but sharing one device across
//! renderers — the point of this type — reopens exactly that hazard, which
//! is why [`GpuServices`] installs the callback exactly once itself and
//! nothing built from it may install a second one (see `GpuStackOrigin` and
//! [`crate::EngineError::SharedServicesNotRecoverable`]).
//!
//! [`GpuServices`] resolves the stack **once per owner thread** and shares
//! it; every `Renderer` built from it borrows rather than rebuilds. Scope is
//! deliberately **one owner thread, not one process** — two realms on two
//! owner threads legitimately get two devices, and this type polices that
//! boundary at runtime (`created_on`, checked in debug builds) rather than
//! merely documenting it.
//!
//! # What is NOT here
//!
//! This slice does not wire `GpuServices` into `flui-app`'s `AppRuntime`,
//! does not migrate `Renderer::new`'s existing call sites, and does not
//! build a windowed (surface-owning) resolution path. A windowed path was
//! drafted and cut: `Instance::new` builds a fresh `wgpu-core` `Global` with
//! its own surface registry, and `Adapter::request_adapter` resolves a
//! `compatible_surface` by raw id against the **receiving** instance's
//! registry — a surface created from any other instance does not exist in
//! this one's registry and `request_adapter` panics. The instance and the
//! surface have to be created together, which is exactly the shape
//! `Renderer::build_windowed_gpu_stack` already uses; a windowed resolver
//! here would need to also return the surface it built the instance
//! alongside, and consuming that surface needs the raw-handle plumbing
//! ADR-0045 decision 1 ties to the (not yet threaded) raster lane. Building
//! it now would ship an uncallable, untested path. What ships here — the
//! offscreen path plus [`Renderer::from_offscreen_services`] — is what
//! decision 2 needs to be checkable at all: "one device per owner thread"
//! is proven only for that path.
use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
#[cfg(debug_assertions)]
use std::thread::ThreadId;

use super::renderer::{GpuCapabilities, Renderer};
use super::shader_compiler::ShaderCache;
use crate::error::{EngineError, EngineResult};

/// Monotonic stamp minted exactly once per [`GpuServices`] construction;
/// never equal across two constructions, even sequential ones on the same
/// thread. The private tuple field means nothing outside this module can
/// fabricate one — see `crates/flui-engine/tests/compile_fail/
/// gpu_resource_generation_private_field.rs`.
///
/// # Named `GpuResourceGeneration`, not `ResourceGeneration`
///
/// ADR-0045 decision 2 calls this field "`ResourceGeneration`", but
/// `flui_foundation::epoch` already declares a `pub struct ResourceGeneration`
/// for a different domain (worker-cache freshness), and port-check trigger
/// 10 (SP-3) forbids the same identifier in two framework crates. Renamed
/// per SP-3's own remedy rather than allowlisted — this is a brand-new type
/// with no callers to break, and the two concepts are genuinely different
/// (see the ADR amendment for the full reasoning).
///
/// **Considered and rejected: a new arm on `flui_foundation`'s
/// `epoch_counters!` macro** (already `flui-engine`'s dependency), which
/// would resolve the name collision the same way and keep one family with
/// one API. Rejected on two grounds: `epoch.rs`'s three existing counters
/// are all named as domain-neutral protocol concepts (`FrameEpoch`,
/// `SurfaceGeneration`, `ResourceGeneration`), and each was promoted into
/// the shared foundation crate specifically because a second crate needed
/// to compare against it (`SurfaceGeneration` is minted by `flui-engine`'s
/// raster owner but carried by `flui-layer`'s `SceneSnapshot`/`FrameStamp`);
/// `GpuResourceGeneration` has no such consumer *in this slice* — but one is
/// already scheduled rather than merely possible: ADR-0045 decision 4
/// compares this axis at the same frame gate as `SurfaceGeneration`, and the
/// only values that cross that boundary are `flui-layer`'s `SceneSnapshot`
/// and `flui-foundation`'s `FrameStamp`, neither of which can name a
/// `flui-engine` type. So treat the promotion as **expected and deferred**,
/// not as unmotivated; the surface is registered `experimental` precisely so
/// that later export move is sanctioned rather than a surprise. What holds
/// on its own is the second ground: the macro's `next()` is an owner bumping an
/// existing value in place; this type is minted fresh from a process-wide
/// counter with no persistent owner yet (that arrives with a later slice's
/// `AppRuntime` wiring), a different-enough mint model that forcing it
/// through `next()` today would need its own wrapper anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GpuResourceGeneration(u64);

impl GpuResourceGeneration {
    /// Mints the next generation from the crate-wide monotonic counter.
    /// Private — the only call site is [`GpuServices::resolve_offscreen`].
    fn mint() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw counter value. Named to match the `.get()` accessor shared by
    /// `flui_foundation::epoch`'s `FrameEpoch`/`SurfaceGeneration`/
    /// `ResourceGeneration` family, which a reader compares this type
    /// against on the same frame-freshness gate (ADR-0045 decision 4).
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for GpuResourceGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Shared GPU services for exactly one owner thread (ADR-0045 decision 2).
///
/// Holds the `Instance`/`Adapter`/`Device`/`Queue` stack, the shared
/// `ShaderCache` (the only cache this decision shares — `PipelineCache`,
/// `TextureCache`, `TexturePool`, `PathCache`, and the glyph atlas all stay
/// per-surface; see the module doc), the one
/// `device_lost: Arc<AtomicBool>` behind exactly one
/// `set_device_lost_callback` install, and an immutable
/// [`GpuResourceGeneration`].
///
/// Not `Send`-blocked at the type level (pinned below via
/// `assert_impl_all!`) — decision 2 explicitly designs the `device_lost`
/// flag to cross to a raster thread (`Arc::clone`, read there via
/// [`GpuServices::device_lost_handle`]/[`Renderer::is_device_lost`]), so a
/// blanket `!Send` would forbid the one crossing this type is built to
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
    /// The thread that constructed this value; compared against
    /// `std::thread::current().id()` on every owner-affine accessor in
    /// debug builds. Absent in release builds — same convention as
    /// `#[cfg(debug_assertions)] mod debug;` / `pub use debug::DebugBackend;`
    /// in `wgpu/mod.rs`, an item that exists only in debug builds.
    #[cfg(debug_assertions)]
    created_on: ThreadId,
}

static_assertions::assert_impl_all!(GpuServices: Send);

impl GpuServices {
    /// Resolve GPU services for the offscreen path (no surface):
    /// `HighPerformance`, `compatible_surface: None`,
    /// `force_fallback_adapter: false` — the same policy
    /// `Renderer::new_offscreen` uses today, diffed field-by-field (the one
    /// difference is the device label).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::AdapterRequest`] when no compatible adapter is
    /// found, or [`EngineError::DeviceCreation`] when the driver rejects the
    /// requested device.
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

        let capabilities = GpuCapabilities::detect(&adapter);
        tracing::info!(
            adapter = capabilities.adapter_name,
            vendor = capabilities.vendor,
            backend = ?capabilities.backend,
            "GpuServices: selected GPU (shared across this owner thread's renderers)"
        );

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

        // The ONLY call site in the crate that installs
        // `set_device_lost_callback` against a device `GpuServices` owns —
        // `Renderer::from_offscreen_services` never requests a device of
        // its own, so there is no second install to race or clobber this
        // one (ADR-0045 decision 2's first hazard).
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
    #[must_use]
    pub fn device(&self) -> &Arc<wgpu::Device> {
        self.assert_owner_thread();
        &self.device
    }

    /// The shared queue. Same sharing contract as [`Self::device`].
    #[must_use]
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        self.assert_owner_thread();
        &self.queue
    }

    /// The instance these services were resolved from.
    #[must_use]
    pub fn instance(&self) -> &wgpu::Instance {
        self.assert_owner_thread();
        &self.instance
    }

    /// The adapter these services were resolved from.
    #[must_use]
    pub fn adapter(&self) -> &wgpu::Adapter {
        self.assert_owner_thread();
        &self.adapter
    }

    /// Detected capabilities of the shared adapter.
    #[must_use]
    pub fn capabilities(&self) -> &GpuCapabilities {
        self.assert_owner_thread();
        &self.capabilities
    }

    /// The shared shader cache — the only cache ADR-0045 decision 2 shares
    /// across an owner thread's renderers; see the module doc.
    #[must_use]
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
    /// never share a generation, and the second construction mints strictly
    /// after the first.
    ///
    /// What this does NOT pin: "never crosses an owner thread" itself —
    /// there is no consumer in this slice that sends a
    /// `GpuResourceGeneration` (or the `GpuServices` that minted it) to
    /// another thread, so that half of the property is enforced by
    /// `assert_owner_thread` (pinned below) rather than exercised
    /// end-to-end here.
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
            second.generation().get() > first.generation().get(),
            "the generation counter must be monotonic across constructions"
        );
    }

    /// Pins the runtime thread-affinity check (ADR-0045 decision 2: "scoped
    /// to one owner thread ... and it is *checked*"): an owner-affine
    /// accessor called from a thread other than the one that constructed
    /// `GpuServices` must panic in debug builds. Also the mechanism that
    /// keeps `GpuResourceGeneration` from crossing an owner thread by
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
