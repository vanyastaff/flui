//! Turning the executor for one window's slot, the failures that turn leaves
//! and the device's health as the host reports it.

use super::shared_host::resume_unless_unwinding;
use super::{HostSlot, Placed, RasterHost, SlotTurn, device_retry};

use flui_engine::{
    DeviceHealth, PainterResets, RasterBackend, RasterCommand, RasterExecutor, RasterSendError,
    SendReceipt, SlotFailure,
};
use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;

impl RasterHost {
    /// Turn the executor for `address`'s window with no frame of its own: a
    /// lost device whose rebuild is due is asked for, and one whose request
    /// answered is installed, whether or not a window
    /// has anything new to draw. What the turn left for the window (a
    /// refusal, a panic) is settled now, so the frame's own turn finds its
    /// failure slot empty.
    pub(crate) fn service(&mut self, address: PresentationAddress) {
        if let Some(placed) = self.placed.as_mut() {
            let _settled = Self::turn_for(placed, address);
        }
    }

    /// Send `command` for `address`'s window; one the full lifecycle queue
    /// refused is sent again once a turn has made room, since a change the
    /// owner made (a release before the native window goes, a visibility, a
    /// device retry) is not asked for again. A panic of that turn is held in
    /// `caught`.
    pub(super) fn send_with_room(
        placed: &mut Placed,
        address: PresentationAddress,
        command: RasterCommand,
        caught: &mut FirstPanic,
    ) -> Result<SendReceipt, RasterSendError> {
        match placed.sender.send(command) {
            Err(RasterSendError::Full(command)) => {
                let _turned = caught.contain(|| Self::turn_for(placed, address));
                placed.sender.send(*command)
            }
            sent => sent,
        }
    }

    /// Turn the executor on `address`'s behalf and settle what the turn left
    /// for its slot: a refusal is logged, and a panic of the slot's commands
    /// or of the turn itself is held for the window's report after its
    /// frame. Answers whether the slot's own commands panicked. What the
    /// window does next (a new surface, another size, nothing until it
    /// changes) is read from the slot's record, not from the failure.
    pub(super) fn turn_for(placed: &mut Placed, address: PresentationAddress) -> SlotTurn {
        let mut caught = FirstPanic::new();
        let _turned = caught.contain(|| placed.executor.turn());
        // A raster thread's failures are taken with its settlements alone
        // (`drain_settled`), so the frame they came with is the one refused.
        let Some(slot) = placed
            .slots
            .get_mut(&address)
            .filter(|_| placed.executor.inline().is_some())
        else {
            caught.resume();
            return SlotTurn::Clean;
        };
        let failure = placed.sender.take_failure(slot.slot);
        // The turn's own panic (a retired scene's `Drop`, a trim, a device
        // rebuild) goes to the window that turned it, for its report. The
        // slot's own panic takes precedence over it, as before this host
        // took a thread's failures with their settlements: it is this
        // window's, while the turn's may be another window's work, and the
        // executor records no order between the two.
        Self::hold_slot_failure(slot, failure, caught, placed.sender.painter_resets())
    }

    /// Settle `failure`, taken from `slot`'s executor slot, with `turned`,
    /// the panic of the turn that left it: a panic of the slot's commands is
    /// held for the window's report in precedence over the turn's (not by time:
    /// the executor records no order between them), and a refusal
    /// is logged last. The log and the refusal's drop run contained, their
    /// panic held with the slot's (a subscriber is user code), so a caller
    /// that drained settlements with the failure still hands them on. `painters` is the
    /// executor's painter-reset count as the slot's failure was taken.
    pub(super) fn hold_slot_failure(
        slot: &mut HostSlot,
        failure: Option<SlotFailure>,
        turned: FirstPanic,
        painters: PainterResets,
    ) -> SlotTurn {
        let refusal = match failure {
            Some(SlotFailure::Panicked(payload)) => {
                // The slot's own panic settles its frame `Retry`, whose retry repaints in full
                // after the frame-failure backoff (ADR-0258 §6), so the painter resets counted
                // by now (`painters`) are that retry's to answer: read again before the next
                // frame, they would repaint at once, into the same panic.
                slot.painters = painters;
                slot.panic.record(payload);
                slot.panic.absorb(turned);
                return SlotTurn::Panicked;
            }
            refusal => refusal,
        };
        slot.panic.absorb(turned);
        match refusal {
            Some(SlotFailure::Refused(error)) => {
                let _logged = slot.panic.contain(move || {
                    tracing::warn!(%error, "raster host: the executor refused a window's command");
                    drop(error);
                });
            }
            Some(failure) => {
                let _logged = slot.panic.contain(move || {
                    tracing::warn!(?failure, "raster host: a window's slot failed");
                    drop(failure);
                });
            }
            None => {}
        }
        SlotTurn::Clean
    }

    /// Give the executor a device probed in fresh instances in place of one
    /// it gave up on (ADR-0305 §8, ADR-0267), on `address`'s behalf: a full
    /// lifecycle queue is turned for that window's slot and sent to again.
    pub(super) fn retry_device(&mut self, address: PresentationAddress) -> device_retry::Retry {
        let Some(placed) = self.placed.as_mut() else {
            return device_retry::Retry::NotGivenUp;
        };
        let mut caught = FirstPanic::new();
        let retry = device_retry::retry_given_up(&self.factory, placed, |placed, command| {
            Self::send_with_room(placed, address, command, &mut caught)
        });
        // The turn that made room ran under the window's frame: its panic is
        // the frame's to report, resumed here as any turn's is.
        resume_unless_unwinding(caught);
        retry
    }

    /// The first panic of `address`'s frames since the last one was taken.
    pub(super) fn take_frame_panic(
        &mut self,
        address: PresentationAddress,
    ) -> Option<Box<dyn std::any::Any + Send>> {
        let slot = self.placed.as_mut()?.slots.get_mut(&address)?;
        std::mem::replace(&mut slot.panic, FirstPanic::new()).into_payload()
    }

    /// Where the context's device stands.
    ///
    /// A backend that cannot rebuild where the executor runs (the web) is
    /// replaced by its owner, which retires the executor (ADR-0305 §8): the
    /// executor's give-up is then the rebuild the owner owes, not one the
    /// application must ask for, and a device the backend reports lost is
    /// lost before any turn has seen it.
    pub(crate) fn device_health(&self) -> DeviceHealth {
        let Some(placed) = self.placed.as_ref() else {
            return DeviceHealth::Healthy;
        };
        let health = placed.sender.device_health();
        // A raster thread's backend rebuilds where it runs: a native one.
        let Some(backend) = placed.executor.inline().map(RasterExecutor::backend) else {
            return health;
        };
        if backend.requests_devices() || !(health.is_lost() || backend.device_lost()) {
            return health;
        }
        DeviceHealth::Rebuilding { attempts: 0 }
    }

    /// When the executor must next turn for a lost device (its next rebuild,
    /// or its request's deadline), for the
    /// loop's wake;
    /// `None` once it died, since a dead executor rebuilds nothing.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "the browser's animation frames wake the page; no deadline does"
        )
    )]
    pub(crate) fn device_retry_at(&self) -> Option<web_time::Instant> {
        self.placed
            .as_ref()
            .filter(|_| !self.executor_died())
            .and_then(|placed| placed.sender.device_retry_at())
    }

    /// Whether the executor died (ADR-0305 §9): terminal for every window.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the web has no raster thread to die")
    )]
    pub(super) fn executor_died(&self) -> bool {
        self.placed
            .as_ref()
            .is_some_and(|placed| placed.sender.health() == flui_engine::ExecutorHealth::Died)
    }
}
