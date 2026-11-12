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

use flui_foundation::{geometry::DeviceSize, memory::MemoryPressureLevel, panic::FirstPanic};
use flui_layer::{DamageRegion, LayerTree, Scene};
use flui_platform::{DisplayColor, DisplayConnection, SurfaceTarget};

use crate::{
    FrameLatency,
    device_domain::DeviceDomain,
    error::{EngineError, EngineResult},
    gpu_backend::{Attempt, GpuBackend},
    layer_dispatcher::LayerDispatcher,
    offscreen::OffscreenRenderer,
    painter::WgpuPainter,
    pipeline_set::PipelineLibrary,
    raster::PresentDisposition,
    raster_executor::{SlotBackend, SlotConfig, SlotId, SlotTarget},
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
    texture: wgpu::Texture,
    format: wgpu::TextureFormat,
    size: (u32, u32),
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
    // which change what the rendered pixels look like.
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("FLUI GPU Context Device"),
            required_features: adapter.features() & wanted_features,
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
    /// The presentation blit of window slots whose frames render into a
    /// retained target, one pipeline per surface format; built at the first
    /// window frame.
    blit: Option<OffscreenRenderer>,
    slots: SlotTable,
    #[cfg(test)]
    builds: ContextBuilds,
    /// How many of the next offscreen frames panic after their recording.
    #[cfg(test)]
    panics_ahead: u32,
    /// When set, the next torn painter's drop panics.
    #[cfg(test)]
    panic_in_next_cleanup: bool,
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
        first: Option<&Arc<dyn crate::WindowTarget>>,
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
        install_device_diagnostics(&domain);
        Self {
            instance,
            adapter,
            recipe,
            domain,
            painter: None,
            pipelines: None,
            blit: None,
            slots: SlotTable::default(),
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
        // What the old device made goes before the new device is used.
        self.painter = None;
        self.pipelines = None;
        self.blit = None;
        self.adapter = adapter;
        self.domain = DeviceDomain::new(Arc::new(device), Arc::new(queue));
        install_device_diagnostics(&self.domain);
        #[cfg(test)]
        {
            self.builds.devices += 1;
        }
        let max_dim = self.max_texture_dimension();
        let Self {
            adapter,
            domain,
            slots,
            ..
        } = self;
        let gpu = gpu_of(adapter, domain);
        let mut refused = Vec::new();
        for (id, slot) in slots.iter_mut() {
            let rebuilt = match slot {
                SurfaceSlot::Offscreen(entry) => {
                    offscreen_texture(domain, max_dim, entry.size, entry.format)
                        .map(|texture| entry.texture = texture)
                }
                SurfaceSlot::Window(window) => window.rebuild(&gpu),
            };
            if let Err(error) = rebuilt {
                refused.push((id, error));
            }
        }
        Ok(refused)
    }

    /// The device domain every slot submits through.
    #[cfg(test)]
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
                texture,
                format,
                size,
            }),
        )
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
        self.insert(slot, SurfaceSlot::Window(Box::new(window)))
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
                if entry.size == size {
                    return Ok(());
                }
                entry.texture = offscreen_texture(domain, max_dim, size, entry.format)?;
                entry.size = size;
                Ok(())
            }
            Some(SurfaceSlot::Window(window)) => window.resize(&gpu_of(adapter, domain), size),
            None => Err(not_attached()),
        }
    }

    /// Detach `slot`; `false` for a stale id or one already detached.
    pub(crate) fn detach(&mut self, slot: SlotId) -> bool {
        self.slots.remove(slot).is_some()
    }

    /// Clear offscreen `slot` to the frame background and draw `tree` over it.
    ///
    /// # Errors
    /// [`EngineError::InvalidRenderTarget`] for a slot that is not an
    /// attached offscreen slot, or the frame's own render and submission
    /// errors.
    ///
    /// # Panics
    /// A panic of the frame is resumed once the painter's pipelines are kept
    /// for the next frame (ADR-0305 §7).
    pub(crate) fn render_slot(&mut self, slot: SlotId, tree: &LayerTree) -> EngineResult<()> {
        let (texture, format, size) = {
            let entry = self.offscreen_slot(slot)?;
            (entry.texture.clone(), entry.format, entry.size)
        };
        let _submission_scope = self.domain.begin_frame_scope()?;
        let mut painter = self.take_painter(format, size);
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
                {
                    let mut backend = LayerDispatcher::new(&mut painter);
                    crate::layer_walk::record_layer_tree(tree, tree.root(), &mut backend)?;
                }
                #[cfg(test)]
                assert!(!panics, "injected panic in the middle of a slot's frame");
                let mut encoder =
                    domain
                        .device()
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("FLUI Slot Render Encoder"),
                        });
                painter.render_to_texture(&texture, &mut encoder)?;
                painter.submit_encoder(encoder).map(|_submitted| ())
            })();
            // Once a frame began, an error closes it as a completed frame
            // does, with its encoder discarded, as the window path's frame
            // protocol closes its own: the painter is whole for the next one.
            painter.finish_frame();
            drawn
        });
        let Some(drawn) = drawn else {
            self.keep_pipelines(painter, &mut caught);
            caught.resume();
            unreachable!("BUG: a contained call that did not return holds its panic")
        };
        self.painter = Some(painter);
        drawn
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
        scene: &Scene,
        damage: DamageRegion,
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
        let mut painter = self.take_painter(format, size);
        painter.resize(size.0, size.1);
        // The blit's pipelines, like the library's, are inserted only once
        // built, so an unwound frame leaves them whole.
        let mut blit = self
            .blit
            .take()
            .unwrap_or_else(|| OffscreenRenderer::with_domain(Arc::clone(&self.domain)));
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
            window.render(
                &gpu_of(adapter, domain),
                &mut painter,
                &mut blit,
                scene,
                damage,
            )
        });
        self.blit = Some(blit);
        let Some(rendered) = rendered else {
            self.keep_pipelines(painter, &mut caught);
            caught.resume();
            unreachable!("BUG: a contained call that did not return holds its panic")
        };
        // An error leaves the painter whole, as on the offscreen path:
        // acquire errors come before it records, and a recording error is
        // closed by the frame protocol's cleanup.
        self.painter = Some(painter);
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
        crate::headless::readback_rgba(&self.domain, &entry.texture, entry.size.0, entry.size.1)
    }

    /// What this context has built over its life.
    #[cfg(test)]
    pub(crate) fn builds(&self) -> ContextBuilds {
        self.builds
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

    /// Whether this frame panics, counting it off.
    #[cfg(test)]
    fn injected_panic(&mut self) -> bool {
        let panics = self.panics_ahead > 0;
        self.panics_ahead = self.panics_ahead.saturating_sub(1);
        panics
    }

    /// The shared painter, retargeted to `format` and sized for `size`; built
    /// first when no complete frame left one, over the pipelines an unwound
    /// frame's painter left.
    fn take_painter(&mut self, format: wgpu::TextureFormat, size: (u32, u32)) -> WgpuPainter {
        let taken = self.painter.take();
        let mut painter = if let Some(painter) = taken {
            painter
        } else {
            self.built_painter(format, size)
        };
        #[cfg(test)]
        let sets_before = painter.pipeline_sets_built();
        painter.retarget(format);
        #[cfg(test)]
        {
            self.builds.pipeline_sets += painter.pipeline_sets_built() - sets_before;
        }
        painter
    }

    /// A painter with empty caches, over the pipelines an unwound frame's
    /// painter left, or over a library built for `format`.
    fn built_painter(&mut self, format: wgpu::TextureFormat, size: (u32, u32)) -> WgpuPainter {
        #[cfg(test)]
        {
            self.builds.glyph_atlases += 1;
        }
        let pipelines = self.pipelines.take().unwrap_or_else(|| {
            #[cfg(test)]
            {
                self.builds.pipeline_sets += 1;
            }
            PipelineLibrary::new(self.domain.device(), format)
        });
        WgpuPainter::with_pipelines(
            Arc::clone(&self.domain),
            pipelines,
            size,
            crate::frame_protocol::FontSource::Ordinary,
        )
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
fn install_device_diagnostics(domain: &Arc<DeviceDomain>) {
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
        if let Some(domain) = lost.upgrade() {
            domain.mark_lost();
        }
    });
}

/// The context as the raster executor's backend. An offscreen slot renders in
/// full each frame and counts as presented once submitted; a window slot
/// renders its damage and presents.
impl SlotBackend for GpuContext {
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

    fn render(
        &mut self,
        slot: SlotId,
        scene: &Scene,
        damage: DamageRegion,
    ) -> EngineResult<PresentDisposition> {
        match self.slots.get(slot) {
            Some(SurfaceSlot::Window(_)) => self.render_window(slot, scene, damage),
            Some(SurfaceSlot::Offscreen(_)) | None => {
                self.render_slot(slot, scene.tree())?;
                Ok(PresentDisposition::Presented)
            }
        }
    }

    fn detach(&mut self, slot: SlotId) {
        GpuContext::detach(self, slot);
    }

    fn reclaim(&mut self, level: MemoryPressureLevel) {
        if let Some(painter) = self.painter.as_mut() {
            painter.reclaim(level);
        }
    }

    fn device_lost(&self) -> bool {
        self.domain.is_lost()
    }

    fn rebuild_device(&mut self) -> EngineResult<Vec<(SlotId, EngineError)>> {
        self.rebuild()
    }
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
    use flui_foundation::{
        FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, PresentationId,
        SurfaceGeneration, UiRuntimeId,
        geometry::{DeviceSize, Offset, Rect},
    };
    use flui_layer::{DamageRegion, LayerTree, Scene, SceneBuilder, SceneSnapshot};
    use flui_painting::{
        Canvas, TextPainter,
        paint::Paint,
        styling::Color,
        typography::{TextDirection, TextSpan, TextStyle},
    };

    use super::{GpuContext, INSTANCES_CREATED};
    use crate::error::EngineError;
    use crate::raster_executor::{
        RasterCommand, RasterExecutor, RasterVerdict, SendReceipt, SlotConfig, SlotId, SlotTarget,
    };

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
                FrameStamp::new(
                    address,
                    FrameEpoch::ZERO,
                    generation,
                    GpuResourceGeneration::ZERO,
                ),
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
            std::sync::Arc::new(flui_foundation::SystemClock),
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
            std::sync::Arc::new(flui_foundation::SystemClock),
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
            RasterExecutor::new(context, std::sync::Arc::new(clock.clone()), || {});
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
            std::sync::Arc::new(flui_foundation::SystemClock),
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

        use crate::raster_executor::SlotBackend;
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
            std::sync::Arc::new(flui_foundation::SystemClock),
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
            std::sync::Arc::new(flui_foundation::SystemClock),
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
            std::sync::Arc::new(flui_foundation::SystemClock),
            || {},
        );
        let slot = sender.mint_slot().expect("a slot id is left");
        context
            .insert(slot, super::SurfaceSlot::Window(Box::new(window)))
            .expect("the slot's index is free");
        let scene = Scene::new(square_and_glyph());
        let full = DamageRegion::Full(flui_layer::FullReason::FirstFrame);
        assert!(matches!(
            context.render_window(slot, &scene, full),
            Err(EngineError::SurfaceReplacementNeeded)
        ));
        assert!(
            matches!(
                context.render_window(slot, &scene, full),
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
}
