//! Attaching, placing and detaching windows' slots on the executor, and the
//! executor's retirement.

use std::rc::Rc;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use super::bounded_wait::{PIPELINE_CACHE_LOAD_BOUND, wait_within};
use super::shared_host::resume_unless_unwinding;
#[cfg(not(target_arch = "wasm32"))]
use super::thread_link::{DetachWait, ThreadLink};
use super::{
    Executor, HostSlot, Placed, RasterFactory, RasterHost, Redraws, RestorePace, WindowHandles,
    damage_mode, in_flight_limit, refused, send_refusal,
};

use flui_engine::{
    EngineError, RasterBackend, RasterCommand, RasterExecutor, RasterPlacement, RasterSendError,
    SlotConfig, SlotFailure, SlotTarget, SlotVisibility,
};
use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
use flui_layer::RasterBase;
use flui_platform::{DisplayColor, PlatformWindow, RedrawRequester, SurfaceTarget};

impl RasterHost {
    /// A host whose executor will be placed on `factory`'s backend, turned
    /// in `placement`.
    pub(crate) fn new(factory: RasterFactory, placement: RasterPlacement) -> Self {
        Self {
            factory,
            placement,
            placed: None,
            carried: rustc_hash::FxHashMap::default(),
            #[cfg(not(target_arch = "wasm32"))]
            detaching: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            death_wake: None,
            #[cfg(not(target_arch = "wasm32"))]
            link: Rc::default(),
        }
    }

    /// Give `window`, presenting `address`, a slot: unconfigured until its
    /// first size. Places the executor if this is the first window.
    ///
    /// # Errors
    /// The window's surface cannot be made, no GPU gives a device for the
    /// first window, or the executor refuses the slot.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the web attaches on a context it awaited, with attach_bootstrapped"
        )
    )]
    pub(crate) fn attach(
        &mut self,
        window: &dyn PlatformWindow,
        address: PresentationAddress,
        config: &crate::app::AppConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        let surface_target = self.surface_target(window);
        // Every window's requester is listed: the raster thread's wake asks
        // for frames through it, and so does a device request's answer on
        // every placement (ADR-0305 §8).
        let redraw = Some(window.redraw_requester());
        self.attach_target(surface_target, redraw, address, config, textures, display)
    }

    /// Give the window whose surfaces are made over `surface_target` a slot,
    /// as [`attach`](Self::attach) does.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the web attaches on a context it awaited, with attach_bootstrapped"
        )
    )]
    pub(super) fn attach_target(
        &mut self,
        surface_target: Arc<dyn SurfaceTarget>,
        redraw: Option<Arc<dyn RedrawRequester>>,
        address: PresentationAddress,
        config: &crate::app::AppConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        let slot_config = SlotConfig::new()
            .color(config.surface_color, display)
            .latency(config.frame_latency);
        self.attach_on(
            WindowHandles {
                surface_target,
                redraw,
            },
            address,
            slot_config,
            textures,
            display,
            None,
        )
    }

    /// Give `window`, presenting `address`, the first slot of an executor
    /// placed on `bootstrap`, a GPU context awaited over the window: the web
    /// awaits its device, which the owner thread cannot block on (ADR-0305
    /// §2).
    ///
    /// # Errors
    /// The executor is placed already, or refuses the slot.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(
            dead_code,
            reason = "only the web awaits its GPU context; a native runner blocks on it in `attach`"
        )
    )]
    pub(crate) fn attach_bootstrapped(
        &mut self,
        bootstrap: flui_engine::GpuBootstrap,
        window: &dyn PlatformWindow,
        address: PresentationAddress,
        slot_config: SlotConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
    ) -> Result<(), EngineError> {
        if self.placed.is_some() {
            return Err(refused("the raster executor is placed already"));
        }
        let surface_target = self.surface_target(window);
        let attached = self.attach_on(
            WindowHandles {
                surface_target,
                redraw: None,
            },
            address,
            slot_config,
            textures,
            display,
            Some(bootstrap),
        );
        // An executor placed for a slot it refused serves no window: it goes,
        // so the next context awaited over the window is placed instead.
        if attached.is_err()
            && self
                .placed
                .as_ref()
                .is_some_and(|placed| placed.slots.is_empty())
        {
            self.retire_executor();
        }
        attached
    }

    /// Give the window whose surfaces are made over `surface_target` a slot
    /// configured by `slot_config`, placing the executor on `bootstrap`, or
    /// on a context bootstrapped now, if this is the first window.
    fn attach_on(
        &mut self,
        window: WindowHandles,
        address: PresentationAddress,
        slot_config: SlotConfig,
        textures: Option<flui_layer::TextureRegistry>,
        display: Option<DisplayColor>,
        bootstrap: Option<flui_engine::GpuBootstrap>,
    ) -> Result<(), EngineError> {
        let WindowHandles {
            surface_target,
            redraw,
        } = window;
        // The target's word, read before anything is placed or attached: it
        // is the platform's code.
        let limit = in_flight_limit(self.placement, surface_target.frame_pacing());
        let target = if let Some(placed) = self.placed.as_ref() {
            SlotTarget::Window(placed.surfaces.create(Arc::clone(&surface_target))?)
        } else {
            let (placed, first) = self.place(Arc::clone(&surface_target), bootstrap)?;
            self.placed = Some(placed);
            first
        };
        let placed = self
            .placed
            .as_mut()
            .expect("BUG: the executor was placed above");
        let slot = placed.sender.mint_slot()?;
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
        // A raster thread attaches on its own turn: the owner waits for its
        // answer, so a window the thread refuses is refused here as it is
        // inline. A thread that dies or does not answer within the deadline
        // (wedged, ADR-0305 §9) leaves the executor dead. The window is not
        // refused: its surface may still be in the thread's hands or in its
        // lifecycle queue, which lives as long as the host, and the native
        // window must outlive it. The answer decides, not a second read of
        // the record, which a thread that answers just after the deadline
        // changes.
        #[cfg(not(target_arch = "wasm32"))]
        let unanswered = match &placed.executor {
            Executor::Threaded(handle) => {
                match handle.await_attach(&placed.sender, slot, flui_engine::DETACH_DEADLINE) {
                    Ok(()) => false,
                    Err(failure) => {
                        let _logged = caught.contain(|| {
                            tracing::error!(%failure, "raster host: the raster thread died or did not answer a window's attach; the window is kept and draws nothing");
                        });
                        true
                    }
                }
            }
            Executor::Inline(_) => false,
        };
        #[cfg(target_arch = "wasm32")]
        let unanswered = false;
        let refusal = match placed.sender.record(slot).map(|record| record.phase) {
            _ if unanswered => None,
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
        let carried = self.carried.remove(&address).unwrap_or_default();
        let mut base = RasterBase::default();
        base.set_mode(damage_mode());
        #[cfg(not(target_arch = "wasm32"))]
        let link_target = Arc::clone(&surface_target);
        placed.slots.insert(
            address,
            HostSlot {
                slot,
                surface_target,
                base,
                textures,
                carried,
                display,
                visibility: SlotVisibility::Shown,
                wanted: None,
                stale_panic: None,
                available: true,
                restore: RestorePace::default(),
                panic: caught,
                redraw,
                in_flight_limit: limit,
                pending: Vec::with_capacity(self.placement.depth().get()),
                painters: placed.sender.painter_resets(),
            },
        );
        placed.redraws.follow(&placed.slots);
        #[cfg(not(target_arch = "wasm32"))]
        if placed.executor.inline().is_none() {
            let _replaced = self
                .link
                .borrow_mut()
                .slots
                .insert(address, (slot, link_target));
        }
        // A death the attach found wakes every window, this one included,
        // once the host is released: each wake quarantines its window.
        #[cfg(not(target_arch = "wasm32"))]
        if unanswered {
            self.death_wake = Some(placed.redraws.clone());
        }
        Ok(())
    }

    /// The executor over the first window, whose surfaces are made over
    /// `first`, and that window's target: on `bootstrap` when the owner
    /// awaited one over `first`, otherwise on a context bootstrapped now.
    fn place(
        &self,
        first: Arc<dyn SurfaceTarget>,
        bootstrap: Option<flui_engine::GpuBootstrap>,
    ) -> Result<(Placed, SlotTarget), EngineError> {
        let (backend, surfaces, target): (Box<dyn RasterBackend + Send>, _, _) = match &self.factory
        {
            RasterFactory::Gpu => {
                let bootstrap = match bootstrap {
                    Some(bootstrap) => bootstrap,
                    None => Self::bootstrap_now(first)?,
                };
                let surface = bootstrap.first.ok_or_else(|| {
                    refused("the GPU context answered without the first window's surface")
                })?;
                (
                    Box::new(bootstrap.context),
                    bootstrap.factory,
                    SlotTarget::Window(surface),
                )
            }
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
            ))]
            RasterFactory::StandIn(renderers) => {
                let surfaces = flui_engine::testing::noop_surface_factory(Arc::new(
                    flui_engine::testing::FakeDisplay,
                ));
                let target = SlotTarget::Window(surfaces.create(first)?);
                (Box::new(renderers.backend()), surfaces, target)
            }
            #[cfg(all(test, feature = "testing"))]
            RasterFactory::Scripted(slots) => {
                let surfaces = flui_engine::testing::noop_surface_factory(Arc::new(
                    flui_engine::testing::FakeDisplay,
                ));
                let target = SlotTarget::Window(surfaces.create(first)?);
                (Box::new(slots.clone()), surfaces, target)
            }
        };
        let redraws = Redraws::default();
        let (executor, sender) = match self.placement {
            RasterPlacement::Inline => {
                // The owner drains a slot's settlements right after its turn,
                // and reads the device's rebuild deadline for its wake.
                let clock: Rc<dyn flui_foundation::MonotonicClock> =
                    Rc::new(flui_foundation::SystemClock);
                let backend: Box<dyn RasterBackend> = backend;
                // A lost device's replacement is asked for on a worker, which
                // asks every attached window for a frame: the frame gate
                // counts the request's deadline, so that frame's turn installs
                // the device (ADR-0305 §8).
                let woken = redraws.clone();
                let (executor, sender) =
                    RasterExecutor::new(backend, clock, || {}, move || woken.wake());
                (Executor::Inline(executor), sender)
            }
            #[cfg(not(target_arch = "wasm32"))]
            RasterPlacement::Threaded => {
                // A settlement, a device's health or a failure wakes every
                // attached window; each window's wake drains its own slot.
                let woken = redraws.clone();
                // The owner waits for the thread's answers (an attach, a
                // detach, the stop) through its platform's wait, which on
                // Win32 serves the messages the thread's DXGI calls send to
                // the windows meanwhile (ADR-0305 §9).
                let owner_wait =
                    super::super::host::with_owner_platform(flui_native::OwnerPlatform::owner_wait)
                        .flatten();
                let (handle, sender) = RasterExecutor::spawn(
                    backend,
                    Arc::new(flui_foundation::SystemClock),
                    move || woken.wake(),
                    owner_wait,
                )
                .map_err(EngineError::surface_creation)?;
                let handle = Rc::new(handle);
                *self.link.borrow_mut() = ThreadLink {
                    thread: Some((Rc::downgrade(&handle), sender.clone(), redraws.clone())),
                    slots: rustc_hash::FxHashMap::default(),
                };
                (Executor::Threaded(handle), sender)
            }
            #[cfg(target_arch = "wasm32")]
            RasterPlacement::Threaded => {
                return Err(refused("the web has no raster thread"));
            }
        };
        Ok((Placed::new(executor, sender, surfaces, redraws), target))
    }

    /// The application's GPU context over `first`, the first window,
    /// bootstrapped on the owner thread, which waits for it, its pipeline
    /// cache loaded from the host's storage before any window attaches, so
    /// a driver without a disk cache of its own does not compile every
    /// pipeline again at each launch.
    #[cfg(not(target_arch = "wasm32"))]
    fn bootstrap_now(
        first: Arc<dyn SurfaceTarget>,
    ) -> Result<flui_engine::GpuBootstrap, EngineError> {
        let display =
            super::super::host::with_owner_platform(flui_native::OwnerPlatform::display_connection)
                .ok_or_else(|| refused("no platform owns this thread"))?;
        let mut bootstrap =
            pollster::block_on(flui_engine::GpuContext::bootstrap(display, Some(first)))?;
        if let Some(storage) = super::super::host::host_storage() {
            // The read runs on the application's IO pool, which may be one
            // the owner thread itself drives: past the bound the context
            // starts with a cold cache rather than wait for a read that
            // cannot come, and its warm-up still saves for the next launch.
            let loaded = wait_within(
                bootstrap.context.load_pipeline_cache(Arc::clone(&storage)),
                PIPELINE_CACHE_LOAD_BOUND,
            );
            if loaded.is_none() {
                tracing::warn!(
                    target: "flui.gpu",
                    event = "pipeline_cache_load_timed_out",
                    "the pipeline cache was not read in time; the GPU context starts it cold"
                );
                bootstrap.context.start_pipeline_cache_cold(storage);
            }
        }
        Ok(bootstrap)
    }

    /// The web cannot wait on its owner thread: its runner awaits the
    /// context and attaches with [`attach_bootstrapped`](Self::attach_bootstrapped).
    #[cfg(target_arch = "wasm32")]
    fn bootstrap_now(
        _first: Arc<dyn SurfaceTarget>,
    ) -> Result<flui_engine::GpuBootstrap, EngineError> {
        Err(refused(
            "the web awaits its GPU context before its first window attaches",
        ))
    }

    /// Detach every window's slot and drop the executor with its GPU
    /// context: the web's context was lost, and the next one is bootstrapped
    /// over the canvas again (ADR-0305 §8). A panic of the slots' detach or
    /// of the context's `Drop` is resumed once the executor is gone.
    #[cfg_attr(
        not(any(target_arch = "wasm32", all(test, feature = "testing"))),
        expect(
            dead_code,
            reason = "only the web bootstraps its context again; a native device is rebuilt in place"
        )
    )]
    pub(crate) fn retire_executor(&mut self) {
        let mut caught = FirstPanic::new();
        let addresses: Vec<PresentationAddress> = self
            .placed
            .as_ref()
            .map_or_else(Vec::new, |placed| placed.slots.keys().copied().collect());
        if let Some(placed) = self.placed.as_mut() {
            // A frame still waiting in a slot's inbox hands its texture
            // frames to the backend in this turn, so they are kept below.
            let _turned = caught.contain(|| placed.executor.turn());
            // The registry hands a frame over once, so the images the context
            // kept are what the next one draws until their producers submit
            // again; images carried here that no frame took yet are newer.
            for (address, slot) in &mut placed.slots {
                // A raster thread's backend is its own: only the web retires
                // an executor, and the web turns inline.
                let mut kept = placed
                    .executor
                    .inline()
                    .map_or_else(flui_layer::TextureUpdates::default, |executor| {
                        executor.backend().kept_textures(slot.slot)
                    });
                kept.merge(std::mem::take(&mut slot.carried));
                self.carried.insert(*address, kept);
            }
        }
        for address in addresses {
            self.detach_held(address, &mut caught);
        }
        #[cfg(not(target_arch = "wasm32"))]
        drop(std::mem::take(&mut *self.link.borrow_mut()));
        let retired = self.placed.take();
        caught.contain(|| drop(retired));
        resume_unless_unwinding(caught);
    }

    /// Whether `address`'s window has a slot.
    #[cfg_attr(
        not(any(target_arch = "wasm32", all(test, feature = "testing"))),
        expect(
            dead_code,
            reason = "only the web attaches after its context arrives and asks whether it has"
        )
    )]
    pub(crate) fn has_slot(&self, address: PresentationAddress) -> bool {
        self.placed
            .as_ref()
            .is_some_and(|placed| placed.slots.contains_key(&address))
    }

    /// What `window`'s surfaces are made over: the window itself, or for the
    /// stand-in, which never presents, a fake window, after naming the
    /// platform window so the test's handle finds the slot.
    fn surface_target(&self, window: &dyn PlatformWindow) -> Arc<dyn SurfaceTarget> {
        match &self.factory {
            RasterFactory::Gpu => window.surface_target(),
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
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
    pub(super) fn detach_held(&mut self, address: PresentationAddress, caught: &mut FirstPanic) {
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
        // A slot the busy path's wait already took down answers stale; any
        // other stale answer is a fault the log keeps.
        #[cfg(not(target_arch = "wasm32"))]
        let taken_down = matches!(placed.executor, Executor::Threaded(_))
            && self
                .link
                .try_borrow()
                .is_ok_and(|link| !link.slots.contains_key(&address));
        #[cfg(target_arch = "wasm32")]
        let taken_down = false;
        match placed
            .sender
            .send(RasterCommand::Detach { slot: slot.slot })
        {
            Ok(_) => {}
            Err(RasterSendError::StaleSlot(_)) if taken_down => {}
            Err(error) => {
                tracing::warn!(%error, "raster host: a window's detach was refused");
            }
        }
        placed.redraws.follow(&placed.slots);
        #[cfg(not(target_arch = "wasm32"))]
        let _forgotten = self.link.borrow_mut().slots.remove(&address);
        // A raster thread takes the slot down on its own turn; the window's
        // surface is given back once it did, which the shared host waits for
        // with its borrow released (ADR-0305 §9).
        #[cfg(not(target_arch = "wasm32"))]
        if let Executor::Threaded(handle) = &placed.executor {
            self.detaching.push(DetachWait {
                handle: Rc::clone(handle),
                sender: placed.sender.clone(),
                slot: slot.slot,
                redraws: placed.redraws.clone(),
                target: Arc::clone(&slot.surface_target),
            });
        }
        let _turned = caught.contain(|| placed.executor.turn());
    }

    /// Trim the GPU memory behind every slot, holding a panic of the turn in
    /// `caught`.
    pub(super) fn reclaim_held(
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
        // A window that closes before its next slot is attached draws nothing.
        drop(self.carried.remove(&address));
        let mut caught = FirstPanic::new();
        self.detach_held(address, &mut caught);
        // Reached from a window's `Drop`, which may run during an unwind.
        resume_unless_unwinding(caught);
    }

    /// Trim the GPU memory behind every slot.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "no browser reports memory pressure to the page")
    )]
    pub(crate) fn reclaim(&mut self, level: flui_foundation::memory::MemoryPressureLevel) {
        let mut caught = FirstPanic::new();
        self.reclaim_held(level, &mut caught);
        // Reached from a window's `Drop`, which may run during an unwind.
        resume_unless_unwinding(caught);
    }
}
