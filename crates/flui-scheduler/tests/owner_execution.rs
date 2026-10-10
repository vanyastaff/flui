//! Complete owner entry protects preparation as part of the admitted turn.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use flui_scheduler::{
    ExecutionError, FrameOutcome, IdleDeadline, Instant, OwnerFrame, UpdateScheduler,
    WeakUpdateScheduler,
};

fn nested_preparation_is_refused_before_invocation() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("owner");
    let nested_preparation = Cell::new(false);
    let nested_pipeline = Cell::new(false);
    let prepared = Cell::new(false);
    let now = Instant::now();
    let result = owner.drive_frame(
        now,
        IdleDeadline::far_future(now),
        || {
            let refused = owner.drive_frame(
                now,
                IdleDeadline::far_future(now),
                || nested_preparation.set(true),
                || nested_pipeline.set(true),
            );
            assert_eq!(refused, Err(ExecutionError::AlreadyExecuting));
            assert!(!nested_preparation.get());
            assert!(!nested_pipeline.get());
            prepared.set(true);
        },
        || {
            assert!(prepared.get());
            42
        },
    );
    assert_eq!(result, Ok(42));
}

fn failed_preparation_does_not_fabricate_a_frame() {
    let scheduler = UpdateScheduler::new();
    let owner = OwnerFrame::new(&scheduler).expect("owner");
    let task_ran = Rc::new(Cell::new(false));
    let observed = Rc::clone(&task_ran);
    let _task = owner.async_driver().spawn_local(Box::pin(async move {
        observed.set(true);
    }));
    let transient_ran = Rc::new(Cell::new(false));
    let observed = Rc::clone(&transient_ran);
    scheduler.schedule_frame_callback(Box::new(move |_| observed.set(true)));
    let mut completed = pin!(scheduler.end_of_frame());
    let mut context = Context::from_waker(Waker::noop());
    assert!(completed.as_mut().poll(&mut context).is_pending());
    let pipeline_ran = Cell::new(false);
    let now = Instant::now();
    let failed = catch_unwind(AssertUnwindSafe(|| {
        owner.drive_frame(
            now,
            IdleDeadline::far_future(now),
            || panic!("preparation failed"),
            || pipeline_ran.set(true),
        )
    }))
    .expect_err("preparation must fail");
    assert_eq!(failed.downcast_ref::<&str>(), Some(&"preparation failed"));
    assert!(!pipeline_ran.get());
    assert!(!task_ran.get());
    assert!(!transient_ran.get());
    assert!(completed.as_mut().poll(&mut context).is_pending());
    owner
        .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
        .expect("recovery frame");
    assert!(task_ran.get());
    assert!(transient_ran.get());
    assert!(matches!(
        completed.as_mut().poll(&mut context),
        Poll::Ready(Ok(FrameOutcome::Completed { .. }))
    ));
}

struct HostileCapture {
    name: &'static str,
    drops: Rc<Cell<usize>>,
}

impl Drop for HostileCapture {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        panic!("{}", self.name);
    }
}

fn callable_retirement_preserves_the_first_failure() {
    for refused in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("owner");
        if refused {
            assert!(owner.retire().is_none());
        }
        let preparation_drops = Rc::new(Cell::new(0));
        let pipeline_drops = Rc::new(Cell::new(0));
        let preparation = HostileCapture {
            name: "preparation retirement",
            drops: Rc::clone(&preparation_drops),
        };
        let pipeline = HostileCapture {
            name: "pipeline retirement",
            drops: Rc::clone(&pipeline_drops),
        };
        let now = Instant::now();
        let failure = catch_unwind(AssertUnwindSafe(|| {
            owner.drive_frame(
                now,
                IdleDeadline::far_future(now),
                move || {
                    std::hint::black_box(&preparation);
                },
                move || {
                    std::hint::black_box(&pipeline);
                },
            )
        }))
        .expect_err("first callable retirement must fail");
        assert_eq!(
            failure.downcast_ref::<String>().map(String::as_str),
            Some("preparation retirement")
        );
        assert_eq!(preparation_drops.get(), 1);
        assert_eq!(pipeline_drops.get(), 0, "later opaque callable is retained");
        if !refused {
            owner
                .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
                .expect("next healthy frame");
        }
    }
}

struct TerminalCapture {
    weak: WeakUpdateScheduler,
    drops: Rc<Cell<usize>>,
    fail: bool,
}

impl Drop for TerminalCapture {
    fn drop(&mut self) {
        assert!(
            self.weak.upgrade().is_none(),
            "terminal cleanup cannot revive authority"
        );
        self.drops.set(self.drops.get() + 1);
        assert!(!self.fail, "terminal capture retirement");
    }
}

struct ProducedResult(Rc<Cell<usize>>);

impl Drop for ProducedResult {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

fn terminal_release_closes_authority_and_preserves_result_custody() {
    for fail in [false, true] {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("owner");
        let driver = owner.async_driver();
        let post_frame = owner.post_frame_handle();
        let pending = driver.spawn_local(Box::pin(std::future::pending()));
        let weak = scheduler.downgrade();
        let capture_drops = Rc::new(Cell::new(0));
        let capture = TerminalCapture {
            weak: weak.clone(),
            drops: Rc::clone(&capture_drops),
            fail,
        };
        scheduler.add_persistent_frame_callback(Rc::new(move |_| {
            std::hint::black_box(&capture);
        }));
        let producer = Rc::new(RefCell::new(Some(scheduler)));
        let result_drops = Rc::new(Cell::new(0));
        let sibling_scheduler = UpdateScheduler::new();
        let sibling = OwnerFrame::new(&sibling_scheduler).expect("independent owner");
        let now = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            owner.drive_frame(
                now,
                IdleDeadline::far_future(now),
                || {},
                || {
                    let last_producer = producer.borrow_mut().take();
                    drop(last_producer);
                    ProducedResult(Rc::clone(&result_drops))
                },
            )
        }));
        if fail {
            let failure = result.err().expect("terminal failure escapes");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&"terminal capture retirement")
            );
            assert_eq!(result_drops.get(), 0, "output remains in failure custody");
        } else {
            let output = result.expect("healthy retirement").expect("admitted turn");
            assert_eq!(result_drops.get(), 0);
            drop(output);
            assert_eq!(result_drops.get(), 1);
        }
        assert_eq!(capture_drops.get(), 1);
        assert!(weak.upgrade().is_none());
        assert!(pending.is_cancelled());
        let late = driver.spawn_local(Box::pin(async {}));
        assert!(late.is_cancelled());
        assert!(post_frame.schedule(|_| {}).is_err());
        assert_eq!(owner.pump_background(|| {}), Err(ExecutionError::Retired));
        sibling
            .drive_frame(now, IdleDeadline::far_future(now), || {}, || {})
            .expect("sibling still executes");
    }
}

#[test]
fn admitted_frame_preparation_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "nested_preparation_is_refused_before_invocation",
            nested_preparation_is_refused_before_invocation,
        ),
        (
            "failed_preparation_does_not_fabricate_a_frame",
            failed_preparation_does_not_fabricate_a_frame,
        ),
        (
            "callable_retirement_preserves_the_first_failure",
            callable_retirement_preserves_the_first_failure,
        ),
        (
            "terminal_release_closes_authority_and_preserves_result_custody",
            terminal_release_closes_authority_and_preserves_result_custody,
        ),
    ];
    if let Ok(selected) = std::env::var("FLUI_OWNER_EXECUTION_CASE") {
        let (_, case) = cases
            .iter()
            .find(|(name, _)| *name == selected)
            .expect("child case");
        case();
        return;
    }
    let mut failures = Vec::new();
    for (name, _) in cases {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "owner_execution::admitted_frame_preparation_contract",
                "--exact",
                "--nocapture",
            ])
            .env("FLUI_OWNER_EXECUTION_CASE", name)
            .output()
            .expect("child test process");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || !stdout.contains("running 1 test")
            || !stdout.contains("test result: ok. 1 passed; 0 failed;")
        {
            failures.push(format!(
                "{name}: {}\n{}",
                stdout,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
