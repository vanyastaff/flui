use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver as ResponseReceiver, SyncSender as ResponseSender, sync_channel},
    },
};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use parking_lot::Mutex;

use crate::traits::{WindowId, WindowOptions};

pub(super) const CONTROL_CAPACITY: usize = 256;

type WakeOwner = Arc<dyn Fn() + Send + Sync>;
type OpenWindowResult = anyhow::Result<WindowId>;

pub(super) enum ControlCommand {
    OpenWindow {
        options: WindowOptions,
        response: ResponseSender<OpenWindowResult>,
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
            .finish_non_exhaustive()
    }
}

pub(super) struct ControlReceiver {
    commands: Receiver<ControlCommand>,
    wake_pending: Arc<AtomicBool>,
    quit_requested: Arc<AtomicBool>,
    admission: Arc<Mutex<bool>>,
    owner_affinity: PhantomData<Rc<()>>,
}

pub(super) fn control_lane(wake_owner: WakeOwner) -> (ControlSender, ControlReceiver) {
    let (commands, receiver) = bounded(CONTROL_CAPACITY);
    let wake_pending = Arc::new(AtomicBool::new(false));
    let quit_requested = Arc::new(AtomicBool::new(false));
    let admission = Arc::new(Mutex::new(true));

    (
        ControlSender {
            commands,
            wake_owner,
            wake_pending: Arc::clone(&wake_pending),
            quit_requested: Arc::clone(&quit_requested),
            admission: Arc::clone(&admission),
        },
        ControlReceiver {
            commands: receiver,
            wake_pending,
            quit_requested,
            admission,
            owner_affinity: PhantomData,
        },
    )
}

impl ControlSender {
    pub(super) fn request_open_window(
        &self,
        options: WindowOptions,
    ) -> Result<ResponseReceiver<OpenWindowResult>, ControlSendError> {
        self.request_open_window_after_admission(options, || {})
    }

    fn request_open_window_after_admission(
        &self,
        options: WindowOptions,
        after_admission: impl FnOnce(),
    ) -> Result<ResponseReceiver<OpenWindowResult>, ControlSendError> {
        let admission = self.admission.lock();
        if !*admission {
            return Err(ControlSendError::OwnerGone { rejected: options });
        }

        let (response, receiver) = sync_channel(1);
        let command = ControlCommand::OpenWindow { options, response };
        after_admission();
        // `try_send` cannot block while the shutdown boundary waits for this
        // critical section. Once it returns, the command is either wholly
        // before the stop boundary or wholly rejected after it.
        let send_result = self.commands.try_send(command);
        drop(admission);

        match send_result {
            Ok(()) => {
                self.wake_owner();
                Ok(receiver)
            }
            Err(TrySendError::Full(rejected)) => Err(ControlSendError::Full {
                capacity: CONTROL_CAPACITY,
                rejected: rejected.into_options(),
            }),
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
            Self::OpenWindow { options, .. } => options,
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

    use flui_types::geometry::{Size, px};
    use static_assertions::{assert_impl_all, assert_not_impl_any};

    use super::{CONTROL_CAPACITY, ControlCommand, ControlSendError, control_lane};
    use crate::traits::{WindowId, WindowOptions};

    assert_impl_all!(super::ControlSender: Clone, Send, Sync);
    assert_not_impl_any!(super::ControlReceiver: Send, Sync);

    fn options(title: impl Into<String>) -> WindowOptions {
        WindowOptions {
            title: title.into(),
            size: Size::new(px(800.0), px(600.0)),
            ..WindowOptions::default()
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
        let ControlCommand::OpenWindow { options, response } =
            receiver.try_recv().expect("queued window request");
        assert_eq!(options.title, "ordered");
        owner_ack_tx
            .send(())
            .expect("release the sending worker after inspection");
        response
            .send(Ok(WindowId(7)))
            .expect("requester keeps its one-shot receiver");

        let reply = worker
            .join()
            .expect("sending worker does not panic")
            .expect("request is accepted");
        assert_eq!(
            reply
                .recv()
                .expect("owner completes the one-shot")
                .expect("window opens"),
            WindowId(7)
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
            let reply = sender
                .request_open_window(options("cross-thread"))
                .expect("owner lane accepts request");
            reply.recv().expect("owner returns a result")
        });

        wake_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker request wakes owner");
        assert_eq!(receiver.begin_drain(), 1);
        assert_eq!(thread::current().id(), owner_thread);
        let ControlCommand::OpenWindow { response, .. } =
            receiver.try_recv().expect("owner receives request");
        response
            .send(Ok(WindowId(11)))
            .expect("worker awaits response");
        assert_eq!(
            worker.join().expect("worker exits").expect("window opens"),
            WindowId(11)
        );
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
            "a rejected command cannot create a wake"
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
        let reply = worker
            .join()
            .expect("admitting worker does not panic")
            .expect("the request linearized before shutdown");
        assert_eq!(
            shutdown_budget, 1,
            "shutdown snapshot contains every request admitted before the stop boundary"
        );

        let ControlCommand::OpenWindow { response, .. } = receiver
            .try_recv()
            .expect("the admitted command remains available for rejection");
        response
            .send(Err(anyhow::anyhow!("owner stopped")))
            .expect("requester remains live");
        assert!(
            reply
                .try_recv()
                .expect("shutdown completes the response before receiver drop")
                .is_err()
        );
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
}
