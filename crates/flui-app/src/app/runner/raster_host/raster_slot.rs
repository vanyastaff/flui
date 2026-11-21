//! A window's frame driver's view of its slot: the sink of one frame, the
//! thread's settlements and the device's health reported to its UI runtime.

use std::num::NonZeroUsize;

use web_time::Instant;

use super::BeforeFrame;

use super::shared_host::SharedRasterHost;

use flui_engine::{DeviceHealth, RasterPlacement, SlotVisibility};
use flui_foundation::panic::FirstPanic;
use flui_foundation::{FrameEpoch, PresentationAddress};
use flui_layer::ScenePublication;
use flui_platform::DisplayColor;
use flui_runtime::pump::SampledClock;
use flui_runtime::sink::{FrameSink, SubmitVerdict};

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

    fn in_flight_limit(&mut self) -> NonZeroUsize {
        // The slot's limit, fixed at its attach (ADR-0305 §5). A frame
        // dispatched while the host is turning reads the least: its submit
        // is answered `Retry`, so the host holds nothing of it.
        let address = self.address;
        self.host
            .try_read(|host| host.in_flight_limit(address))
            .unwrap_or(NonZeroUsize::MIN)
    }

    fn submit(&mut self, epoch: FrameEpoch, publication: ScenePublication) -> SubmitVerdict {
        // A frame dispatched while the host is turning (a platform callback
        // re-entering from the GPU work) keeps its work for the next wake.
        self.host
            .try_with(|host| host.submit(self.address, self.surface, epoch, publication))
            .unwrap_or(SubmitVerdict::Retry)
    }

    fn take_gpu_times(&mut self, into: &mut Vec<flui_foundation::GpuFrameTime>) {
        // A host that is turning keeps them: the executor holds the times
        // until a later frame of the window takes them.
        let address = self.address;
        let _taken = self
            .host
            .try_read(|host| host.drain_gpu_times(address, into));
    }
}

/// What a window's frames a raster thread finished became, as
/// [`RasterSlot::settle`] answers it.
#[cfg_attr(
    all(
        any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
        not(all(test, feature = "testing"))
    ),
    expect(dead_code, reason = "only the desktop loop turns a raster thread")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadSettled {
    /// No frame of the window settled.
    Nothing,
    /// One presented: the first reveals the window, and the pacing fallback
    /// stands down.
    Presented,
    /// Frames settled and none presented (refused, retried, failed, not
    /// shown): the wake counts toward the first reveal's fallback as a wake
    /// whose own frame presented nothing does.
    NotPresented,
}

impl ThreadSettled {
    /// Fold in one more of the window's frames, `shown` when it presented.
    fn and(self, shown: bool) -> Self {
        if shown || self == Self::Presented {
            Self::Presented
        } else {
            Self::NotPresented
        }
    }
}

/// One window's slot as its frame driver holds it: the host, the window's
/// address on it, the size the platform last announced, and the device's
/// health as the window last reported it to its UI runtime, and whether it
/// reported the executor dead.
pub(crate) struct RasterSlot {
    pub(crate) host: SharedRasterHost,
    pub(crate) address: PresentationAddress,
    pub(crate) size: (u32, u32),
    pub(crate) device: DeviceHealth,
    /// Whether the window told its UI runtime that the executor died
    /// (ADR-0305 §9): terminal, so no device health is reported after it
    /// and no rebuild the application asks for reaches the executor.
    pub(crate) renderer_dead: bool,
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

    /// Hand `ui_runtime` what a raster thread settled of the window's frames
    /// (`UiRuntime::settle_frame`), with no borrow of the host held. Run
    /// first in every wake of the window, whatever the wake goes on to do,
    /// so a frame the thread finished frees its place before the wake's gate
    /// reads it. Answers what the window's frames became, for its pacing and
    /// its first reveal, and the first panic of the code the settlements
    /// ran, which the window's wake resumes once the rest of it ran: it is
    /// this window's, and no other window's wake reports it. An inline
    /// executor leaves nothing to settle here.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn settle(
        &mut self,
        ui_runtime: &mut crate::app::ui_runtime::UiRuntime,
    ) -> (ThreadSettled, FirstPanic) {
        let mut caught = FirstPanic::new();
        if self.host.placement == RasterPlacement::Inline {
            return (ThreadSettled::Nothing, caught);
        }
        let address = self.address;
        let mut drained = self
            .host
            .settled
            .try_borrow_mut()
            .map_or_else(|_| Vec::new(), |mut scratch| std::mem::take(&mut *scratch));
        drained.clear();
        let _drained = self
            .host
            .try_with(|host| host.drain_settled(address, &mut drained));
        // Every settlement reaches the runtime whatever an earlier one ran
        // into: a frame left unsettled holds its place for good.
        let mut settled = ThreadSettled::Nothing;
        for settlement in drained.drain(..) {
            let shown = settlement.verdict == SubmitVerdict::Presented;
            let answer = caught.contain(|| ui_runtime.settle_frame(settlement));
            // A settlement the runtime held no frame for is another life's.
            // One whose settling panicked is still this window's frame, and
            // a presented one is on the window's screen: it reveals the
            // window while its panic resumes at the end of the wake.
            if matches!(answer, Some(flui_runtime::sink::Settled::Stale)) {
                continue;
            }
            settled = settled.and(shown);
        }
        self.give_back(drained);
        // After the settlements, which commit what a dead executor never
        // will: the runtime learns of the death once.
        let _dead = caught.contain(|| self.settle_death(ui_runtime));
        (settled, caught)
    }

    /// Keep `scratch`'s room for the next wake's settlements.
    #[cfg_attr(
        all(
            any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
            not(all(test, feature = "testing"))
        ),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    fn give_back(&self, scratch: Vec<flui_runtime::sink::FrameSettlement>) {
        if let Ok(mut kept) = self.host.settled.try_borrow_mut() {
            *kept = scratch;
        }
    }

    /// Whether a raster thread holds frames of the window it has not
    /// settled: the window's pacing then waits for their settlement, which
    /// wakes it, instead of arming its fallback.
    #[cfg_attr(
        any(target_os = "android", target_os = "ios", target_arch = "wasm32"),
        expect(dead_code, reason = "only the desktop loop turns a raster thread")
    )]
    pub(crate) fn has_pending(&self) -> bool {
        let address = self.address;
        self.host
            .try_read(|host| host.has_pending(address))
            .unwrap_or(false)
    }

    /// The deadline this window's own frame serves, for its wake's dirty
    /// gate: [`RasterHost::window_deadline`], which the loop also wakes for
    /// while the window's frames run.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn frame_deadline(&self) -> Option<Instant> {
        let address = self.address;
        self.host
            .try_read(|host| host.window_deadline(address))
            .flatten()
    }

    /// Tell the executor whether the window can be seen (`shown`): a slot
    /// that cannot is not drawn, and draws in full when it shows again. A host
    /// already turning is told on the next wake.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "a hidden tab runs no frames, so the page's slot stays shown"
        )
    )]
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

    /// Whether the window's executor can draw a lent scene: one that turns
    /// inline, which draws it within the call. A raster thread cannot.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn lends_scenes(&self) -> bool {
        self.host.placement == RasterPlacement::Inline
    }

    /// Show the window and draw `scene`, lent for this call alone, into it
    /// at the size the platform last announced
    /// ([`RasterHost::render_lent`]): the development reload plugin's scene.
    /// Answers whether it was shown; `false` while the host is turning.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn render_lent(&self, scene: &flui_layer::Scene, reset_fonts: bool) -> bool {
        // A window back from the background draws the plugin's scene first.
        self.show(true);
        let (address, size) = (self.address, self.size);
        self.host
            .try_with(|host| host.render_lent(address, size, scene, reset_fonts))
            .unwrap_or(false)
    }

    /// Whether the application asked, through `ui_runtime`, for a rebuild of
    /// the device the executor gave up on: the runtime no longer reports the
    /// abandonment this window last told it of
    /// (`UiRuntime::retry_render_device`). A dead executor takes no rebuild.
    pub(super) fn device_retry_asked(
        &self,
        ui_runtime: &crate::app::ui_runtime::UiRuntime,
    ) -> bool {
        !self.renderer_dead
            && matches!(self.device, DeviceHealth::GivenUp { .. })
            && !ui_runtime
                .primary_device_recovery()
                .is_some_and(flui_runtime::frame_failure::DeviceRecovery::is_abandoned)
    }

    /// Run the window's frame at `now` through its slot and answer whether it
    /// presented.
    ///
    /// Before the frame: a probe for a device to replace a given-up one,
    /// when the application asked for it (a probe that answers none tells
    /// the application the device is given up again), a surface the window is owed whose pace is due
    /// ([`RasterHost::restore_due`]), the colour of the display the window is
    /// on (`display`; a changed profile sets the surface up again, ADR-0256
    /// §6), and a turn of the executor, which asks for a lost device that is
    /// due, or installs one whose request answered, even when the frame has
    /// nothing new to draw. A device or a surface
    /// that came back, a display that changed, or a painter the executor let go (a device rebuilt, a render that panicked), repaints the window in
    /// full. After it: the frame's GPU
    /// panic goes to the application's frame-failure handler, and a device
    /// the frame lost is reported with the frames it drops; its next turn is
    /// due at the executor's deadline, which [`RasterHost::window_deadline`] reads.
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
        let before = self
            .host
            .try_with(|host| host.before_frame(address, now, display, retry));
        if let (
            Some(BeforeFrame {
                retry_failed: true, ..
            }),
            DeviceHealth::GivenUp { attempts },
        ) = (before, self.device)
        {
            // The request is answered: the device is given up again, and
            // the next frame probes nothing until the application asks.
            ui_runtime.note_primary_device_recovery(Some(
                flui_runtime::frame_failure::DeviceRecovery::Abandoned { attempts },
            ));
        }
        let repaint = before.is_some_and(|before| before.repaint);
        if repaint | self.settle_device(ui_runtime) {
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
        // next settle to see. The executor's failure is the whole executor's:
        // the first window to settle logs it.
        let Some((health, failure, executor_failure)) = self.host.try_read(|host| {
            let placed = host.placed.as_ref();
            (
                host.device_health(),
                placed.and_then(|placed| placed.sender.take_device_failure()),
                placed.and_then(|placed| placed.sender.take_executor_failure()),
            )
        }) else {
            return false;
        };
        // The death is told before any log line, whose subscriber is not ours.
        let dead = self.settle_death(ui_runtime);
        if let Some(error) = failure {
            tracing::warn!(%error, "GPU device rebuild failed");
        }
        if let Some(error) = executor_failure {
            tracing::error!(%error, "raster executor failed");
        }
        if dead {
            return false;
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

    /// Tell `ui_runtime` once that the executor died
    /// ([`DeviceRecovery::Dead`](flui_runtime::frame_failure::DeviceRecovery::Dead),
    /// ADR-0305 §9), and answer whether it is dead: each window learns it
    /// from the executor's health, which stays.
    fn settle_death(&mut self, ui_runtime: &crate::app::ui_runtime::UiRuntime) -> bool {
        if self.renderer_dead {
            return true;
        }
        let died = self
            .host
            .try_read(|host| {
                host.placed.as_ref().is_some_and(|placed| {
                    placed.sender.health() == flui_engine::ExecutorHealth::Died
                })
            })
            .unwrap_or(false);
        if died {
            self.renderer_dead = true;
            ui_runtime.note_primary_device_recovery(Some(
                flui_runtime::frame_failure::DeviceRecovery::Dead,
            ));
        }
        died
    }
}

impl Drop for RasterSlot {
    fn drop(&mut self) {
        self.host.detach(self.address);
    }
}
