//! One GPU context serves every presentation as a slot (ADR-0305 §1, §2).
//!
//! [`GpuContext`] owns the instance, the adapter, the device and its queue,
//! and one painter, whose pipeline library (one set per target format), glyph
//! atlas, image and path caches every slot shares, and the presentation blit
//! pipelines (one per surface format). What is keyed by the presentation is
//! the slot's: an offscreen slot's target texture, or a window slot's
//! surface, configuration and frame protocol (`window_slot`).
//!
//! Slots render one at a time on the context's one mutator, so sharing the
//! caches takes no lock: the context is `Send`, so a raster thread can own it,
//! and `!Sync`, so no `&GpuContext` crosses a thread.
//!
//! [`GpuContext::bootstrap`] makes the application's one context on the
//! host's display connection, and the [`SurfaceFactory`] the owner makes
//! every window's surface with on the same instance.

use std::{cell::Cell, marker::PhantomData, sync::Arc};

use flui_foundation::{
    FrameStamp, geometry::DeviceSize, memory::MemoryPressureLevel, panic::FirstPanic,
};
use flui_layer::{DamageRegion, Scene, TextureUpdates};
use flui_platform::{DisplayColor, DisplayConnection, SurfaceTarget};
use rustc_hash::FxHashMap;

#[cfg(not(target_arch = "wasm32"))]
use crate::pipeline_store::PipelineStore;
use crate::{
    FrameLatency,
    device_domain::DeviceDomain,
    error::{EngineError, EngineResult},
    external_texture_registry::{ExternalTextureRegistry, KeptTextures},
    gpu_backend::{Attempt, GpuBackend},
    layer_dispatcher::LayerDispatcher,
    offscreen::OffscreenRenderer,
    painter::WgpuPainter,
    pipeline_device::PipelineDevice,
    pipeline_set::PipelineLibrary,
    raster::PresentDisposition,
    raster_executor::{RasterBackend, SlotConfig, SlotId, SlotTarget},
    surface_factory::{SurfaceFactory, UnconfiguredSurface, checked_target, instance_descriptor},
    window_slot::{Gpu, WindowSlot},
};

#[cfg(test)]
thread_local! {
    /// `wgpu::Instance`s this thread's contexts created. `wgpu::Adapter`'s
    /// equality cannot tell whether two contexts share an instance (an
    /// adapter from a fresh instance compares equal), so tests count instead.
    /// Per thread, so tests running in parallel threads cannot move each
    /// other's count.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    pub(crate) static INSTANCES_CREATED: Cell<usize> = const { Cell::new(0) };
}

/// An offscreen slot: a texture the executor reads back.
struct OffscreenSlot {
    /// `None` after a device rebuild that could not make it at the slot's
    /// size: the slot renders nothing until a resize makes one.
    texture: Option<wgpu::Texture>,
    format: wgpu::TextureFormat,
    size: (u32, u32),
}

impl OffscreenSlot {
    /// The slot's target.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] while the device has no target of
    /// the slot's size (a rebuilt device with a smaller limit).
    fn target(&self) -> EngineResult<&wgpu::Texture> {
        self.texture.as_ref().ok_or(EngineError::InvalidTargetSize {
            width: self.size.0,
            height: self.size.1,
        })
    }
}

/// What one slot renders into.
enum SurfaceSlot {
    Offscreen(OffscreenSlot),
    Window(Box<WindowSlot>),
}

/// The slots of one context, by the index of the id the owner minted.
#[derive(Default)]
struct SlotTable {
    entries: Vec<Option<(SlotId, SurfaceSlot)>>,
}

impl SlotTable {
    /// Hold `slot` under `id`; `false` when `id`'s index is taken.
    fn insert(&mut self, id: SlotId, slot: SurfaceSlot) -> bool {
        let index = id.index() as usize;
        if self.entries.len() <= index {
            self.entries.resize_with(index + 1, || None);
        }
        if self.entries[index].is_some() {
            return false;
        }
        self.entries[index] = Some((id, slot));
        true
    }

    fn get_mut(&mut self, id: SlotId) -> Option<&mut SurfaceSlot> {
        match self.entries.get_mut(id.index() as usize)? {
            Some((held, slot)) if *held == id => Some(slot),
            _ => None,
        }
    }

    fn get(&self, id: SlotId) -> Option<&SurfaceSlot> {
        match self.entries.get(id.index() as usize)? {
            Some((held, slot)) if *held == id => Some(slot),
            _ => None,
        }
    }

    fn iter(&self) -> impl Iterator<Item = (SlotId, &SurfaceSlot)> {
        self.entries.iter().flatten().map(|(id, slot)| (*id, slot))
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = (SlotId, &mut SurfaceSlot)> {
        self.entries
            .iter_mut()
            .flatten()
            .map(|(id, slot)| (*id, slot))
    }

    /// Takes the slot out; `None` for a stale id or one already detached.
    fn remove(&mut self, id: SlotId) -> Option<SurfaceSlot> {
        let entry = self.entries.get_mut(id.index() as usize)?;
        if entry.as_ref().is_some_and(|(held, _)| *held == id) {
            return entry.take().map(|(_, slot)| slot);
        }
        None
    }
}

/// The slots a device rebuild found the new device cannot serve, kept across
/// attempts: a window whose surface an attempt dropped answers `Ok` to the
/// next (it is released), so a rebuild that unwinds or fails after the drop
/// must not forget that the owner owes it a surface (ADR-0305 §8).
#[derive(Default)]
struct RebuildRefusals {
    held: Vec<(SlotId, EngineError)>,
}

impl RebuildRefusals {
    /// The new device cannot serve `slot`, for `error`.
    fn refuse(&mut self, slot: SlotId, error: EngineError) {
        self.held.retain(|(held, _)| *held != slot);
        self.held.push((slot, error));
    }

    /// The new device serves `slot`; one left without its target (a window
    /// an earlier attempt released) keeps its refusal.
    fn served(&mut self, slot: SlotId, holds_target: bool) {
        if holds_target {
            self.held.retain(|(held, _)| *held != slot);
        }
    }

    /// The refusals of the rebuild that finished, for the slots the context
    /// holds.
    fn take(&mut self, holds: impl Fn(SlotId) -> bool) -> Vec<(SlotId, EngineError)> {
        let mut refused = std::mem::take(&mut self.held);
        refused.retain(|(slot, _)| holds(*slot));
        refused
    }
}

/// How a context's device was requested, so a rebuild asks for the same kind
/// on the same instance (ADR-0305 §8).
#[derive(Debug, Clone, Copy)]
enum DeviceRecipe {
    /// The backend ADR-0244's probe chose, with the capabilities the renderer
    /// negotiates.
    Probed(Attempt),
    /// wgpu's default device plus whichever of these features the adapter
    /// offers: offscreen contexts.
    Offscreen(wgpu::Features),
}

impl DeviceRecipe {
    /// An adapter on `instance` that presents to `surface` when one is named,
    /// and its device.
    async fn request(
        self,
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'static>>,
    ) -> EngineResult<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
        match self {
            Self::Probed(attempt) => {
                let (adapter, _, device, queue) =
                    crate::Renderer::adapter_and_device(instance, attempt, surface).await?;
                Ok((adapter, device, queue))
            }
            Self::Offscreen(wanted_features) => {
                let adapter = instance
                    .request_adapter(&crate::adapter::trusted_adapter_options(
                        wgpu::PowerPreference::HighPerformance,
                        surface,
                    ))
                    .await
                    .map_err(EngineError::adapter_request)?;
                let (device, queue) = offscreen_device(&adapter, wanted_features).await?;
                Ok((adapter, device, queue))
            }
        }
    }
}

/// An offscreen context's device on `adapter`.
async fn offscreen_device(
    adapter: &wgpu::Adapter,
    wanted_features: wgpu::Features,
) -> EngineResult<(wgpu::Device, wgpu::Queue)> {
    // Deliberately NOT `adapter::request_flui_device`: an offscreen context
    // wants wgpu's default (downlevel-friendly) device rather than the
    // renderer's capability-negotiated one, plus the features asked for,
    // which change what the rendered pixels look like, and the pipeline
    // cache, which changes none, wherever the adapter offers them.
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("FLUI GPU Context Device"),
            required_features: adapter.features()
                & (wanted_features | wgpu::Features::PIPELINE_CACHE),
            ..Default::default()
        })
        .await
        .map_err(EngineError::device_creation)
}

/// Wait for a device request on the executor's thread: the inline executor
/// rebuilds on the owner thread, where nothing else is polled meanwhile.
#[cfg(not(target_arch = "wasm32"))]
fn block_on<T>(request: impl std::future::Future<Output = EngineResult<T>>) -> EngineResult<T> {
    pollster::block_on(request)
}

/// On the web a request is a future the owner spawns, which the inline
/// executor cannot wait for: the device stays lost.
#[cfg(target_arch = "wasm32")]
fn block_on<T>(request: impl std::future::Future<Output = EngineResult<T>>) -> EngineResult<T> {
    drop(request);
    Err(EngineError::GpuUnavailable)
}

/// What a context has built over its life, read by the tests that pin the
/// sharing ADR-0305 §1 decides.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextBuilds {
    /// Devices requested: one per context.
    pub(crate) devices: usize,
    /// Glyph atlases built: one per painter the context built.
    pub(crate) glyph_atlases: usize,
    /// Pipeline sets built: one per painter and per further target format.
    pub(crate) pipeline_sets: usize,
}

/// The one GPU context of an application (ADR-0305 §1).
///
/// Owned by the code that rasterises; never a `static` and never reached
/// ambiently (ADR-0097). `Send`, so a raster thread can take it, and not
/// `Sync`, so it has one mutator and its caches need no lock.
pub struct GpuContext {
    /// The instance the factory makes every window surface on (ADR-0305 §2),
    /// made on the host's display connection. The executor makes no surface
    /// on it; the device rebuild (§8) requests its adapter here.
    instance: wgpu::Instance,
    /// The adapter the device was requested on; a rebuild can pick another.
    adapter: wgpu::Adapter,
    /// How the device was requested, so a rebuild asks for the same kind.
    recipe: DeviceRecipe,
    domain: Arc<DeviceDomain>,
    /// A device rebuild is under way: set before the new device replaces
    /// the old one and cleared once every slot is on it, so a rebuild that
    /// unwinds part way leaves the device lost (ADR-0305 §8).
    rebuilding: bool,
    /// What the attempts of the rebuild under way refused.
    refusals: RebuildRefusals,
    /// The painter every slot records and replays with: its pipelines per
    /// target format, the glyph atlas and the image and path caches.
    ///
    /// Taken for the length of a frame. A frame that unwinds tears what it
    /// was recording, so the painter is dropped then and only its pipeline
    /// library is kept, in `pipelines` (ADR-0305 §7).
    painter: Option<WgpuPainter>,
    /// The pipelines of a painter a frame unwound with, for the next painter
    /// to build on: a panic costs the caches every slot shares, not the
    /// pipelines.
    pipelines: Option<PipelineLibrary>,
    /// The library of the first format a slot named, built on a worker until
    /// a frame takes it (ADR-0305 §1).
    #[cfg(not(target_arch = "wasm32"))]
    warmup: Option<crate::pipeline_warmup::PipelineWarmup>,
    /// The driver's pipeline cache every pipeline of the device is created
    /// through, and the storage it is saved in between runs; `None` until
    /// [`Self::load_pipeline_cache`], and where the device has none.
    #[cfg(not(target_arch = "wasm32"))]
    pipeline_store: Option<PipelineStore>,
    /// The presentation blit of window slots whose frames render into a
    /// retained target, one pipeline per surface format; built at the first
    /// window frame.
    blit: Option<OffscreenRenderer>,
    slots: SlotTable,
    /// Each slot's external textures: a presentation mints its texture ids
    /// alone, so two slots' ids collide in the shared painter, which is lent
    /// a slot's textures for the length of its frame. Kept from a slot's
    /// first update until it detaches, across device rebuilds.
    textures: FxHashMap<SlotId, KeptTextures>,
    /// The lineage each slot last drew a version of: the painter's
    /// recordings of one no slot draws any more are dropped.
    lineages: crate::recording_cache::DrawnLineages<SlotId>,
    /// What the driver's report of a lost device wakes, besides marking the
    /// device lost: the owner that replaces the context (the web).
    loss_wake: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    builds: ContextBuilds,
    /// Flush submissions of texture writes a frame did not submit.
    #[cfg(test)]
    texture_flushes: Cell<usize>,
    /// How many of the next offscreen frames panic after their recording.
    #[cfg(test)]
    panics_ahead: u32,
    /// When set, the next frame's painter setup panics, as a tracing
    /// subscriber can.
    #[cfg(test)]
    panic_in_next_setup: bool,
    /// When set, the next warm-up panics on its worker.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    panic_in_warmup: bool,
    /// When set, the next torn painter's drop panics.
    #[cfg(test)]
    panic_in_next_cleanup: bool,
    /// What the last offscreen frame's walk and recording did.
    #[cfg(test)]
    slot_counters: crate::frame_protocol::RasterCounters,
    /// How many of the next device rebuilds fail.
    #[cfg(test)]
    rebuilds_failing: u32,
    _one_mutator: PhantomData<Cell<()>>,
}

static_assertions::assert_impl_all!(GpuContext: Send);
static_assertions::assert_not_impl_any!(GpuContext: Sync);

/// The application's one GPU context, the factory its windows' surfaces are
/// made with, and the first window's surface when there was one
/// ([`GpuContext::bootstrap`]).
#[non_exhaustive]
pub struct GpuBootstrap {
    /// The context, for the raster executor.
    pub context: GpuContext,
    /// The owner's factory over the context's instance.
    pub factory: SurfaceFactory,
    /// The first window's surface, for its slot's attach.
    pub first: Option<UnconfiguredSurface>,
}

impl std::fmt::Debug for GpuBootstrap {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GpuBootstrap")
            .field("first", &self.first.is_some())
            .finish_non_exhaustive()
    }
}

/// One backend attempt's instance, adapter, device and the first window's
/// surface.
struct Answered {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    first: Option<wgpu::Surface<'static>>,
    /// The backend attempt that answered, for a rebuild to ask the same.
    attempt: Attempt,
}

impl GpuContext {
    /// The application's one context, on the host's `display`, with the
    /// factory the owner makes every window's surface with (ADR-0305 §2).
    ///
    /// Runs ADR-0244 decision 1's probe once: the preferred backend, then its
    /// fallback, each in an instance of its own, over the `first` window when
    /// there is one, so the adapter chosen presents to it; `WGPU_BACKEND`
    /// keeps only the backends it names. The winning instance is the
    /// context's and the factory's, for the context's life. `first` is
    /// `None` for a context of offscreen slots only.
    ///
    /// # Errors
    /// - [`EngineError::SurfaceTargetUnavailable`] or
    ///   [`EngineError::SurfaceOnForeignDisplay`]: `first` fails the factory's
    ///   checks, before any GPU work.
    /// - [`EngineError::NoSupportedGpu`]: no backend gave a device, with why
    ///   each one failed.
    pub async fn bootstrap(
        display: Arc<dyn DisplayConnection>,
        first: Option<Arc<dyn SurfaceTarget>>,
    ) -> EngineResult<GpuBootstrap> {
        let first = first
            .map(|target| checked_target(&*display, &target).map(|leased| (leased, target)))
            .transpose()?;
        let attempts = crate::gpu_backend::attempts(
            crate::gpu_backend::Family::CURRENT.order(),
            wgpu::Backends::from_env(),
        );
        let leased = first.as_ref().map(|(leased, _)| leased);
        let display_ref = &display;
        let answered = crate::gpu_backend::first_answering(&attempts, |attempt| async move {
            Self::answer(attempt, display_ref, leased)
                .await
                .map_err(|error| error.to_string())
        })
        .await;
        let (backend, answered) =
            answered.map_err(|tried| EngineError::NoSupportedGpu { tried })?;
        tracing::info!(%backend, adapter = %answered.adapter.get_info().name, "GPU backend selected");
        let Answered {
            instance,
            adapter,
            device,
            queue,
            first: surface,
            attempt,
        } = answered;
        let first = match (first, surface) {
            (Some((leased, target)), Some(surface)) => {
                Some(UnconfiguredSurface::new(leased, target, surface))
            }
            _ => None,
        };
        let factory = SurfaceFactory::new(instance.clone(), display);
        let context = Self::from_device(
            instance,
            adapter,
            device,
            queue,
            DeviceRecipe::Probed(attempt),
        );
        Ok(GpuBootstrap {
            context,
            factory,
            first,
        })
    }

    /// One backend attempt: an instance over `display` for `attempt` alone,
    /// the first window's surface on it, an adapter that presents to it, and
    /// its device.
    async fn answer(
        attempt: Attempt,
        display: &Arc<dyn DisplayConnection>,
        first: Option<&Arc<dyn crate::window_target::WindowTarget>>,
    ) -> EngineResult<Answered> {
        #[cfg(test)]
        INSTANCES_CREATED.with(|created| created.set(created.get() + 1));
        let instance = wgpu::Instance::new(instance_descriptor(
            display,
            attempt.backends,
            wgpu::BackendOptions {
                dx12: crate::surface_present::dx12_options(),
                ..Default::default()
            },
        ));
        let surface = |instance: &wgpu::Instance| {
            first
                .map(|target| {
                    instance
                        .create_surface(Arc::clone(target))
                        .map_err(EngineError::surface_creation)
                })
                .transpose()
        };
        // A canvas keeps the first context kind it hands out, so WebGPU gets
        // its device before it claims the canvas (as `Renderer::new` does).
        if attempt.backend == GpuBackend::WebGpu {
            let (adapter, _, device, queue) =
                crate::Renderer::adapter_and_device(&instance, attempt, None).await?;
            let first = surface(&instance)?;
            return Ok(Answered {
                instance,
                adapter,
                device,
                queue,
                first,
                attempt,
            });
        }
        let first = surface(&instance)?;
        let (adapter, _, device, queue) =
            crate::Renderer::adapter_and_device(&instance, attempt, first.as_ref()).await?;
        Ok(Answered {
            instance,
            adapter,
            device,
            queue,
            first,
            attempt,
        })
    }

    /// A context with no window: offscreen slots only (headless, tests).
    ///
    /// Requests whichever of `wanted_features` the adapter offers.
    /// `WGPU_BACKEND` picks the backend, so a readback suite runs on the GL
    /// fallback with `WGPU_BACKEND=gl` (ADR-0244 §1).
    ///
    /// # Errors
    /// [`EngineError`] when no GPU adapter or device is available.
    pub(crate) async fn offscreen(wanted_features: wgpu::Features) -> EngineResult<Self> {
        #[cfg(test)]
        INSTANCES_CREATED.with(|created| created.set(created.get() + 1));
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let recipe = DeviceRecipe::Offscreen(wanted_features);
        let (adapter, device, queue) = recipe.request(&instance, None).await?;
        Ok(Self::from_device(instance, adapter, device, queue, recipe))
    }

    /// A second context on this one's adapter and instance, its device
    /// requested with `wanted_features`.
    #[cfg(test)]
    pub(crate) fn twin(
        &self,
        wanted_features: wgpu::Features,
    ) -> impl std::future::Future<Output = EngineResult<Self>> + use<> {
        Self::on_adapter(self.instance.clone(), self.adapter.clone(), wanted_features)
    }

    /// An offscreen context on `adapter` of `instance`.
    #[cfg(test)]
    async fn on_adapter(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        wanted_features: wgpu::Features,
    ) -> EngineResult<Self> {
        let (device, queue) = offscreen_device(&adapter, wanted_features).await?;
        let recipe = DeviceRecipe::Offscreen(wanted_features);
        Ok(Self::from_device(instance, adapter, device, queue, recipe))
    }

    fn from_device(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        recipe: DeviceRecipe,
    ) -> Self {
        let domain = DeviceDomain::new(Arc::new(device), Arc::new(queue));
        install_device_diagnostics(&domain, None);
        Self {
            instance,
            adapter,
            recipe,
            domain,
            rebuilding: false,
            refusals: RebuildRefusals::default(),
            painter: None,
            pipelines: None,
            #[cfg(not(target_arch = "wasm32"))]
            warmup: None,
            #[cfg(not(target_arch = "wasm32"))]
            pipeline_store: None,
            blit: None,
            slots: SlotTable::default(),
            textures: FxHashMap::default(),
            lineages: crate::recording_cache::DrawnLineages::new(),
            loss_wake: None,
            #[cfg(test)]
            texture_flushes: Cell::new(0),
            #[cfg(test)]
            builds: ContextBuilds {
                devices: 1,
                glyph_atlases: 0,
                pipeline_sets: 0,
            },
            #[cfg(test)]
            panics_ahead: 0,
            #[cfg(test)]
            panic_in_next_cleanup: false,
            #[cfg(test)]
            slot_counters: crate::frame_protocol::RasterCounters::default(),
            #[cfg(test)]
            panic_in_next_setup: false,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            panic_in_warmup: false,
            #[cfg(test)]
            rebuilds_failing: 0,
            _one_mutator: PhantomData,
        }
    }

    /// Make a new device on the context's instance and configure every slot
    /// on it again (ADR-0305 §8). The surfaces are kept: the adapter is asked
    /// to present to the first window slot holding one. The pipelines, the
    /// glyph atlas, the image and path caches and the presentation blit were
    /// the old device's, so the next frame builds them again; an offscreen
    /// slot gets a new target of its size, cleared.
    ///
    /// Answers the slots the new device cannot present to, with why; their
    /// surface was dropped.
    ///
    /// # Errors
    /// The adapter or device request failed; the context keeps its lost
    /// device and every slot as it was.
    fn rebuild(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>> {
        #[cfg(test)]
        if self.rebuilds_failing > 0 {
            self.rebuilds_failing -= 1;
            return Err(EngineError::GpuUnavailable);
        }
        let compatible = self.slots.iter().find_map(|(_, slot)| match slot {
            SurfaceSlot::Window(window) => window.surface(),
            SurfaceSlot::Offscreen(_) => None,
        });
        let (adapter, device, queue) = block_on(self.recipe.request(&self.instance, compatible))?;
        // The device counts as lost until every slot is on the new one, so a
        // rebuild that unwinds part way is asked for again from the start.
        self.rebuilding = true;
        // What the old device made goes before the new device is used.
        self.painter = None;
        self.pipelines = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.warmup = None;
        }
        self.blit = None;
        for kept in self.textures.values_mut() {
            kept.leave_device();
        }
        self.adapter = adapter;
        self.domain = DeviceDomain::new(Arc::new(device), Arc::new(queue));
        install_device_diagnostics(&self.domain, self.loss_wake.clone());
        // The lost device's cache goes unsaved, since a lost device's answers
        // are not trusted, and the new device's starts cold: the rebuild
        // waits for no storage read, and the warm-up and retirement save the
        // new one as they saved the first.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pipeline_store = self
                .pipeline_store
                .take()
                .and_then(|lost| lost.restart(self.domain.device(), &self.adapter.get_info()));
        }
        #[cfg(test)]
        {
            self.builds.devices += 1;
        }
        let max_dim = self.max_texture_dimension();
        let Self {
            adapter,
            domain,
            slots,
            refusals,
            ..
        } = self;
        let gpu = gpu_of(adapter, domain);
        for (id, slot) in slots.iter_mut() {
            let rebuilt = match slot {
                SurfaceSlot::Offscreen(entry) => {
                    // A target the new device cannot make at the slot's size
                    // is gone, not left on the old device: the slot renders
                    // nothing until a resize makes one.
                    let made = offscreen_texture(domain, max_dim, entry.size, entry.format);
                    entry.texture = made.as_ref().ok().cloned();
                    made.map(|_| true)
                }
                SurfaceSlot::Window(window) => {
                    window.rebuild(&gpu).map(|()| window.surface().is_some())
                }
            };
            // Each answer is recorded as it is found, so an attempt that
            // unwinds part way leaves what it refused for the next.
            match rebuilt {
                Ok(holds_target) => refusals.served(id, holds_target),
                Err(error) => refusals.refuse(id, error),
            }
        }
        self.rebuilding = false;
        let slots = &self.slots;
        Ok(self.refusals.take(|id| slots.get(id).is_some()))
    }

    /// The device domain every slot submits through.
    pub(crate) fn domain(&self) -> &Arc<DeviceDomain> {
        &self.domain
    }

    /// The device's features.
    #[cfg(test)]
    pub(crate) fn features(&self) -> wgpu::Features {
        self.domain.device().features()
    }

    /// The device's largest 2D texture side, the largest slot it renders.
    pub(crate) fn max_texture_dimension(&self) -> u32 {
        self.domain.device().limits().max_texture_dimension_2d
    }

    /// Attach an offscreen slot under `slot`, of `size` device pixels in
    /// `format`, an encoded RGBA8 or BGRA8 colour format.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] for a zero or over-limit side;
    /// [`EngineError::InvalidRenderTarget`] for another format or an id
    /// whose index holds a slot.
    pub(crate) fn attach_offscreen(
        &mut self,
        slot: SlotId,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> EngineResult<()> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(EngineError::InvalidRenderTarget {
                reason: "an offscreen slot is RGBA8 or BGRA8",
            });
        }
        let texture = self.slot_texture(size, format)?;
        self.insert(
            slot,
            SurfaceSlot::Offscreen(OffscreenSlot {
                texture: Some(texture),
                format,
                size,
            }),
        )?;
        self.warm_pipelines(format);
        Ok(())
    }

    /// Attach a window slot under `slot`, presenting to `surface`.
    ///
    /// # Errors
    /// [`EngineError::IncompatibleSurface`] when the context's adapter cannot
    /// present to `surface`; [`EngineError::InvalidRenderTarget`] for an id
    /// whose index holds a slot.
    fn attach_window(
        &mut self,
        slot: SlotId,
        surface: UnconfiguredSurface,
        config: SlotConfig,
    ) -> EngineResult<()> {
        let window = WindowSlot::attach(
            &self.gpu(),
            surface,
            config.color,
            config.display,
            config.latency,
        )?;
        let format = window.expected_format(&self.gpu());
        self.insert(slot, SurfaceSlot::Window(Box::new(window)))?;
        if let Some(format) = format {
            self.warm_pipelines(format);
        }
        Ok(())
    }

    fn insert(&mut self, slot: SlotId, surface: SurfaceSlot) -> EngineResult<()> {
        if self.slots.insert(slot, surface) {
            Ok(())
        } else {
            Err(EngineError::InvalidRenderTarget {
                reason: "the slot's index holds another slot",
            })
        }
    }

    /// Give `slot` a target of `size` device pixels; an offscreen slot's
    /// previous contents are dropped. Nothing changes for a size the slot
    /// already has.
    ///
    /// # Errors
    /// [`EngineError::InvalidTargetSize`] for a zero or over-limit side,
    /// which leaves the slot as it was; [`EngineError::InvalidRenderTarget`]
    /// for a slot that is not attached.
    pub(crate) fn resize_slot(&mut self, slot: SlotId, size: (u32, u32)) -> EngineResult<()> {
        let max_dim = self.max_texture_dimension();
        let (adapter, domain) = (&self.adapter, &self.domain);
        match self.slots.get_mut(slot) {
            Some(SurfaceSlot::Offscreen(entry)) => {
                if entry.size == size && entry.texture.is_some() {
                    return Ok(());
                }
                entry.texture = Some(offscreen_texture(domain, max_dim, size, entry.format)?);
                entry.size = size;
                Ok(())
            }
            Some(SurfaceSlot::Window(window)) => window.resize(&gpu_of(adapter, domain), size),
            None => Err(not_attached()),
        }
    }

    /// Call `wake` whenever the driver reports this context's device lost,
    /// after the device is marked lost: an owner that replaces a lost context
    /// itself (the web, ADR-0305 §8) hears of the loss with no frame to find
    /// it. `wake` only schedules the owner, since the driver may report the
    /// loss from inside a call of the context.
    pub fn on_device_lost(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.loss_wake = Some(Arc::clone(&wake));
        install_device_diagnostics(&self.domain, Some(wake));
    }

    /// Create the driver's pipeline cache from the data `storage` kept for
    /// this adapter, and save the cache's data there once the first format's
    /// pipelines are built, off the frame path. A driver without a disk cache
    /// of its own (Android's Vulkan drivers) then compiles those pipelines
    /// once, not at every launch.
    ///
    /// Call it once, after [`Self::bootstrap`] and before the first slot
    /// attaches: a pipeline built before it is not in the cache. It does
    /// nothing where the device has no pipeline cache (every backend but
    /// Vulkan). It never fails: a value that cannot be read starts the cache
    /// cold, and one that is not this adapter's cache is emptied as well, so
    /// it is not read again.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn load_pipeline_cache(&mut self, storage: Arc<dyn flui_platform::Storage>) {
        let adapter = self.adapter.get_info();
        self.pipeline_store = PipelineStore::load(self.domain.device(), &adapter, storage).await;
    }

    /// A cold pipeline cache saved to `storage`, for a context whose stored
    /// data could not be read in time: this run compiles every pipeline, and
    /// its warm-up still saves them for the next. Nothing where the device has
    /// no pipeline cache.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_pipeline_cache_cold(&mut self, storage: Arc<dyn flui_platform::Storage>) {
        let adapter = self.adapter.get_info();
        self.pipeline_store = PipelineStore::cold(self.domain.device(), &adapter, storage);
    }

    /// Whether wgpu created the pipeline cache from stored data; `None`
    /// without a cache.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn pipeline_cache_seeded(&self) -> Option<bool> {
        self.pipeline_store.as_ref().map(PipelineStore::seeded)
    }

    /// The device, and the pipeline cache its pipelines are created through.
    fn pipeline_device(&self) -> PipelineDevice<'_> {
        PipelineDevice::new(self.domain.device(), self.driver_cache())
    }

    /// The pipeline cache the device's pipelines are created through.
    #[cfg(not(target_arch = "wasm32"))]
    fn driver_cache(&self) -> Option<&wgpu::PipelineCache> {
        self.pipeline_store.as_ref().map(PipelineStore::cache)
    }

    /// WebGPU has no pipeline cache.
    #[cfg(target_arch = "wasm32")]
    #[expect(
        clippy::unused_self,
        reason = "WebGPU has no pipeline cache; native targets read the context's"
    )]
    fn driver_cache(&self) -> Option<&wgpu::PipelineCache> {
        None
    }

    /// Detach `slot`; `false` for a stale id or one already detached.
    pub(crate) fn detach(&mut self, slot: SlotId) -> bool {
        self.textures.remove(&slot);
        if let Some(dead) = self.lineages.draws(slot, None)
            && let Some(painter) = self.painter.as_mut()
        {
            painter.drop_recordings_of(dead);
        }
        self.slots.remove(slot).is_some()
    }

    /// Notes that `slot` draws `scene` with `painter`, which drops the
    /// recordings of the lineage the slot drew before once no slot draws it.
    fn note_lineage(
        &mut self,
        slot: SlotId,
        scene: flui_layer::SceneContent<'_>,
        painter: &mut WgpuPainter,
    ) {
        if let Some(dead) = self.lineages.draws(slot, Some(scene)) {
            painter.drop_recordings_of(dead);
        }
    }

    /// Apply `updates` to `slot`'s external textures.
    ///
    /// # Errors
    /// [`EngineError::InvalidRenderTarget`] for a slot that is not attached;
    /// the first frame no texture of the device can hold, the others still
    /// applied.
    pub(crate) fn apply_slot_textures(
        &mut self,
        slot: SlotId,
        updates: TextureUpdates,
    ) -> EngineResult<()> {
        if self.slots.get(slot).is_none() {
            return Err(not_attached());
        }
        // A slot that never drew a texture keeps nothing for a retirement.
        if updates.frames.is_empty() && !self.textures.contains_key(&slot) {
            return Ok(());
        }
        self.textures
            .entry(slot)
            .or_default()
            .apply(&self.domain, updates)
    }

    /// Put `slot`'s external textures on the current device before its
    /// frame takes the painter, so the lend itself uploads nothing and a
    /// panic here (a tracing subscriber is not ours) costs neither the
    /// painter nor the blit. Answers whether it wrote to the queue: a frame
    /// that then submits nothing flushes the writes
    /// ([`Self::flush_texture_writes`]).
    fn prepare_textures(&mut self, slot: SlotId) -> bool {
        let Some(kept) = self.textures.get_mut(&slot) else {
            return false;
        };
        let domain = &self.domain;
        let mut caught = FirstPanic::new();
        let wrote = caught.contain(|| kept.prepare(domain));
        if wrote.is_none() {
            // Whatever the upload wrote before it unwound is flushed, so a
            // panic that repeats each frame cannot pile up staging memory.
            let _flushed = caught.contain(|| self.flush_texture_writes());
            caught.resume();
        }
        wrote.unwrap_or(true)
    }

    /// Run the frame setup that follows [`Self::prepare_textures`] (taking
    /// the painter, building the blit): a panic there (a tracing subscriber
    /// is not ours) flushes the texture writes the frame made, as a panic of
    /// the draw does, before it is resumed.
    fn after_uploads<T>(&mut self, wrote: bool, setup: impl FnOnce(&mut Self) -> T) -> T {
        let mut caught = FirstPanic::new();
        let made = caught.contain(|| setup(&mut *self));
        if let Some(made) = made {
            return made;
        }
        if wrote {
            let _flushed = caught.contain(|| self.flush_texture_writes());
        }
        caught.resume();
        unreachable!("BUG: a contained call that did not return holds its panic")
    }

    /// Submit the texture writes a frame made and did not submit:
    /// `write_texture` holds staging memory until a queue submission, and a
    /// frame can end without one (no damage, a surface not acquired, an
    /// error, a panic). Bounded by the domain's submission ledger as a frame
    /// is; a refused flush leaves the writes to the next submission, which a
    /// frame admitted by the same ledger makes.
    fn flush_texture_writes(&self) {
        let flushed = self
            .domain
            .prepare(Vec::new(), Vec::new())
            .and_then(|prepared| self.domain.submit(prepared));
        #[cfg(test)]
        self.texture_flushes.set(self.texture_flushes.get() + 1);
        drop(flushed);
    }

    /// Lend `slot`'s external textures to `painter` for the slot's frame,
    /// handing back the painter's own registry, which
    /// [`Self::take_back_textures`] returns to it.
    ///
    /// While lent, the slot keeps no allocations: a frame that unwinds before
    /// they come back leaves the slot to upload its kept images again on its
    /// next frame, never to draw from the painter's empty registry.
    fn lend_textures(
        &mut self,
        slot: SlotId,
        painter: &mut WgpuPainter,
    ) -> Option<ExternalTextureRegistry> {
        let mut lent = self.textures.get_mut(&slot)?.lend(&self.domain);
        painter.swap_external_textures(&mut lent);
        Some(lent)
    }

    /// Take `slot`'s external textures back from `painter`, giving the
    /// painter the registry [`Self::lend_textures`] handed out.
    fn take_back_textures(
        &mut self,
        slot: SlotId,
        painter: &mut WgpuPainter,
        own: Option<ExternalTextureRegistry>,
    ) {
        let Some(mut registry) = own else {
            return;
        };
        painter.swap_external_textures(&mut registry);
        if let Some(kept) = self.textures.get_mut(&slot) {
            kept.take_back(registry);
        }
    }

    /// Clear offscreen `slot` to the frame background and draw `tree` over it,
    /// replaying kept recordings as `reads` allows.
    ///
    /// # Errors
    /// [`EngineError::InvalidRenderTarget`] for a slot that is not an
    /// attached offscreen slot, or the frame's own render and submission
    /// errors.
    ///
    /// # Panics
    /// A panic of the frame is resumed once the painter's pipelines are kept
    /// for the next frame (ADR-0305 §7).
    pub(crate) fn render_slot(
        &mut self,
        slot: SlotId,
        scene: flui_layer::SceneContent<'_>,
        reads: crate::recording_cache::CacheReads,
    ) -> EngineResult<()> {
        let (texture, format, size) = {
            let entry = self.offscreen_slot(slot)?;
            (entry.target()?.clone(), entry.format, entry.size)
        };
        let submission_scope = self.domain.begin_frame_scope()?;
        let wrote = self.prepare_textures(slot);
        let mut painter = self.after_uploads(wrote, |this| this.take_painter(format, size));
        self.note_lineage(slot, scene, &mut painter);
        let own_textures = self.lend_textures(slot, &mut painter);
        #[cfg(test)]
        let panics = self.injected_panic();
        let domain = &self.domain;
        let mut caught = FirstPanic::new();
        let drawn = caught.contain(|| {
            painter.begin_frame_in_scope();
            painter.resize(size.0, size.1);
            let drawn = (|| {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                crate::headless::clear_to_background(domain, &view)?;
                let counters = {
                    let mut backend = LayerDispatcher::new(&mut painter);
                    crate::layer_walk::record_scene(scene, None, reads, &mut backend)?
                };
                painter.settle_recording_cache();
                #[cfg(test)]
                assert!(!panics, "injected panic in the middle of a slot's frame");
                let mut encoder =
                    domain
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("FLUI Slot Render Encoder"),
                        });
                painter.render_to_texture(&texture, &mut encoder)?;
                painter.submit_encoder(encoder).map(|_submitted| counters)
            })();
            // Once a frame began, an error closes it as a completed frame
            // does, with its encoder discarded, as the window path's frame
            // protocol closes its own: the painter is whole for the next one.
            painter.finish_frame();
            drawn
        });
        self.take_back_textures(slot, &mut painter, own_textures);
        drop(submission_scope);
        let Some(drawn) = drawn else {
            self.keep_pipelines(painter, &mut caught);
            if wrote {
                let _flushed = caught.contain(|| self.flush_texture_writes());
            }
            caught.resume();
            unreachable!("BUG: a contained call that did not return holds its panic")
        };
        self.painter = Some(painter);
        // A submitted frame carried the texture writes made before it.
        if wrote && drawn.is_err() {
            self.flush_texture_writes();
        }
        // A slot's frame reports no counters; a test reads them here.
        let counters = drawn?;
        #[cfg(test)]
        {
            self.slot_counters = counters;
        }
        #[cfg(not(test))]
        let _ = counters;
        Ok(())
    }

    /// What the last offscreen frame's walk and recording did.
    #[cfg(all(test, feature = "testing"))]
    pub(crate) fn slot_counters(&self) -> crate::frame_protocol::RasterCounters {
        self.slot_counters
    }

    /// Render `scene` into window `slot` and present it, repainting at least
    /// `damage`.
    ///
    /// # Errors
    /// [`EngineError::DeviceLost`] once the device is lost; the frame's
    /// acquire, render and submission errors.
    ///
    /// # Panics
    /// A panic of the frame is resumed once the painter's pipelines and the
    /// blit pipelines are kept for the next frame (ADR-0305 §7).
    fn render_window(
        &mut self,
        slot: SlotId,
        frame: FrameStamp,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        self.draw_window(slot, scene, WindowDraw::Frame { damage, frame })
    }

    /// Draw `scene` into window `slot` as `draw` asks and present it, as
    /// [`render_window`](Self::render_window) does.
    fn draw_window(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        draw: WindowDraw,
    ) -> EngineResult<PresentDisposition> {
        if self.domain.is_lost() {
            return Err(EngineError::DeviceLost);
        }
        let target = match self.slots.get(slot) {
            Some(SurfaceSlot::Window(window)) => window.target_format(),
            Some(SurfaceSlot::Offscreen(_)) | None => return Err(not_attached()),
        };
        let Some((format, size)) = target else {
            // Released, or not sized yet: nothing to present into.
            return Ok(PresentDisposition::NotShown);
        };
        // Uploads run before the painter and the blit are taken, and the
        // painter is taken only once it is built and retargeted, so a panic
        // in either step (a tracing subscriber is not ours) costs neither.
        let wrote = self.prepare_textures(slot);
        let (mut painter, mut blit) = self.after_uploads(wrote, |this| {
            let painter = this.take_painter(format, size);
            // The blit's pipelines, like the library's, are inserted only
            // once built, so an unwound frame leaves them whole.
            let blit = this.blit.take().unwrap_or_else(|| {
                let cache = this.driver_cache().cloned();
                OffscreenRenderer::with_domain(Arc::clone(&this.domain), cache)
            });
            (painter, blit)
        });
        self.note_lineage(slot, scene.content(), &mut painter);
        let own_textures = self.lend_textures(slot, &mut painter);
        let Self {
            adapter,
            domain,
            slots,
            ..
        } = self;
        let Some(SurfaceSlot::Window(window)) = slots.get_mut(slot) else {
            unreachable!("BUG: the window slot was found above and nothing removed it");
        };
        let mut caught = FirstPanic::new();
        let rendered = caught.contain(|| {
            painter.resize(size.0, size.1);
            let gpu = gpu_of(adapter, domain);
            match draw {
                WindowDraw::Frame { damage, frame } => {
                    window.render(&gpu, &mut painter, &mut blit, scene, damage, frame)
                }
                WindowDraw::Lent { reset_fonts } => {
                    window.render_lent(&gpu, &mut painter, &mut blit, scene, reset_fonts)
                }
            }
        });
        self.blit = Some(blit);
        self.take_back_textures(slot, &mut painter, own_textures);
        let Some(rendered) = rendered else {
            self.keep_pipelines(painter, &mut caught);
            if wrote {
                let _flushed = caught.contain(|| self.flush_texture_writes());
            }
            caught.resume();
            unreachable!("BUG: a contained call that did not return holds its panic")
        };
        // An error leaves the painter whole, as on the offscreen path:
        // acquire errors come before it records, and a recording error is
        // closed by the frame protocol's cleanup.
        self.painter = Some(painter);
        // A presented frame submitted the texture writes made before it.
        if wrote && !matches!(rendered, Ok(PresentDisposition::Presented)) {
            self.flush_texture_writes();
        }
        rendered
    }

    /// Drop `painter`, whose frame unwound, and keep its pipelines for the
    /// next painter: its glyph atlas, image and path caches and scratch pools
    /// can be torn, and every slot shares them.
    ///
    /// The drop runs under `caught`, which holds the frame's panic: a cached
    /// value's destructor is not ours, and one that panics costs the library
    /// too, while the frame's panic stays the one the slot's record gets.
    fn keep_pipelines(&mut self, painter: WgpuPainter, caught: &mut FirstPanic) {
        #[cfg(test)]
        let cleanup_panics = std::mem::take(&mut self.panic_in_next_cleanup);
        self.pipelines = caught.contain(|| {
            let pipelines = painter.into_pipelines();
            #[cfg(test)]
            assert!(!cleanup_panics, "injected panic dropping a torn painter");
            pipelines
        });
    }

    /// The pixels offscreen `slot` holds, tightly packed, top row first,
    /// four bytes a pixel in the slot format's channel order.
    ///
    /// # Errors
    /// [`EngineError::InvalidRenderTarget`] for a slot that is not an
    /// attached offscreen slot; a readback error.
    pub(crate) fn read_slot(&self, slot: SlotId) -> EngineResult<Vec<u8>> {
        let entry = self.offscreen_slot(slot)?;
        crate::headless::readback_rgba(&self.domain, entry.target()?, entry.size.0, entry.size.1)
    }

    /// What this context has built over its life.
    #[cfg(test)]
    pub(crate) fn builds(&self) -> ContextBuilds {
        self.builds
    }

    /// Whether a pipeline warm-up is under way with its library not taken.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn warming_up(&self) -> bool {
        self.warmup
            .as_ref()
            .is_some_and(crate::pipeline_warmup::PipelineWarmup::is_pending)
    }

    /// Wait for a warm-up's worker, its save included.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn join_warmup(&mut self) {
        self.warmup = None;
    }

    /// Make the next offscreen frame panic after its recording, before its
    /// submission.
    #[cfg(test)]
    pub(crate) fn panic_in_next_frame(&mut self) {
        self.panic_in_next_frames(1);
    }

    /// Make each of the next `frames` offscreen frames panic after its
    /// recording, whichever slots they are of.
    #[cfg(test)]
    pub(crate) fn panic_in_next_frames(&mut self, frames: u32) {
        self.panics_ahead = frames;
    }

    /// Make the next frame's painter setup panic, after its texture
    /// uploads, as a tracing subscriber can.
    #[cfg(test)]
    pub(crate) fn panic_in_next_setup(&mut self) {
        self.panic_in_next_setup = true;
    }

    /// Make the next pipeline warm-up panic on its worker, as wgpu's error
    /// handler can.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn panic_in_next_warmup(&mut self) {
        self.panic_in_warmup = true;
    }

    /// Make the drop of the next torn painter panic, as a cached value's
    /// destructor can.
    #[cfg(test)]
    pub(crate) fn panic_in_next_cleanup(&mut self) {
        self.panic_in_next_cleanup = true;
    }

    /// Lose the device, as wgpu's device-lost callback does.
    #[cfg(test)]
    pub(crate) fn lose_device(&self) {
        self.domain.mark_lost();
    }

    /// Make the next `rebuilds` device rebuilds fail before any request.
    #[cfg(test)]
    pub(crate) fn fail_next_rebuilds(&mut self, rebuilds: u32) {
        self.rebuilds_failing = rebuilds;
    }

    /// How many recordings the shared painter's cache keeps; `None` before
    /// a frame built the painter.
    #[cfg(test)]
    pub(crate) fn kept_recordings(&mut self) -> Option<usize> {
        self.painter
            .as_mut()
            .map(|painter| painter.recording_cache_work().0)
    }

    /// Whether this frame panics, counting it off.
    #[cfg(test)]
    fn injected_panic(&mut self) -> bool {
        let panics = self.panics_ahead > 0;
        self.panics_ahead = self.panics_ahead.saturating_sub(1);
        panics
    }

    /// Start building `format`'s pipeline library on a worker, when no frame
    /// has built one and none is under way: the first slot to name its
    /// format overlaps the driver's compilation with the application's first
    /// build (ADR-0305 §1). wasm32 builds it on the frame.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            clippy::unused_self,
            reason = "wasm32 has no thread to warm pipelines on; the frame builds them"
        )
    )]
    pub(crate) fn warm_pipelines(&mut self, format: wgpu::TextureFormat) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.painter.is_none() && self.pipelines.is_none() && self.warmup.is_none() {
            let domain = Arc::clone(&self.domain);
            let store = self.pipeline_store.clone();
            #[cfg(test)]
            let panics = std::mem::take(&mut self.panic_in_warmup);
            self.warmup = crate::pipeline_warmup::PipelineWarmup::start(
                {
                    let domain = Arc::clone(&domain);
                    let store = store.clone();
                    move || {
                        #[cfg(test)]
                        assert!(!panics, "injected panic in a pipeline warm-up");
                        let cache = store.as_ref().map(PipelineStore::cache);
                        PipelineLibrary::new(PipelineDevice::new(domain.device(), cache), format)
                    }
                },
                // The first format's pipelines are most of what a launch
                // compiles, so they are saved here, off the frame path and
                // after the library is handed over; a lost device's data is
                // not. A storage that panics costs the save, not the library.
                move || {
                    if let Some(store) = store.filter(|_| !domain.is_lost()) {
                        let mut caught = FirstPanic::new();
                        caught.contain(|| store.save());
                        if let Some(payload) = caught.into_payload() {
                            tracing::error!(
                                panic = flui_foundation::panic::payload_text(&*payload)
                                    .unwrap_or("<non-text payload>"),
                                "the storage panicked saving the pipeline cache"
                            );
                            flui_foundation::panic::discard_payload(payload);
                        }
                    }
                },
            );
            #[cfg(test)]
            if self.warmup.is_some() {
                self.builds.pipeline_sets += 1;
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = format;
    }

    /// The library a warm-up built, once the worker hands it over; `None`
    /// when no warm-up is under way or its library was taken.
    #[cfg(not(target_arch = "wasm32"))]
    fn take_warmed(&mut self) -> Option<PipelineLibrary> {
        self.warmup
            .as_mut()
            .and_then(crate::pipeline_warmup::PipelineWarmup::take)
    }

    /// wasm32 warms nothing up: the frame builds the library.
    #[cfg(target_arch = "wasm32")]
    #[expect(
        clippy::unused_self,
        reason = "wasm32 has no thread to warm pipelines on; the frame builds them"
    )]
    fn take_warmed(&mut self) -> Option<PipelineLibrary> {
        None
    }

    /// The shared painter, retargeted to `format`; built first, sized for
    /// `size`, when no complete frame left one, over the pipelines an unwound
    /// frame's painter left.
    ///
    /// Everything that can fail runs while the painter and the library are
    /// still fields, and the painter is taken last: a panic here (a tracing
    /// subscriber, wgpu's error handler) leaves both for the next frame, as
    /// [`Self::keep_pipelines`] does for a panic of the draw.
    fn take_painter(&mut self, format: wgpu::TextureFormat, size: (u32, u32)) -> WgpuPainter {
        if self.painter.is_none() {
            #[cfg(test)]
            {
                self.builds.glyph_atlases += 1;
            }
            if self.pipelines.is_none() {
                // A warm-up's panic resumes here with the warm-up gone, so
                // the next frame builds the library itself.
                let library = if let Some(warmed) = self.take_warmed() {
                    warmed
                } else {
                    #[cfg(test)]
                    {
                        self.builds.pipeline_sets += 1;
                    }
                    PipelineLibrary::new(self.pipeline_device(), format)
                };
                self.pipelines = Some(library);
            }
            self.painter = Some(WgpuPainter::with_pipelines(
                Arc::clone(&self.domain),
                &mut self.pipelines,
                size,
                crate::frame_protocol::FontSource::Ordinary,
            ));
        }
        let painter = self
            .painter
            .as_mut()
            .expect("BUG: the painter was put in place above");
        #[cfg(test)]
        assert!(
            !std::mem::take(&mut self.panic_in_next_setup),
            "injected panic in a frame's painter setup"
        );
        #[cfg(test)]
        let sets_before = painter.pipeline_sets_built();
        painter.retarget(format);
        #[cfg(test)]
        {
            self.builds.pipeline_sets += painter.pipeline_sets_built() - sets_before;
        }
        self.painter
            .take()
            .expect("BUG: the painter was put in place above")
    }

    fn gpu(&self) -> Gpu<'_> {
        gpu_of(&self.adapter, &self.domain)
    }

    fn offscreen_slot(&self, slot: SlotId) -> EngineResult<&OffscreenSlot> {
        match self.slots.get(slot) {
            Some(SurfaceSlot::Offscreen(entry)) => Ok(entry),
            Some(SurfaceSlot::Window(_)) => Err(EngineError::InvalidRenderTarget {
                reason: "the slot is a window slot, which is not read back",
            }),
            None => Err(not_attached()),
        }
    }

    fn window_slot(&mut self, slot: SlotId) -> Option<(&mut WindowSlot, Gpu<'_>)> {
        let Self {
            adapter,
            domain,
            slots,
            ..
        } = self;
        match slots.get_mut(slot) {
            Some(SurfaceSlot::Window(window)) => Some((window, gpu_of(adapter, domain))),
            Some(SurfaceSlot::Offscreen(_)) | None => None,
        }
    }

    /// A slot target of `size` in `format`, bounded by the device's limit.
    fn slot_texture(
        &self,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> EngineResult<wgpu::Texture> {
        offscreen_texture(&self.domain, self.max_texture_dimension(), size, format)
    }
}

fn gpu_of<'a>(adapter: &'a wgpu::Adapter, domain: &'a DeviceDomain) -> Gpu<'a> {
    Gpu {
        adapter,
        device: domain.device(),
        queue: domain.queue(),
    }
}

fn not_attached() -> EngineError {
    EngineError::InvalidRenderTarget {
        reason: "the slot is not attached to this context",
    }
}

/// An offscreen slot's target of `size` in `format`, bounded by the device's
/// limit `max_dim` here, where the size enters: `create_texture` reports an
/// over-limit size through wgpu's uncaptured-error path, and wgpu rejects a
/// zero-sized texture and readback buffer by panicking.
fn offscreen_texture(
    domain: &DeviceDomain,
    max_dim: u32,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> EngineResult<wgpu::Texture> {
    let (width, height) = size;
    if width == 0 || height == 0 || width > max_dim || height > max_dim {
        return Err(EngineError::InvalidTargetSize { width, height });
    }
    Ok(domain.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("FLUI Offscreen Slot Target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }))
}

/// Route the device's faults through `tracing`, and mark the domain lost when
/// the driver reports the device gone, so the next window frame answers
/// [`EngineError::DeviceLost`] instead of failing blind.
///
/// wgpu's default turns an uncaptured error into a panic; the callbacks run on
/// whichever thread wgpu reports from and reach the domain through a weak
/// reference, since the device the domain holds owns them.
fn install_device_diagnostics(
    domain: &Arc<DeviceDomain>,
    loss_wake: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    let device = domain.device();
    device.on_uncaptured_error(Arc::new(|error: wgpu::Error| {
        tracing::error!(
            %error,
            "wgpu uncaptured error (validation / out-of-memory / internal)",
        );
    }));
    let lost = Arc::downgrade(domain);
    device.set_device_lost_callback(move |reason, message| {
        tracing::error!(?reason, %message, "wgpu device lost: the GPU context is gone");
        let Some(domain) = lost.upgrade() else {
            return;
        };
        domain.mark_lost();
        // After the mark, so the owner the wake runs reads the device lost.
        if let Some(wake) = &loss_wake {
            wake();
        }
    });
}

/// The context as the raster executor's backend. An offscreen slot renders in
/// full each frame and counts as presented once submitted; a window slot
/// renders its damage and presents.
impl RasterBackend for GpuContext {
    fn attach(&mut self, slot: SlotId, target: SlotTarget, config: SlotConfig) -> EngineResult<()> {
        match target {
            SlotTarget::Window(surface) => self.attach_window(slot, surface, config),
            SlotTarget::Offscreen { size, format } => {
                self.attach_offscreen(slot, device_extent(size)?, format)
            }
        }
    }

    fn resize(&mut self, slot: SlotId, size: DeviceSize) -> EngineResult<()> {
        self.resize_slot(slot, device_extent(size)?)
    }

    fn release(&mut self, slot: SlotId) {
        if let Some((window, _)) = self.window_slot(slot) {
            window.release();
        }
    }

    fn restore(&mut self, slot: SlotId, surface: UnconfiguredSurface) -> EngineResult<()> {
        let Some((window, gpu)) = self.window_slot(slot) else {
            return Err(EngineError::InvalidRenderTarget {
                reason: "a surface is restored to an attached window slot",
            });
        };
        window.restore(&gpu, surface)
    }

    fn set_display_color(&mut self, slot: SlotId, color: DisplayColor) -> EngineResult<()> {
        match self.window_slot(slot) {
            Some((window, gpu)) => window.set_display_color(&gpu, color),
            None => Ok(()),
        }
    }

    fn set_latency(&mut self, slot: SlotId, latency: FrameLatency) {
        if let Some((window, gpu)) = self.window_slot(slot) {
            window.set_latency(&gpu, latency);
        }
    }

    fn apply_textures(&mut self, slot: SlotId, updates: TextureUpdates) -> EngineResult<()> {
        self.apply_slot_textures(slot, updates)
    }

    fn render(
        &mut self,
        slot: SlotId,
        frame: FrameStamp,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        // A rebuild that unwound part way left slots on the old device.
        if self.rebuilding {
            return Err(EngineError::DeviceLost);
        }
        match self.slots.get(slot) {
            Some(SurfaceSlot::Window(_)) => self.render_window(slot, frame, scene, damage),
            Some(SurfaceSlot::Offscreen(_)) | None => {
                self.render_slot(
                    slot,
                    scene.content(),
                    crate::recording_cache::CacheReads::for_damage(&damage),
                )?;
                Ok(PresentDisposition::Presented)
            }
        }
    }

    fn render_lent(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        reset_fonts: bool,
    ) -> EngineResult<PresentDisposition> {
        if self.rebuilding {
            return Err(EngineError::DeviceLost);
        }
        match self.slots.get(slot) {
            Some(SurfaceSlot::Window(_)) => {
                self.draw_window(slot, scene, WindowDraw::Lent { reset_fonts })
            }
            Some(SurfaceSlot::Offscreen(_)) | None => Err(not_attached()),
        }
    }

    fn detach(&mut self, slot: SlotId) {
        GpuContext::detach(self, slot);
    }

    fn kept_textures(&self, slot: SlotId) -> TextureUpdates {
        self.textures
            .get(&slot)
            .map_or_else(TextureUpdates::default, KeptTextures::newest)
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        if let Some(painter) = self.painter.as_mut() {
            painter.reclaim(level);
        }
    }

    fn device_lost(&self) -> bool {
        self.rebuilding || self.domain.is_lost()
    }

    fn rebuild_device(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>> {
        self.rebuild()
    }

    fn rebuilds_inline(&self) -> bool {
        cfg!(not(target_arch = "wasm32"))
    }
}

/// What a window slot draws: one of its frames, or a scene lent outside
/// them.
#[derive(Clone, Copy)]
enum WindowDraw {
    /// A frame, repainting at least its damage, presented with its stamp.
    Frame {
        damage: DamageRegion,
        frame: FrameStamp,
    },
    /// A lent scene, in full, with the fonts it carries.
    Lent { reset_fonts: bool },
}

/// `size` as a texture extent; a side of zero or less is refused.
fn device_extent(size: DeviceSize) -> EngineResult<(u32, u32)> {
    match (u32::try_from(size.width), u32::try_from(size.height)) {
        (Ok(width), Ok(height)) if width > 0 && height > 0 => Ok((width, height)),
        _ => Err(EngineError::InvalidTargetSize {
            width: u32::try_from(size.width).unwrap_or(0),
            height: u32::try_from(size.height).unwrap_or(0),
        }),
    }
}

#[cfg(test)]
mod tests {
    use flui_foundation::TextDirection;
    use flui_foundation::{
        FrameEpoch, FrameStamp, PresentationAddress, PresentationId, SurfaceGeneration,
        UiRuntimeId,
        geometry::{DeviceSize, Offset, Rect},
    };
    use flui_layer::{DamageRegion, LayerTree, Scene, SceneBuilder, SceneSnapshot};
    use flui_painting::{
        Canvas, TextPainter,
        paint::Paint,
        styling::Color,
        typography::{TextSpan, TextStyle},
    };

    use super::{GpuContext, INSTANCES_CREATED};
    use crate::error::EngineError;
    use crate::raster_executor::{
        RasterCommand, RasterExecutor, RasterVerdict, SendReceipt, SlotConfig, SlotId, SlotTarget,
    };

    /// A window an interrupted attempt released answers `Ok` to the next
    /// attempt: its refusal survives to the rebuild that finishes, while a
    /// slot the next attempt serves, or one detached meanwhile, drops its own.
    #[test]
    fn a_refusal_outlives_the_attempt_that_found_it() {
        let [released, retried, detached] = [0, 1, 2]
            .map(|index| SlotId::new_gen(index, flui_foundation::generation::Generation::FIRST));
        let mut refusals = super::RebuildRefusals::default();
        // The first attempt unwinds after these.
        refusals.refuse(released, EngineError::IncompatibleSurface);
        refusals.refuse(retried, EngineError::GpuUnavailable);
        refusals.refuse(detached, EngineError::IncompatibleSurface);
        // The next finishes: the released window is still without a surface.
        refusals.served(released, false);
        refusals.served(retried, true);
        let refused = refusals.take(|slot| slot != detached);
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].0, released);
        assert!(matches!(refused[0].1, EngineError::IncompatibleSurface));
        assert!(refusals.take(|_| true).is_empty(), "taken once");
    }

    /// A red square beside a black glyph, so a frame samples both the shape
    /// pipelines and the glyph atlas.
    fn square_and_glyph() -> LayerTree {
        let mut painter = TextPainter::new()
            .with_text(
                TextSpan::new("I").with_style(
                    TextStyle::new()
                        .with_font_size(14.0)
                        .with_color(Color::rgb(0, 0, 0)),
                ),
            )
            .with_text_direction(TextDirection::Ltr);
        painter
            .layout(
                &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
                0.0,
                64.0,
            )
            .expect("the fixture's layout inputs are valid");
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            &Paint::fill(Color::rgb(255, 0, 0)).with_anti_alias(false),
        );
        painter.paint(&mut canvas, Offset::new(10.0, 0.0));
        let mut builder = SceneBuilder::new();
        builder.add_picture(canvas.finish());
        builder.build()
    }

    fn context_or_skip() -> Option<GpuContext> {
        match pollster::block_on(GpuContext::offscreen(wgpu::Features::DUAL_SOURCE_BLENDING)) {
            Ok(context) => Some(context),
            Err(error) => {
                crate::test_support::resolve_unavailable_gpu(
                    &error.to_string(),
                    crate::test_support::require_gpu(),
                );
                None
            }
        }
    }

    /// The stamp of a frame a test draws straight into a window slot.
    #[cfg(feature = "testing")]
    fn drawn_frame() -> FrameStamp {
        FrameStamp::new(
            PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(1),
            },
            FrameEpoch::ZERO.next(),
            SurfaceGeneration::ZERO.next(),
        )
    }

    /// A 32×32 version of one chunk, a red square.
    #[cfg(feature = "testing")]
    fn one_chunk_scene() -> Scene {
        use std::sync::Arc;

        use flui_foundation::RenderId;
        use flui_layer::{BoundaryContent, Chunk, SceneOp, SceneOwner};

        let mut owner = SceneOwner::new();
        {
            let mut writer = owner.writer();
            let mut canvas = Canvas::new();
            canvas.draw_rect(
                Rect::from_xywh(4.0, 4.0, 8.0, 8.0),
                &Paint::fill(Color::RED),
            );
            let mut content = BoundaryContent::builder();
            content.chunk(Arc::new(Chunk::new(
                RenderId::new(2),
                0,
                writer.next_content_version().expect("a content version"),
                canvas.finish(),
            )));
            writer.push(SceneOp::Publish {
                node: writer.current().root(),
                content: Arc::new(content.finish()),
            });
        }
        owner
            .apply(flui_layer::SurfaceKey {
                size: (32, 32),
                generation: SurfaceGeneration::ZERO,
            })
            .expect("the scene applies");
        Scene::of_version(owner.current().clone())
    }

    /// An offscreen slot reads the recording cache as a window does: a
    /// frame of damage `Full(DamageOff)` records every chunk, and the next
    /// one replays them again.
    #[cfg(feature = "testing")]
    #[test]
    fn an_offscreen_frame_with_damage_off_records_every_chunk() {
        use flui_layer::FullReason;

        use crate::raster_executor::{RasterBackend, SlotMint};

        let Some(mut context) = context_or_skip() else {
            return;
        };
        let slot = SlotMint::default().mint().expect("a slot id");
        context
            .attach_offscreen(slot, (32, 32), wgpu::TextureFormat::Rgba8Unorm)
            .expect("the slot attaches");
        let scene = one_chunk_scene();
        let mut draw = |reason: FullReason| {
            context
                .render(slot, drawn_frame(), &scene, DamageRegion::Full(reason))
                .expect("the slot draws");
            let counters = context.slot_counters();
            (counters.chunks_recorded, counters.recording_cache_hits)
        };
        assert_eq!(
            draw(FullReason::FirstFrame),
            (1, 0),
            "the first frame records"
        );
        assert_eq!(draw(FullReason::FirstFrame), (0, 1), "the next replays");
        assert_eq!(draw(FullReason::DamageOff), (1, 0), "damage off records");
        assert_eq!(draw(FullReason::FirstFrame), (0, 1), "and the next replays");
    }

    /// No kept recording reads the target format, so a slot of another
    /// format replays what one of the first format recorded, and draws what
    /// a fresh context draws.
    #[cfg(feature = "testing")]
    #[test]
    fn a_slot_of_another_format_replays_the_recordings_of_the_first() {
        use crate::raster_executor::{RasterBackend, SlotMint};

        let Some(mut context) = context_or_skip() else {
            return;
        };
        let Some(mut fresh) = context_or_skip() else {
            return;
        };
        let scene = one_chunk_scene();
        let mut mint = SlotMint::default();
        let rgba = mint.mint().expect("a slot id");
        let bgra = mint.mint().expect("a slot id");
        let full = || DamageRegion::Full(flui_layer::FullReason::FirstFrame);
        let attach = |context: &mut GpuContext, slot, format| {
            context
                .attach_offscreen(slot, (32, 32), format)
                .expect("the slot attaches");
        };
        attach(&mut context, rgba, wgpu::TextureFormat::Rgba8Unorm);
        attach(&mut context, bgra, wgpu::TextureFormat::Bgra8Unorm);
        attach(&mut fresh, bgra, wgpu::TextureFormat::Bgra8Unorm);
        context
            .render(rgba, drawn_frame(), &scene, full())
            .expect("the first slot draws");
        context
            .render(bgra, drawn_frame(), &scene, full())
            .expect("the second slot draws");
        let counters = context.slot_counters();
        assert_eq!(
            (counters.chunks_recorded, counters.recording_cache_hits),
            (0, 1),
            "the second format replays the first one's recording"
        );
        fresh
            .render(bgra, drawn_frame(), &scene, full())
            .expect("the fresh slot draws");
        assert_eq!(
            context.read_slot(bgra).expect("readback"),
            fresh.read_slot(bgra).expect("readback"),
            "the replay draws what a fresh context draws"
        );
    }

    /// A frame of `tree` for the slot at `address`, stamped with `generation`.
    fn frame(
        slot: SlotId,
        address: PresentationAddress,
        generation: SurfaceGeneration,
        tree: LayerTree,
    ) -> RasterCommand {
        RasterCommand::Frame {
            slot,
            snapshot: SceneSnapshot::new(
                FrameStamp::new(address, FrameEpoch::ZERO, generation),
                DamageRegion::Full(flui_layer::FullReason::FirstFrame),
                Scene::new(tree),
            ),
        }
    }

    /// Gate G5: slots of two formats and two sizes, driven through the raster
    /// executor, share one instance, one device, one glyph atlas and one
    /// pipeline set per format, and drawing every slot again builds nothing.
    /// Before the context, each headless capture and each window built all of
    /// them for itself.
    ///
    /// Red-check: build a painter per slot instead of sharing the context's
    /// (or rebuild it when the format changes) and the counts grow.
    #[test]
    fn every_slot_shares_one_device_one_glyph_atlas_and_one_pipeline_per_format() {
        let instances = || INSTANCES_CREATED.with(std::cell::Cell::get);
        let before = instances();
        let Some(context) = context_or_skip() else {
            return;
        };
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let targets = [
            ((32, 16), wgpu::TextureFormat::Rgba8Unorm),
            ((48, 16), wgpu::TextureFormat::Rgba8Unorm),
            ((32, 16), wgpu::TextureFormat::Bgra8Unorm),
        ];
        let slots = targets.map(|((width, height), format)| {
            let slot = sender.mint_slot().expect("a slot id is left");
            let address = PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(slot.index() as usize + 1),
            };
            let receipt = sender
                .send(RasterCommand::Attach {
                    slot,
                    address,
                    target: SlotTarget::Offscreen {
                        size: DeviceSize::new(width, height),
                        format,
                    },
                    config: SlotConfig::new(),
                })
                .expect("the attach is accepted");
            let SendReceipt::Configured(generation) = receipt else {
                panic!("an attach of a positive size configures the slot: {receipt:?}");
            };
            (slot, address, generation)
        });
        let draw_all = |executor: &mut RasterExecutor<GpuContext>| {
            for (slot, address, generation) in slots {
                sender
                    .send(frame(slot, address, generation, square_and_glyph()))
                    .expect("the frame is accepted");
            }
            let _ = executor.turn();
            let mut settled = Vec::new();
            for (slot, ..) in slots {
                sender.drain_settlements(slot, &mut settled);
            }
            assert!(
                settled
                    .iter()
                    .all(|settlement| settlement.verdict == RasterVerdict::Presented),
                "every slot presents: {settled:?}"
            );
            assert_eq!(settled.len(), slots.len());
        };
        draw_all(&mut executor);
        let first = executor.backend().builds();
        assert_eq!(instances() - before, 1, "one instance serves every slot");
        assert_eq!(first.devices, 1, "one device serves every slot");
        assert_eq!(first.glyph_atlases, 1, "one glyph atlas serves every slot");
        assert_eq!(
            first.pipeline_sets, 2,
            "one pipeline set per target format, two formats"
        );

        draw_all(&mut executor);
        draw_all(&mut executor);
        assert_eq!(
            executor.backend().builds(),
            first,
            "drawing every slot again builds no device, atlas or pipeline"
        );

        // The shared painter draws the same pixels into either format, and
        // a slot's size is its own.
        let [(rgba, ..), (wider, ..), (bgra, ..)] = slots;
        let context = executor.backend();
        let rgba_pixels = context.read_slot(rgba).expect("the RGBA slot reads back");
        let bgra_pixels = context.read_slot(bgra).expect("the BGRA slot reads back");
        let swizzled: Vec<u8> = bgra_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|[b, g, r, a]| [*r, *g, *b, *a])
            .collect();
        assert_eq!(swizzled, rgba_pixels, "both formats draw the same frame");
        assert_eq!(&rgba_pixels[..4], &[255, 0, 0, 255], "the square is red");
        assert!(
            rgba_pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|[r, g, b, _]| r == g && g == b && *r < 128),
            "the glyph inks"
        );
        assert_eq!(
            context
                .read_slot(wider)
                .expect("the wider slot reads back")
                .len(),
            48 * 16 * 4
        );
        assert!(matches!(
            context.read_slot(sender.mint_slot().expect("a slot id is left")),
            Err(EngineError::InvalidRenderTarget { .. })
        ));
    }

    /// A frame that panics costs the caches every slot shares, not the
    /// pipelines: two slots whose frames both panic in one turn, after their
    /// recording and before their submission, each settle `Retry` and each
    /// keep their own payload, and the next turn draws every slot over a
    /// fresh glyph atlas with no pipeline built again (ADR-0305 §7).
    ///
    /// An offscreen slot is cleared before it records, so a frame that panics
    /// leaves its target cleared, not as it was: the slot's committed image
    /// does not survive, and its `Retry` is what makes the owner draw it again.
    ///
    /// Red-check: drop the pipelines with the painter in
    /// `GpuContext::keep_pipelines` and the next painter builds its pipeline
    /// set again.
    #[test]
    fn a_panic_in_one_slot_rebuilds_the_shared_caches_and_keeps_the_pipelines() {
        use crate::raster_executor::SlotFailure;

        let Some(context) = context_or_skip() else {
            return;
        };
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slots = [(); 2].map(|()| {
            let slot = sender.mint_slot().expect("a slot id is left");
            let address = PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(slot.index() as usize + 1),
            };
            let receipt = sender
                .send(RasterCommand::Attach {
                    slot,
                    address,
                    target: SlotTarget::Offscreen {
                        size: DeviceSize::new(32, 16),
                        format: wgpu::TextureFormat::Rgba8Unorm,
                    },
                    config: SlotConfig::new(),
                })
                .expect("the attach is accepted");
            let SendReceipt::Configured(generation) = receipt else {
                panic!("an attach of a positive size configures the slot: {receipt:?}");
            };
            (slot, address, generation)
        });
        let draw = |executor: &mut RasterExecutor<GpuContext>, tree: fn() -> LayerTree| {
            for (slot, address, generation) in slots {
                sender
                    .send(frame(slot, address, generation, tree()))
                    .expect("the frame is accepted");
            }
            let _ = executor.turn();
            slots.map(|(slot, ..)| {
                let mut settled = Vec::new();
                sender.drain_settlements(slot, &mut settled);
                assert_eq!(settled.len(), 1, "each frame settles once");
                settled[0].verdict
            })
        };
        let [(first_slot, ..), (second_slot, ..)] = slots;

        assert_eq!(
            draw(&mut executor, square_and_glyph),
            [RasterVerdict::Presented; 2]
        );
        let built = executor.backend().builds();
        let committed = executor
            .backend()
            .read_slot(second_slot)
            .expect("the slot reads back");

        executor.backend_mut().panic_in_next_frames(2);
        assert_eq!(
            draw(&mut executor, square_and_glyph),
            [RasterVerdict::Retry; 2],
            "both panicking frames settle Retry"
        );
        for slot in [first_slot, second_slot] {
            assert!(
                matches!(sender.take_failure(slot), Some(SlotFailure::Panicked(_))),
                "each slot's record holds its own panic"
            );
        }
        assert_ne!(
            executor
                .backend()
                .read_slot(second_slot)
                .expect("the slot reads back"),
            committed,
            "the panicking frame cleared its target before it recorded"
        );

        assert_eq!(
            draw(&mut executor, square_and_glyph),
            [RasterVerdict::Presented; 2],
            "the turn after the panics draws every slot"
        );
        let after = executor.backend().builds();
        assert_eq!(
            after.pipeline_sets, built.pipeline_sets,
            "no pipeline is built again"
        );
        assert!(
            after.glyph_atlases > built.glyph_atlases,
            "the shared caches are rebuilt"
        );
        for slot in [first_slot, second_slot] {
            assert_eq!(
                executor
                    .backend()
                    .read_slot(slot)
                    .expect("the slot reads back"),
                committed,
                "the rebuilt caches draw the same frame"
            );
        }
    }

    /// A lost device is rebuilt on the context's own instance: no instance is
    /// made, every slot keeps its target at its size, a frame stamped before
    /// the rebuild is refused on its generation, and the next frame draws
    /// every slot in full on the new device, with its pipelines and glyph
    /// atlas built again (ADR-0305 §8). The first rebuild fails, so the
    /// second runs only once the backoff on the executor's clock allows it.
    ///
    /// Red-check: keep the old painter across the rebuild and the frames on
    /// the new device still settle `DeviceLost` through the old device's
    /// painter; skip the restamp in the executor's rebuild and the frames
    /// stamped before it render.
    #[test]
    fn a_device_rebuild_keeps_the_surfaces_and_redraws_every_slot_in_full() {
        use flui_foundation::{ManualClock, MonotonicClock};

        use crate::raster_executor::DeviceHealth;

        let instances = || INSTANCES_CREATED.with(std::cell::Cell::get);
        let before = instances();
        let Some(context) = context_or_skip() else {
            return;
        };
        let clock = ManualClock::new();
        let (mut executor, sender) =
            RasterExecutor::new(context, std::rc::Rc::new(clock.clone()), || {});
        let slots = [(32, 16), (48, 16)].map(|(width, height)| {
            let slot = sender.mint_slot().expect("a slot id is left");
            let address = PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(slot.index() as usize + 1),
            };
            let _receipt = sender
                .send(RasterCommand::Attach {
                    slot,
                    address,
                    target: SlotTarget::Offscreen {
                        size: DeviceSize::new(width, height),
                        format: wgpu::TextureFormat::Rgba8Unorm,
                    },
                    config: SlotConfig::new(),
                })
                .expect("the attach is accepted");
            (slot, address)
        });
        let required = |slot: SlotId| {
            sender
                .record(slot)
                .expect("the owner holds the slot")
                .required_generation
        };
        let draw = |executor: &mut RasterExecutor<GpuContext>, stamped: [SurfaceGeneration; 2]| {
            for ((slot, address), generation) in slots.into_iter().zip(stamped) {
                sender
                    .send(frame(slot, address, generation, square_and_glyph()))
                    .expect("the frame is accepted");
            }
            let _ = executor.turn();
            slots.map(|(slot, _)| {
                let mut settled = Vec::new();
                sender.drain_settlements(slot, &mut settled);
                assert_eq!(settled.len(), 1, "each frame settles once");
                settled[0].verdict
            })
        };
        let current = || slots.map(|(slot, _)| required(slot));

        assert_eq!(
            draw(&mut executor, current()),
            [RasterVerdict::Presented; 2]
        );
        let built = executor.backend().builds();
        let committed = slots.map(|(slot, _)| {
            executor
                .backend()
                .read_slot(slot)
                .expect("the slot reads back")
        });

        executor.backend().lose_device();
        executor.backend_mut().fail_next_rebuilds(1);
        let stale = current();
        assert_eq!(draw(&mut executor, stale), [RasterVerdict::DeviceLost; 2]);
        assert_eq!(
            sender.device_health(),
            DeviceHealth::Rebuilding { attempts: 1 }
        );
        assert!(matches!(
            sender.take_device_failure(),
            Some(EngineError::GpuUnavailable)
        ));
        let retry_at = sender.device_retry_at().expect("the next rebuild is paced");
        clock.advance(retry_at.duration_since(MonotonicClock::now(&clock)));

        assert_eq!(
            draw(&mut executor, stale),
            [RasterVerdict::SurfaceStale; 2],
            "the frames stamped before the rebuild are refused"
        );
        assert_eq!(sender.device_health(), DeviceHealth::Healthy);
        let rebuilt = executor.backend().builds();
        assert_eq!(rebuilt.devices, 2, "one device more");
        assert_eq!(instances() - before, 1, "the instance is the context's");

        assert_eq!(
            draw(&mut executor, current()),
            [RasterVerdict::Presented; 2]
        );
        let drawn = executor.backend().builds();
        assert_eq!(
            drawn.pipeline_sets,
            built.pipeline_sets + 1,
            "the new device's pipelines are built once"
        );
        assert_eq!(drawn.glyph_atlases, built.glyph_atlases + 1);
        for ((slot, _), committed) in slots.into_iter().zip(committed) {
            assert_eq!(
                executor
                    .backend()
                    .read_slot(slot)
                    .expect("the slot reads back"),
                committed,
                "the slot keeps its size and draws the same frame"
            );
        }
    }

    /// Two presentations each mint texture id 1 and draw it through slots
    /// that share the context's painter: each slot draws its own image, after
    /// a device rebuild too, and a detach drops a slot's textures.
    ///
    /// Red-check: skip lending the slot's textures to the shared painter in
    /// `lend_textures` and no frame can draw its texture; skip the
    /// re-upload in `KeptTextures::prepare` and the frames after the rebuild fail.
    #[test]
    fn each_slot_draws_its_own_textures_under_the_same_id_across_a_rebuild() {
        use flui_foundation::ManualClock;
        use flui_layer::{Layer, TextureLayer, TextureRegistry};
        use flui_painting::paint::Image;

        const SIDE: u32 = 8;
        let Some(context) = context_or_skip() else {
            return;
        };
        let clock = ManualClock::new();
        let (mut executor, sender) = RasterExecutor::new(context, std::rc::Rc::new(clock), || {});
        let colors = [[255, 0, 0, 255], [0, 0, 255, 255]];
        let presentations = colors.map(|rgba| {
            let slot = sender.mint_slot().expect("a slot id is left");
            let address = PresentationAddress {
                ui_runtime_id: UiRuntimeId::new(1),
                presentation_id: PresentationId::new(slot.index() as usize + 1),
            };
            let _receipt = sender
                .send(RasterCommand::Attach {
                    slot,
                    address,
                    target: SlotTarget::Offscreen {
                        size: DeviceSize::new(8, 8),
                        format: wgpu::TextureFormat::Rgba8Unorm,
                    },
                    config: SlotConfig::new(),
                })
                .expect("the attach is accepted");
            let registry = TextureRegistry::new(std::sync::Arc::new(|| {}));
            let texture = registry.register().expect("an id is left");
            texture.submit_frame(Image::from_rgba8(2, 2, rgba.repeat(4)));
            (slot, address, registry, texture)
        });
        assert_eq!(
            presentations[0].3.id(),
            presentations[1].3.id(),
            "each presentation mints its own ids"
        );
        let draw = |executor: &mut RasterExecutor<GpuContext>| {
            for (slot, address, registry, texture) in &presentations {
                let generation = sender
                    .record(*slot)
                    .expect("the owner holds the slot")
                    .required_generation;
                let tree = LayerTree::new(Layer::from(TextureLayer::new(
                    texture.id(),
                    Rect::from_xywh(0.0, 0.0, f64::from(SIDE), f64::from(SIDE)),
                )));
                let RasterCommand::Frame { slot, snapshot } =
                    frame(*slot, *address, generation, tree)
                else {
                    unreachable!("BUG: `frame` makes a frame command");
                };
                sender
                    .send(RasterCommand::Frame {
                        slot,
                        snapshot: snapshot.textures(registry.take_updates()),
                    })
                    .expect("the frame is accepted");
            }
            let _ = executor.turn();
            presentations.each_ref().map(|(slot, ..)| {
                let mut settled = Vec::new();
                sender.drain_settlements(*slot, &mut settled);
                assert_eq!(settled.len(), 1, "each frame settles once");
                let pixels = executor
                    .backend()
                    .read_slot(*slot)
                    .expect("the slot reads back");
                (
                    settled[0].verdict,
                    [pixels[0], pixels[1], pixels[2], pixels[3]],
                )
            })
        };

        let expected = colors.map(|rgba| (RasterVerdict::Presented, rgba));
        assert_eq!(
            draw(&mut executor),
            expected,
            "each slot draws its own image"
        );

        executor.backend().lose_device();
        let _ = draw(&mut executor);
        // The rebuild ran in that turn; frames stamped before it are stale.
        let _ = draw(&mut executor);
        assert_eq!(
            executor.backend().builds().devices,
            2,
            "the device was rebuilt"
        );
        assert_eq!(
            draw(&mut executor),
            expected,
            "the new device has each slot's image back"
        );
        // What an owner that replaces the context gives each slot's next one.
        for (slot, _, _, texture) in &presentations {
            let kept = crate::RasterBackend::kept_textures(executor.backend(), *slot);
            let ids: Vec<flui_painting::paint::TextureId> =
                kept.frames.iter().map(|(id, _)| *id).collect();
            assert_eq!(ids, vec![texture.id()], "the slot keeps its drawn image");
        }

        let detached = presentations[0].0;
        let _receipt = sender
            .send(RasterCommand::Detach { slot: detached })
            .expect("the detach is accepted");
        let _ = executor.turn();
        assert!(
            !executor.backend().textures.contains_key(&detached),
            "a detach drops the slot's textures"
        );
    }

    /// A frame that unwinds while it holds a slot's lent textures, outside
    /// the contained draw (building the window blit, say), drops them with
    /// the painter: the slot's next frame uploads its kept images again
    /// instead of drawing from the painter's empty registry.
    ///
    /// Red-check: lend by swapping the slot's registry with the painter's,
    /// leaving the painter's empty one in the slot, and the next frame
    /// cannot find the texture.
    #[test]
    fn a_frame_unwound_with_lent_textures_leaves_the_slot_its_images() {
        use flui_foundation::ManualClock;
        use flui_layer::{Layer, TextureLayer, TextureRegistry};
        use flui_painting::paint::Image;

        const RED: [u8; 4] = [255, 0, 0, 255];
        let Some(context) = context_or_skip() else {
            return;
        };
        let clock = ManualClock::new();
        let (mut executor, sender) = RasterExecutor::new(context, std::rc::Rc::new(clock), || {});
        let slot = sender.mint_slot().expect("a slot id is left");
        let address = PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(1),
        };
        let _receipt = sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target: SlotTarget::Offscreen {
                    size: DeviceSize::new(8, 8),
                    format: wgpu::TextureFormat::Rgba8Unorm,
                },
                config: SlotConfig::new(),
            })
            .expect("the attach is accepted");
        let registry = TextureRegistry::new(std::sync::Arc::new(|| {}));
        let texture = registry.register().expect("an id is left");
        texture.submit_frame(Image::from_rgba8(2, 2, RED.repeat(4)));
        let draw = |executor: &mut RasterExecutor<GpuContext>| {
            let generation = sender
                .record(slot)
                .expect("the owner holds the slot")
                .required_generation;
            let tree = LayerTree::new(Layer::from(TextureLayer::new(
                texture.id(),
                Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            )));
            let RasterCommand::Frame { slot, snapshot } = frame(slot, address, generation, tree)
            else {
                unreachable!("BUG: `frame` makes a frame command");
            };
            sender
                .send(RasterCommand::Frame {
                    slot,
                    snapshot: snapshot.textures(registry.take_updates()),
                })
                .expect("the frame is accepted");
            let _ = executor.turn();
            let mut settled = Vec::new();
            sender.drain_settlements(slot, &mut settled);
            let pixels = executor
                .backend()
                .read_slot(slot)
                .expect("the slot reads back");
            (
                settled[0].verdict,
                [pixels[0], pixels[1], pixels[2], pixels[3]],
            )
        };
        assert_eq!(draw(&mut executor), (RasterVerdict::Presented, RED));

        // What an unwind between the lend and the take-back leaves.
        let context = executor.backend_mut();
        context.prepare_textures(slot);
        let mut painter = context.take_painter(wgpu::TextureFormat::Rgba8Unorm, (8, 8));
        let own = context.lend_textures(slot, &mut painter);
        assert!(own.is_some(), "the slot has textures to lend");
        drop(painter);
        drop(own);

        assert_eq!(
            draw(&mut executor),
            (RasterVerdict::Presented, RED),
            "the slot draws its kept image after the unwound frame"
        );
    }

    /// A hidden slot whose producer keeps sending frames writes none of them
    /// to the queue, which holds staging memory until a submission and gets
    /// none from a hidden slot: it keeps the newest image of each texture,
    /// and shown again, it uploads and draws that one.
    ///
    /// Red-check: upload each frame as it is applied and the hidden slot's
    /// frames reach the queue: nothing is left to upload while it is hidden.
    #[test]
    fn a_hidden_slot_keeps_its_newest_images_off_the_queue() {
        use crate::raster_executor::{SlotChange, SlotVisibility};
        use flui_foundation::ManualClock;
        use flui_layer::{Layer, TextureLayer, TextureRegistry};
        use flui_painting::paint::Image;

        let Some(context) = context_or_skip() else {
            return;
        };
        let clock = ManualClock::new();
        let (mut executor, sender) = RasterExecutor::new(context, std::rc::Rc::new(clock), || {});
        let slot = sender.mint_slot().expect("a slot id is left");
        let address = PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(1),
        };
        let _receipt = sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target: SlotTarget::Offscreen {
                    size: DeviceSize::new(8, 8),
                    format: wgpu::TextureFormat::Rgba8Unorm,
                },
                config: SlotConfig::new(),
            })
            .expect("the attach is accepted");
        let registry = TextureRegistry::new(std::sync::Arc::new(|| {}));
        let texture = registry.register().expect("an id is left");
        let draw = |executor: &mut RasterExecutor<GpuContext>, shade: u8| {
            texture.submit_frame(Image::from_rgba8(2, 2, [shade, 0, 0, 255].repeat(4)));
            let generation = sender
                .record(slot)
                .expect("the owner holds the slot")
                .required_generation;
            let tree = LayerTree::new(Layer::from(TextureLayer::new(
                texture.id(),
                Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            )));
            let RasterCommand::Frame { slot, snapshot } = frame(slot, address, generation, tree)
            else {
                unreachable!("BUG: `frame` makes a frame command");
            };
            sender
                .send(RasterCommand::Frame {
                    slot,
                    snapshot: snapshot.textures(registry.take_updates()),
                })
                .expect("the frame is accepted");
            let _ = executor.turn();
            let mut settled = Vec::new();
            sender.drain_settlements(slot, &mut settled);
            settled[0].verdict
        };
        let show = |executor: &mut RasterExecutor<GpuContext>, visibility| {
            let _receipt = sender
                .send(RasterCommand::Configure {
                    slot,
                    change: SlotChange::Shown(visibility),
                })
                .expect("the change is accepted");
            let _ = executor.turn();
        };

        assert_eq!(draw(&mut executor, 10), RasterVerdict::Presented);
        show(&mut executor, SlotVisibility::Hidden);
        for shade in 20..=23 {
            assert_eq!(draw(&mut executor, shade), RasterVerdict::NotShown);
            assert_eq!(
                executor.backend().textures[&slot].waiting(),
                1,
                "the hidden slot keeps one image per texture off the queue"
            );
        }
        show(&mut executor, SlotVisibility::Shown);
        assert_eq!(draw(&mut executor, 30), RasterVerdict::Presented);
        assert_eq!(executor.backend().textures[&slot].waiting(), 0);
        let pixels = executor
            .backend()
            .read_slot(slot)
            .expect("the slot reads back");
        assert_eq!(
            [pixels[0], pixels[1], pixels[2], pixels[3]],
            [30, 0, 0, 255],
            "shown again, the slot draws its newest image"
        );
    }

    /// A frame that uploads a texture image and then ends without its own
    /// submission (here it panics; on a window, no damage and a surface not
    /// acquired end the same way) flushes the writes, since `write_texture`
    /// holds staging memory until a submission; a frame that submits flushes
    /// nothing more, and the next frame draws the newest image. The
    /// offscreen frame cleared its target, a submission of its own, before
    /// the injected panic, so the count pins the flush on the unwound exit,
    /// not memory a missing flush would have held; the window exits run the
    /// same call on wgpu's no-op backend only.
    ///
    /// Red-check: drop the flush from the unwound exit of `render_slot` and
    /// no flush is counted.
    #[test]
    fn a_frame_that_submits_nothing_flushes_its_texture_writes() {
        use flui_foundation::ManualClock;
        use flui_layer::{Layer, TextureLayer, TextureRegistry};
        use flui_painting::paint::Image;

        let Some(context) = context_or_skip() else {
            return;
        };
        let clock = ManualClock::new();
        let (mut executor, sender) = RasterExecutor::new(context, std::rc::Rc::new(clock), || {});
        let slot = sender.mint_slot().expect("a slot id is left");
        let address = PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(1),
        };
        let _receipt = sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target: SlotTarget::Offscreen {
                    size: DeviceSize::new(8, 8),
                    format: wgpu::TextureFormat::Rgba8Unorm,
                },
                config: SlotConfig::new(),
            })
            .expect("the attach is accepted");
        let registry = TextureRegistry::new(std::sync::Arc::new(|| {}));
        let texture = registry.register().expect("an id is left");
        let draw = |executor: &mut RasterExecutor<GpuContext>, shade: Option<u8>| {
            if let Some(shade) = shade {
                texture.submit_frame(Image::from_rgba8(2, 2, [shade, 0, 0, 255].repeat(4)));
            }
            let generation = sender
                .record(slot)
                .expect("the owner holds the slot")
                .required_generation;
            let tree = LayerTree::new(Layer::from(TextureLayer::new(
                texture.id(),
                Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
            )));
            let RasterCommand::Frame { slot, snapshot } = frame(slot, address, generation, tree)
            else {
                unreachable!("BUG: `frame` makes a frame command");
            };
            sender
                .send(RasterCommand::Frame {
                    slot,
                    snapshot: snapshot.textures(registry.take_updates()),
                })
                .expect("the frame is accepted");
            let _ = executor.turn();
            let mut settled = Vec::new();
            sender.drain_settlements(slot, &mut settled);
            settled[0].verdict
        };
        let flushes =
            |executor: &RasterExecutor<GpuContext>| executor.backend().texture_flushes.get();

        assert_eq!(draw(&mut executor, Some(10)), RasterVerdict::Presented);
        assert_eq!(
            flushes(&executor),
            0,
            "a submitted frame carries its writes"
        );

        executor.backend_mut().panic_in_next_frame();
        let _ = draw(&mut executor, Some(20));
        let _ = sender.take_failure(slot);
        assert_eq!(
            flushes(&executor),
            1,
            "the unwound frame flushed its writes"
        );

        assert_eq!(draw(&mut executor, None), RasterVerdict::Presented);
        let pixels = executor
            .backend()
            .read_slot(slot)
            .expect("the slot reads back");
        assert_eq!(pixels[0], 20, "the next frame draws the newest image");
        assert_eq!(flushes(&executor), 1);

        // A panic after the uploads but before the draw (retargeting the
        // painter) flushes too, and keeps the painter and its pipelines.
        let builds = executor.backend().builds();
        executor.backend_mut().panic_in_next_setup();
        let _ = draw(&mut executor, Some(30));
        let _ = sender.take_failure(slot);
        assert_eq!(
            flushes(&executor),
            2,
            "a frame unwound in its painter setup flushed its writes"
        );
        assert_eq!(draw(&mut executor, None), RasterVerdict::Presented);
        let pixels = executor
            .backend()
            .read_slot(slot)
            .expect("the slot reads back");
        assert_eq!(
            pixels[0], 30,
            "the next frame draws the image the unwound one uploaded"
        );
        let after = executor.backend().builds();
        assert_eq!(
            (after.glyph_atlases, after.pipeline_sets),
            (builds.glyph_atlases, builds.pipeline_sets),
            "the unwound setup left the painter and its pipelines for the next frame"
        );
    }

    /// Dropping the painter a frame unwound with runs destructors that are not
    /// ours (a cached value's), so it runs contained: when one panics too, the
    /// slot's record still holds the frame's panic, not the cleanup's, and the
    /// next turn draws the slot (ADR-0305 §7, ADR-0281).
    ///
    /// Red-check: drop the torn painter outside `caught` in
    /// `GpuContext::keep_pipelines` and the cleanup's panic unwinds out of
    /// the frame, discarding the frame's: the record holds the cleanup's.
    #[test]
    fn a_torn_painter_whose_drop_panics_keeps_the_frames_panic() {
        use crate::raster_executor::SlotFailure;

        let Some(context) = context_or_skip() else {
            return;
        };
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = sender.mint_slot().expect("a slot id is left");
        let address = PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(1),
        };
        let receipt = sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target: SlotTarget::Offscreen {
                    size: DeviceSize::new(32, 16),
                    format: wgpu::TextureFormat::Rgba8Unorm,
                },
                config: SlotConfig::new(),
            })
            .expect("the attach is accepted");
        let SendReceipt::Configured(generation) = receipt else {
            panic!("an attach of a positive size configures the slot: {receipt:?}");
        };
        let draw = |executor: &mut RasterExecutor<GpuContext>| {
            sender
                .send(frame(slot, address, generation, square_and_glyph()))
                .expect("the frame is accepted");
            let _ = executor.turn();
            let mut settled = Vec::new();
            sender.drain_settlements(slot, &mut settled);
            assert_eq!(settled.len(), 1, "the frame settles once");
            settled[0].verdict
        };
        assert_eq!(draw(&mut executor), RasterVerdict::Presented);

        executor.backend_mut().panic_in_next_frame();
        executor.backend_mut().panic_in_next_cleanup();
        assert_eq!(draw(&mut executor), RasterVerdict::Retry);
        let Some(SlotFailure::Panicked(payload)) = sender.take_failure(slot) else {
            panic!("the slot's record holds a panic");
        };
        assert_eq!(
            payload.downcast_ref::<&str>().copied(),
            Some("injected panic in the middle of a slot's frame"),
            "the record holds the frame's panic, not the cleanup's"
        );
        flui_foundation::panic::discard_payload(payload);

        assert_eq!(
            draw(&mut executor),
            RasterVerdict::Presented,
            "the turn after the panic draws the slot"
        );
    }

    /// A window whose surface the context's adapter cannot present to is
    /// refused at attach with `IncompatibleSurface`, before any configure: the
    /// no-op adapter reports no surface capabilities, as an adapter of another
    /// GPU than the window's would.
    ///
    /// Red-check: let `WindowSlot::attach` skip the support check and the
    /// attach answers `Ok`.
    #[cfg(feature = "testing")]
    #[test]
    fn a_window_its_adapter_cannot_present_to_is_refused_at_attach() {
        use std::sync::Arc;

        use crate::raster_executor::RasterBackend;
        use crate::testing::{FakeDisplay, FakeWindow};

        let display: Arc<dyn flui_platform::DisplayConnection> = Arc::new(FakeDisplay);
        let instance = crate::testing::noop_instance(&display);
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("the no-op backend has an adapter");
        let mut context = pollster::block_on(GpuContext::on_adapter(
            instance.clone(),
            adapter,
            wgpu::Features::empty(),
        ))
        .expect("the no-op adapter gives a device");
        let factory = crate::SurfaceFactory::new(instance, display);
        let surface = factory
            .create(Arc::new(FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window");
        let (_executor, sender) = RasterExecutor::new(
            crate::testing::ScriptedSlots::new(),
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = sender.mint_slot().expect("a slot id is left");
        assert!(matches!(
            context.attach(slot, SlotTarget::Window(surface), SlotConfig::new()),
            Err(EngineError::IncompatibleSurface)
        ));
        assert!(!context.detach(slot), "a refused window holds no slot");
    }

    /// A context and its factory on wgpu's no-op backend: no GPU, and
    /// surfaces for any window, which no adapter presents to.
    #[cfg(feature = "testing")]
    fn noop_context() -> (GpuContext, crate::SurfaceFactory) {
        use std::sync::Arc;

        let display: Arc<dyn flui_platform::DisplayConnection> =
            Arc::new(crate::testing::FakeDisplay);
        let instance = crate::testing::noop_instance(&display);
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .expect("the no-op backend has an adapter");
        let context = pollster::block_on(GpuContext::on_adapter(
            instance.clone(),
            adapter,
            wgpu::Features::empty(),
        ))
        .expect("the no-op adapter gives a device");
        (context, crate::SurfaceFactory::new(instance, display))
    }

    /// A window frame that fails before it records (the no-op surface never
    /// hands out a texture) puts the shared painter back: the next frame, of
    /// this window or any other slot, builds no glyph atlas and no pipelines.
    ///
    /// Red-check: keep the painter only after a frame that succeeded, and the
    /// second frame builds a second atlas.
    #[cfg(feature = "testing")]
    #[test]
    fn a_failed_window_frame_keeps_the_shared_caches() {
        use std::sync::Arc;

        use crate::{testing::ScriptedAcquire, window_slot::WindowSlot};

        let (mut context, factory) = noop_context();
        let surface = factory
            .create(Arc::new(crate::testing::FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window");
        let window = WindowSlot::configured_without_capabilities(
            surface,
            (32, 16),
            ScriptedAcquire::Timeout,
        );
        let (_executor, sender) = RasterExecutor::new(
            crate::testing::ScriptedSlots::new(),
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = sender.mint_slot().expect("a slot id is left");
        context
            .insert(slot, super::SurfaceSlot::Window(Box::new(window)))
            .expect("the slot's index is free");
        let scene = Scene::new(square_and_glyph());
        for _ in 0..2 {
            assert!(
                context
                    .render_window(
                        slot,
                        drawn_frame(),
                        &scene,
                        DamageRegion::Full(flui_layer::FullReason::FirstFrame)
                    )
                    .is_err(),
                "the no-op surface hands out no texture"
            );
        }
        assert_eq!(
            context.builds().glyph_atlases,
            1,
            "the failed frame left the painter for the next one"
        );
        assert_eq!(context.builds().pipeline_sets, 1);
    }

    /// A window surface still lost after a reconfigure is dropped, not made
    /// again: the executor may not be on the thread the window system serves,
    /// so the frame answers `SurfaceReplacementNeeded` for the owner to make a
    /// new surface with its factory, and the slot's next frame finds it
    /// released (ADR-0305 §2, §8).
    ///
    /// Red-check: drop the released-surface check from `WindowSlot::present`
    /// and the frame answers `Ok(NotShown)`: the owner never hears that it
    /// owes a surface.
    #[cfg(feature = "testing")]
    #[test]
    fn a_lost_window_surface_is_left_for_its_owner_to_replace() {
        use std::sync::Arc;

        use crate::{testing::ScriptedAcquire, window_slot::WindowSlot};

        let (mut context, factory) = noop_context();
        let surface = factory
            .create(Arc::new(crate::testing::FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window");
        let window =
            WindowSlot::configured_without_capabilities(surface, (32, 16), ScriptedAcquire::Lost);
        let (_executor, sender) = RasterExecutor::new(
            crate::testing::ScriptedSlots::new(),
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = sender.mint_slot().expect("a slot id is left");
        context
            .insert(slot, super::SurfaceSlot::Window(Box::new(window)))
            .expect("the slot's index is free");
        let scene = Scene::new(square_and_glyph());
        let full = DamageRegion::Full(flui_layer::FullReason::FirstFrame);
        assert!(matches!(
            context.render_window(slot, drawn_frame(), &scene, full),
            Err(EngineError::SurfaceReplacementNeeded)
        ));
        assert!(
            matches!(
                context.render_window(slot, drawn_frame(), &scene, full),
                Ok(crate::raster::PresentDisposition::NotShown)
            ),
            "the slot holds no surface until its owner restores one"
        );
    }

    /// A resize whose surface configure unwinds leaves the window at the size
    /// its surface last took, so the executor's retry of that size configures
    /// again instead of finding it already applied.
    ///
    /// Red-check: write the new size into the slot before the configure, and
    /// the slot reports the size the surface never took.
    #[cfg(feature = "testing")]
    #[test]
    fn a_resize_whose_configure_panics_leaves_the_old_size() {
        use std::sync::Arc;

        use flui_foundation::panic::FirstPanic;

        use crate::{testing::ScriptedAcquire, window_slot::WindowSlot};

        let (context, factory) = noop_context();
        let surface = factory
            .create(Arc::new(crate::testing::FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window");
        let mut window = WindowSlot::configured_without_capabilities(
            surface,
            (32, 16),
            ScriptedAcquire::Timeout,
        );
        window.panic_in_next_configure = true;
        let gpu = context.gpu();
        let mut caught = FirstPanic::new();
        assert!(
            caught.contain(|| window.resize(&gpu, (64, 32))).is_none(),
            "the injected configure panicked"
        );
        if let Some(payload) = caught.into_payload() {
            flui_foundation::panic::discard_payload(payload);
        }
        let size = |window: &WindowSlot| window.target_format().map(|(_, size)| size);
        assert_eq!(
            size(&window),
            Some((32, 16)),
            "the surface never took 64×32"
        );
        window
            .resize(&gpu, (64, 32))
            .expect("the retry configures the surface");
        assert_eq!(size(&window), Some((64, 32)));
    }

    /// A latency change reconfigures the swapchain, which may hold none of
    /// the pixels shown, so the next frame of an unchanged scene repaints in
    /// full instead of skipping.
    ///
    /// Red-check: drop the `surface_changed` from `WindowSlot::set_latency`
    /// and the unchanged frame is skipped.
    #[cfg(feature = "testing")]
    #[test]
    fn a_latency_change_repaints_the_next_frame() {
        use std::sync::Arc;

        use crate::{testing::ScriptedAcquire, window_slot::WindowSlot};

        let (context, factory) = noop_context();
        let surface = factory
            .create(Arc::new(crate::testing::FakeWindow::new(7)))
            .expect("the no-op backend makes a surface for any window");
        let mut window = WindowSlot::configured_without_capabilities(
            surface,
            (32, 16),
            ScriptedAcquire::Timeout,
        );
        window.settle_presented();
        assert!(!window.owes_a_frame(), "a presented slot owes nothing");
        window.set_latency(&context.gpu(), crate::FrameLatency::Low);
        assert!(
            window.owes_a_frame(),
            "the reconfigured swapchain is repainted"
        );
    }

    /// Attach one RGBA offscreen slot of 32×16 to `sender`'s executor.
    #[cfg(not(target_arch = "wasm32"))]
    fn attach_rgba(
        sender: &crate::raster_executor::RasterSender,
    ) -> (SlotId, PresentationAddress, SurfaceGeneration) {
        let slot = sender.mint_slot().expect("a slot id is left");
        let address = PresentationAddress {
            ui_runtime_id: UiRuntimeId::new(1),
            presentation_id: PresentationId::new(slot.index() as usize + 1),
        };
        let receipt = sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target: SlotTarget::Offscreen {
                    size: DeviceSize::new(32, 16),
                    format: wgpu::TextureFormat::Rgba8Unorm,
                },
                config: SlotConfig::new(),
            })
            .expect("the attach is accepted");
        let SendReceipt::Configured(generation) = receipt else {
            panic!("an attach of a positive size configures the slot: {receipt:?}");
        };
        (slot, address, generation)
    }

    /// Send `slot` one frame, turn the executor and answer its verdict.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_once(
        executor: &mut RasterExecutor<GpuContext>,
        sender: &crate::raster_executor::RasterSender,
        (slot, address, generation): (SlotId, PresentationAddress, SurfaceGeneration),
    ) -> RasterVerdict {
        sender
            .send(frame(slot, address, generation, square_and_glyph()))
            .expect("the frame is accepted");
        let _ = executor.turn();
        let mut settled = Vec::new();
        sender.drain_settlements(slot, &mut settled);
        assert_eq!(settled.len(), 1, "the frame settles once");
        settled[0].verdict
    }

    /// The first slot's attach starts its format's pipeline library on a
    /// worker, and the first frame takes that library instead of building
    /// another (ADR-0305 §1).
    ///
    /// Red-check: drop the `warm_pipelines` call from `attach_offscreen` and
    /// no warm-up is under way after the attach.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn the_first_slots_attach_builds_its_pipelines_on_a_worker_once() {
        let Some(context) = context_or_skip() else {
            return;
        };
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = attach_rgba(&sender);
        // The executor applies the attach on its turn.
        let _ = executor.turn();
        assert!(
            executor.backend().warming_up(),
            "the attach starts the warm-up"
        );
        assert_eq!(executor.backend().builds().pipeline_sets, 1);

        assert_eq!(
            draw_once(&mut executor, &sender, slot),
            RasterVerdict::Presented
        );
        assert!(!executor.backend().warming_up(), "the frame took it");
        assert_eq!(
            executor.backend().builds().pipeline_sets,
            1,
            "the frame built no second library"
        );
        // A second slot of the same format starts nothing.
        let _second = attach_rgba(&sender);
        let _ = executor.turn();
        assert!(!executor.backend().warming_up());
    }

    /// A warm-up whose worker panics resumes that panic on the frame that
    /// joins it, so the slot settles `Retry` and keeps the panic; the next
    /// frame builds the library itself and presents.
    ///
    /// Red-check: discard the worker's payload in `PipelineWarmup::finish`
    /// instead of resuming it and the first frame has no library to draw with.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_warm_up_that_panics_fails_the_frame_that_joins_it_and_the_next_builds_inline() {
        use crate::raster_executor::SlotFailure;

        let Some(mut context) = context_or_skip() else {
            return;
        };
        context.panic_in_next_warmup();
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = attach_rgba(&sender);
        assert_eq!(
            draw_once(&mut executor, &sender, slot),
            RasterVerdict::Retry
        );
        let Some(SlotFailure::Panicked(payload)) = sender.take_failure(slot.0) else {
            panic!("the slot's record holds the worker's panic");
        };
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"injected panic in a pipeline warm-up")
        );
        flui_foundation::panic::discard_payload(payload);
        assert!(!executor.backend().warming_up(), "the join took it");

        assert_eq!(
            draw_once(&mut executor, &sender, slot),
            RasterVerdict::Presented
        );
        assert_eq!(
            executor.backend().builds().pipeline_sets,
            2,
            "the worker's attempt and the frame's own build"
        );
    }

    /// A device rebuild drops a warm-up still under way: its library is the
    /// lost device's, and the next frame builds one on the new device.
    ///
    /// Red-check: keep the warm-up across `GpuContext::rebuild` and it is still
    /// under way after the rebuild, its library the lost device's.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_device_rebuild_drops_a_warm_up_under_way() {
        let Some(context) = context_or_skip() else {
            return;
        };
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let (slot, address, _generation) = attach_rgba(&sender);
        let _ = executor.turn();
        assert!(executor.backend().warming_up());
        executor.backend().lose_device();
        let _ = executor.turn();
        assert!(!executor.backend().warming_up(), "the rebuild dropped it");
        let generation = sender
            .record(slot)
            .expect("the owner holds the slot")
            .required_generation;
        assert_eq!(
            draw_once(&mut executor, &sender, (slot, address, generation)),
            RasterVerdict::Presented
        );
        assert_eq!(executor.backend().builds().pipeline_sets, 2);
    }

    /// A context whose pipeline cache `storage` keeps, with its adapter's
    /// cache key; `None` off Vulkan, where no device has a pipeline cache.
    #[cfg(not(target_arch = "wasm32"))]
    fn cached_context_or_skip(
        storage: &flui_testing::storage::MemoryStorage,
    ) -> Option<(GpuContext, String)> {
        let mut context = context_or_skip()?;
        let Some(key) = wgpu::util::pipeline_cache_key(&context.adapter.get_info()) else {
            eprintln!("skipping: the adapter is not Vulkan's, which alone has a pipeline cache");
            return None;
        };
        if !context
            .adapter
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
        {
            eprintln!("skipping: the Vulkan adapter offers no pipeline cache");
            return None;
        }
        pollster::block_on(context.load_pipeline_cache(std::sync::Arc::new(storage.clone())));
        assert!(
            context.pipeline_cache_seeded().is_some(),
            "a device on an adapter that offers PIPELINE_CACHE asks for it and keeps a cache"
        );
        Some((context, key))
    }

    /// A stored value that is not this adapter's cache (a real envelope with
    /// a payload byte flipped, which wgpu itself would take, since it checks
    /// only its own header) is never handed to wgpu: the context starts
    /// cold, the value is emptied at once so no later launch reads it, and
    /// the context renders.
    ///
    /// Red-check: skip the envelope's digest check in `decode` and the cache
    /// is created from the flipped bytes (seeded); drop `empty` from
    /// `PipelineStore::load` and the corrupt value stays.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_corrupt_stored_pipeline_cache_is_emptied_and_the_context_starts_cold() {
        use crate::pipeline_store::{PIPELINE_CACHE, encode};

        let storage = flui_testing::storage::MemoryStorage::new();
        // A payload wgpu's own header check accepts: data a cache returned.
        let Some((probe, key)) =
            cached_context_or_skip(&flui_testing::storage::MemoryStorage::new())
        else {
            return;
        };
        let payload = probe
            .pipeline_store
            .as_ref()
            .and_then(|store| store.cache().get_data())
            .expect("a Vulkan pipeline cache hands out its data");
        drop(probe);
        let mut corrupt = encode(&key, &payload).expect("a cache's data fits an envelope");
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0x01;
        storage.put(&PIPELINE_CACHE, corrupt);

        let Some((context, _)) = cached_context_or_skip(&storage) else {
            panic!("the context is on the probe's adapter");
        };
        assert_eq!(
            context.pipeline_cache_seeded(),
            Some(false),
            "it starts cold"
        );
        assert_eq!(
            storage.contents(&PIPELINE_CACHE),
            Some(Vec::new()),
            "the refused value is emptied before anything else is written"
        );
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = attach_rgba(&sender);
        assert_eq!(
            draw_once(&mut executor, &sender, slot),
            RasterVerdict::Presented
        );
    }

    /// The warm-up saves the pipeline cache once its library is built, while
    /// the context still runs, as an envelope for this adapter's key.
    ///
    /// Red-check: drop the save from the worker `warm_pipelines` starts and
    /// nothing is stored.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_finished_warm_up_saves_the_pipeline_cache_for_this_adapter() {
        use crate::pipeline_store::{PIPELINE_CACHE, decode};

        let storage = flui_testing::storage::MemoryStorage::new();
        let Some((context, key)) = cached_context_or_skip(&storage) else {
            return;
        };
        assert_eq!(
            storage.contents(&PIPELINE_CACHE),
            None,
            "nothing stored yet"
        );
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = attach_rgba(&sender);
        // The frame takes the warm-up's library; the worker saves after
        // handing it over.
        assert_eq!(
            draw_once(&mut executor, &sender, slot),
            RasterVerdict::Presented
        );
        executor.backend_mut().join_warmup();
        let stored = storage
            .contents(&PIPELINE_CACHE)
            .expect("the warm-up saved the cache");
        let payload = decode(&key, &stored)
            .expect("the stored value is an envelope for this adapter")
            .expect("the envelope holds the cache's data");
        assert!(
            !payload.is_empty(),
            "wgpu's cache data has a header at least"
        );
        drop(executor);
    }

    /// The data one context saved seeds the next context's cache.
    ///
    /// Red-check: create the cache in `PipelineStore::load` without the
    /// decoded payload and the next context starts cold.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_saved_pipeline_cache_seeds_the_next_context() {
        let storage = flui_testing::storage::MemoryStorage::new();
        let Some((first, _)) = cached_context_or_skip(&storage) else {
            return;
        };
        assert_eq!(
            first.pipeline_cache_seeded(),
            Some(false),
            "nothing stored yet"
        );
        let (mut executor, sender) =
            RasterExecutor::new(first, std::rc::Rc::new(flui_foundation::SystemClock), || {});
        let slot = attach_rgba(&sender);
        assert_eq!(
            draw_once(&mut executor, &sender, slot),
            RasterVerdict::Presented
        );
        drop(executor);

        let Some((next, _)) = cached_context_or_skip(&storage) else {
            panic!("the second context is on the same adapter as the first");
        };
        assert_eq!(next.pipeline_cache_seeded(), Some(true));
    }

    /// A device rebuild leaves the lost device's cache and starts a cold one
    /// on the new device, which the context goes on creating its pipelines
    /// through and saving.
    ///
    /// Red-check: drop the cache at the rebuild instead of restarting it and
    /// the rebuilt context has none.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn a_device_rebuild_restarts_the_pipeline_cache_cold() {
        let storage = flui_testing::storage::MemoryStorage::new();
        let Some((mut first, _)) = cached_context_or_skip(&storage) else {
            return;
        };
        // The first context's warm-up saves the cache the second is seeded from.
        first.warm_pipelines(wgpu::TextureFormat::Rgba8Unorm);
        first.join_warmup();
        drop(first);
        let Some((context, _)) = cached_context_or_skip(&storage) else {
            panic!("the second context is on the same adapter as the first");
        };
        assert_eq!(context.pipeline_cache_seeded(), Some(true));
        let (mut executor, sender) = RasterExecutor::new(
            context,
            std::rc::Rc::new(flui_foundation::SystemClock),
            || {},
        );
        let (slot, address, _generation) = attach_rgba(&sender);
        let _ = executor.turn();
        executor.backend().lose_device();
        let _ = executor.turn();
        assert_eq!(
            executor.backend().pipeline_cache_seeded(),
            Some(false),
            "the new device's cache starts cold"
        );
        let generation = sender
            .record(slot)
            .expect("the owner holds the slot")
            .required_generation;
        assert_eq!(
            draw_once(&mut executor, &sender, (slot, address, generation)),
            RasterVerdict::Presented
        );
    }
}
