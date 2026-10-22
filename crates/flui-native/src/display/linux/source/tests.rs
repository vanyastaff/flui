//! The display thread against a scripted session: no compositor, but a real
//! thread, a real stop descriptor and a real owner signal.

use std::collections::VecDeque;
use std::os::fd::BorrowedFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::DisplayKey;

use super::*;
use crate::display::linux::session::wait_readable;
use crate::display::linux::view::Identity;

enum Action {
    Change(Vec<OutputView>),
    Fail(&'static str),
    Panic,
}

fn doorbell() -> Arc<OwnedFd> {
    Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).expect("an eventfd"))
}

/// A session driven by a script instead of a compositor: each ring of its
/// doorbell performs the next scripted action on the display thread.
struct ScriptedSession {
    views: Vec<OutputView>,
    script: Arc<Mutex<VecDeque<Action>>>,
    doorbell: Arc<OwnedFd>,
    closed: Arc<AtomicBool>,
}

impl Drop for ScriptedSession {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

impl Session for ScriptedSession {
    fn views(&self) -> Vec<OutputView> {
        self.views.clone()
    }

    fn step(&mut self, stop: BorrowedFd<'_>) -> Result<Step, DisplayError> {
        let ready = wait_readable(self.doorbell.as_fd(), stop)
            .map_err(|error| DisplayError::connection("test", error))?;
        if ready.stop {
            return Ok(Step::Stopped);
        }
        let mut ring = [0_u8; 8];
        rustix::io::read(self.doorbell.as_fd(), &mut ring)
            .map_err(|error| DisplayError::connection("test", error))?;
        let action = self.script.lock().pop_front();
        match action {
            Some(Action::Change(views)) => {
                self.views = views;
                Ok(Step::Changed)
            }
            Some(Action::Fail(message)) => Err(DisplayError::connection("test", message)),
            Some(Action::Panic) => panic!("scripted panic"),
            None => Ok(Step::Idle),
        }
    }
}

/// One watched source with a scripted session and a counting owner.
struct Rig {
    displays: Arc<LinuxDisplays>,
    signal: Arc<OwnerSignal>,
    watcher: Option<DisplayWatcher>,
    script: Arc<Mutex<VecDeque<Action>>>,
    doorbell: Arc<OwnedFd>,
    closed: Arc<AtomicBool>,
    wakes: Arc<AtomicUsize>,
}

fn view(output: u32) -> OutputView {
    let bounds = Bounds::new(Point::new(0, 0), Size::new(1920, 1080));
    OutputView {
        identity: Identity::Keyed(DisplayKey::X11Output(output)),
        natives: vec![output + 100],
        name: format!("Output {output}"),
        bounds,
        usable_bounds: bounds,
        scale_factor: 1.0,
        refresh_period: None,
        is_primary: false,
        persistent_key: None,
    }
}

/// A session with the handles that drive and observe it: its script, its
/// doorbell and its closed flag.
type Parts = (
    ScriptedSession,
    Arc<Mutex<VecDeque<Action>>>,
    Arc<OwnedFd>,
    Arc<AtomicBool>,
);

fn session(views: Vec<OutputView>) -> Parts {
    let script = Arc::new(Mutex::new(VecDeque::new()));
    let doorbell = doorbell();
    let closed = Arc::new(AtomicBool::new(false));
    let session = ScriptedSession {
        views,
        script: Arc::clone(&script),
        doorbell: Arc::clone(&doorbell),
        closed: Arc::clone(&closed),
    };
    (session, script, doorbell, closed)
}

fn rig(initial: Vec<OutputView>) -> Rig {
    let wakes = Arc::new(AtomicUsize::new(0));
    let signal = OwnerSignal::new({
        let wakes = Arc::clone(&wakes);
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    signal.start().expect("the owner signal starts");
    let displays = LinuxDisplays::new();
    let (session, script, doorbell, closed) = session(initial);
    let watcher = displays
        .watch_session(Protocol::X11, &signal, Box::new(session))
        .expect("the scripted session starts");
    Rig {
        displays,
        signal,
        watcher: Some(watcher),
        script,
        doorbell,
        closed,
        wakes,
    }
}

fn ring(script: &Mutex<VecDeque<Action>>, doorbell: &OwnedFd, action: Action) {
    script.lock().push_back(action);
    rustix::io::write(doorbell.as_fd(), &1_u64.to_ne_bytes()).expect("the doorbell rings");
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

impl Rig {
    /// Make the display thread perform `action`.
    fn run(&self, action: Action) {
        ring(&self.script, &self.doorbell, action);
    }

    /// Wait for the owner to be woken, then take the turn the wake asked for.
    fn wait_for_wake(&self) {
        wait_for("a wake", || self.wakes.load(Ordering::SeqCst) > 0);
        self.signal.drive();
    }
}

fn a_change_on_the_thread_reaches_the_next_snapshot_and_wakes_the_owner() {
    let rig = rig(vec![view(1)]);
    let first = rig.displays.snapshot().expect("the first snapshot");
    assert_eq!(first.displays().len(), 1);
    // Nothing changed: the same snapshot, the same revision.
    assert_eq!(
        rig.displays.snapshot().expect("snapshot").revision(),
        first.revision()
    );

    rig.run(Action::Change(vec![view(1), view(2)]));
    rig.wait_for_wake();
    let second = rig.displays.snapshot().expect("the next snapshot");
    assert_eq!(second.displays().len(), 2);
    assert!(second.revision() > first.revision());
    assert_eq!(
        second.displays()[0].id,
        first.displays()[0].id,
        "the display that stayed kept its id"
    );
}

fn stopping_joins_the_thread_and_closes_the_connection_before_returning() {
    let mut rig = rig(vec![view(1)]);
    assert!(!rig.closed.load(Ordering::SeqCst));
    drop(rig.watcher.take());
    assert!(
        rig.closed.load(Ordering::SeqCst),
        "the session outlived the join"
    );
}

fn a_failed_session_is_reported_by_the_snapshot_and_wakes_the_owner() {
    let mut rig = rig(vec![view(1)]);
    rig.displays.snapshot().expect("snapshot while healthy");
    rig.run(Action::Fail("the compositor went away"));
    rig.wait_for_wake();
    for _ in 0..2 {
        let error = rig.displays.snapshot().expect_err("the source has stopped");
        assert!(
            error.to_string().contains("the compositor went away"),
            "{error}"
        );
    }
    // The thread has ended on its own; stopping it afterwards is still clean.
    drop(rig.watcher.take());
    assert!(rig.closed.load(Ordering::SeqCst));
    // A watcher started again brings the source back.
    let (session, ..) = session(vec![view(3)]);
    let again = rig
        .displays
        .watch_session(Protocol::X11, &rig.signal, Box::new(session))
        .expect("the second session starts");
    assert_eq!(
        rig.displays.snapshot().expect("recovered").displays().len(),
        1
    );
    drop(again);
}

fn a_panicking_session_is_contained_and_reported() {
    let mut rig = rig(vec![view(1)]);
    rig.run(Action::Panic);
    rig.wait_for_wake();
    let error = rig.displays.snapshot().expect_err("the source has stopped");
    assert!(error.to_string().contains("panicked"), "{error}");
    assert!(
        rig.closed.load(Ordering::SeqCst),
        "the session was released after the panic"
    );
    drop(rig.watcher.take());
}

fn a_change_after_the_owner_closed_wakes_nobody() {
    let rig = rig(vec![view(1)]);
    let before = rig.displays.snapshot().expect("snapshot");
    // The owner closes admission first, as shutdown does.
    rig.signal.close();
    rig.run(Action::Change(vec![view(1), view(2)]));
    // No wake can be observed, so wait on what the thread did: the list it
    // swapped in reaches the next snapshot.
    wait_for("the thread's change", || {
        rig.displays
            .snapshot()
            .is_ok_and(|snapshot| snapshot.revision() > before.revision())
    });
    assert_eq!(
        rig.wakes.load(Ordering::SeqCst),
        0,
        "a closed owner was woken"
    );
}

fn a_change_after_the_owner_signal_is_gone_is_harmless() {
    let Rig {
        displays,
        signal,
        watcher,
        script,
        doorbell,
        ..
    } = rig(vec![view(1)]);
    // Only the thread's weak handle is left once this drops.
    drop(signal);
    ring(&script, &doorbell, Action::Change(vec![view(2)]));
    wait_for("the thread's change", || {
        displays.snapshot().is_ok_and(|snapshot| {
            snapshot
                .displays()
                .first()
                .is_some_and(|display| display.name == "Output 2")
        })
    });
    drop(watcher);
}

fn a_window_finds_its_display_by_native_number_in_the_published_snapshot() {
    let rig = rig(vec![view(1), view(2)]);
    // Nothing is published before the first snapshot.
    assert_eq!(rig.displays.display_of_native(Protocol::X11, 101), None);
    let snapshot = rig.displays.snapshot().expect("snapshot");
    let [first, second] = snapshot.displays() else {
        panic!("BUG: two outputs were scripted");
    };
    assert_eq!(
        rig.displays.display_of_native(Protocol::X11, 101),
        Some(first.id)
    );
    assert_eq!(
        rig.displays.display_of_native(Protocol::X11, 102),
        Some(second.id)
    );
    assert_eq!(rig.displays.display_of_native(Protocol::X11, 999), None);
    // A window of the other protocol is not on any of these outputs.
    assert_eq!(rig.displays.display_of_native(Protocol::Wayland, 101), None);
}

fn a_window_on_an_output_the_snapshot_lacks_asks_for_one_fresh_sample() {
    let rig = rig(vec![view(1)]);
    let first = rig.displays.snapshot().expect("snapshot");
    // The outputs change behind the source's back, with no invalidation: only
    // a fresh sample can see it.
    rig.displays.state.lock().views = Some(vec![view(1), view(5)]);
    assert_eq!(
        rig.displays.snapshot().expect("snapshot").revision(),
        first.revision(),
        "the published snapshot is kept while nothing marks it stale"
    );
    // A window on CRTC 105 asks for a display the snapshot lacks.
    assert_eq!(rig.displays.display_of_native(Protocol::X11, 105), None);
    let sampled = rig.displays.snapshot().expect("snapshot");
    assert_eq!(
        sampled.displays().len(),
        2,
        "the miss caused a fresh sample"
    );
    // The same miss again costs nothing more.
    assert_eq!(rig.displays.display_of_native(Protocol::X11, 106), None);
    assert_eq!(
        rig.displays.display_of_native(Protocol::X11, 105),
        Some(sampled.displays()[1].id)
    );
}

fn a_watcher_whose_thread_has_ended_reports_it_finished() {
    let rig = rig(vec![view(1)]);
    let watcher = rig.watcher.as_ref().expect("a watcher");
    assert!(!watcher.finished());
    rig.run(Action::Fail("the compositor went away"));
    wait_for("the thread to end", || {
        rig.watcher.as_ref().is_some_and(DisplayWatcher::finished)
    });
}

#[test]
fn display_thread_contract() {
    crate::table_test::run_table(
        "display_thread_contract",
        &[
            (
                "a_change_on_the_thread_reaches_the_next_snapshot_and_wakes_the_owner",
                a_change_on_the_thread_reaches_the_next_snapshot_and_wakes_the_owner as fn(),
            ),
            (
                "stopping_joins_the_thread_and_closes_the_connection_before_returning",
                stopping_joins_the_thread_and_closes_the_connection_before_returning,
            ),
            (
                "a_failed_session_is_reported_by_the_snapshot_and_wakes_the_owner",
                a_failed_session_is_reported_by_the_snapshot_and_wakes_the_owner,
            ),
            (
                "a_panicking_session_is_contained_and_reported",
                a_panicking_session_is_contained_and_reported,
            ),
            (
                "a_change_after_the_owner_closed_wakes_nobody",
                a_change_after_the_owner_closed_wakes_nobody,
            ),
            (
                "a_change_after_the_owner_signal_is_gone_is_harmless",
                a_change_after_the_owner_signal_is_gone_is_harmless,
            ),
            (
                "a_window_finds_its_display_by_native_number_in_the_published_snapshot",
                a_window_finds_its_display_by_native_number_in_the_published_snapshot,
            ),
            (
                "a_watcher_whose_thread_has_ended_reports_it_finished",
                a_watcher_whose_thread_has_ended_reports_it_finished,
            ),
            (
                "a_window_on_an_output_the_snapshot_lacks_asks_for_one_fresh_sample",
                a_window_on_an_output_the_snapshot_lacks_asks_for_one_fresh_sample,
            ),
        ],
    );
}
