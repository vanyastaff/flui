//! Cross-thread signal writes are routed by the slot's graph (ADR-0074 §5.8,
//! ADR-0085 §1): each presentation's `BuildOwner` has its own `Reactive`
//! graph, and a `UiCommand::SignalWrite` runs against the one that minted its
//! target, whichever presentation of the UI runtime that is. A write whose graph no
//! presentation of this UI runtime owns is dropped and counted as stale.

use std::sync::atomic::AtomicU32;
use std::sync::mpsc;
use std::{panic::AssertUnwindSafe, panic::catch_unwind};

use flui_view::{Reactive, Signal, SignalError};

use super::*;

type Builds = Arc<AtomicU32>;

fn builds() -> Builds {
    Arc::new(AtomicU32::new(0))
}

fn count(builds: &Builds) -> u32 {
    builds.load(Ordering::Relaxed)
}

/// Reads one `Signal<u32>` in `build` and counts its builds.
#[derive(Clone)]
struct Reader {
    sig: Signal<u32>,
    builds: Builds,
}

impl flui_view::View for Reader {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Reader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.fetch_add(1, Ordering::Relaxed);
        SizedBox::square(self.sig.get(ctx) as f64)
    }
}

fn graph_of(ui_runtime: &UiRuntime, id: PresentationId) -> Reactive {
    ui_runtime
        .presentation_widgets_for_test(id)
        .with_build_owner(|owner| owner.reactive().clone())
}

pub(crate) fn a_write_to_a_secondary_presentations_signal_rebuilds_its_reader() {
    let mut ui_runtime = UiRuntime::for_test();
    let a = ui_runtime.presentation_id();
    let b = ui_runtime.install_second_presentation_for_test();
    let graph_a = graph_of(&ui_runtime, a);
    let graph_b = graph_of(&ui_runtime, b);
    let sig_a = graph_a.signal(1u32);
    let sig_b = graph_b.signal(1u32);
    let (builds_a, builds_b) = (builds(), builds());
    ui_runtime
        .attach_root_widget_to_for_test(
            a,
            &Reader {
                sig: sig_a,
                builds: Arc::clone(&builds_a),
            },
        )
        .expect("primary root attaches");
    ui_runtime
        .attach_root_widget_to_for_test(
            b,
            &Reader {
                sig: sig_b,
                builds: Arc::clone(&builds_b),
            },
        )
        .expect("secondary root attaches");
    let mut backend = ScriptedSink::always_presents();
    ui_runtime.render_frame(&mut backend);
    assert_eq!((count(&builds_a), count(&builds_b)), (1, 1));

    let (tx, rx) = mpsc::channel::<Result<(), SignalError>>();
    ui_runtime
        .command_sender()
        .send_signal_write(sig_b.detach(), move |s, r| {
            tx.send(s.set(r, 7)).expect("test receiver alive");
        })
        .expect("send");
    let report = ui_runtime.drain_commands();

    assert_eq!(
        report,
        DrainReport {
            invoked: 1,
            dropped_stale: 0
        }
    );
    assert_eq!(
        rx.try_recv().expect("the write ran"),
        Ok(()),
        "the write must run against the graph that minted its slot"
    );
    assert_eq!(sig_b.peek(&graph_b, |v| *v), Ok(7));

    ui_runtime.render_frame(&mut backend);
    assert_eq!(
        (count(&builds_a), count(&builds_b)),
        (1, 2),
        "the secondary's reader rebuilds; the primary is untouched"
    );
}

pub(crate) fn stale_signal_command_disposal_panic_rearms_its_fifo_tail() {
    struct DropBomb;

    impl Drop for DropBomb {
        fn drop(&mut self) {
            panic!("stale command capture destructor probe");
        }
    }

    let (wake, wake_count) = counting_wake();
    let mut ui_runtime = new_runtime(wake).expect("runtime");
    let closed = ui_runtime.install_second_presentation_for_test();
    let stale = graph_of(&ui_runtime, closed).signal(1u32);
    assert!(ui_runtime.close_presentation_entered(closed));
    let live_graph = graph_of(&ui_runtime, ui_runtime.presentation_id());
    let tail = live_graph.signal(0u32);
    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_callback = Arc::clone(&ran);
    let capture = DropBomb;
    let sender = ui_runtime.command_sender();
    sender
        .send_signal_write(stale.detach(), move |_signal, _graph| {
            let _capture_stays_owned_by_the_command = &capture;
            ran_in_callback.store(true, Ordering::Relaxed);
        })
        .expect("stale command enqueues");
    sender
        .send_signal_write(tail.detach(), move |tail, graph| {
            tail.set(graph, 9).expect("tail signal remains live");
        })
        .expect("tail command enqueues");
    let wakes_after_sends = wake_count.load(Ordering::Relaxed);

    let outcome = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_commands()));

    let payload = outcome.expect_err("the capture destructor panic must propagate");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"stale command capture destructor probe")
    );
    assert!(
        !ran.load(Ordering::Relaxed),
        "the stale callback must not run"
    );
    assert_eq!(
        tail.peek(&live_graph, |value| *value),
        Ok(0),
        "the panic leaves the FIFO tail queued"
    );
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_sends + 1,
        "discarding the stale command must rearm its FIFO tail"
    );

    assert!(ui_runtime.drain_owner_inbox());
    assert_eq!(tail.peek(&live_graph, |value| *value), Ok(9));
}

pub(crate) fn a_failed_signal_write_rearm_retries_at_the_next_owner_boundary() {
    let panic_on_wake = Arc::new(AtomicBool::new(false));
    let panic_on_wake_in_callback = Arc::clone(&panic_on_wake);
    let wake_count = Arc::new(AtomicUsize::new(0));
    let wake_count_in_callback = Arc::clone(&wake_count);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_count_in_callback.fetch_add(1, Ordering::Relaxed);
        assert!(
            !panic_on_wake_in_callback.swap(false, Ordering::Relaxed),
            "command wake probe"
        );
    });
    let ui_runtime = new_runtime(wake).expect("runtime");
    let graph = graph_of(&ui_runtime, ui_runtime.presentation_id());
    let signal = graph.signal(1u32);
    let tail = graph.signal(0u32);
    let sender = ui_runtime.command_sender();
    sender
        .send_signal_write(signal.detach(), move |signal, graph| {
            let _ = signal.update(graph, |value| {
                *value = 7;
                panic!("command updater probe");
            });
        })
        .expect("send while wake is healthy");
    sender
        .send_signal_write(tail.detach(), move |tail, graph| {
            tail.set(graph, 9).expect("tail signal remains live");
        })
        .expect("tail send while wake is healthy");
    let wakes_after_sends = wake_count.load(Ordering::Relaxed);
    panic_on_wake.store(true, Ordering::Relaxed);

    let outcome = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_commands()));

    let payload = outcome.expect_err("the command panic must resume");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"command updater probe"),
        "the secondary wake panic must not replace the updater panic"
    );
    assert_eq!(signal.peek(&graph, |value| *value), Ok(7));
    assert_eq!(
        tail.peek(&graph, |value| *value),
        Ok(0),
        "the callback panic leaves the FIFO tail queued"
    );
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_sends + 1,
        "the first rearm attempt reaches the hook and fails"
    );

    assert!(
        ui_runtime.drain_owner_inbox(),
        "the next owner boundary must preserve the partial commit's redraw demand"
    );
    assert_eq!(
        tail.peek(&graph, |value| *value),
        Ok(9),
        "the next owner boundary drains the preserved FIFO tail"
    );
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_sends + 2,
        "the completed owner boundary must pay the failed wake debt"
    );

    let wakes_after_recovery = wake_count.load(Ordering::Relaxed);
    assert_eq!(ui_runtime.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_recovery,
        "a successful retry clears the debt instead of waking every boundary"
    );
}

pub(crate) fn an_older_overlapping_wake_cannot_clear_newer_failed_delivery_debt() {
    let first_started = Arc::new(std::sync::Barrier::new(2));
    let release_first = Arc::new(std::sync::Barrier::new(2));
    let wake_count = Arc::new(AtomicUsize::new(0));
    let first_started_in_wake = Arc::clone(&first_started);
    let release_first_in_wake = Arc::clone(&release_first);
    let wake_count_in_callback = Arc::clone(&wake_count);
    let wake: Arc<dyn Fn() + Send + Sync> =
        Arc::new(
            move || match wake_count_in_callback.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    first_started_in_wake.wait();
                    release_first_in_wake.wait();
                }
                1 => panic!("newer wake probe"),
                _ => {}
            },
        );
    let ui_runtime = new_runtime(wake).expect("runtime");
    let first_sender = ui_runtime.command_sender();
    let second_sender = first_sender.clone();

    let first = std::thread::spawn(move || first_sender.request_redraw());
    first_started.wait();
    let second = std::thread::spawn(move || {
        catch_unwind(AssertUnwindSafe(|| second_sender.request_redraw()))
    });
    let second_outcome = second
        .join()
        .expect("second sender contains its wake panic");
    assert!(second_outcome.is_err(), "the newer wake must fail");
    release_first.wait();
    first.join().expect("older wake eventually succeeds");
    assert_eq!(wake_count.load(Ordering::SeqCst), 2);

    assert_eq!(ui_runtime.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        3,
        "the older success acknowledges only its own generation; the next owner boundary must \
         retry the newer failed generation"
    );
    assert!(
        ui_runtime.take_redraw_request(),
        "the overlapping requests retain their coalesced redraw demand"
    );

    assert_eq!(ui_runtime.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        3,
        "the successful retry clears the newer generation"
    );
}
