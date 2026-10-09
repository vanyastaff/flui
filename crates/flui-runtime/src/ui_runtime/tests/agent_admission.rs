//! The private close seam exercises a host wake re-entering the real public port.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn run_child(action: bool) {
    let target = Arc::new(Mutex::new(Weak::<DevAgentPort>::new()));
    let target_in_wake = Arc::clone(&target);
    let closed = Arc::new(AtomicUsize::new(0));
    let closed_in_wake = Arc::clone(&closed);
    let ui_runtime = super::super::tests::new_runtime(Arc::new(move || {
        let port = target_in_wake.lock().upgrade();
        if let Some(port) = port {
            port.close();
            closed_in_wake.fetch_add(1, Ordering::Relaxed);
        }
    }))
    .expect("runtime");
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("window");
    let port = {
        let state = ui_runtime
            .presentations
            .get(ui_runtime.presentation_id())
            .expect("presentation");
        let slot = state.dev_agent.borrow();
        Arc::clone(&slot.as_ref().expect("vended port").agent)
    };
    *target.lock() = Arc::downgrade(&port);
    let request = || {
        ActionRequest::new(
            ElementId::from_u64(1).expect("non-zero"),
            ActionName::Invoke,
        )
    };
    if action {
        let mut answer = window.act(request()).expect("accepted before close");
        assert!(answer.try_take().is_none());
        assert_eq!(ui_runtime.drain_commands().invoked, 1);
        assert!(answer.try_take().expect("owner answered").is_err());
    } else {
        let mut answer = window
            .read(ReadQuery::new())
            .expect("accepted before close");
        assert!(answer.try_take().is_none());
        assert_eq!(ui_runtime.drain_commands().invoked, 1);
        assert!(answer.try_take().expect("owner answered").is_err());
    }
    assert!(!window.is_open());
    assert_eq!(closed.load(Ordering::Relaxed), 1);
    assert_eq!(
        window.read(ReadQuery::new()).expect_err("closed").code(),
        ErrorCode::Gone
    );
    assert_eq!(
        window.act(request()).expect_err("closed").code(),
        ErrorCode::Gone
    );
    assert_eq!(
        ui_runtime.drain_commands(),
        super::super::DrainReport::default()
    );
}

fn child(kind: &str) {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "ui_runtime::agent::admission_tests::agent_port_admission_matrix",
            "--nocapture",
        ])
        .env("FLUI_AGENT_ADMISSION_CHILD", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("child");
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().expect("status").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            let output = child.wait_with_output().expect("reap child");
            panic!("agent admission deadlocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed; 0 failed;"),
        "agent admission failed: {output:?}"
    );
}
fn read_wake_can_close_admission() {
    child("read");
}
fn action_wake_can_close_admission() {
    child("action");
}

#[test]
fn agent_port_admission_matrix() {
    if let Ok(kind) = std::env::var("FLUI_AGENT_ADMISSION_CHILD") {
        match kind.as_str() {
            "read" => run_child(false),
            "action" => run_child(true),
            _ => panic!("unknown admission child"),
        }
        return;
    }
    crate::table_test::run_table(
        "agent_port_admission_matrix",
        &[
            (
                "agent_motion_sets_rate_and_steps_a_paused_window",
                agent_motion_sets_rate_and_steps_a_paused_window,
            ),
            (
                "read_wake_can_close_admission",
                read_wake_can_close_admission as fn(),
            ),
            (
                "action_wake_can_close_admission",
                action_wake_can_close_admission as fn(),
            ),
        ],
    );
}

fn agent_motion_sets_rate_and_steps_a_paused_window() {
    use flui_protocol::MotionRequest;
    let runtime = super::super::tests::new_runtime(Arc::new(|| {})).expect("runtime");
    let window = runtime
        .dev_agent_window(runtime.presentation_id())
        .expect("window");
    let mut paused = window
        .motion(MotionRequest::new().with_rate(0.0))
        .expect("enqueued");
    assert!(
        paused.try_take().is_none(),
        "a worker cannot mutate the clock inline"
    );
    assert_eq!(runtime.drain_commands().invoked, 1);
    let state = paused
        .try_take()
        .expect("owner answered")
        .expect("valid motion request");
    assert_eq!(state.rate, 0.0);
    let mut stepped = window
        .motion(MotionRequest::new().with_step_ms(100))
        .expect("enqueued");
    assert_eq!(runtime.drain_commands().invoked, 1);
    let next = stepped
        .try_take()
        .expect("owner answered")
        .expect("valid step");
    assert_eq!(next.rate, 0.0);
    assert!((next.time_ms - state.time_ms - 100.0).abs() < 1e-9);
    for rate in [-1.0, f64::NAN, f64::INFINITY] {
        let mut refused = window
            .motion(MotionRequest::new().with_rate(rate).with_step_ms(500))
            .expect("enqueued");
        assert_eq!(runtime.drain_commands().invoked, 1);
        assert_eq!(
            refused
                .try_take()
                .expect("answered")
                .expect_err("invalid rate")
                .code(),
            ErrorCode::InvalidArgument
        );
    }
    let mut observed = window.motion(MotionRequest::new()).expect("enqueued");
    assert_eq!(runtime.drain_commands().invoked, 1);
    assert_eq!(
        observed.try_take().expect("answered").expect("snapshot"),
        next,
        "an invalid request applies neither rate nor step"
    );
    drop(runtime);
    assert_eq!(
        window
            .motion(MotionRequest::new())
            .expect_err("closed")
            .code(),
        ErrorCode::Gone
    );
}
