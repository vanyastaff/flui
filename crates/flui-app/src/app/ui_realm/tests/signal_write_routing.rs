//! Cross-thread signal writes are routed by the slot's graph (ADR-0074 §5.8,
//! ADR-0085 §1): each presentation's `BuildOwner` has its own `Reactive`
//! graph, and a `UiCommand::SignalWrite` runs against the one that minted its
//! target, whichever presentation of the realm that is. A write whose graph no
//! presentation of this realm owns is dropped and counted as stale.

use std::sync::atomic::AtomicU32;
use std::sync::mpsc;

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
    let mut backend = TestRasterBackend::always_presents();
    realm.render_frame_entered(&mut backend);
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

    realm.render_frame_entered(&mut backend);
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
