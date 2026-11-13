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
    RasterVerdict, SendReceipt, SlotBackend, SlotChange, SlotConfig, SlotFailure, SlotId,
    SlotSettlement, SlotTarget, SlotVisibility, SurfaceFactory,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::panic::FirstPanic;
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
///
/// The host is busy while it turns the executor, which runs code that is not
/// ours (a tracing subscriber, a scene's or target's `Drop`). Work that
/// finds it busy and must not be lost (a window's detach, a memory trim) is
/// kept here and done when the turn that holds the host ends.
#[derive(Clone, Default)]
pub(crate) struct SharedRasterHost {
    host: Rc<RefCell<RasterHost>>,
    deferred: Rc<Deferred>,
}

/// Work that met a busy host.
#[derive(Default)]
struct Deferred {
    detach: RefCell<Vec<PresentationAddress>>,
    trim: std::cell::Cell<Option<flui_foundation::memory::MemoryPressureLevel>>,
    /// The first panic of that work, held until a window's wake resumes it
    /// ([`SharedRasterHost::take_deferred_panic`]) instead of in place of
    /// the answer of whatever ran before the work.
    panic: RefCell<FirstPanic>,
}

/// Resume `caught`, or, on a thread already unwinding (a window dropped by
/// another panic), say it before `FirstPanic` discards it.
fn resume_unless_unwinding(caught: FirstPanic) {
    if std::thread::panicking() {
        if let Some(payload) = caught.into_payload() {
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a GPU panic met another panic's unwind"
            );
            flui_foundation::panic::discard_payload(payload);
        }
        return;
    }
    caught.resume();
}

impl Drop for Deferred {
    fn drop(&mut self) {
        // The run ended before a wake resumed it: said, not lost silently.
        if let Some(payload) = std::mem::take(self.panic.get_mut()).into_payload() {
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a closed window's GPU panic outlived the run"
            );
            flui_foundation::panic::discard_payload(payload);
        }
    }
}

impl SharedRasterHost {
    /// A host whose executor will be placed on `factory`'s backend.
    pub(crate) fn new(factory: RasterFactory) -> Self {
        Self {
            host: Rc::new(RefCell::new(RasterHost::new(factory))),
            deferred: Rc::default(),
        }
    }

    /// Run `with` on the host, then the work that met it busy meanwhile;
    /// `None` while the host is turning.
    pub(crate) fn try_with<R>(&self, with: impl FnOnce(&mut RasterHost) -> R) -> Option<R> {
        let mut host = self.host.try_borrow_mut().ok()?;
        let answer = with(&mut host);
        self.run_deferred(&mut host);
        Some(answer)
    }

    /// Read the host; `None` while it is turning.
    pub(crate) fn try_read<R>(&self, read: impl FnOnce(&RasterHost) -> R) -> Option<R> {
        let host = self.host.try_borrow().ok()?;
        Some(read(&host))
    }

    /// Release `address`'s slot now, or when the turn that holds the host
    /// ends.
    pub(crate) fn detach(&self, address: PresentationAddress) {
        if self.try_with(|host| host.detach(address)).is_none() {
            self.deferred.detach.borrow_mut().push(address);
            return;
        }
        // A detach answers nothing, so the deferred work it ran resumes its
        // panic here: the window it closed may have been the last to wake.
        resume_unless_unwinding(self.take_deferred_panic());
    }

    /// Trim the GPU memory behind every slot now, or when the turn that
    /// holds the host ends; the strongest of the trims asked meanwhile.
    pub(crate) fn reclaim(&self, level: flui_foundation::memory::MemoryPressureLevel) {
        if self.try_with(|host| host.reclaim(level)).is_none() {
            let strongest = self
                .deferred
                .trim
                .get()
                .map_or(level, |held| held.max(level));
            self.deferred.trim.set(Some(strongest));
            return;
        }
        // A trim answers nothing, as a detach does.
        resume_unless_unwinding(self.take_deferred_panic());
    }

    /// Take the first panic of the work that met the host busy, for a
    /// window's wake to resume once it has nothing left to answer.
    pub(crate) fn take_deferred_panic(&self) -> FirstPanic {
        self.deferred
            .panic
            .try_borrow_mut()
            .map_or_else(|_| FirstPanic::new(), |mut held| std::mem::take(&mut *held))
    }

    /// Run the work that met the host busy. Each item runs whatever an
    /// earlier one did; the first panic is held for
    /// [`take_deferred_panic`](Self::take_deferred_panic), so it does not
    /// take the place of the answer `try_with` returns.
    fn run_deferred(&self, host: &mut RasterHost) {
        let mut caught = FirstPanic::new();
        loop {
            let detach = std::mem::take(&mut *self.deferred.detach.borrow_mut());
            let trim = self.deferred.trim.take();
            if detach.is_empty() && trim.is_none() {
                break;
            }
            for address in detach {
                host.detach_held(address, &mut caught);
            }
            if let Some(level) = trim {
                host.reclaim_held(level, &mut caught);
            }
        }
        if let Ok(mut held) = self.deferred.panic.try_borrow_mut() {
            held.absorb(caught);
        } else {
            // Only a payload's own `Drop` runs while it is borrowed.
            resume_unless_unwinding(caught);
        }
    }
}

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
    /// A size the device cannot draw at (past its texture limit), and the
    /// surface generation it was refused at: the window shows nothing at
    /// it, and is not sent it again, until the platform announces another
    /// size or the generation moves (a rebuilt device, a new surface, which
    /// may take it).
    refused_size: Option<(DeviceSize, SurfaceGeneration)>,
    /// Whether the slot's next frame is the retry of a frame whose resize
    /// or upload panicked: a stale surface it meets again is not retried.
    retrying_panic: bool,
    /// Whether the host sent the slot a size its next turn answers. The slot
    /// holds one failure a turn, so a refusal of that size behind another
    /// failure goes unheard: the host then sends the size again.
    size_sent: bool,
    /// The first panic of the slot's frames since the window last reported
    /// one to its UI runtime ([`RasterSlot::report_frame_panic`]).
    panic: flui_foundation::panic::FirstPanic,
}

/// The raster executor refused a window's slot.
#[derive(Debug, thiserror::Error)]
#[error("the raster executor refused the window's slot: {0}")]
pub(crate) struct SlotRefused(&'static str);

fn refused(reason: &'static str) -> EngineError {
    EngineError::surface_creation(SlotRefused(reason))
}

/// The refusal of a window's slot asked for while the host is turning.
pub(crate) fn host_busy() -> EngineError {
    refused("the raster host is turning")
}

/// Whether a turn ran the slot's own commands into a panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotTurn {
    Clean,
    Panicked,
    /// The GPU refused the window's new surface: a frame has nowhere to go,
    /// and its retry would be refused the same way.
    SurfaceRefused,
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
        // The turn runs code that is not ours; whatever stops the slot from
        // attaching (a refusal, a panic) detaches it before it is answered,
        // so no executor slot outlives its window unnamed.
        let mut caught = FirstPanic::new();
        let _turned = caught.contain(|| placed.executor.turn());
        let refusal = match placed.sender.record(slot).map(|record| record.phase) {
            // A panic of the turn alone (not of the slot's attach) leaves the
            // slot attached; the window reports it after its first frame.
            Some(flui_engine::SlotPhase::Attached) => None,
            _ => match placed.sender.take_failure(slot) {
                Some(SlotFailure::Refused(error)) => Some(error),
                Some(SlotFailure::Panicked(payload)) => {
                    caught.record(payload);
                    Some(refused("attaching the slot panicked"))
                }
                // A failure added later is a refusal until this match names it.
                Some(_) => Some(refused("the raster executor refused the slot")),
                None => Some(refused("the raster executor did not attach the slot")),
            },
        };
        if let Some(error) = refusal {
            match placed.sender.send(RasterCommand::Detach { slot }) {
                Ok(_) => {
                    let _turned = caught.contain(|| placed.executor.turn());
                }
                Err(send) => {
                    tracing::warn!(error = %send, "raster host: a refused slot's detach was refused");
                }
            }
            caught.resume();
            return Err(error);
        }
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
                refused_size: None,
                retrying_panic: false,
                size_sent: false,
                panic: caught,
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

    /// Release `address`'s slot and its GPU target. A panic of the turn
    /// (a target's `Drop` is not ours) is held in `caught` once the slot is
    /// gone.
    fn detach_held(&mut self, address: PresentationAddress, caught: &mut FirstPanic) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        let Some(mut slot) = placed.slots.remove(&address) else {
            return;
        };
        if let Some(payload) = std::mem::replace(&mut slot.panic, FirstPanic::new()).into_payload()
        {
            // A window that closed before it reported its frame's panic (a
            // wake that unwound first, a window hidden since): the closing
            // window has no handler left to tell, and its close must not
            // throw a panic that is not the close's.
            tracing::error!(
                panic =
                    flui_foundation::panic::payload_text(&*payload).unwrap_or("<non-text payload>"),
                "raster host: a closing window's GPU panic was never reported"
            );
            flui_foundation::panic::discard_payload(payload);
        }
        if let Err(error) = placed
            .sender
            .send(RasterCommand::Detach { slot: slot.slot })
        {
            tracing::warn!(%error, "raster host: a window's detach was refused");
        }
        let _turned = caught.contain(|| placed.executor.turn());
    }

    /// Trim the GPU memory behind every slot, holding a panic of the turn in
    /// `caught`.
    fn reclaim_held(
        &mut self,
        level: flui_foundation::memory::MemoryPressureLevel,
        caught: &mut FirstPanic,
    ) {
        let Some(placed) = self.placed.as_mut() else {
            return;
        };
        if let Err(error) = placed.sender.send(RasterCommand::Trim(level)) {
            tracing::warn!(%error, "raster host: a memory trim was refused");
        }
        let _turned = caught.contain(|| placed.executor.turn());
    }

    /// Release `address`'s slot and its GPU target.
    pub(crate) fn detach(&mut self, address: PresentationAddress) {
        let mut caught = FirstPanic::new();
        self.detach_held(address, &mut caught);
        // Reached from a window's `Drop`, which may run during an unwind.
        resume_unless_unwinding(caught);
    }

    /// Trim the GPU memory behind every slot.
    pub(crate) fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        let mut caught = FirstPanic::new();
        self.reclaim_held(level, &mut caught);
        // Reached from a window's `Drop`, which may run during an unwind.
        resume_unless_unwinding(caught);
    }

    /// Turn the executor for `address`'s window with no frame of its own: a
    /// lost device whose rebuild is due is rebuilt, whether or not a window
    /// has anything new to draw. What the turn left for the window (a
    /// refusal, a panic) is settled now, so the frame's own turn finds its
    /// failure slot empty.
    pub(crate) fn service(&mut self, address: PresentationAddress) {
        if let Some(placed) = self.placed.as_mut() {
            let _settled = Self::turn_for(placed, address);
        }
    }

    /// Turn the executor on `address`'s behalf and settle what the turn left
    /// for its slot: a refusal is logged (a size past the device's limit is
    /// remembered), and a panic of the slot's commands or of the turn itself
    /// is held for the window's report after its frame. Answers whether the
    /// slot's own commands panicked.
    fn turn_for(placed: &mut Placed, address: PresentationAddress) -> SlotTurn {
        let mut caught = FirstPanic::new();
        let _turned = caught.contain(|| placed.executor.turn());
        let Some(slot) = placed.slots.get_mut(&address) else {
            caught.resume();
            return SlotTurn::Clean;
        };
        let size_sent = std::mem::take(&mut slot.size_sent);
        let failure = placed.sender.take_failure(slot.slot);
        let size_refused = matches!(
            failure,
            Some(SlotFailure::Refused(EngineError::InvalidTargetSize { .. }))
        );
        if size_sent && failure.is_some() && !size_refused {
            // The failure held may hide a refusal of the size sent with it:
            // the next frame sends the size again and hears its own answer.
            slot.size = None;
        }
        let record = placed.sender.record(slot.slot);
        let settled = match failure {
            Some(SlotFailure::Panicked(payload)) => {
                slot.panic.record(payload);
                SlotTurn::Panicked
            }
            Some(SlotFailure::Refused(error)) => {
                if size_refused {
                    let generation =
                        record.map_or(SurfaceGeneration::ZERO, |record| record.required_generation);
                    slot.refused_size = slot.size.take().map(|size| (size, generation));
                }
                tracing::warn!(%error, "raster host: the executor refused a window's command");
                SlotTurn::Clean
            }
            Some(failure) => {
                tracing::warn!(?failure, "raster host: a window's slot failed");
                SlotTurn::Clean
            }
            None => SlotTurn::Clean,
        };
        // The new surface the host sent last was refused, and the slot has
        // none: not a surface dropped after a restore the backend took (a
        // rebuilt device, a render that needs another), which the frame's
        // retry asks for again.
        let settled = if settled == SlotTurn::Clean
            && record.is_some_and(|record| record.wants_surface && record.surface_refused)
        {
            SlotTurn::SurfaceRefused
        } else {
            settled
        };
        // The turn's own panic (a retired scene's `Drop`, a trim, a device
        // rebuild) goes to the window that turned it, for its report.
        slot.panic.absorb(caught);
        settled
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

    /// The first panic of `address`'s frames since the last one was taken.
    fn take_frame_panic(
        &mut self,
        address: PresentationAddress,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        let slot = self.placed.as_mut()?.slots.get_mut(&address)?;
        std::mem::replace(&mut slot.panic, FirstPanic::new()).into_payload()
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
        // Recorded whether or not it is taken: a send is refused only for a
        // slot that is gone, which asking again cannot change.
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
        // Recorded whether or not it is taken, as a colour change is.
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
        // A size past `i32` is one no device configures: unconfigured.
        let (Ok(width), Ok(height)) = (i32::try_from(size.0), i32::try_from(size.1)) else {
            return None;
        };
        let size = DeviceSize::new(width, height);
        // Checked before a new surface is asked for: nothing is drawn at a
        // refused size, and the restore would move the generation the
        // refusal is held for.
        if slot.refused_size == Some((size, record.required_generation)) {
            return None;
        }
        slot.refused_size = None;
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
        if slot.size != Some(size) {
            match placed.sender.send(RasterCommand::Configure {
                slot: slot.slot,
                change: SlotChange::Size(size),
            }) {
                Ok(SendReceipt::Configured(_)) => {
                    slot.size = Some(size);
                    slot.size_sent = true;
                }
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
    /// the executor and answer what became of the frame. A panic of the
    /// frame is held for the window to report to its UI runtime after the
    /// frame ([`RasterSlot::report_frame_panic`]); the frame fails, and the
    /// window's next change draws in full. A size past the device's limit
    /// fails the frame too, until the platform announces another size.
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
        // Whatever this frame becomes, it is the only retry the last one
        // asked for.
        let retrying = placed
            .slots
            .get_mut(&address)
            .is_some_and(|slot| std::mem::take(&mut slot.retrying_panic));
        let Some(generation) = Self::configure(placed, address, surface) else {
            // `configure` keeps a refused size only while the window is at
            // it: past the device's limit, a retry is refused the same way.
            // Otherwise (a zero size, no surface yet) nothing is drawn until
            // the next resize.
            let refused = placed
                .slots
                .get(&address)
                .is_some_and(|slot| slot.refused_size.is_some());
            return if refused {
                SubmitVerdict::Failed
            } else {
                SubmitVerdict::NotShown
            };
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
        // The turn runs code that is not ours; its settlements are drained
        // whatever it did, so a panic leaves none for the next frame to trip
        // over. A panic of the turn itself (a retired scene's `Drop`) is held
        // for the window's report and does not change what the frame became.
        let turned = Self::turn_for(placed, address);
        placed.settlements.clear();
        placed.sender.drain_settlements(id, &mut placed.settlements);
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: the slot is still there");
        let settled = placed
            .settlements
            .iter()
            .find(|settlement| settlement.epoch == epoch)
            .map(|settlement| settlement.verdict);
        if settled == Some(RasterVerdict::Presented) {
            // A size refusal of this turn (a restore configured at the old
            // size) did not stop the frame: the size it was drawn at is taken.
            slot.refused_size = None;
        }
        if turned == SlotTurn::Panicked {
            let verdict = match settled {
                // The render itself panicked, which the executor settles as
                // a retry. Not retried: a renderer that panics on this scene
                // panics on its retry too, and the window's next change
                // draws again.
                Some(RasterVerdict::Retry) | None => SubmitVerdict::Failed,
                // A command before the frame panicked (a resize, an upload)
                // and the frame settled otherwise: what it became stands, so
                // a stale surface is retried and applies what is still
                // pending. Once: a retry whose resize panics again is the
                // size's or the driver's failure, not the moment's; the
                // window's next change tries it again.
                Some(RasterVerdict::SurfaceStale) if retrying => SubmitVerdict::Failed,
                Some(verdict) => {
                    let verdict = Self::verdict(&mut slot.base, verdict, shown, key);
                    slot.retrying_panic = matches!(verdict, SubmitVerdict::SurfaceStale);
                    verdict
                }
            };
            // The image may be torn or miss an upload: the next frame is
            // drawn in full.
            slot.base.forget();
            return verdict;
        }
        if turned == SlotTurn::SurfaceRefused && settled != Some(RasterVerdict::Presented) {
            // No surface to draw on, and none the GPU takes: nothing until
            // the window's next change asks for one again. Retried, the
            // frame would meet a stale surface every display period.
            slot.base.forget();
            return SubmitVerdict::Failed;
        }
        if slot.refused_size.is_some() {
            // Drawn at no size the device accepts: nothing until the next
            // resize, and no retry, which would be refused the same way.
            slot.base.forget();
            return SubmitVerdict::Failed;
        }
        let Some(verdict) = settled else {
            tracing::error!(?epoch, "raster host: an inline frame settled nothing");
            return SubmitVerdict::Failed;
        };
        Self::verdict(&mut slot.base, verdict, shown, key)
    }

    /// What a settled frame of `version`, drawn on `surface`, became, with
    /// the slot's base moved to what the screen now holds.
    fn verdict(
        base: &mut RasterBase,
        verdict: RasterVerdict,
        version: flui_layer::SceneVersionId,
        surface: SurfaceKey,
    ) -> SubmitVerdict {
        match verdict {
            RasterVerdict::Presented => {
                base.shown(version, surface);
                SubmitVerdict::Presented
            }
            RasterVerdict::NoPresent => {
                base.shown(version, surface);
                SubmitVerdict::NoPresent
            }
            RasterVerdict::NotShown => SubmitVerdict::NotShown,
            RasterVerdict::Retry => SubmitVerdict::Retry,
            RasterVerdict::SurfaceStale => {
                // The surface the image was on is gone.
                base.forget();
                SubmitVerdict::SurfaceStale
            }
            RasterVerdict::DeviceLost => {
                // The image went with the device: the next frame is drawn in full.
                base.forget();
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
        self.host
            .try_with(|host| host.submit(self.address, self.surface, publication))
            .unwrap_or(SubmitVerdict::Retry)
    }
}

/// When the loop's executor next rebuilds a lost device, for a window's wake
/// deadline. `None` while the host is turning: the turn that is running
/// answers it.
pub(crate) fn device_retry_at() -> Option<web_time::Instant> {
    super::host::APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        state.raster_host.try_read(RasterHost::device_retry_at)?
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
        SlotFrame::new(self.host.clone(), self.address, self.size)
    }

    /// Report the first panic of the window's frames since the last report
    /// to `ui_runtime`, which hands it to the application's frame-failure
    /// handler, then resume a panic of the work that met the host busy (a
    /// closed window's detach), which no window's handler owns. Run at the
    /// end of a wake, with no borrow of the host held: the handler is not
    /// ours.
    pub(crate) fn report_frame_panic(&self, ui_runtime: &crate::app::ui_runtime::UiRuntime) {
        let address = self.address;
        let mut caught = FirstPanic::new();
        if let Some(payload) = self
            .host
            .try_with(|host| host.take_frame_panic(address))
            .flatten()
        {
            let _reported =
                caught.contain(|| ui_runtime.report_contained_panic(address, &*payload));
            flui_foundation::panic::discard_payload(payload);
        }
        caught.absorb(self.host.take_deferred_panic());
        caught.resume();
    }

    /// Tell `ui_runtime` where the device's rebuild stands after a frame, so
    /// the frames it drops carry it (ADR-0267). Answers whether the device
    /// came back, which repaints the window in full.
    pub(crate) fn settle_device(&mut self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        use flui_runtime::frame_failure::DeviceRecovery;

        // A host still turning up the stack leaves the device unseen, for the
        // next settle to see.
        let Some((health, failure)) = self.host.try_read(|host| {
            (
                host.device_health(),
                host.placed
                    .as_ref()
                    .and_then(|placed| placed.sender.take_device_failure()),
            )
        }) else {
            return false;
        };
        if let Some(error) = failure {
            tracing::warn!(%error, "GPU device rebuild failed");
        }
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
        self.host.detach(self.address);
    }
}
