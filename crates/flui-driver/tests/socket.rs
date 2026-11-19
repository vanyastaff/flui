//! The socket transport and process ownership: an app that closes mid-call, an app that
//! exits before it answers, and a lane worker that keeps serving past a cancelled or panicking job.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use flui_driver::lane::LaneWorker;
use flui_driver::os_input::OsInputLock;
use flui_driver::protocol::act::EffectKind;
use flui_driver::protocol::error::{ErrorCode, ErrorObject, HandleKind, Retry};
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::method::{self, Method};
use flui_driver::protocol::tree::{ElementRef, WindowId};
use flui_driver::steps::OsEvent;
use flui_driver::testing::{Reply, ScriptedAgent, ScriptedLane};
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
fn a_lock_file_another_user_owns_is_busy() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("os-input.lock");
    std::fs::write(&path, b"").expect("the lock file");
    let mut permissions = std::fs::metadata(&path)
        .expect("its metadata")
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).expect("read-only, as another user's file is");
    let refused = OsInputLock::acquire(&path);
    assert!(matches!(refused, Err(Error::Busy { .. })), "{refused:?}");
    let mut writable = std::fs::metadata(&path)
        .expect("its metadata")
        .permissions();
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test gives its own file back so its directory can be removed"
    )]
    writable.set_readonly(false);
    let _ = std::fs::set_permissions(&path, writable);
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
