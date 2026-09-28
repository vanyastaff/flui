//! Cross-thread signal writes are routed by the slot's graph (ADR-0074 §5.8,
//! ADR-0085 §1): each presentation's `BuildOwner` has its own `Reactive`
//! graph, and a `UiCommand::SignalWrite` runs against the one that minted its
//! target, whichever presentation of the realm that is. A write whose graph no
//! presentation of this realm owns is dropped and counted as stale.

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
        SizedBox::square(self.sig.get(ctx) as f32)
    }
}

fn graph_of(realm: &UiRealm, id: PresentationId) -> Reactive {
    realm
        .presentation_widgets_for_test(id)
        .with_build_owner(|owner| owner.reactive().clone())
}

#[test]
fn a_write_to_a_secondary_presentations_signal_rebuilds_its_reader() {
    let mut realm = UiRealm::for_test();
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    let graph_a = graph_of(&realm, a);
    let graph_b = graph_of(&realm, b);
    let sig_a = graph_a.signal(1u32);
    let sig_b = graph_b.signal(1u32);
    let (builds_a, builds_b) = (builds(), builds());
    realm
        .attach_root_widget_to_for_test(
            a,
            &Reader {
                sig: sig_a,
                builds: Arc::clone(&builds_a),
            },
        )
        .expect("primary root attaches");
    realm
        .attach_root_widget_to_for_test(
            b,
            &Reader {
                sig: sig_b,
                builds: Arc::clone(&builds_b),
            },
        )
        .expect("secondary root attaches");
    let mut backend = ScriptedSink::always_presents();
    realm.render_frame(&mut backend);
    assert_eq!((count(&builds_a), count(&builds_b)), (1, 1));

    let (tx, rx) = mpsc::channel::<Result<(), SignalError>>();
    realm
        .command_sender()
        .send_signal_write(sig_b.detach(), move |s, r| {
            tx.send(s.set(r, 7)).expect("test receiver alive");
        })
        .expect("send");
    let report = realm.drain_commands();

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

    realm.render_frame(&mut backend);
    assert_eq!(
        (count(&builds_a), count(&builds_b)),
        (1, 2),
        "the secondary's reader rebuilds; the primary is untouched"
    );
}

#[test]
fn a_write_whose_presentation_closed_is_dropped_and_counted() {
    let mut realm = UiRealm::for_test();
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    let sig_b = graph_of(&realm, b).signal(1u32);
    assert!(realm.close_presentation_entered(b));
    let graph_a = graph_of(&realm, a);
    let live_in_a = graph_a.live_slot_count();

    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_write = Arc::clone(&ran);
    realm
        .command_sender()
        .send_signal_write(sig_b.detach(), move |s, r| {
            ran_in_write.store(true, Ordering::Relaxed);
            let _ = s.set(r, 7);
        })
        .expect("send");
    let report = realm.drain_commands();

    assert_eq!(
        report,
        DrainReport {
            invoked: 0,
            dropped_stale: 1
        }
    );
    assert!(
        !ran.load(Ordering::Relaxed),
        "a write whose graph has closed must not run against a sibling's graph"
    );
    assert_eq!(graph_a.live_slot_count(), live_in_a);
}

#[test]
fn stale_signal_command_disposal_panic_rearms_its_fifo_tail() {
    struct DropBomb;

    impl Drop for DropBomb {
        fn drop(&mut self) {
            panic!("stale command capture destructor probe");
        }
    }

    let (wake, wake_count) = counting_wake();
    let mut realm = new_runtime(wake).expect("runtime");
    let closed = realm.install_second_presentation_for_test();
    let stale = graph_of(&realm, closed).signal(1u32);
    assert!(realm.close_presentation_entered(closed));
    let live_graph = graph_of(&realm, realm.presentation_id());
    let tail = live_graph.signal(0u32);
    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_callback = Arc::clone(&ran);
    let capture = DropBomb;
    let sender = realm.command_sender();
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

    let outcome = catch_unwind(AssertUnwindSafe(|| realm.drain_commands()));

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

    assert!(realm.drain_owner_inbox());
    assert_eq!(tail.peek(&live_graph, |value| *value), Ok(9));
}

#[test]
fn a_write_for_a_graph_outside_the_realm_never_runs_here() {
    let realm = UiRealm::for_test();
    let graph_a = graph_of(&realm, realm.presentation_id());
    let live_in_a = graph_a.live_slot_count();
    let outsider = Reactive::new();
    let foreign = outsider.signal(1u32);

    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_write = Arc::clone(&ran);
    realm
        .command_sender()
        .send_signal_write(foreign.detach(), move |s, r| {
            ran_in_write.store(true, Ordering::Relaxed);
            let _ = s.set(r, 7);
        })
        .expect("send");
    let report = realm.drain_commands();

    assert_eq!(
        report,
        DrainReport {
            invoked: 0,
            dropped_stale: 1
        }
    );
    assert!(!ran.load(Ordering::Relaxed));
    assert_eq!(graph_a.live_slot_count(), live_in_a);
    assert_eq!(foreign.peek(&outsider, |v| *v), Ok(1));
}

#[test]
fn a_routed_write_requests_the_owning_presentations_frame() {
    let mut realm = UiRealm::for_test();
    let b = realm.install_second_presentation_for_test();
    let sig_b = graph_of(&realm, b).signal(1u32);
    // Clear whatever installing the presentation requested.
    let _ = realm.take_redraw_request();
    let _ = realm
        .presentations
        .get(b)
        .expect("presentation installed")
        .take_redraw_pending();

    realm
        .command_sender()
        .send_signal_write(sig_b.detach(), move |s, r| {
            let _ = s.set(r, 7);
        })
        .expect("send");
    let report = realm.drain_commands();

    assert_eq!(report.invoked, 1);
    assert!(
        realm.take_redraw_request(),
        "a routed write must wake the event loop"
    );
    assert!(
        realm
            .presentations
            .get(b)
            .expect("presentation installed")
            .take_redraw_pending(),
        "a routed write must request the owning presentation's frame, whose BuildOwner has \
         no wake hook of its own"
    );
}

#[test]
fn a_panicking_routed_write_still_requests_its_owning_presentations_frame() {
    let (wake, wake_count) = counting_wake();
    let mut realm = new_runtime(wake).expect("runtime");
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    let graph_b = graph_of(&realm, b);
    let signal = graph_b.signal(1u32);
    let tail = graph_b.signal(0u32);
    let builds_b = builds();
    realm
        .attach_root_widget_to_for_test(
            b,
            &Reader {
                sig: signal,
                builds: Arc::clone(&builds_b),
            },
        )
        .expect("secondary root attaches");
    let mut backend = ScriptedSink::always_presents();
    realm.render_frame(&mut backend);
    let _ = realm.take_redraw_request();
    let _ = realm
        .presentations
        .get(b)
        .expect("secondary installed")
        .take_redraw_pending();

    let sender = realm.command_sender();
    sender
        .send_signal_write(signal.detach(), move |signal, graph| {
            let _ = signal.update(graph, |value| {
                *value = 7;
                panic!("command updater probe");
            });
        })
        .expect("send");
    sender
        .send_signal_write(tail.detach(), move |tail, graph| {
            tail.set(graph, 9).expect("tail signal remains live");
        })
        .expect("tail send");
    let wakes_after_sends = wake_count.load(Ordering::Relaxed);
    let outcome = catch_unwind(AssertUnwindSafe(|| realm.drain_commands()));

    let payload = outcome.expect_err("the command panic must resume");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"command updater probe")
    );
    assert_eq!(signal.peek(&graph_b, |value| *value), Ok(7));
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_sends + 1,
        "the unwinding owner turn must re-arm a platform wake"
    );
    assert_eq!(
        tail.peek(&graph_b, |value| *value),
        Ok(0),
        "the FIFO tail remains queued when the first command unwinds"
    );
    assert!(
        realm.drain_owner_inbox(),
        "the re-armed owner turn must observe redraw demand"
    );
    assert_eq!(
        tail.peek(&graph_b, |value| *value),
        Ok(9),
        "the next owner turn drains the preserved FIFO tail"
    );
    assert!(
        realm
            .presentations
            .get(b)
            .expect("secondary installed")
            .take_redraw_pending(),
        "the addressed presentation must retain redraw demand"
    );
    assert!(
        !realm
            .presentations
            .get(a)
            .expect("primary installed")
            .take_redraw_pending(),
        "the sibling presentation stays untouched"
    );

    realm.render_frame(&mut backend);
    assert_eq!(count(&builds_b), 2, "the partial commit becomes visible");
}

#[test]
fn signal_command_panic_keeps_priority_over_its_captures_destructor_panic() {
    struct DropBomb;

    impl Drop for DropBomb {
        fn drop(&mut self) {
            panic!("command capture destructor probe");
        }
    }

    let (wake, wake_count) = counting_wake();
    let realm = new_runtime(wake).expect("runtime");
    let graph = graph_of(&realm, realm.presentation_id());
    let signal = graph.signal(1u32);
    let capture = DropBomb;
    realm
        .command_sender()
        .send_signal_write(signal.detach(), move |signal, graph| {
            let _capture_stays_owned_by_the_command = &capture;
            let _ = signal.update(graph, |value| {
                *value = 7;
                panic!("command callback probe");
            });
        })
        .expect("send while wake is healthy");
    let wakes_after_send = wake_count.load(Ordering::Relaxed);

    let outcome = catch_unwind(AssertUnwindSafe(|| realm.drain_commands()));

    let payload = outcome.expect_err("the command callback panic must resume");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"command callback probe"),
        "the capture destructor panic must remain secondary"
    );
    assert_eq!(
        signal.peek(&graph, |value| *value),
        Ok(7),
        "the partial commit survives both contained panics"
    );
    assert!(
        realm.take_redraw_request(),
        "the partial commit retains redraw demand"
    );
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_send + 1,
        "the owner turn is rearmed after the command panic"
    );
}

#[test]
fn a_failed_signal_write_rearm_retries_at_the_next_owner_boundary() {
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
    let realm = new_runtime(wake).expect("runtime");
    let graph = graph_of(&realm, realm.presentation_id());
    let signal = graph.signal(1u32);
    let tail = graph.signal(0u32);
    let sender = realm.command_sender();
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

    let outcome = catch_unwind(AssertUnwindSafe(|| realm.drain_commands()));

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
        realm.drain_owner_inbox(),
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
    assert_eq!(realm.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_count.load(Ordering::Relaxed),
        wakes_after_recovery,
        "a successful retry clears the debt instead of waking every boundary"
    );
}

#[test]
fn an_older_overlapping_wake_cannot_clear_newer_failed_delivery_debt() {
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
    let realm = new_runtime(wake).expect("runtime");
    let first_sender = realm.command_sender();
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

    assert_eq!(realm.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        3,
        "the older success acknowledges only its own generation; the next owner boundary must \
         retry the newer failed generation"
    );
    assert!(
        realm.take_redraw_request(),
        "the overlapping requests retain their coalesced redraw demand"
    );

    assert_eq!(realm.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        3,
        "the successful retry clears the newer generation"
    );
}

/// The primary-graph case keeps working: a write to a signal the primary
/// presentation minted runs against that graph at the next drain.
#[test]
fn a_signal_write_command_reaches_the_realms_graph_at_the_next_drain() {
    let realm = new_runtime(noop_wake()).expect("runtime");
    let graph = realm
        .widgets()
        .with_build_owner(|owner| owner.reactive().clone());
    let counter = graph.signal(1u32);

    realm
        .command_sender()
        .send_signal_write(counter.detach(), |s, r| {
            s.update(r, |c| *c += 41).expect("signal alive");
        })
        .expect("send");
    assert_eq!(counter.peek(&graph, |c| *c), Ok(1), "nothing runs at send");

    let report = realm.drain_commands();

    assert_eq!(report.invoked, 1);
    assert_eq!(counter.peek(&graph, |c| *c), Ok(42));
}
