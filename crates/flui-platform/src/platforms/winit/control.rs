use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::{ClaimHandle, ClaimSlot, claim_slot};
use parking_lot::Mutex;

use crate::traits::{PlatformWindow, WindowId, WindowOptions, owner::OpenWindowError};

pub(super) const CONTROL_CAPACITY: usize = 256;

type WakeOwner = Arc<dyn Fn() + Send + Sync>;

/// The lane's reply payload: the fully resolved window handle, or a typed
/// failure — exactly `OwnerPlatform`/`PlatformProxy`'s `PendingWindow`
/// payload (ADR-0039 §3). The owner resolves `WindowId -> Arc<dyn
/// PlatformWindow>` before delivering, so no caller needs a second lookup.
pub(super) type OpenWindowResult = Result<Arc<dyn PlatformWindow>, OpenWindowError>;

pub(super) enum ControlCommand {
    OpenWindow {
        options: WindowOptions,
        /// The owner-side half of the claim-slot reply protocol
        /// (`flui-foundation`'s `ClaimSlot`, ADR-0039 §3). Replaces the
        /// buffered `sync_channel(1)` one-shot: a requester that abandons
        /// the request *after* the owner delivers no longer leaks the
        /// created window, because the abandonment transition is visible
        /// to the owner (see `WinitApp::sweep_settled_replies`) instead of
        /// riding a reply send that always "succeeds" whether or not
        /// anyone ever reads it.
        reply: ClaimSlot<OpenWindowResult>,
    },
}

impl std::fmt::Debug for ControlCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenWindow { options, .. } => f
                .debug_struct("ControlCommand::OpenWindow")
                .field("options", options)
                .finish_non_exhaustive(),
        }
    }
}

/// Failure to enqueue a request on the owner lane. Unchanged vocabulary
/// (registry evidence pin `runtime-contract.toml`): this is the *admission*
/// failure (lane full, or the owner is already gone at send time) — a
/// distinct, narrower concern from the claim-slot reply protocol, which
/// governs what happens after a request is admitted.
#[derive(Debug)]
pub(super) enum ControlSendError {
    Full {
        capacity: usize,
        rejected: WindowOptions,
    },
    OwnerGone {
        rejected: WindowOptions,
    },
}

#[derive(Clone)]
pub(super) struct ControlSender {
    commands: Sender<ControlCommand>,
    wake_owner: WakeOwner,
    wake_pending: Arc<AtomicBool>,
    quit_requested: Arc<AtomicBool>,
    /// Coalesced "re-consult the exit-policy hook" flag — the same
    /// bypass-the-queue shape as `quit_requested`, for the same reason: a
    /// keep-alive service completing must be able to end a lingering
    /// zero-window loop even if the command lane is saturated.
    exit_reevaluation_requested: Arc<AtomicBool>,
    /// Windows whose programmatic `PlatformWindow::close` is waiting for the
    /// owner to run the close teardown (issue #919), in request order, each
    /// at most once. Not a slot in the bounded command lane: a close is
    /// lifecycle traffic — losing one leaves a window that is hidden but
    /// still tracked, which is exactly the "no window visible, process
    /// never exits" defect — so it must never be rejected for lane
    /// capacity, and a burst of requests for one window must cost one
    /// teardown, not several. A `Vec` with a linear membership check, not a
    /// set type: the length is bounded by the number of open windows (a
    /// handful), and request order is part of the contract the owner's
    /// drain relies on.
    close_requested: Arc<Mutex<Vec<WindowId>>>,
    // Serializes the accepting check with the non-blocking enqueue. Shutdown
    // takes the same short gate before its final queue snapshot.
    admission: Arc<Mutex<bool>>,
}

impl std::fmt::Debug for ControlSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ControlSender")
            .field("capacity", &CONTROL_CAPACITY)
            .field("pending", &self.commands.len())
            .field("wake_pending", &self.wake_pending.load(Ordering::Relaxed))
            .field(
                "quit_requested",
                &self.quit_requested.load(Ordering::Relaxed),
            )
            .field(
                "exit_reevaluation_requested",
                &self.exit_reevaluation_requested.load(Ordering::Relaxed),
            )
            .field("close_requested", &*self.close_requested.lock())
            .finish_non_exhaustive()
    }
}

pub(super) struct ControlReceiver {
    commands: Receiver<ControlCommand>,
    wake_pending: Arc<AtomicBool>,
    quit_requested: Arc<AtomicBool>,
    exit_reevaluation_requested: Arc<AtomicBool>,
    close_requested: Arc<Mutex<Vec<WindowId>>>,
    admission: Arc<Mutex<bool>>,
    owner_affinity: PhantomData<Rc<()>>,
}

pub(super) fn control_lane(wake_owner: WakeOwner) -> (ControlSender, ControlReceiver) {
    let (commands, receiver) = bounded(CONTROL_CAPACITY);
    let wake_pending = Arc::new(AtomicBool::new(false));
    let quit_requested = Arc::new(AtomicBool::new(false));
    let exit_reevaluation_requested = Arc::new(AtomicBool::new(false));
    let close_requested = Arc::new(Mutex::new(Vec::new()));
    let admission = Arc::new(Mutex::new(true));

    (
        ControlSender {
            commands,
            wake_owner,
            wake_pending: Arc::clone(&wake_pending),
            quit_requested: Arc::clone(&quit_requested),
            exit_reevaluation_requested: Arc::clone(&exit_reevaluation_requested),
            close_requested: Arc::clone(&close_requested),
            admission: Arc::clone(&admission),
        },
        ControlReceiver {
            commands: receiver,
            wake_pending,
            quit_requested,
            exit_reevaluation_requested,
            close_requested,
            admission,
            owner_affinity: PhantomData,
        },
    )
}

impl ControlSender {
    pub(super) fn request_open_window(
        &self,
        options: WindowOptions,
    ) -> Result<ClaimHandle<OpenWindowResult>, ControlSendError> {
        self.request_open_window_after_admission(options, || {})
    }

    fn request_open_window_after_admission(
        &self,
        options: WindowOptions,
        after_admission: impl FnOnce(),
    ) -> Result<ClaimHandle<OpenWindowResult>, ControlSendError> {
        let admission = self.admission.lock();
        if !*admission {
            return Err(ControlSendError::OwnerGone { rejected: options });
        }

        // The claim slot's wake fires only on abandonment (never on
        // delivery/claim), reusing the exact same coalesced owner wake as a
        // successful enqueue -- an abandoned-before-drain request wakes the
        // owner promptly so it can skip creating a window nobody wants.
        //
        // Captures only `wake_pending` + `wake_owner` (both plain `Arc`s
        // unrelated to the channel), mirroring `Self::wake_owner`'s body
        // exactly, rather than cloning the whole `ControlSender` (which
        // holds `commands: Sender<ControlCommand>`). This command is about
        // to be enqueued into that very channel, so a wake closure that
        // captured a full `ControlSender` clone would put a `Sender`
        // reachable from inside its own queued message -- a message the
        // channel's `Arc`-backed internal state keeps alive until dequeued,
        // creating a self-reference cycle that a request left in the queue
        // forever (owner gone, never drained) would leak.
        let wake_pending_for_slot = Arc::clone(&self.wake_pending);
        let wake_owner_for_slot = Arc::clone(&self.wake_owner);
        let (slot, handle): (ClaimSlot<OpenWindowResult>, ClaimHandle<OpenWindowResult>) =
            claim_slot(Arc::new(move || {
                if !wake_pending_for_slot.swap(true, Ordering::AcqRel) {
                    (wake_owner_for_slot)();
                }
            }));
        let command = ControlCommand::OpenWindow {
            options,
            reply: slot,
        };
        after_admission();
        // `try_send` cannot block while the shutdown boundary waits for this
        // critical section. Once it returns, the command is either wholly
        // before the stop boundary or wholly rejected after it.
        let send_result = self.commands.try_send(command);
        drop(admission);

        match send_result {
            Ok(()) => {
                self.wake_owner();
                Ok(handle)
            }
            Err(TrySendError::Full(rejected)) => {
                // `handle`'s Drop fires an extra (harmless, self-correcting)
                // abandonment wake here: no owner ever saw this request, so
                // the wake is spurious but not incorrect -- the coalescing
                // flag this call itself never set stays consistent for the
                // next real enqueue.
                Err(ControlSendError::Full {
                    capacity: CONTROL_CAPACITY,
                    rejected: rejected.into_options(),
                })
            }
            Err(TrySendError::Disconnected(rejected)) => Err(ControlSendError::OwnerGone {
                rejected: rejected.into_options(),
            }),
        }
    }

    pub(super) fn request_quit(&self) {
        let should_wake = {
            let admission = self.admission.lock();
            *admission && !self.quit_requested.swap(true, Ordering::AcqRel)
        };
        if should_wake {
            self.wake_owner();
        }
    }

    /// Coalesced exit-policy re-evaluation request — `request_quit`'s
    /// twin, except the owner responds by re-CONSULTING the exit-policy
    /// hook (only if its window map is empty) rather than by exiting
    /// unconditionally. See `Platform::request_exit_policy_reevaluation`.
    pub(super) fn request_exit_reevaluation(&self) {
        let should_wake = {
            let admission = self.admission.lock();
            *admission
                && !self
                    .exit_reevaluation_requested
                    .swap(true, Ordering::AcqRel)
        };
        if should_wake {
            self.wake_owner();
        }
    }

    /// Asks the owner to run the close teardown for `window_id` — the
    /// programmatic [`PlatformWindow::close`] route (issue #919). The owner
    /// answers on its next turn with the SAME teardown a compositor close
    /// takes after its should-close veto (per-window close callback, map
    /// removal, cursor/drag cleanup, callback clear, exit-policy consult),
    /// minus the veto itself — a programmatic close is a decision already
    /// made, as on AppKit (`-close` never sends `windowShouldClose:`) and
    /// Win32 (`DestroyWindow` never sends `WM_CLOSE`).
    ///
    /// Callable from **any thread**. Rides the per-window list rather than
    /// the bounded command lane so it can never be rejected for capacity;
    /// a second request for a window the owner has not yet torn down
    /// coalesces (one teardown). A request after admission closed is
    /// dropped: the loop has already stopped, so no owner turn will ever
    /// run the teardown — that window's `on_close` does not fire, its
    /// callbacks clear when the embedder's last `Arc` drops
    /// (`WinitWindow::drop`), and the loop's own quit callback is the
    /// embedder's signal that everything is over. The owner drains closes
    /// posted BEFORE a quit ahead of the exit itself, so this is only ever
    /// a close issued after the loop is already gone.
    pub(super) fn request_close_window(&self, window_id: WindowId) {
        let should_wake = {
            let admission = self.admission.lock();
            if !*admission {
                tracing::debug!(
                    ?window_id,
                    "programmatic close after the event loop stopped; nothing left to tear down"
                );
                return;
            }
            let mut pending = self.close_requested.lock();
            if pending.contains(&window_id) {
                false
            } else {
                pending.push(window_id);
                true
            }
        };
        if should_wake {
            self.wake_owner();
        }
    }

    fn wake_owner(&self) {
        // Successful enqueue always happens before this release/coalescing
        // transition, so observing the wake implies work is already visible.
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            (self.wake_owner)();
        }
    }
}

impl ControlCommand {
    fn into_options(self) -> WindowOptions {
        match self {
            Self::OpenWindow { options, reply } => {
                // No owner ever saw this request (the enqueue itself
                // failed): the paired `ClaimHandle` (`request_open_window_
                // after_admission`'s local `handle`) already dropped when
                // that function returned `Err`, transitioning the slot
                // `Pending -> Abandoned(None)` and firing the abandonment
                // wake, before this `reply: ClaimSlot` is ever dropped here.
                // `ClaimSlot`'s own `Drop` (ADR-0039 §3 slice-2 amendment:
                // owner-disconnect -> `OwnerGone`) only fires on a slot
                // still `Pending`, so dropping `reply` here is a no-op --
                // the request is already resolved.
                drop(reply);
                options
            }
        }
    }
}

impl ControlReceiver {
    pub(super) fn begin_drain(&self) -> usize {
        // Clear before reading the bounded snapshot: a producer arriving
        // during or after the read must enqueue a fresh wake for the next
        // owner turn instead of being swallowed by this one.
        self.wake_pending.store(false, Ordering::Release);
        self.commands.len().min(CONTROL_CAPACITY)
    }

    pub(super) fn try_recv(&self) -> Option<ControlCommand> {
        self.commands.try_recv().ok()
    }

    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.commands.len()
    }

    pub(super) fn take_quit_requested(&self) -> bool {
        self.quit_requested.swap(false, Ordering::AcqRel)
    }

    /// Consumes one exit-policy re-evaluation transition, exactly once —
    /// same consume-on-read shape as [`Self::take_quit_requested`].
    pub(super) fn take_exit_reevaluation_requested(&self) -> bool {
        self.exit_reevaluation_requested
            .swap(false, Ordering::AcqRel)
    }

    /// Takes every window whose programmatic close is still waiting for
    /// the owner, in request order, exactly once — the set is emptied by
    /// the take, so a request arriving during the owner's teardown of this
    /// batch lands in the NEXT batch with its own wake.
    pub(super) fn take_close_requests(&self) -> Vec<WindowId> {
        std::mem::take(&mut *self.close_requested.lock())
    }

    pub(super) fn stop_accepting(&self) {
        *self.admission.lock() = false;
        // No producer can pass admission after the guarded false write, and
        // every producer that passed it completed `try_send` first.
        self.wake_pending.store(false, Ordering::Release);
    }
}

impl Drop for ControlReceiver {
    fn drop(&mut self) {
        *self.admission.lock() = false;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    use flui_foundation::ClaimOutcome;
    use flui_types::geometry::{Size, px};
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::{CONTROL_CAPACITY, ControlCommand, ControlSendError, control_lane};
    use crate::traits::{WindowOptions, owner::OpenWindowError};

    assert_impl_all!(super::ControlSender: Clone, Send, Sync);
    assert_not_impl_any!(super::ControlReceiver: Send, Sync);

    fn options(title: impl Into<String>) -> WindowOptions {
        WindowOptions {
            title: title.into(),
            size: Size::new(px(800.0), px(600.0)),
            ..WindowOptions::default()
        }
    }

    /// A tiny stand-in `PlatformWindow` so tests can build an
    /// `Arc<dyn PlatformWindow>` reply payload without depending on a real
    /// winit window.
    struct StubWindow;

    impl crate::traits::PlatformWindow for StubWindow {
        fn id(&self) -> crate::traits::WindowId {
            crate::traits::WindowId(1)
        }

        fn physical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
            flui_types::geometry::Size::default()
        }
        fn logical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::Pixels> {
            flui_types::geometry::Size::default()
        }
        fn scale_factor(&self) -> f64 {
            1.0
        }
        fn request_redraw(&self) {}
        fn is_focused(&self) -> bool {
            false
        }
        fn is_visible(&self) -> bool {
            true
        }
        fn set_cursor(
            &self,
            _cursor: cursor_icon::CursorIcon,
        ) -> Result<(), crate::traits::CursorError> {
            Ok(())
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn winit_control_enqueues_before_waking_the_owner() {
        let (wake_tx, wake_rx) = crossbeam_channel::bounded(0);
        let (owner_ack_tx, owner_ack_rx) = crossbeam_channel::bounded(0);
        let wake = Arc::new(move || {
            wake_tx.send(()).expect("owner wake receiver remains live");
            owner_ack_rx
                .recv()
                .expect("owner acknowledges after observing the queue");
        });
        let (sender, receiver) = control_lane(wake);

        let worker = thread::spawn(move || sender.request_open_window(options("ordered")));

        wake_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("successful enqueue wakes the owner");
        assert_eq!(receiver.begin_drain(), 1, "the command precedes its wake");
        let ControlCommand::OpenWindow { options, reply } =
            receiver.try_recv().expect("queued window request");
        assert_eq!(options.title, "ordered");
        owner_ack_tx
            .send(())
            .expect("release the sending worker after inspection");
        assert!(
            reply.deliver(Ok(Arc::new(StubWindow))).is_ok(),
            "slot is still Pending"
        );

        let mut handle = worker
            .join()
            .expect("sending worker does not panic")
            .expect("request is accepted");
        let window = handle
            .try_take()
            .expect("owner already delivered")
            .expect("window opens");
        assert_eq!(
            window.physical_size(),
            flui_types::geometry::Size::default()
        );
    }

    #[test]
    fn winit_control_cross_thread_request_is_processed_on_the_owner() {
        let owner_thread = thread::current().id();
        let (wake_tx, wake_rx) = crossbeam_channel::bounded(1);
        let wake = Arc::new(move || {
            let _ = wake_tx.try_send(());
        });
        let (sender, receiver) = control_lane(wake);

        let worker = thread::spawn(move || {
            let handle = sender
                .request_open_window(options("cross-thread"))
                .expect("owner lane accepts request");
            handle.wait()
        });

        wake_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker request wakes owner");
        assert_eq!(receiver.begin_drain(), 1);
        assert_eq!(thread::current().id(), owner_thread);
        let ControlCommand::OpenWindow { reply, .. } =
            receiver.try_recv().expect("owner receives request");
        assert!(
            reply.deliver(Ok(Arc::new(StubWindow))).is_ok(),
            "slot is still Pending"
        );
        let window = match worker.join().expect("worker exits") {
            ClaimOutcome::Delivered(result) => result.expect("window opens"),
            ClaimOutcome::AlreadyClaimed => {
                panic!("this handle is never polled by another caller before wait")
            }
            ClaimOutcome::OwnerGone => panic!("the owner never disconnects in this test"),
        };
        assert!(window.is_visible());
    }

    #[test]
    fn winit_control_drain_is_fifo_snapshot_and_rearms_for_nested_sends() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, receiver) = control_lane(wake);

        let _first_reply = sender
            .request_open_window(options("first"))
            .expect("first request");
        let _second_reply = sender
            .request_open_window(options("second"))
            .expect("second request");
        assert_eq!(wake_count.load(Ordering::Relaxed), 1, "burst coalesces");

        let drain_budget = receiver.begin_drain();
        assert_eq!(drain_budget, 2);
        let ControlCommand::OpenWindow { options: first, .. } =
            receiver.try_recv().expect("first command");
        assert_eq!(first.title, "first");

        let _nested_reply = sender
            .request_open_window(options("nested"))
            .expect("nested request");
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            2,
            "a send after drain entry re-arms the owner"
        );

        let ControlCommand::OpenWindow {
            options: second, ..
        } = receiver.try_recv().expect("second command");
        assert_eq!(second.title, "second");
        assert_eq!(
            receiver.pending_count(),
            1,
            "nested send is outside the pre-read drain snapshot"
        );

        assert_eq!(receiver.begin_drain(), 1);
        let ControlCommand::OpenWindow {
            options: nested, ..
        } = receiver.try_recv().expect("nested command");
        assert_eq!(nested.title, "nested");
    }

    #[test]
    fn winit_control_full_returns_original_options_without_an_extra_wake() {
        assert_eq!(CONTROL_CAPACITY, 256, "the owner lane has a fixed bound");
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, _receiver) = control_lane(wake);

        for index in 0..CONTROL_CAPACITY {
            let _reply = sender
                .request_open_window(options(format!("queued-{index}")))
                .expect("queue accepts exactly its capacity");
        }

        let error = sender
            .request_open_window(options("rejected"))
            .expect_err("the next request observes bounded backpressure");
        match error {
            ControlSendError::Full { capacity, rejected } => {
                assert_eq!(capacity, CONTROL_CAPACITY);
                assert_eq!(rejected.title, "rejected");
            }
            ControlSendError::OwnerGone { .. } => panic!("owner is still alive"),
        }
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "a rejected command's abandonment wake still coalesces onto the \
             already-pending flag from the queue-filling sends"
        );
    }

    #[test]
    fn winit_control_receiver_drop_returns_owner_gone_with_payload() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, receiver) = control_lane(wake);
        drop(receiver);

        let error = sender
            .request_open_window(options("orphan"))
            .expect_err("dropped owner refuses work");
        match error {
            ControlSendError::OwnerGone { rejected } => {
                assert_eq!(rejected.title, "orphan");
            }
            ControlSendError::Full { .. } => panic!("a dropped owner is not backpressure"),
        }
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            0,
            "an owner-gone rejection cannot wake an inert loop"
        );
    }

    #[test]
    fn winit_control_stop_linearizes_after_an_in_flight_admission() {
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        let (admitted_tx, admitted_rx) = crossbeam_channel::bounded(0);
        let (release_tx, release_rx) = crossbeam_channel::bounded(0);

        let worker = thread::spawn(move || {
            sender.request_open_window_after_admission(options("admitted"), || {
                admitted_tx
                    .send(())
                    .expect("owner observes the admission critical section");
                release_rx
                    .recv()
                    .expect("owner releases the paused admission");
            })
        });

        admitted_rx
            .recv()
            .expect("sender pauses after checking admission");
        release_tx
            .send(())
            .expect("sender can finish while still holding the admission gate");

        receiver.stop_accepting();
        let shutdown_budget = receiver.begin_drain();
        let handle = worker
            .join()
            .expect("admitting worker does not panic")
            .expect("the request linearized before shutdown");
        assert_eq!(
            shutdown_budget, 1,
            "shutdown snapshot contains every request admitted before the stop boundary"
        );

        let ControlCommand::OpenWindow { reply, .. } = receiver
            .try_recv()
            .expect("the admitted command remains available for rejection");
        assert!(
            reply
                .deliver(Err(OpenWindowError::OwnerGone { rejected: None }))
                .is_ok(),
            "slot is still Pending"
        );
        match handle.wait() {
            ClaimOutcome::Delivered(result) => assert!(result.is_err()),
            ClaimOutcome::AlreadyClaimed => {
                panic!("this handle is never polled by another caller before wait")
            }
            ClaimOutcome::OwnerGone => panic!("the owner never disconnects in this test"),
        }
    }

    #[test]
    fn winit_control_quit_is_nonstarvable_and_consumed_once_when_queue_is_full() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, receiver) = control_lane(wake);

        for index in 0..CONTROL_CAPACITY {
            let _reply = sender
                .request_open_window(options(format!("queued-{index}")))
                .expect("fill the bounded window lane");
        }
        sender.request_quit();

        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "the already-pending wake carries the independent quit flag"
        );
        assert!(
            receiver.take_quit_requested(),
            "quit bypasses queue capacity"
        );
        assert!(
            !receiver.take_quit_requested(),
            "the owner consumes one quit transition exactly once"
        );
    }

    /// `request_quit`'s twin flag: an exit-policy re-evaluation request
    /// bypasses queue capacity (a keep-alive service completing must be
    /// able to end a lingering zero-window loop even with the command lane
    /// saturated), coalesces a burst into one wake + one owner-visible
    /// transition, and is consumed exactly once.
    #[test]
    fn winit_control_exit_reevaluation_is_nonstarvable_coalesced_and_consumed_once() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, receiver) = control_lane(wake);

        for index in 0..CONTROL_CAPACITY {
            let _reply = sender
                .request_open_window(options(format!("queued-{index}")))
                .expect("fill the bounded window lane");
        }
        sender.request_exit_reevaluation();
        sender.request_exit_reevaluation();

        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "a burst of re-evaluation requests coalesces into the already-pending wake"
        );
        assert!(
            receiver.take_exit_reevaluation_requested(),
            "the re-evaluation flag bypasses queue capacity"
        );
        assert!(
            !receiver.take_exit_reevaluation_requested(),
            "the owner consumes one re-evaluation transition exactly once"
        );
        assert!(
            !receiver.take_quit_requested(),
            "a re-evaluation request must not masquerade as an unconditional quit"
        );
    }

    /// The programmatic-close request (issue #919) has the same
    /// non-starvable shape as the quit/re-evaluation flags — it must reach
    /// the owner even with the command lane full — but is keyed per
    /// window: a burst for ONE window coalesces into one wake and one
    /// owner-visible entry, distinct windows each keep theirs, and the take
    /// empties the set so nothing is torn down twice.
    #[test]
    fn winit_control_close_request_is_nonstarvable_coalesced_per_window_and_taken_once() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, receiver) = control_lane(wake);

        for index in 0..CONTROL_CAPACITY {
            let _reply = sender
                .request_open_window(options(format!("queued-{index}")))
                .expect("fill the bounded window lane");
        }
        let wakes_before = wake_count.load(Ordering::Relaxed);

        sender.request_close_window(crate::traits::WindowId(7));
        sender.request_close_window(crate::traits::WindowId(7));
        sender.request_close_window(crate::traits::WindowId(9));

        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            wakes_before,
            "close requests coalesce into the wake the saturated lane already has pending"
        );
        assert_eq!(
            receiver.take_close_requests(),
            vec![crate::traits::WindowId(7), crate::traits::WindowId(9)],
            "one entry per window, in request order, despite the full command lane"
        );
        assert!(
            receiver.take_close_requests().is_empty(),
            "the owner takes each batch exactly once"
        );
        assert!(
            !receiver.take_quit_requested(),
            "a close request must not masquerade as an unconditional quit"
        );

        // A request landing AFTER the take is a fresh batch with its own wake.
        receiver.begin_drain();
        sender.request_close_window(crate::traits::WindowId(7));
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            wakes_before + 1,
            "a close after the owner's drain boundary wakes the owner again"
        );
        assert_eq!(
            receiver.take_close_requests(),
            vec![crate::traits::WindowId(7)]
        );
    }

    /// Once admission closes (loop shutdown) a close request is dropped,
    /// not parked: the owner is exiting and its own teardown covers every
    /// window it still tracks, so a parked entry would only ever be a leak.
    #[test]
    fn winit_control_close_request_after_admission_closed_is_dropped() {
        let wake_count = Arc::new(AtomicUsize::new(0));
        let wake_count_for_callback = Arc::clone(&wake_count);
        let wake = Arc::new(move || {
            wake_count_for_callback.fetch_add(1, Ordering::Relaxed);
        });
        let (sender, receiver) = control_lane(wake);
        receiver.stop_accepting();

        sender.request_close_window(crate::traits::WindowId(3));

        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            0,
            "no wake after shutdown"
        );
        assert!(
            receiver.take_close_requests().is_empty(),
            "nothing is parked past admission"
        );
    }

    #[test]
    fn winit_control_dropped_handle_before_delivery_abandons_the_slot() {
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        let handle = sender
            .request_open_window(options("abandon-me"))
            .expect("request is admitted");
        drop(handle);

        assert_eq!(receiver.begin_drain(), 1);
        let ControlCommand::OpenWindow { reply, .. } =
            receiver.try_recv().expect("owner dequeues the request");
        assert!(
            reply.is_abandoned(),
            "the owner must see the abandonment before creating anything"
        );
    }

    #[test]
    fn winit_control_dropped_handle_after_delivery_is_reclaimable() {
        let (sender, receiver) = control_lane(Arc::new(|| {}));
        let handle = sender
            .request_open_window(options("late-abandon"))
            .expect("request is admitted");

        assert_eq!(receiver.begin_drain(), 1);
        let ControlCommand::OpenWindow { reply, .. } =
            receiver.try_recv().expect("owner dequeues the request");
        assert!(
            reply.deliver(Ok(Arc::new(StubWindow))).is_ok(),
            "slot is still Pending"
        );

        drop(handle); // claimed delivery, never read -- late abandonment
        let reclaimed = reply
            .take_abandoned()
            .expect("the owner must be able to reclaim the orphaned window")
            .expect("a successful delivery was abandoned, not a Backend error");
        assert!(reclaimed.is_visible());
    }
}
