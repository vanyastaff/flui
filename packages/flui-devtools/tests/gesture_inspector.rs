//! `GestureInspector` through its public API: a scripted contest builds the
//! inspector's model, a recording replays to the model the live run built,
//! and detaching stops observation. `GestureInspectorOverlay` draws those
//! contests over the app while shown and mounted, and only then observes.
#![cfg(feature = "inspector")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_devtools::gesture_inspector::{
    ContestReport, GestureInspector, InspectedMember, MemberReport, Verdict,
};
use flui_sdk::foundation::ManualClock;
use flui_sdk::foundation::geometry::Offset;
use flui_sdk::interaction::{
    CompositionBranch, CompositionKind, ContestCancel, EventTime, GestureArena, GestureArenaEntry,
    GestureArenaMember, GestureComposition, GestureDisposition, PointerId, RejectionReason,
};
use flui_testing::recording::{
    Privacy, RecordingReader, RecordingWriter, ReplayableRecording, record,
};
use flui_testing::replay::{PointerScript, TouchScript};
use flui_testing::{HeadlessHost, HeadlessWindow};

use flui_devtools::gesture_overlay::{GestureInspectorOverlay, GestureOverlayController};
use flui_sdk::foundation::RebuildReason;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::prelude::*;

fn run_rows(rows: &[(&str, fn())]) {
    for &(name, row) in rows {
        if let Err(payload) = std::panic::catch_unwind(row) {
            eprintln!("row `{name}` failed");
            std::panic::resume_unwind(payload);
        }
    }
}

#[test]
fn gesture_inspector_contract() {
    run_rows(&[
        (
            "a_scripted_contest_builds_the_inspector_model",
            a_scripted_contest_builds_the_inspector_model,
        ),
        (
            "a_recording_replays_to_the_model_the_live_run_built",
            a_recording_replays_to_the_model_the_live_run_built,
        ),
        ("detaching_stops_observation", detaching_stops_observation),
    ]);
}

fn pointer(raw: u64) -> PointerId {
    PointerId::try_from(raw).expect("nonzero pointer id")
}

/// A member that may own a deadline, and withdraws when it is polled.
#[derive(Default)]
struct Probe {
    deadline: Cell<Option<EventTime>>,
    entry: RefCell<Option<GestureArenaEntry>>,
}

impl GestureArenaMember for Probe {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {}

    fn deadline(&self) -> Option<EventTime> {
        self.deadline.get()
    }

    fn poll_deadline(&self, _: EventTime) {
        self.deadline.set(None);
        let entry = self.entry.borrow().clone();
        if let Some(entry) = entry {
            entry.resolve(GestureDisposition::Rejected);
        }
    }
}

// The number the inspector gave a member.
fn number(member: InspectedMember) -> u64 {
    member.get().get()
}

/// A fallback claims while its preferred branch still competes, a plain
/// member's deadline elapses and it withdraws, then the preferred branch
/// withdraws and the fallback's claim wins; a second contact is abandoned.
fn a_scripted_contest_builds_the_inspector_model() {
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::with_clock(clock.clone());
    let inspector = GestureInspector::attach(&arena).expect("observer registration");
    let (preferred, fallback) = arena
        .compose(GestureComposition::RequireFailure)
        .expect("composition")
        .into_branches();
    let (p, f, d, e) = (
        Rc::new(Probe::default()),
        Rc::new(Probe::default()),
        Rc::new(Probe::default()),
        Rc::new(Probe::default()),
    );

    let first = pointer(1);
    let p_entry = preferred.add(first, &p);
    let f_entry = fallback.add(first, &f);
    *d.entry.borrow_mut() = Some(arena.add(first, &d));
    d.deadline.set(Some(EventTime::from_nanos(5)));
    arena.close(first);
    f_entry.resolve(GestureDisposition::Accepted);
    clock.advance(Duration::from_millis(1));
    arena.poll_deadlines();
    p_entry.resolve(GestureDisposition::Rejected);
    arena.drain_deferred_resolutions();

    let second = pointer(2);
    let _e_entry = arena.add(second, &e);
    arena.abandon(second);

    let contests = inspector.contests();
    assert_eq!(contests.len(), 2, "{contests:#?}");
    let [decided, abandoned] = [&contests[0], &contests[1]];

    assert_eq!(decided.pointer, first);
    assert_eq!(decided.sequence, None, "no binding drives this arena");
    assert!(decided.closed);
    assert_eq!(
        decided.winner.map(number),
        Some(2),
        "the fallback's claim won"
    );
    assert_eq!(decided.cancelled, None);
    let by_number = |n: u64| -> &MemberReport {
        decided
            .members
            .iter()
            .find(|report| number(report.member) == n)
            .expect("a member of the contest")
    };
    let (pm, fm, dm) = (by_number(1), by_number(2), by_number(3));
    assert_eq!(
        decided
            .members
            .iter()
            .map(|report| number(report.member))
            .collect::<Vec<_>>(),
        [1, 2, 3],
        "members in join order"
    );
    let shape = |report: &MemberReport| {
        report
            .composition
            .iter()
            .map(|step| (step.kind, step.branch))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shape(pm),
        [(CompositionKind::RequireFailure, CompositionBranch::First)]
    );
    assert_eq!(
        shape(fm),
        [(CompositionKind::RequireFailure, CompositionBranch::Second)]
    );
    assert_eq!(
        pm.composition[0].node, fm.composition[0].node,
        "both branches of one relation"
    );
    assert_eq!(dm.composition, [], "an uncomposed member");
    assert_eq!(
        [pm.claimed, fm.claimed, dm.claimed],
        [false, true, false],
        "only the fallback claimed"
    );
    assert!(matches!(fm.verdict, Some(Verdict::Won { .. })));
    assert!(matches!(
        pm.verdict,
        Some(Verdict::Rejected {
            reason: RejectionReason::Withdrew,
            ..
        })
    ));
    assert!(matches!(
        dm.verdict,
        Some(Verdict::Rejected {
            reason: RejectionReason::Withdrew,
            ..
        })
    ));
    assert_eq!(
        decided
            .deadlines
            .iter()
            .map(|deadline| number(deadline.member))
            .collect::<Vec<_>>(),
        [3],
        "the plain member's deadline elapsed"
    );
    assert!(fm.type_name.is_some_and(|name| name.ends_with("Probe")));

    assert_eq!(abandoned.pointer, second);
    assert_eq!(abandoned.cancelled, Some(ContestCancel::Abandoned));
    assert_eq!(abandoned.winner, None);
    assert_eq!(abandoned.members.len(), 1);
    assert_eq!(number(abandoned.members[0].member), 4);
    assert!(matches!(
        abandoned.members[0].verdict,
        Some(Verdict::Rejected {
            reason: RejectionReason::Cancelled,
            ..
        })
    ));
}

/// A box that taps and long-presses.
fn host() -> HeadlessHost {
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
    host.attach(
        &flui_sdk::widgets::GestureDetector::new()
            .on_tap(|_| {})
            .on_long_press(|_| {})
            .child(flui_sdk::widgets::ColoredBox::new(
                flui_sdk::painting::Color::RED,
            )),
    )
    .expect("attach");
    let _ = host.pump(Duration::from_millis(16));
    host
}

fn session(host: &mut HeadlessHost) {
    let at = Offset::new(50.0, 50.0);
    host.run_script(&PointerScript::tap(at));
    let hold = TouchScript::new("hold")
        .down(1, at)
        .wait(Duration::from_millis(800))
        .frame()
        .up(1)
        .build()
        .expect("a paired contact");
    host.run_script(&hold);
    let _ = host.pump(Duration::from_millis(16));
}

fn winner_type(contest: &ContestReport) -> Option<&'static str> {
    let winner = contest.winner?;
    contest
        .members
        .iter()
        .find(|report| report.member == winner)
        .and_then(|report| report.type_name)
        .and_then(|name| name.rsplit("::").next())
}

fn a_recording_replays_to_the_model_the_live_run_built() {
    let mut live = host();
    let inspector = GestureInspector::attach(live.arena()).expect("observer registration");
    let writer = RecordingWriter::new(Vec::new(), &live.recording_header(Privacy::Redact))
        .expect("the header writes");
    let recording = record(live.input_taps(), writer).expect("first tap registration");
    session(&mut live);
    let bytes = recording.finish().expect("a complete recording");
    let recorded = inspector.contests();
    assert_eq!(
        recorded.iter().map(winner_type).collect::<Vec<_>>(),
        [Some("TapArenaMember"), Some("LongPressGestureRecognizer")],
        "{recorded:#?}"
    );
    assert!(
        recorded.iter().all(|contest| contest.sequence.is_some()),
        "each contest decides the sequence its Down started"
    );
    assert_ne!(recorded[0].sequence, recorded[1].sequence);
    assert!(
        !recorded[1].deadlines.is_empty(),
        "the long press won at its deadline"
    );

    let recording = ReplayableRecording::read(RecordingReader::new(&bytes[..]).expect("header"))
        .expect("a whole recording");
    let mut fresh = host();
    let replayed = GestureInspector::attach(fresh.arena()).expect("observer registration");
    let _ = fresh.replay(&recording);
    assert_eq!(
        replayed.contests(),
        recorded,
        "the replay builds the model the live run built"
    );
}

fn detaching_stops_observation() {
    let mut host = host();
    let mut inspector = GestureInspector::attach(host.arena()).expect("observer registration");
    assert!(inspector.is_attached());
    host.run_script(&PointerScript::tap(Offset::new(50.0, 50.0)));
    assert_eq!(inspector.contests().len(), 1);

    inspector.detach();
    assert!(!inspector.is_attached());
    host.run_script(&PointerScript::tap(Offset::new(50.0, 50.0)));
    assert_eq!(
        inspector.contests().len(),
        1,
        "a detached inspector keeps what it saw and sees nothing more"
    );
}

#[test]
fn gesture_overlay_contract() {
    run_rows(&[
        (
            "the_overlay_draws_each_contest_by_its_pointer_then_fades_it",
            the_overlay_draws_each_contest_by_its_pointer_then_fades_it,
        ),
        (
            "toggling_hides_the_overlay_without_rebuilding_the_app",
            toggling_hides_the_overlay_without_rebuilding_the_app,
        ),
        (
            "the_overlay_observes_only_while_mounted_and_shown",
            the_overlay_observes_only_while_mounted_and_shown,
        ),
    ]);
}

/// The application under the overlay: a box that taps and long-presses,
/// counting its builds.
#[derive(Clone, StatelessView)]
struct App {
    builds: Rc<Cell<u32>>,
}

impl StatelessView for App {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        flui_sdk::widgets::GestureDetector::new()
            .on_tap(|_| {})
            .on_long_press(|_| {})
            .child(flui_sdk::widgets::ColoredBox::new(
                flui_sdk::painting::Color::RED,
            ))
    }
}

/// What a test holds to drive the root: whether the overlay is mounted, the
/// root's rebuild handle, the overlay's controller and the app's build count.
#[derive(Clone)]
struct Fixture {
    mounted: Rc<Cell<bool>>,
    rebuild: Rc<RefCell<Option<RebuildHandle>>>,
    controller: GestureOverlayController,
    app_builds: Rc<Cell<u32>>,
}

impl Fixture {
    fn new(shown: bool) -> Self {
        Self {
            mounted: Rc::new(Cell::new(true)),
            rebuild: Rc::default(),
            controller: GestureOverlayController::new(shown),
            app_builds: Rc::default(),
        }
    }

    fn host(&self) -> HeadlessHost {
        let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200));
        host.attach(&Root {
            fixture: self.clone(),
        })
        .expect("attach");
        host.enable_semantics();
        let _ = host.pump(Duration::from_millis(16));
        host
    }

    fn mount(&self, host: &mut HeadlessHost, mounted: bool) {
        self.mounted.set(mounted);
        if let Some(rebuild) = &*self.rebuild.borrow() {
            rebuild.schedule(RebuildReason::StateChange);
        }
        let _ = host.pump(Duration::from_millis(16));
    }
}

/// Mounts the app, under the overlay while the fixture says so.
#[derive(Clone, StatefulView)]
struct Root {
    fixture: Fixture,
}

struct RootState {
    fixture: Fixture,
}

impl StatefulView for Root {
    type State = RootState;

    fn create_state(&self) -> RootState {
        RootState {
            fixture: self.fixture.clone(),
        }
    }
}

impl ViewState<Root> for RootState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.fixture.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
    }

    fn build(&self, _view: &Root, _ctx: &dyn BuildContext) -> impl IntoView {
        let app = App {
            builds: Rc::clone(&self.fixture.app_builds),
        };
        if self.fixture.mounted.get() {
            GestureInspectorOverlay::new(self.fixture.controller.clone(), app).boxed()
        } else {
            app.boxed()
        }
    }
}

/// Every label assistive technology reads in the host's window.
fn labels(host: &HeadlessHost) -> Vec<String> {
    host.published_a11y_tree(host.primary_window())
        .map(|tree| {
            tree.nodes()
                .filter_map(|node| node.label().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn contest_headers(host: &HeadlessHost) -> Vec<String> {
    labels(host)
        .into_iter()
        .filter(|label| label.starts_with("contest for pointer"))
        .collect()
}

fn tap(host: &mut HeadlessHost, x: f64, y: f64) {
    host.run_script(&PointerScript::tap(Offset::new(x, y)));
    let _ = host.pump(Duration::from_millis(16));
}

fn the_overlay_draws_each_contest_by_its_pointer_then_fades_it() {
    let fixture = Fixture::new(true);
    let mut host = fixture.host();
    tap(&mut host, 50.0, 60.0);

    // The detector composes its tap as the fallback of a double-tap
    // relation; the tap's win defeats the long press.
    assert_eq!(
        labels(&host),
        [
            "contest for pointer 1, sequence 1, won by #1",
            "#1 TapArenaMember in RequireFailure@1 second, won",
            "#2 LongPressGestureRecognizer, rejected (Defeated)",
        ]
    );
    let headers = contest_headers(&host);
    let tree = host
        .published_a11y_tree(host.primary_window())
        .expect("a published tree");
    let header = tree
        .find_by_label(&headers[0])
        .expect("one header for the contest");
    let bounds = tree.window_bounds(&header).expect("the header is laid out");
    assert!(
        (55.0..80.0).contains(&bounds.x0) && (65.0..90.0).contains(&bounds.y0),
        "the contest is drawn by the point its contact went down: {bounds:?}"
    );

    for _ in 0..4 {
        let _ = host.pump(Duration::from_millis(500));
    }
    assert_eq!(
        contest_headers(&host),
        Vec::<String>::new(),
        "a settled contest fades out"
    );
}

fn toggling_hides_the_overlay_without_rebuilding_the_app() {
    let fixture = Fixture::new(true);
    let mut host = fixture.host();
    tap(&mut host, 50.0, 50.0);
    assert_eq!(contest_headers(&host).len(), 1);
    let builds = fixture.app_builds.get();

    fixture.controller.set_shown(false);
    let _ = host.pump(Duration::from_millis(16));
    assert_eq!(contest_headers(&host), Vec::<String>::new());
    assert!(
        !fixture.controller.is_observing(),
        "hidden, it observes nothing"
    );

    fixture.controller.toggle();
    let _ = host.pump(Duration::from_millis(16));
    assert!(fixture.controller.is_observing());
    tap(&mut host, 50.0, 50.0);
    assert_eq!(
        contest_headers(&host).len(),
        1,
        "shown again, it draws the contests made since"
    );
    assert_eq!(
        fixture.app_builds.get(),
        builds,
        "toggling rebuilds the overlay's layer, never the app"
    );
}

fn the_overlay_observes_only_while_mounted_and_shown() {
    let fixture = Fixture::new(false);
    let mut host = fixture.host();
    assert!(
        !fixture.controller.is_observing(),
        "mounted hidden, no observer is registered"
    );
    fixture.controller.set_shown(true);
    let _ = host.pump(Duration::from_millis(16));
    assert!(fixture.controller.is_observing());

    fixture.mount(&mut host, false);
    assert!(
        !fixture.controller.is_observing(),
        "unmounting releases the observer"
    );
    fixture.controller.toggle();
    fixture.controller.toggle();
    assert!(
        !fixture.controller.is_observing(),
        "an unmounted overlay ignores its controller"
    );

    fixture.mount(&mut host, true);
    assert!(
        fixture.controller.is_observing(),
        "remounted shown, it observes"
    );
}
