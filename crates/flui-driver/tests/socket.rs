//! The socket transport and process ownership: an app that closes mid-call, an app that
//! exits before it answers, and a lane worker that keeps serving past a cancelled or panicking job.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use flui_driver::lane::LaneWorker;
use flui_driver::os_input::OsInputLock;
use flui_driver::protocol::act::EffectKind;
use flui_driver::protocol::error::{ErrorCode, ErrorObject, HandleKind, Retry};
use flui_driver::protocol::events::Event;
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{ActionSequence, InputSource};
use flui_driver::protocol::method::{self, Method};
use flui_driver::protocol::tree::{ElementRef, WindowId};
use flui_driver::steps::OsEvent;
use flui_driver::testing::{AfterLines, Reply, ScriptedAgent, ScriptedLane};
use flui_driver::{Driver, Error, InputPath, InputPolicy, Isolation, Launch};
use serde_json::json;

#[test]
fn a_driver_moves_to_the_thread_that_runs_it() {
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME],
        Box::new(|_, _| Reply::Result(json!({ "windows": ["w1"] }))),
    )
    .expect("the stand-in binds its endpoint");
    let driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    let windows = std::thread::spawn(move || {
        let mut driver = driver;
        driver.windows()
    })
    .join()
    .expect("the thread runs the driver");
    assert_eq!(windows.expect("the windows").len(), 1);
}

#[test]
fn an_app_that_closes_mid_call_answers_its_process_gone_then_and_after() {
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME, method::SemanticsRead::NAME],
        Box::new(|name, _| match name {
            "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
            _ => Reply::Close,
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    assert_eq!(driver.app().map(|app| app.name.as_str()), Some("scripted"));
    let mut window = driver.main_window().expect("one window");
    let started = Instant::now();
    let read = window.read();
    assert!(
        matches!(
            read,
            Err(Error::Gone {
                kind: Some(HandleKind::Process),
                ..
            })
        ),
        "{read:?}"
    );
    let later = driver.windows();
    assert!(
        matches!(
            later,
            Err(Error::Gone {
                kind: Some(HandleKind::Process),
                ..
            })
        ),
        "every later call fails the same way: {later:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "neither waited out its budget"
    );
    agent
        .join(Duration::from_secs(5))
        .expect("the stand-in served its connection");
}

/// A stand-in that answers `windows.list` with one window.
fn one_window_agent() -> ScriptedAgent {
    ScriptedAgent::start(
        &[method::WindowsList::NAME],
        Box::new(|_, _| Reply::Result(json!({ "windows": ["w1"] }))),
    )
    .expect("the stand-in binds its endpoint")
}

fn attach(agent: &ScriptedAgent) -> Driver {
    Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches")
}

#[test]
fn a_driver_that_goes_closes_its_connection_on_every_exit() {
    let dropped = one_window_agent();
    let mut driver = attach(&dropped);
    driver.windows().expect("the windows");
    drop(driver);
    dropped
        .join(Duration::from_secs(5))
        .expect("the app sees a dropped driver leave");

    let closed = one_window_agent();
    let mut driver = attach(&closed);
    driver.windows().expect("the windows");
    let _ = driver.close();
    closed
        .join(Duration::from_secs(5))
        .expect("the app sees a closed session leave");

    let unwound = one_window_agent();
    let driver = attach(&unwound);
    let panicked = std::thread::spawn(move || {
        let mut driver = driver;
        let _ = driver.windows();
        panic!("the caller panics while it holds the driver");
    })
    .join();
    assert!(panicked.is_err(), "the caller panicked");
    unwound
        .join(Duration::from_secs(5))
        .expect("the app sees the driver leave when a panic unwinds past it");
}

#[test]
fn a_connection_the_app_closed_between_calls_answers_gone_at_once() {
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME],
        Box::new(|_, _| Reply::ResultThenClose(json!({ "windows": ["w1"] }))),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    driver
        .windows()
        .expect("the last answer before the app closed");
    agent
        .join(Duration::from_secs(5))
        .expect("the stand-in closed its end");
    // The close reaches the connection between calls, and taking events hears it first.
    std::thread::sleep(Duration::from_millis(200));
    let (events, dropped) = driver.take_events();
    assert_eq!(
        (events.len(), dropped),
        (0, 0),
        "the stand-in sends no events"
    );
    let started = Instant::now();
    let later = driver.windows();
    match later {
        // Why the connection ended is what taking events heard, kept for every later call.
        Err(Error::Gone {
            kind: Some(HandleKind::Process),
            ref message,
            ..
        }) => assert!(
            message.contains("the app closed the connection"),
            "the first failure is the one reported: {message}"
        ),
        other => panic!("{other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "it did not wait out its budget"
    );
}

/// A frame event numbered `frame`, a short line.
fn frame_event(frame: u64) -> serde_json::Value {
    json!({
        "event": "frame",
        "params": { "window": "w1", "frame": frame, "presented": true },
    })
}

/// A frame event numbered `frame`, padded to about 16 KiB so a few of them fill a socket's
/// buffers.
fn padded_frame(frame: u64) -> serde_json::Value {
    json!({
        "event": "frame",
        "params": { "window": "w1", "frame": frame, "presented": true, "padding": "x".repeat(16 << 10) },
    })
}

/// The frame numbers of `events`, in order.
fn frames(events: &[Event]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Frame(frame) => Some(frame.frame),
            _ => None,
        })
        .collect()
}

#[test]
fn a_caller_that_stops_reading_stops_the_connection_reading_and_loses_nothing() {
    const SENT: u64 = 256;
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME],
        Box::new(|_, _| Reply::ResultThenLines {
            result: json!({ "windows": ["w1"] }),
            lines: (1..=SENT).map(padded_frame).collect(),
            then: AfterLines::Serve,
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    driver.windows().expect("the answer before the lines");
    // The caller reads nothing for a while: the connection holds its 16 lines, stops reading, and
    // the app's writes back up behind the socket's buffers. Wait until the app's writing stalls.
    let mut written = agent.lines_written();
    let stalled_by = Instant::now() + Duration::from_secs(20);
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let now = agent.lines_written();
        if now == written || Instant::now() >= stalled_by {
            break;
        }
        written = now;
    }
    let all = usize::try_from(SENT).expect("a small count") + 2;
    assert!(
        written < all,
        "the connection stopped reading: the app wrote {written} of {all} lines"
    );
    // Once the caller reads again, every line arrives, in order, none dropped.
    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while seen.len() < usize::try_from(SENT).expect("a small count") {
        assert!(
            Instant::now() < deadline,
            "only {} lines arrived",
            seen.len()
        );
        let (events, dropped) = driver.take_events();
        assert_eq!(dropped, 0, "no event was dropped");
        seen.extend(frames(&events));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(seen, (1..=SENT).collect::<Vec<_>>());
}

#[test]
fn lines_the_app_sent_before_it_closed_still_reach_the_caller_after_a_failed_send() {
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME],
        Box::new(|_, _| Reply::ResultThenLines {
            result: json!({ "windows": ["w1"] }),
            lines: vec![padded_frame(7)],
            then: AfterLines::Close,
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    driver
        .windows()
        .expect("the answer before the event and the close");
    agent
        .join(Duration::from_secs(5))
        .expect("the stand-in closed its end");
    // Let the connection's thread take the event and the end, and stop.
    std::thread::sleep(Duration::from_millis(300));
    match driver.windows() {
        Err(Error::Gone {
            kind: Some(HandleKind::Process),
            ref message,
            ..
        }) => assert!(
            message.contains("the app closed the connection"),
            "the first failure is the one reported: {message}"
        ),
        other => panic!("{other:?}"),
    }
    let (events, dropped) = driver.take_events();
    assert_eq!(
        (frames(&events), dropped),
        (vec![7], 0),
        "the event sent before the close is kept"
    );
}

#[test]
fn lines_waiting_for_the_caller_still_reach_it_when_a_write_to_the_app_fails() {
    // More than wait for a caller that is not reading, and few enough that the stand-in's own
    // buffer takes the rest, so it reaches its stop.
    const SENT: u64 = 20;
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME, method::InputInject::NAME],
        Box::new(|_, _| Reply::ResultThenLines {
            result: json!({ "windows": ["w1"] }),
            lines: (1..=SENT).map(frame_event).collect(),
            then: AfterLines::StopReading(Duration::from_secs(20)),
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    let mut window = driver.main_window().expect("the answer before the events");
    // Let the connection's thread fill the caller's room and stop reading.
    std::thread::sleep(Duration::from_millis(300));
    // A request larger than the stream's buffers, to an app that reads nothing: the write fails.
    let text = ActionSequence::new(vec![InputSource::Text {
        text: "x".repeat(900 << 10),
    }]);
    match window.perform(text) {
        Err(Error::Gone {
            kind: Some(HandleKind::Process),
            ref message,
            ..
        }) => assert!(
            message.contains("the app stopped reading"),
            "the write's failure is the one reported: {message}"
        ),
        other => panic!("{other:?}"),
    }
    let (events, dropped) = driver.take_events();
    assert_eq!(
        (frames(&events), dropped),
        ((1..=SENT).collect::<Vec<u64>>(), 0),
        "every event sent before the write failed is kept, in order"
    );
}

#[test]
fn an_element_that_turns_actionable_on_the_last_frame_is_acted_on() {
    let readings = Arc::new(AtomicUsize::new(0));
    let read = Arc::clone(&readings);
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::ElementActionability::NAME,
            method::WaitUntil::NAME,
            method::InputInject::NAME,
        ],
        Box::new(move |name, params| match name {
            "element.actionability" => {
                // Disabled until the frame that ends the wait.
                let enabled = read.fetch_add(1, Ordering::SeqCst) > 0;
                Reply::Result(json!({
                    "attached": true,
                    "rect": { "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 },
                    "visible": true,
                    "stable": true,
                    "enabled": enabled,
                    "receives": true,
                    "point": { "x": 5.0, "y": 5.0 },
                    "revision": 1,
                }))
            }
            "wait.until" => {
                // The frame presents at the very end of the wait's timeout.
                let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(5_000).min(2_000);
                std::thread::sleep(Duration::from_millis(timeout_ms));
                Reply::Result(json!({
                    "satisfied": true,
                    "frames": 1,
                    "observation": { "revision": 2, "count": 0 },
                }))
            }
            "input.inject" => Reply::Result(json!({
                "via": "synthetic",
                "effect": { "kind": "ran" },
            })),
            _ => Reply::Result(json!({ "windows": ["w1"] })),
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    let element: ElementRef = "w1/e1".parse().expect("an element handle");
    let tapped = driver
        .window(element.window)
        .element(element)
        .timeout(Duration::from_millis(150))
        .tap()
        .expect("the element turned actionable on the frame the wait ended on");
    assert_eq!(tapped.effect.kind, EffectKind::Ran);
    assert_eq!(
        readings.load(Ordering::SeqCst),
        2,
        "the facts were read again after the last frame"
    );
}

#[test]
fn actionability_is_waited_for_by_the_clock_on_a_window_that_draws_nothing() {
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::ElementActionability::NAME,
            method::WaitUntil::NAME,
            method::InputInject::NAME,
        ],
        Box::new(|name, params| match name {
            "element.actionability" => Reply::Result(json!({
                "attached": true,
                "rect": { "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 },
                "visible": true,
                "stable": true,
                "enabled": false,
                "receives": true,
                "point": { "x": 5.0, "y": 5.0 },
                "revision": 1,
            })),
            "wait.until" => {
                // A window that presents no frame holds a wait for all of its timeout.
                let timeout_ms = params["timeout_ms"].as_u64().unwrap_or(5_000).min(2_000);
                std::thread::sleep(Duration::from_millis(timeout_ms));
                Reply::Result(json!({
                    "satisfied": false,
                    "frames": 0,
                    "observation": { "revision": 1, "count": 0 },
                }))
            }
            _ => Reply::Result(json!({ "windows": ["w1"] })),
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    let element: ElementRef = "w1/e1".parse().expect("an element handle");
    let started = Instant::now();
    let tapped = driver
        .window(element.window)
        .element(element)
        .timeout(Duration::from_millis(300))
        .tap();
    assert!(
        matches!(
            tapped,
            Err(Error::NotActionable {
                failed: Some(flui_driver::protocol::act::ActionabilityCheck::Enabled),
                ..
            })
        ),
        "{tapped:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "a 300 ms timeout lasted {:?}",
        started.elapsed()
    );
}

#[test]
fn os_input_whose_effect_cannot_be_read_back_may_have_run() {
    let journal_reads = Arc::new(AtomicUsize::new(0));
    let reads = Arc::clone(&journal_reads);
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::WindowPlacement::NAME,
            method::InputJournal::NAME,
        ],
        Box::new(move |name, _| match name {
            "window.placement" => Reply::Result(json!({
                "window": "w1",
                "size": { "width": 800.0, "height": 600.0 },
                "scale": 1.0,
                "visible": true,
                "focused": true,
            })),
            "input.journal" if reads.fetch_add(1, Ordering::SeqCst) == 0 => {
                Reply::Result(json!({ "entries": [] }))
            }
            "input.journal" => Reply::Error(
                ErrorObject::new(ErrorCode::Timeout, "the window did not answer")
                    .retry(Retry::Soon),
            ),
            _ => Reply::Result(json!({ "windows": ["w1"] })),
        }),
    )
    .expect("the stand-in binds its endpoint");
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (lane, log) = ScriptedLane::new(Isolation::Xvfb);
    let mut driver = attach(&agent)
        .with_os_input_lock(dir.path().join("os-input.lock"))
        .with_input_policy(InputPolicy::require(InputPath::Os))
        .with_lane(lane)
        .expect("the lane starts");
    let window: WindowId = "w1".parse().expect("a window handle");
    let outcome = driver
        .window(window)
        .tap_at(LogicalPoint::new(10.0, 10.0))
        .expect("the input went out, so the tap answers its effect");
    assert_eq!(outcome.via, InputPath::Os);
    assert_eq!(outcome.effect.kind, EffectKind::MayHaveRun, "{outcome:?}");
    assert!(
        matches!(log.sent().first(), Some(OsEvent::PointerMove { .. })),
        "the lane sent the tap"
    );
    assert_eq!(journal_reads.load(Ordering::SeqCst), 2);
}

#[test]
fn the_desktop_lock_is_busy_only_while_another_session_holds_it() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("os-input.lock");
    let held = OsInputLock::acquire(&path).expect("the first session takes the lock");
    let refused = OsInputLock::acquire(&path);
    assert!(
        matches!(
            refused,
            Err(Error::Busy {
                retry: Retry::Soon,
                ..
            })
        ),
        "a held lock is busy, worth asking again soon: {refused:?}"
    );
    drop(held);
    drop(OsInputLock::acquire(&path).expect("free once the first session ends"));

    // What cannot be a lock file at all (here a directory) is refused as what it is, with no
    // retry: no session holds it, so waiting never makes it open.
    let refused = OsInputLock::acquire(dir.path());
    match refused {
        Err(ref error @ Error::Io(_)) => assert_eq!(error.retry(), Retry::Never),
        other => panic!("a lock file that cannot open is not busy: {other:?}"),
    }
}

#[test]
fn an_app_that_exits_before_its_agent_answers_reports_its_exit_code() {
    #[cfg(windows)]
    let launch = Launch::executable("cmd.exe").arg("/C").arg("exit 3");
    #[cfg(unix)]
    let launch = Launch::executable("/bin/sh").arg("-c").arg("exit 3");
    let started = Instant::now();
    let launched = Driver::launch(launch.connect_wait(Duration::from_secs(30)));
    match launched {
        Err(Error::Gone {
            kind: Some(HandleKind::Process),
            exit,
            ..
        }) => assert_eq!(exit, Some(3)),
        other => panic!("{other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the driver stopped waiting once the app had exited"
    );
}

#[test]
fn an_artifact_without_its_lane_is_named_as_the_capability_missing() {
    let launched = Driver::launch(Launch::artifact(flui_driver::Artifact::Apk(
        "app.apk".into(),
    )));
    match launched {
        Err(Error::NotSupported {
            capability: Some(capability),
            ..
        }) => assert_eq!(capability, "android_lane"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_lane_worker_cancels_a_queued_job_and_serves_past_a_panicking_one() {
    let (lane, log) = ScriptedLane::new(Isolation::Xvfb);
    let worker = LaneWorker::start(lane).expect("the worker starts");
    let gate = Arc::new(Barrier::new(2));
    let held = Arc::clone(&gate);
    let running = worker
        .submit(move |_| {
            held.wait();
        })
        .expect("the first job is queued");
    let queued = worker.submit(|_| ()).expect("the second job is queued");
    assert!(queued.cancel(), "a job that has not started is cancelled");
    gate.wait();
    running.wait().expect("the first job ran");
    let cancelled = queued.wait();
    assert!(
        matches!(cancelled, Err(Error::Lane { .. })),
        "{cancelled:?}"
    );

    let releases = log.releases();
    #[expect(
        clippy::disallowed_methods,
        reason = "the test catches the job's panic, resumed on this thread, to assert on it"
    )]
    let resumed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker.run(|_| -> () { panic!("a lane's code panics") })
    }));
    assert!(
        resumed.is_err(),
        "the job's panic is resumed on the waiting thread"
    );
    assert_eq!(
        log.releases(),
        releases + 1,
        "the lane released what it held"
    );
    assert_eq!(worker.run(|_| 7).expect("the worker serves on"), 7);
}
