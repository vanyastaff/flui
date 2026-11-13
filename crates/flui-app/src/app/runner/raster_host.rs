//! The loop's raster host (ADR-0305 §12): one [`RasterExecutor`] over the
//! application's one GPU context, a slot for each window that draws, run
//! inline on the owner thread.
//!
//! The first window to attach places the executor: its surface picks the
//! adapter (ADR-0305 §2). Every later window is a slot on the same device.
//! A frame is sent to the window's slot and the executor turns at once, so
//! the frame's verdict is in hand before the UI runtime's frame transaction
//! returns. A frame dispatched while the host is already turning (a platform
//! callback re-entering from inside the GPU work) is answered
//! [`SubmitVerdict::Retry`] and keeps its work for the next wake.
//!
//! The host also keeps what only the owner knows about each slot: the size
//! it last sent, what the window's surfaces are made over, the version on
//! screen that damage is measured from, and the presentation's external
//! textures, whose updates ride each frame. A frame is stamped with the
//! configuration generation the executor's record requires at that moment.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use flui_engine::{
    DeviceHealth, EngineError, RasterCommand, RasterExecutor, RasterSendError, RasterSender,
    RasterVerdict, SendReceipt, SlotBackend, SlotChange, SlotConfig, SlotId, SlotSettlement,
    SlotTarget, SlotVisibility, SurfaceFactory,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::{
    FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, SurfaceGeneration,
};
use flui_layer::{DamageMode, RasterBase, ScenePublication, SceneSnapshot, SurfaceKey};
use flui_platform::{DisplayColor, PlatformWindow, SurfaceTarget};
use flui_runtime::sink::{FrameSink, SubmitVerdict};

/// Which backend the loop's executor is placed on.
#[derive(Clone, Default)]
pub(crate) enum RasterFactory {
    /// The application's GPU context, bootstrapped on the first window.
    #[default]
    Gpu,
    /// A GPU-free stand-in, for an application run on the headless platform.
    #[cfg(feature = "testing")]
    StandIn(super::stand_in_renderer::StandInRenderers),
}

impl std::fmt::Debug for RasterFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu => f.write_str("Gpu"),
            #[cfg(feature = "testing")]
            Self::StandIn(_) => f.write_str("StandIn"),
        }
    }
}

/// The loop's raster host, owner-local (ADR-0323 §1).
pub(crate) type SharedRasterHost = Rc<RefCell<RasterHost>>;

/// The executor, once placed, and every window's slot on it.
#[derive(Default)]
pub(crate) struct RasterHost {
    factory: RasterFactory,
    placed: Option<Placed>,
}

struct Placed {
    executor: RasterExecutor<Box<dyn SlotBackend>>,
    sender: RasterSender,
    surfaces: SurfaceFactory,
    slots: rustc_hash::FxHashMap<PresentationAddress, HostSlot>,
    /// Scratch for a slot's settlements, kept between frames.
    settlements: Vec<SlotSettlement>,
}

/// What the owner keeps about one window's slot.
struct HostSlot {
    slot: SlotId,
    /// What the window's surfaces are made over: a surface the executor
    /// dropped (a device rebuild the window's surface did not survive) is
    /// made again over it.
    surface_target: Arc<dyn SurfaceTarget>,
    /// The size last sent to the executor; `None` until one is, and after
    /// the executor refused one.
    size: Option<DeviceSize>,
    epoch: FrameEpoch,
    /// The version on screen, which a publication's damage starts from
    /// (ADR-0304 §7). Moved only by a frame the slot showed.
    base: RasterBase,
    textures: Option<flui_layer::TextureRegistry>,
    display: Option<DisplayColor>,
    visibility: SlotVisibility,
}

/// The raster executor refused a window's slot.
#[derive(Debug, thiserror::Error)]
#[error("the raster executor refused the window's slot: {0}")]
pub(crate) struct SlotRefused(&'static str);

fn refused(reason: &'static str) -> EngineError {
    EngineError::surface_creation(SlotRefused(reason))
}

impl RasterHost {
    /// A host whose executor will be placed on `factory`'s backend.
    pub(crate) fn new(factory: RasterFactory) -> Self {
        Self {
            factory,
            placed: None,
        }
    }

    /// Give `window`, presenting `address`, a slot: unconfigured until its
    /// first size. Places the executor if this is the first window.
    ///
    /// # Errors
    /// The window's surface cannot be made, no GPU gives a device for the
    /// first window, or the executor refuses the slot.
    pub(crate) fn attach(
        &mut self,
        window: &dyn PlatformWindow,
        address: PresentationAddress,
        config: &crate::app::AppConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        let surface_target = self.surface_target(window);
        let target = if let Some(placed) = self.placed.as_ref() {
            SlotTarget::Window(placed.surfaces.create(Arc::clone(&surface_target))?)
        } else {
            let (placed, first) = self.place(Arc::clone(&surface_target))?;
            self.placed = Some(placed);
            first
        };
        let placed = self
            .placed
            .as_mut()
            .expect("BUG: the executor was placed above");
        let slot = placed.sender.mint_slot()?;
        let slot_config = SlotConfig::new()
            .color(config.surface_color, display)
            .latency(config.frame_latency);
        placed
            .sender
            .send(RasterCommand::Attach {
                slot,
                address,
                target,
                config: slot_config,
            })
            .map_err(|error| refused(send_refusal(&error)))?;
        placed.executor.turn();
        let mut base = RasterBase::default();
        base.set_mode(damage_mode());
        placed.slots.insert(
            address,
            HostSlot {
                slot,
                surface_target,
                size: None,
                epoch: FrameEpoch::ZERO,
                base,
                textures,
                display,
                visibility: SlotVisibility::Shown,
            },
        );
        Ok(())
    }

    /// The executor over the first window, whose surfaces are made over
    /// `first`, and that window's target.
    fn place(&self, first: Arc<dyn SurfaceTarget>) -> Result<(Placed, SlotTarget), EngineError> {
        let clock: Rc<dyn flui_foundation::MonotonicClock> = Rc::new(flui_foundation::SystemClock);
        // Inline: the owner drains a slot's settlements right after its turn,
        // and reads the device's rebuild deadline for its wake.
        let wake = || {};
        match &self.factory {
            RasterFactory::Gpu => {
                let display = super::host::with_owner_platform(
                    flui_native::OwnerPlatform::display_connection,
                )
                .ok_or_else(|| refused("no platform owns this thread"))?;
                let bootstrap =
                    pollster::block_on(flui_engine::GpuContext::bootstrap(display, Some(first)))?;
                let surface = bootstrap.first.ok_or_else(|| {
                    refused("the GPU context answered without the first window's surface")
                })?;
                let (executor, sender) = RasterExecutor::new(
                    Box::new(bootstrap.context) as Box<dyn SlotBackend>,
                    clock,
                    wake,
                );
                Ok((
                    Placed::new(executor, sender, bootstrap.factory),
                    SlotTarget::Window(surface),
                ))
            }
            #[cfg(feature = "testing")]
            RasterFactory::StandIn(renderers) => {
                let surfaces = flui_engine::testing::noop_surface_factory(Arc::new(
                    flui_engine::testing::FakeDisplay,
                ));
                let target = SlotTarget::Window(surfaces.create(first)?);
                let (executor, sender) = RasterExecutor::new(
                    Box::new(renderers.backend()) as Box<dyn SlotBackend>,
                    clock,
                    wake,
                );
                Ok((Placed::new(executor, sender, surfaces), target))
            }
        }
    }

    /// What `window`'s surfaces are made over: the window itself, or for the
    /// stand-in, which never presents, a fake window, after naming the
    /// platform window so the test's handle finds the slot.
    fn surface_target(&self, window: &dyn PlatformWindow) -> Arc<dyn SurfaceTarget> {
        match &self.factory {
            RasterFactory::Gpu => window.surface_target(),
            #[cfg(feature = "testing")]
            RasterFactory::StandIn(renderers) => {
                renderers.expect_window(window.id());
                Arc::new(flui_engine::testing::FakeWindow::new(1))
            }
        }
    }

    /// Release `address`'s slot and its GPU target.
    pub(crate) fn detach(&mut self, address: PresentationAddress) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        let Some(slot) = placed.slots.remove(&address) else {
            return;
        };
        if let Err(error) = placed
            .sender
            .send(RasterCommand::Detach { slot: slot.slot })
        {
            tracing::warn!(%error, "raster host: a window's detach was refused");
        }
        placed.executor.turn();
    }

    /// Trim the GPU memory behind every slot.
    pub(crate) fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        if let Err(error) = placed.sender.send(RasterCommand::Trim(level)) {
            tracing::warn!(%error, "raster host: a memory trim was refused");
        }
        placed.executor.turn();
    }

    /// Turn the executor with no frame of the caller's: a lost device whose
    /// rebuild is due is rebuilt, whether or not a window has anything new
    /// to draw.
    pub(crate) fn service(&mut self) {
        if let Some(placed) = self.placed.as_mut() {
            placed.executor.turn();
        }
    }

    /// Ask the executor to rebuild a device it gave up on (ADR-0267).
    pub(crate) fn retry_device(&mut self) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        if let Err(error) = placed.sender.send(RasterCommand::RetryDevice) {
            tracing::warn!(%error, "raster host: a device retry was refused");
        }
    }

    /// Where the context's device stands.
    pub(crate) fn device_health(&self) -> DeviceHealth {
        self.placed
            .as_ref()
            .map_or(DeviceHealth::Healthy, |placed| {
                placed.sender.device_health()
            })
    }

    /// When the executor next rebuilds a lost device, for the loop's wake.
    pub(crate) fn device_retry_at(&self) -> Option<web_time::Instant> {
        self.placed
            .as_ref()
            .and_then(|placed| placed.sender.device_retry_at())
    }

    /// Follow the display `address`'s window is on: a changed colour profile
    /// sets its surface up again before the next frame. Answers whether it
    /// changed, so the caller repaints in full.
    pub(crate) fn follow_display(
        &mut self,
        address: PresentationAddress,
        display: Option<DisplayColor>,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        let Some(color) = display else {
            return false;
        };
        if slot.display == Some(color) {
            return false;
        }
        slot.display = Some(color);
        match placed.sender.send(RasterCommand::Configure {
            slot: slot.slot,
            change: SlotChange::Color(color),
        }) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "raster host: a display colour change was refused");
                false
            }
        }
    }

    /// Tell the executor whether `address`'s window can be seen: a slot
    /// that cannot is not drawn, and draws in full when it shows again.
    pub(crate) fn set_visibility(
        &mut self,
        address: PresentationAddress,
        visibility: SlotVisibility,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return;
        };
        if slot.visibility == visibility {
            return;
        }
        slot.visibility = visibility;
        if let Err(error) = placed.sender.send(RasterCommand::Configure {
            slot: slot.slot,
            change: SlotChange::Shown(visibility),
        }) {
            tracing::warn!(%error, "raster host: a visibility change was refused");
        }
    }

    /// Bring `address`'s slot to `size`, and a surface if the executor
    /// dropped its own, and answer the configuration generation its next
    /// frame is stamped with: `None` while the slot has no size the executor
    /// accepted or no surface.
    ///
    /// The generation is the executor's, read after the size is sent: a
    /// device rebuild or a lost surface moves it without the owner asking,
    /// so a cached one would stamp every later frame stale.
    fn configure(
        placed: &mut Placed,
        address: PresentationAddress,
        size: (u32, u32),
    ) -> Option<SurfaceGeneration> {
        let slot = placed.slots.get_mut(&address)?;
        let record = placed.sender.record(slot.slot)?;
        if record.wants_surface {
            let surface = match placed.surfaces.create(Arc::clone(&slot.surface_target)) {
                Ok(surface) => surface,
                Err(error) => {
                    tracing::warn!(%error, "raster host: a window's surface could not be made again");
                    return None;
                }
            };
            if let Err(error) = placed.sender.send(RasterCommand::Restore {
                slot: slot.slot,
                surface,
            }) {
                tracing::warn!(%error, "raster host: a window's new surface was refused");
                return None;
            }
        }
        // A size past `i32` is one no device configures: unconfigured.
        let (Ok(width), Ok(height)) = (i32::try_from(size.0), i32::try_from(size.1)) else {
            return None;
        };
        let size = DeviceSize::new(width, height);
        if slot.size != Some(size) {
            match placed.sender.send(RasterCommand::Configure {
                slot: slot.slot,
                change: SlotChange::Size(size),
            }) {
                Ok(SendReceipt::Configured(_)) => slot.size = Some(size),
                Ok(SendReceipt::Unconfigured | SendReceipt::Accepted) => {
                    slot.size = None;
                    return None;
                }
                Err(error) => {
                    tracing::warn!(%error, "raster host: a window's size was refused");
                    slot.size = None;
                    return None;
                }
            }
        }
        let generation = placed.sender.record(slot.slot)?.required_generation;
        (generation != SurfaceGeneration::ZERO).then_some(generation)
    }

    /// Send `publication` to `address`'s slot, measured on `surface`, turn
    /// the executor and answer what became of the frame.
    fn submit(
        &mut self,
        address: PresentationAddress,
        surface: (u32, u32),
        publication: ScenePublication,
    ) -> SubmitVerdict {
        let Some(placed) = self.placed.as_mut() else {
            tracing::error!("BUG: raster host: a frame for a window with no executor");
            return SubmitVerdict::Failed;
        };
        let Some(generation) = Self::configure(placed, address, surface) else {
            // No size the executor accepted (a zero-sized or over-limit
            // window): nothing can be drawn until the next resize.
            return SubmitVerdict::NotShown;
        };
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: `configure` found the slot");
        slot.epoch = slot.epoch.next();
        let epoch = slot.epoch;
        let key = SurfaceKey {
            size: surface,
            generation,
        };
        let stamp = FrameStamp::new(address, epoch, generation, GpuResourceGeneration::ZERO);
        let damage = slot.base.damage_for(&publication, key);
        let shown = publication.scene.id();
        let mut snapshot =
            SceneSnapshot::of_version(stamp, publication.base, damage, publication.scene);
        if let Some(textures) = &slot.textures {
            snapshot = snapshot.textures(textures.take_updates());
        }
        let id = slot.slot;
        if let Err(error) = placed
            .sender
            .send(RasterCommand::Frame { slot: id, snapshot })
        {
            tracing::error!(%error, "raster host: a frame was refused");
            return SubmitVerdict::Failed;
        }
        placed.executor.turn();
        placed.settlements.clear();
        placed.sender.drain_settlements(id, &mut placed.settlements);
        let Some(settlement) = placed
            .settlements
            .iter()
            .find(|settlement| settlement.epoch == epoch)
        else {
            tracing::error!(?epoch, "raster host: an inline frame settled nothing");
            return SubmitVerdict::Failed;
        };
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: the slot is still there");
        match settlement.verdict {
            RasterVerdict::Presented => {
                slot.base.shown(shown, key);
                SubmitVerdict::Presented
            }
            RasterVerdict::NoPresent => {
                slot.base.shown(shown, key);
                SubmitVerdict::NoPresent
            }
            RasterVerdict::NotShown => SubmitVerdict::NotShown,
            RasterVerdict::Retry => SubmitVerdict::Retry,
            RasterVerdict::SurfaceStale => {
                // The surface the image was on is gone.
                slot.base.forget();
                SubmitVerdict::SurfaceStale
            }
            RasterVerdict::DeviceLost => {
                // The image went with the device: the next frame is drawn in full.
                slot.base.forget();
                SubmitVerdict::DeviceLost
            }
            RasterVerdict::Failed => SubmitVerdict::Failed,
        }
    }
}

impl Placed {
    fn new(
        executor: RasterExecutor<Box<dyn SlotBackend>>,
        sender: RasterSender,
        surfaces: SurfaceFactory,
    ) -> Self {
        Self {
            executor,
            sender,
            surfaces,
            slots: rustc_hash::FxHashMap::default(),
            settlements: Vec::new(),
        }
    }
}

/// Why `send` refused a command, for a log line.
fn send_refusal(error: &RasterSendError) -> &'static str {
    match error {
        RasterSendError::Full(_) => "the lifecycle queue is full",
        RasterSendError::StaleSlot(_) => "the command names a stale slot",
        RasterSendError::Gone(_) => "the raster executor has stopped",
        _ => "the raster executor refused the command",
    }
}

/// `FLUI_DAMAGE=off` sends every frame in full.
fn damage_mode() -> DamageMode {
    crate::app::raster_lane::damage_mode_from_env()
}

/// One frame's sink: the window's size measured once at frame start, and the
/// host it submits through.
pub(crate) struct SlotFrame {
    host: SharedRasterHost,
    address: PresentationAddress,
    surface: (u32, u32),
}

impl SlotFrame {
    /// The sink for one frame of `address`, measured at `surface`.
    pub(crate) fn new(
        host: SharedRasterHost,
        address: PresentationAddress,
        surface: (u32, u32),
    ) -> Self {
        Self {
            host,
            address,
            surface,
        }
    }
}

impl FrameSink for SlotFrame {
    fn surface_size(&mut self) -> (u32, u32) {
        self.surface
    }

    fn submit(&mut self, publication: ScenePublication) -> SubmitVerdict {
        // A frame dispatched while the host is turning (a platform callback
        // re-entering from the GPU work) keeps its work for the next wake.
        let Ok(mut host) = self.host.try_borrow_mut() else {
            return SubmitVerdict::Retry;
        };
        host.submit(self.address, self.surface, publication)
    }
}

/// When the loop's executor next rebuilds a lost device, for a window's wake
/// deadline. `None` while the host is turning: the turn that is running
/// answers it.
pub(crate) fn device_retry_at() -> Option<web_time::Instant> {
    super::host::APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        let host = state.raster_host.try_borrow().ok()?;
        host.device_retry_at()
    })
}

/// One window's slot as its frame driver holds it: the host, the window's
/// address on it, the size the platform last announced, and the device's
/// health as the window last reported it to its UI runtime.
pub(crate) struct RasterSlot {
    pub(crate) host: SharedRasterHost,
    pub(crate) address: PresentationAddress,
    pub(crate) size: (u32, u32),
    pub(crate) device: DeviceHealth,
}

impl RasterSlot {
    /// The sink of one frame, measured now.
    pub(crate) fn frame(&self) -> SlotFrame {
        SlotFrame::new(Rc::clone(&self.host), self.address, self.size)
    }

    /// Tell `ui_runtime` where the device's rebuild stands after a frame, so
    /// the frames it drops carry it (ADR-0267). Answers whether the device
    /// came back, which repaints the window in full.
    pub(crate) fn settle_device(&mut self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        use flui_runtime::frame_failure::DeviceRecovery;

        let Ok(host) = self.host.try_borrow() else {
            return false;
        };
        let health = host.device_health();
        drop(host);
        if health == self.device {
            return false;
        }
        let was_lost = self.device.is_lost();
        self.device = health;
        match health {
            DeviceHealth::Healthy => {
                ui_runtime.note_primary_device_recovery(None);
                if was_lost {
                    tracing::warn!("GPU device lost — recovered successfully");
                }
                was_lost
            }
            DeviceHealth::Rebuilding { attempts: 0 } => false,
            DeviceHealth::Rebuilding { attempts } => {
                ui_runtime.note_primary_device_recovery(Some(DeviceRecovery::Failed { attempts }));
                false
            }
            DeviceHealth::GivenUp { attempts } => {
                ui_runtime
                    .note_primary_device_recovery(Some(DeviceRecovery::Abandoned { attempts }));
                false
            }
            // A health state added later reports nothing until this match names it.
            _ => false,
        }
    }
}

impl Drop for RasterSlot {
    fn drop(&mut self) {
        if let Ok(mut host) = self.host.try_borrow_mut() {
            host.detach(self.address);
        } else {
            tracing::error!(
                "BUG: raster host: a window's slot dropped while the host is turning; its \
                 GPU target stays until the loop ends"
            );
        }
    }
}
