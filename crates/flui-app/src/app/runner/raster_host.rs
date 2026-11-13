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

use web_time::Instant;

use flui_engine::{
    DeviceHealth, EngineError, RasterCommand, RasterExecutor, RasterSendError, RasterSender,
    RasterVerdict, SendReceipt, SlotBackend, SlotChange, SlotConfig, SlotFailure, SlotId,
    SlotSettlement, SlotSize, SlotTarget, SlotVisibility, SurfaceFactory, SurfaceHold,
};
use flui_foundation::geometry::DeviceSize;
use flui_foundation::panic::FirstPanic;
use flui_foundation::{
    FrameEpoch, FrameStamp, GpuResourceGeneration, PresentationAddress, SurfaceGeneration,
};
use flui_layer::{DamageMode, RasterBase, ScenePublication, SceneSnapshot, SurfaceKey};
use flui_platform::{DisplayColor, PlatformWindow, SurfaceTarget};
use flui_runtime::pump::SampledClock;
use flui_runtime::sink::{FrameSink, SubmitVerdict};

/// Which backend the loop's executor is placed on.
#[derive(Clone, Default)]
pub(crate) enum RasterFactory {
    /// The application's GPU context, bootstrapped on the first window.
    #[default]
    Gpu,
    /// A GPU-free stand-in, for an application run on the headless platform.
    #[cfg(all(
        feature = "testing",
        not(any(target_os = "android", target_os = "ios"))
    ))]
    StandIn(super::stand_in_renderer::StandInRenderers),
    /// The engine's scripted slots, which record what each command asked,
    /// for this module's tests.
    #[cfg(all(test, feature = "testing"))]
    Scripted(flui_engine::testing::ScriptedSlots),
}

impl std::fmt::Debug for RasterFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu => f.write_str("Gpu"),
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios"))
            ))]
            Self::StandIn(_) => f.write_str("StandIn"),
            #[cfg(all(test, feature = "testing"))]
            Self::Scripted(_) => f.write_str("Scripted"),
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
    /// Surfaces the platform released or gave back, in the order it did.
    surfaces: RefCell<Vec<(PresentationAddress, bool)>>,
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
    #[cfg_attr(
        any(target_os = "android", target_os = "ios"),
        expect(
            dead_code,
            reason = "the mobile runners keep the default GPU host; only the desktop loop picks a factory"
        )
    )]
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

    /// The platform gave `address`'s window a surface (`available`) or is
    /// taking it away ([`RasterHost::set_surface`]), now, or when the turn
    /// that holds the host ends; a release then lands after the platform's
    /// callback returned, which only a platform callback re-entering the GPU
    /// work can cause. Answers whether the window's next frame repaints in
    /// full: a surface given back while the host is busy is taken as on its
    /// way.
    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(
            dead_code,
            reason = "only Android and iOS take a window's surface away; no desktop platform does"
        )
    )]
    pub(crate) fn set_surface(&self, address: PresentationAddress, available: bool) -> bool {
        if let Some(restoring) = self.try_with(|host| host.set_surface(address, available)) {
            // A surface change answers nothing the deferred work can take
            // the place of, as a detach does.
            resume_unless_unwinding(self.take_deferred_panic());
            return restoring;
        }
        tracing::warn!("raster host: a window's surface change met the host turning");
        self.deferred
            .surfaces
            .borrow_mut()
            .push((address, available));
        available
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
            let surfaces = std::mem::take(&mut *self.deferred.surfaces.borrow_mut());
            let detach = std::mem::take(&mut *self.deferred.detach.borrow_mut());
            let trim = self.deferred.trim.take();
            if surfaces.is_empty() && detach.is_empty() && trim.is_none() {
                break;
            }
            for (address, available) in surfaces {
                let _restoring = host.set_surface_held(address, available, &mut caught);
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
    epoch: FrameEpoch,
    /// The version on screen, which a publication's damage starts from
    /// (ADR-0304 §7). Moved only by a frame the slot showed.
    base: RasterBase,
    /// Whether the platform gives the window a surface: `false` from the
    /// platform's release (Android's `Pause` and `TerminateWindow`, a UIKit
    /// scene's backgrounding) until it gives one back. Only then is a
    /// surface the owner released made again.
    available: bool,
    /// When the window's next surface may be made, after attempts that did
    /// not give it one.
    restore: RestorePace,
    textures: Option<flui_layer::TextureRegistry>,
    display: Option<DisplayColor>,
    visibility: SlotVisibility,
    /// Whether the slot's next frame is the retry of a frame whose resize
    /// or upload panicked: a stale surface it meets again is not retried.
    retrying_panic: bool,
    /// The first panic of the slot's frames since the window last reported
    /// one to its UI runtime ([`RasterSlot::report_frame_panic`]).
    panic: flui_foundation::panic::FirstPanic,
}

impl HostSlot {
    /// Whether the window is owed a surface it can be given now: one the
    /// executor dropped or refused, or one the platform released and gave
    /// back.
    fn needs_surface(&self, hold: SurfaceHold) -> bool {
        self.available
            && matches!(
                hold,
                SurfaceHold::Wanted | SurfaceHold::Refused | SurfaceHold::Released
            )
    }
}

/// The pace of a window's attempts at a new surface: the first at once, and
/// each after one that left the window without a surface later than the one
/// before, from a frame at 60 Hz up to a second, as ADR-0267's backoff climbs.
/// Reset once the window holds a surface or the platform reports it again.
#[derive(Clone, Copy, Debug, Default)]
struct RestorePace {
    attempts: u32,
    next: Option<Instant>,
}

impl RestorePace {
    const FIRST: std::time::Duration = std::time::Duration::from_millis(16);
    const CEILING: std::time::Duration = std::time::Duration::from_secs(1);

    /// Whether an attempt may be made at `now`.
    fn due(self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    /// An attempt was made at `now`: the next waits longer.
    fn attempted(&mut self, now: Instant) {
        let wait = Self::FIRST
            .saturating_mul(1 << self.attempts.min(6))
            .min(Self::CEILING);
        self.attempts = self.attempts.saturating_add(1);
        self.next = Some(now.checked_add(wait).unwrap_or(now));
    }
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
}

impl RasterHost {
    /// A host whose executor will be placed on `factory`'s backend.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios"),
            not(all(test, feature = "testing"))
        ),
        expect(
            dead_code,
            reason = "the mobile runners keep the default GPU host; only the desktop loop picks a factory"
        )
    )]
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
        self.attach_target(surface_target, address, config, textures, display)
    }

    /// Give the window whose surfaces are made over `surface_target` a slot,
    /// as [`attach`](Self::attach) does.
    fn attach_target(
        &mut self,
        surface_target: Arc<dyn SurfaceTarget>,
        address: PresentationAddress,
        config: &crate::app::AppConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
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
                epoch: FrameEpoch::ZERO,
                base,
                textures,
                display,
                visibility: SlotVisibility::Shown,
                retrying_panic: false,
                available: true,
                restore: RestorePace::default(),
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
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios"))
            ))]
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
            #[cfg(all(test, feature = "testing"))]
            RasterFactory::Scripted(slots) => {
                let surfaces = flui_engine::testing::noop_surface_factory(Arc::new(
                    flui_engine::testing::FakeDisplay,
                ));
                let target = SlotTarget::Window(surfaces.create(first)?);
                let (executor, sender) = RasterExecutor::new(
                    Box::new(slots.clone()) as Box<dyn SlotBackend>,
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
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios"))
            ))]
            RasterFactory::StandIn(renderers) => {
                renderers.expect_window(window.id());
                Arc::new(flui_engine::testing::FakeWindow::new(1))
            }
            #[cfg(all(test, feature = "testing"))]
            RasterFactory::Scripted(_) => Arc::new(flui_engine::testing::FakeWindow::new(1)),
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
    /// for its slot: a refusal is logged, and a panic of the slot's commands
    /// or of the turn itself is held for the window's report after its
    /// frame. Answers whether the slot's own commands panicked. What the
    /// window does next (a new surface, another size, nothing until it
    /// changes) is read from the slot's record, not from the failure.
    fn turn_for(placed: &mut Placed, address: PresentationAddress) -> SlotTurn {
        let mut caught = FirstPanic::new();
        let _turned = caught.contain(|| placed.executor.turn());
        let Some(slot) = placed.slots.get_mut(&address) else {
            caught.resume();
            return SlotTurn::Clean;
        };
        let settled = match placed.sender.take_failure(slot.slot) {
            Some(SlotFailure::Panicked(payload)) => {
                slot.panic.record(payload);
                SlotTurn::Panicked
            }
            Some(SlotFailure::Refused(error)) => {
                tracing::warn!(%error, "raster host: the executor refused a window's command");
                SlotTurn::Clean
            }
            Some(failure) => {
                tracing::warn!(?failure, "raster host: a window's slot failed");
                SlotTurn::Clean
            }
            None => SlotTurn::Clean,
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
        let slot = placed.slots.get(&address)?;
        let record = placed.sender.record(slot.slot)?;
        // A size past `i32` is one no device configures: unconfigured.
        let (Ok(width), Ok(height)) = (i32::try_from(size.0), i32::try_from(size.1)) else {
            return None;
        };
        let size = DeviceSize::new(width, height);
        // Nothing is drawn at a size the device refused, and asking again
        // is refused the same way: the window waits for another size, or
        // for a rebuilt device, which the record shows pending again.
        if record.surface.size == SlotSize::Refused(size) {
            return None;
        }
        // A surface released by the platform waits for it; one on its way is
        // not asked for again; another attempt waits out its pace.
        if record.surface.hold == SurfaceHold::Held {
            placed
                .slots
                .get_mut(&address)
                .expect("BUG: the slot was found above")
                .restore = RestorePace::default();
        } else if slot.needs_surface(record.surface.hold) {
            if !Self::restore_surface(placed, address, web_time::Instant::now()) {
                return None;
            }
        } else if record.surface.hold != SurfaceHold::Restoring {
            // Released, and the platform has not given it back.
            return None;
        }
        let slot = placed.slots.get(&address)?;
        if record.surface.size.newest() != Some(size) {
            match placed.sender.send(RasterCommand::Configure {
                slot: slot.slot,
                change: SlotChange::Size(size),
            }) {
                Ok(SendReceipt::Configured(_)) => {}
                Ok(SendReceipt::Unconfigured | SendReceipt::Accepted) => return None,
                Err(error) => {
                    tracing::warn!(%error, "raster host: a window's size was refused");
                    return None;
                }
            }
        }
        let generation = placed.sender.record(slot.slot)?.required_generation;
        (generation != SurfaceGeneration::ZERO).then_some(generation)
    }

    /// Make `address`'s window a new surface and send it to its slot, when
    /// the window's pace allows an attempt at `now`: answers whether one is
    /// on its way. A window whose native surface is not there yet (an
    /// Android `Resume` before its `InitWindow`) is taken as released by the
    /// platform until it reports one again.
    fn restore_surface(placed: &mut Placed, address: PresentationAddress, now: Instant) -> bool {
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        if !slot.restore.due(now) {
            return false;
        }
        slot.restore.attempted(now);
        let surface = match placed.surfaces.create(Arc::clone(&slot.surface_target)) {
            Ok(surface) => surface,
            Err(EngineError::SurfaceTargetUnavailable { .. }) => {
                slot.available = false;
                slot.restore = RestorePace::default();
                tracing::trace!("raster host: the window has no native surface yet");
                return false;
            }
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's surface could not be made again");
                return false;
            }
        };
        match placed.sender.send(RasterCommand::Restore {
            slot: slot.slot,
            surface,
        }) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's new surface was refused");
                false
            }
        }
    }

    /// The platform gave `address`'s window a surface (`available`) or is
    /// taking it away. A release is applied before this returns, since the
    /// native window behind the surface may go as soon as the platform's
    /// callback does; a surface given back is made and sent at once, and a
    /// device the executor gave up on is asked to rebuild, the window's
    /// return being the moment to try again. Answers whether the window has a
    /// surface on its way, so its next frame repaints in full.
    #[cfg_attr(
        not(any(
            target_os = "android",
            target_os = "ios",
            all(test, feature = "testing")
        )),
        expect(
            dead_code,
            reason = "only Android and iOS take a window's surface away; no desktop platform does"
        )
    )]
    pub(crate) fn set_surface(&mut self, address: PresentationAddress, available: bool) -> bool {
        let mut caught = FirstPanic::new();
        let restored = self.set_surface_held(address, available, &mut caught);
        // Reached from a platform callback, which may run during an unwind.
        resume_unless_unwinding(caught);
        restored
    }

    fn set_surface_held(
        &mut self,
        address: PresentationAddress,
        available: bool,
        caught: &mut FirstPanic,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        slot.available = available;
        slot.restore = RestorePace::default();
        let id = slot.slot;
        let restoring = if available {
            if matches!(placed.sender.device_health(), DeviceHealth::GivenUp { .. })
                && let Err(error) = placed.sender.send(RasterCommand::RetryDevice)
            {
                tracing::warn!(%error, "raster host: a device retry was refused");
            }
            Self::restore_surface(placed, address, Instant::now())
        } else {
            if let Err(error) = placed.sender.send(RasterCommand::Release { slot: id }) {
                tracing::warn!(%error, "raster host: a window's surface release was refused");
            }
            false
        };
        let _turned = caught.contain(|| Self::turn_for(placed, address));
        restoring
            && placed
                .sender
                .record(id)
                .is_some_and(|record| record.surface.hold == SurfaceHold::Held)
    }

    /// Draw `scene`, lent for this call alone, into `address`'s window at
    /// `size`, outside its frames: the development reload plugin's scene.
    /// Answers whether it was shown. The window's next frame draws in full.
    #[cfg_attr(
        not(any(target_os = "android", all(test, feature = "testing"))),
        expect(dead_code, reason = "only the Android runner drives a scene plugin")
    )]
    pub(crate) fn render_lent(
        &mut self,
        address: PresentationAddress,
        size: (u32, u32),
        scene: &flui_layer::Scene,
        reset_fonts: bool,
    ) -> bool {
        let Some(placed) = self.placed.as_mut() else {
            return false;
        };
        if Self::configure(placed, address, size).is_none() {
            return false;
        }
        // The size the configuration sent is applied before the scene is.
        let _settled = Self::turn_for(placed, address);
        let Some(slot) = placed.slots.get_mut(&address) else {
            return false;
        };
        // The screen shows a scene no publication described.
        slot.base.forget();
        let id = slot.slot;
        // A panic of the render is the slot's, held for the window's report.
        let mut caught = FirstPanic::new();
        let rendered = caught.contain(|| placed.executor.render_lent(id, scene, reset_fonts));
        slot.panic.absorb(caught);
        match rendered {
            Some(Ok(shown)) => shown == flui_engine::PresentDisposition::Presented,
            Some(Err(error)) => {
                tracing::error!(%error, "raster host: the plugin's scene failed to render");
                false
            }
            None => false,
        }
    }

    /// The earliest moment a window owed a surface may try for one again.
    fn surface_retry_at(&self) -> Option<Instant> {
        let placed = self.placed.as_ref()?;
        placed
            .slots
            .values()
            .filter(|slot| {
                placed
                    .sender
                    .record(slot.slot)
                    .is_some_and(|record| slot.needs_surface(record.surface.hold))
            })
            .filter_map(|slot| slot.restore.next)
            .min()
    }

    /// Whether the device refused `surface`, the size `address`'s window is
    /// at, and nothing has asked it again since.
    fn size_refused(placed: &Placed, address: PresentationAddress, surface: (u32, u32)) -> bool {
        let (Ok(width), Ok(height)) = (i32::try_from(surface.0), i32::try_from(surface.1)) else {
            return false;
        };
        placed
            .slots
            .get(&address)
            .and_then(|slot| placed.sender.record(slot.slot))
            .is_some_and(|record| {
                record.surface.size == SlotSize::Refused(DeviceSize::new(width, height))
            })
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
            // Past the device's limit, a retry is refused the same way.
            // Otherwise (a zero size, no surface yet) nothing is drawn until
            // the next resize.
            return if Self::size_refused(placed, address, surface) {
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
        let settled = placed
            .settlements
            .iter()
            .find(|settlement| settlement.epoch == epoch)
            .map(|settlement| settlement.verdict);
        // No surface the GPU takes, or no size it draws at: nothing until the
        // window's next change (or a rebuilt device) asks again. Retried, the
        // frame would meet a stale surface every display period.
        let stuck = settled != Some(RasterVerdict::Presented)
            && (placed
                .sender
                .record(id)
                .is_some_and(|record| record.surface.hold == SurfaceHold::Refused)
                || Self::size_refused(placed, address, surface));
        let slot = placed
            .slots
            .get_mut(&address)
            .expect("BUG: the slot is still there");
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
        if stuck {
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

/// The environment variable that switches damage production off for every
/// window the process opens: `FLUI_DAMAGE=off` sends every frame as `Full`
/// and never renders a partial frame (ADR-0087 §3). It changes what the host
/// hands the executor and nothing else: the pipeline still retains its
/// repaint boundaries' contents, and the backend still renders into its
/// retained target when the surface needs an intermediate image. It is the
/// fallback for stale pixels in the field, read once per window, and stays
/// until an `AppConfig` switch carries the same choice per app.
const DAMAGE_ENV: &str = "FLUI_DAMAGE";

/// The damage mode [`DAMAGE_ENV`]'s value selects: `off` (any case) turns
/// damage off; unset, empty or `on` keeps the default; anything else keeps
/// the default and says so, since a typo must not silently disable the
/// fallback someone reached for.
fn damage_mode_from(value: Option<&str>) -> DamageMode {
    match value.map(str::trim) {
        None | Some("") => DamageMode::default(),
        Some(value) if value.eq_ignore_ascii_case("off") => DamageMode::Off,
        Some(value) if value.eq_ignore_ascii_case("on") => DamageMode::default(),
        Some(value) => {
            tracing::warn!(value, "{DAMAGE_ENV} takes `on` or `off`; damage stays on");
            DamageMode::default()
        }
    }
}

/// The damage mode the process's [`DAMAGE_ENV`] selects.
fn damage_mode() -> DamageMode {
    damage_mode_from(std::env::var(DAMAGE_ENV).ok().as_deref())
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

/// When the loop's raster host next has work of its own, for a window's wake
/// deadline: the executor's next rebuild of a lost device, or a window's next
/// attempt at a surface it is owed. `None` while the host is turning: the
/// turn that is running answers it.
pub(crate) fn raster_wake_at() -> Option<Instant> {
    super::host::APP_RUNTIME.with(|slot| {
        let state = slot.try_borrow().ok()?;
        state.raster_host.try_read(|host| {
            match (host.device_retry_at(), host.surface_retry_at()) {
                (Some(device), Some(surface)) => Some(device.min(surface)),
                (device, surface) => device.or(surface),
            }
        })?
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

    /// Tell the executor whether the window can be seen (`shown`): a slot
    /// that cannot is not drawn, and draws in full when it shows again. A host
    /// already turning is told on the next wake.
    pub(crate) fn show(&self, shown: bool) {
        let visibility = if shown {
            SlotVisibility::Shown
        } else {
            SlotVisibility::Hidden
        };
        let address = self.address;
        let _told = self
            .host
            .try_with(|host| host.set_visibility(address, visibility));
    }

    /// Whether the application asked, through `ui_runtime`, for a rebuild of
    /// the device the executor gave up on: the runtime no longer reports the
    /// abandonment this window last told it of
    /// (`UiRuntime::retry_render_device`).
    fn device_retry_asked(&self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        matches!(self.device, DeviceHealth::GivenUp { .. })
            && !ui_runtime
                .primary_device_recovery()
                .is_some_and(flui_runtime::frame_failure::DeviceRecovery::is_abandoned)
    }

    /// Run the window's frame at `now` through its slot and answer whether it
    /// presented.
    ///
    /// Before the frame: a rebuild of a given-up device the application
    /// asked for, the colour of the display the window is on (`display`; a
    /// changed profile sets the surface up again, ADR-0256 §6), and a turn of
    /// the executor, which rebuilds a lost device that is due even when the
    /// frame has nothing new to draw. A device that came back or a display
    /// that changed repaints the window in full. After it: the frame's GPU
    /// panic goes to the application's frame-failure handler, and a device
    /// the frame lost is reported with the frames it drops; its rebuild is
    /// due at the executor's deadline, which [`raster_wake_at`] reads.
    pub(crate) fn pump(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
        now: Instant,
        display: Option<DisplayColor>,
    ) -> bool {
        let address = self.address;
        let retry = self.device_retry_asked(ui_runtime);
        // A host already turning is asked again on the next wake: the
        // device stays given up and the runtime's request stands.
        let display_changed = self
            .host
            .try_with(|host| {
                if retry {
                    host.retry_device();
                }
                let changed = host.follow_display(address, display);
                host.service(address);
                changed
            })
            .unwrap_or(false);
        if display_changed | self.settle_device(ui_runtime) {
            ui_runtime.mark_primary_needs_full_repaint();
        }
        let mut sink = self.frame();
        let presented = ui_runtime
            .pump(&mut SampledClock(now), &mut sink)
            .presented();
        self.report_frame_panic(ui_runtime);
        let _ = self.settle_device(ui_runtime);
        presented
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

#[cfg(all(test, feature = "testing"))]
mod tests;
