//! The socket transport and process ownership: an app that closes mid-call, an app that
//! exits before it answers, and a lane worker that keeps serving past a cancelled or panicking job.

use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use flui_driver::lane::LaneWorker;
use flui_driver::protocol::error::HandleKind;
use flui_driver::protocol::method::{self, Method};
use flui_driver::testing::{Reply, ScriptedAgent, ScriptedLane};
use flui_driver::{Driver, Error, Isolation, Launch};
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
    agent.join().expect("the stand-in served its connection");
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
