//! Public dedicated bootstrap and coordinator acceptance on the headless host.
use flui_app::testing::HeadlessRun;
use flui_app::{AppHandle, Application, RuntimeWindowConfig, WindowRequest, open_dedicated_window};
use flui_view::prelude::*;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    thread::ThreadId,
    time::{Duration, Instant},
};

#[derive(Clone, StatelessView)]
struct LocalRoot {
    capture: Rc<LocalCapture>,
}

struct LocalCapture {
    created: ThreadId,
    builds: Cell<usize>,
    events: mpsc::Sender<(&'static str, ThreadId)>,
}

impl Drop for LocalCapture {
    fn drop(&mut self) {
        assert_eq!(self.created, std::thread::current().id());
        let _ = self.events.send(("drop", std::thread::current().id()));
    }
}

impl StatelessView for LocalRoot {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        assert_eq!(self.capture.created, std::thread::current().id());
        self.capture.builds.set(self.capture.builds.get() + 1);
        self.capture
            .events
            .send(("build", std::thread::current().id()))
            .expect("observer lives");
        flui_widgets::SizedBox::square(20.0)
    }
}

fn completed(
    request: &mut WindowRequest,
) -> Result<flui_foundation::PresentationAddress, flui_app::AppWindowError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = request.try_result() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "dedicated launch did not settle independently"
        );
        std::thread::yield_now();
    }
}

#[test]
fn dedicated_factory_constructs_and_retires_local_captures_on_its_owner() {
    let handle = Arc::new(Mutex::new(None::<AppHandle>));
    let ready = Arc::clone(&handle);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0))
            .on_ready(move |handle| *ready.lock().expect("ready") = Some(handle.clone())),
    );
    let (events, observed) = mpsc::channel();
    let mut receipt = open_dedicated_window(move || {
        let created = std::thread::current().id();
        events.send(("factory", created)).expect("observer lives");
        (
            LocalRoot {
                capture: Rc::new(LocalCapture {
                    created,
                    builds: Cell::new(0),
                    events,
                }),
            },
            RuntimeWindowConfig::new(),
        )
    })
    .expect("headless dedicated admission");
    completed(&mut receipt).expect("mounted on destination");
    let (kind, owner) = observed
        .recv_timeout(Duration::from_secs(10))
        .expect("factory ran");
    assert_eq!(kind, "factory");
    assert_ne!(owner, std::thread::current().id());
    assert_eq!(
        observed
            .recv_timeout(Duration::from_secs(10))
            .expect("root built"),
        ("build", owner)
    );
    handle
        .lock()
        .expect("handle")
        .as_ref()
        .expect("ready called")
        .request_quit()
        .expect("global quit");
    let deadline = Instant::now() + Duration::from_secs(10);
    while run.is_running() {
        run.run_owner_turn();
        assert!(
            Instant::now() < deadline,
            "global quit did not retire all owners"
        );
        std::thread::yield_now();
    }
    assert_eq!(run.join().expect("run completed").code(), 0);
    let events = observed.try_iter().collect::<Vec<_>>();
    assert!(
        events.contains(&("drop", owner)),
        "local capture was not destroyed"
    );
    assert!(events.iter().all(|(_, thread)| *thread == owner));
}

#[test]
fn factory_panic_settles_receipt_and_allows_another_owner() {
    let run = HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    let mut failed = open_dedicated_window(|| -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
        panic!("factory failure")
    })
    .expect("accepted");
    assert!(matches!(
        completed(&mut failed),
        Err(flui_app::AppWindowError::FactoryPanicked { .. })
    ));
    let mut next = open_dedicated_window(|| {
        (
            flui_widgets::SizedBox::square(20.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("next accepted");
    completed(&mut next).expect("next owner mounted");
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[test]
fn global_quit_waits_for_starting_owner_and_consults_its_window() {
    let handle = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&handle);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0)).on_ready(move |handle| {
            ready.replace(Some(handle.clone()));
        }),
    );
    let (release, held) = mpsc::channel();
    let (entered, started) = mpsc::channel();
    let (consulted, answer) = mpsc::channel();
    let receipt = open_dedicated_window(move || {
        entered.send(()).expect("observer alive");
        held.recv().expect("factory released");
        (
            flui_widgets::SizedBox::square(20.0),
            RuntimeWindowConfig::new().with_close_request_handler(
                flui_app::CloseRequestHandler::new(move |_| {
                    consulted.send(()).expect("observer alive");
                    flui_app::CloseResponse::KeepOpen
                }),
            ),
        )
    })
    .expect("starting owner admitted");
    started
        .recv_timeout(Duration::from_secs(10))
        .expect("factory entered");
    // Admission survives abandoning the observer.
    drop(receipt);
    handle
        .borrow()
        .as_ref()
        .expect("ready")
        .request_quit()
        .expect("quit admitted");
    for _ in 0..3 {
        run.run_owner_turn();
    }
    assert!(run.is_running(), "quit overtook an admitted factory");
    release.send(()).expect("release factory");
    answer
        .recv_timeout(Duration::from_secs(10))
        .expect("new window joined consultation");
    for _ in 0..3 {
        run.run_owner_turn();
    }
    assert!(run.is_running(), "new window's refusal was lost");
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[test]
fn saturated_owner_admission_does_not_execute_rejected_factory_or_block_quit() {
    let limit = std::num::NonZeroUsize::MIN;
    let limits = flui_app::OwnerLimits::default()
        .with_owners(limit)
        .with_launches(limit)
        .with_commands(limit);
    let handle = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&handle);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0))
            .with_config(flui_app::AppConfig::new().with_owner_limits(limits))
            .on_ready(move |handle| {
                ready.replace(Some(handle.clone()));
            }),
    );
    let (release, held) = mpsc::channel();
    let mut first = open_dedicated_window(move || {
        held.recv().expect("release");
        (
            flui_widgets::SizedBox::square(20.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("first admitted");
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let rejected = Arc::clone(&called);
    let result = open_dedicated_window(move || {
        rejected.store(true, std::sync::atomic::Ordering::SeqCst);
        (
            flui_widgets::SizedBox::square(20.0),
            RuntimeWindowConfig::new(),
        )
    });
    assert!(matches!(
        result,
        Err(flui_app::AppWindowError::OwnerCapacity { capacity: 1 })
    ));
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
    let handle = handle.borrow().as_ref().expect("ready").clone();
    let ordinary = handle.request_show_main_window().expect("ordinary slot");
    assert!(matches!(
        handle.request_show_main_window(),
        Err(flui_app::AppControlError::Capacity { capacity: 1 })
    ));
    handle
        .request_quit()
        .expect("reserved quit lane survives saturation");
    release.send(()).expect("released");
    completed(&mut first).expect("accepted launch completed");
    drop(ordinary);
    let deadline = Instant::now() + Duration::from_secs(10);
    while run.is_running() {
        run.run_owner_turn();
        assert!(
            Instant::now() < deadline,
            "saturation prevented shutdown: {}",
            flui_app::testing::owner_diagnostics()
        );
        std::thread::yield_now();
    }
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[test]
fn cross_owner_document_rejects_stale_reply_without_losing_edit() {
    use flui_app::DocumentOutcome;
    let handle = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&handle);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0)).on_ready(move |handle| {
            ready.replace(Some(handle.clone()));
        }),
    );
    let document = handle
        .borrow()
        .as_ref()
        .expect("ready")
        .create_document(
            b"initial".to_vec(),
            std::num::NonZeroUsize::new(32).expect("limit"),
        )
        .expect("document");
    let first = document.bind().expect("initial owner's port");
    let (send, receive) = mpsc::channel();
    let mut launch = open_dedicated_window(move || {
        send.send(document.bind().expect("destination port"))
            .expect("observer");
        (
            flui_widgets::SizedBox::square(10.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("dedicated owner");
    completed(&mut launch).expect("mounted");
    let second = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("second owner bound");
    let mut old_read = second.read().expect("read admitted");
    run.run_owner_turn();
    // Leave the first answer unobserved while a newer edit commits.
    let mut current = first.read().expect("first read");
    run.run_owner_turn();
    let Some(DocumentOutcome::Current(current)) = current.try_result() else {
        panic!("current read did not complete");
    };
    let mut write = first
        .replace(current.revision(), b"accepted edit".to_vec())
        .expect("write admitted");
    run.run_owner_turn();
    let Some(DocumentOutcome::Current(written)) = write.try_result() else {
        panic!("write did not complete");
    };
    let Some(DocumentOutcome::Current(stale)) = old_read.try_result() else {
        panic!("old read did not complete");
    };
    assert_ne!(stale.revision(), written.revision());
    let edit = b"late edit".to_vec();
    let mut late = second
        .replace(stale.revision(), edit.clone())
        .expect("transport admitted");
    run.run_owner_turn();
    match late.try_result().expect("late operation answered") {
        DocumentOutcome::Conflict { current, rejected } => {
            assert_eq!(current.bytes(), b"accepted edit");
            assert_eq!(rejected, edit);
        }
        other => panic!("stale reply overwrote an accepted edit: {other:?}"),
    }
    assert_eq!(run.join().expect("run completed").code(), 0);
    assert!(matches!(
        second.read(),
        Err(flui_app::DocumentSendError {
            reason: flui_app::DocumentRefusal::OwnerGone,
            ..
        })
    ));
}

#[test]
fn document_payload_budget_covers_unobserved_completions_and_quit() {
    use flui_app::{DocumentOutcome, DocumentRefusal};
    let handle = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&handle);
    let limits = flui_app::OwnerLimits::default()
        .with_payload_bytes(std::num::NonZeroUsize::new(16).expect("limit"));
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0))
            .with_config(flui_app::AppConfig::new().with_owner_limits(limits))
            .on_ready(move |handle| {
                ready.replace(Some(handle.clone()));
            }),
    );
    let handle = handle.borrow().as_ref().expect("ready").clone();
    let document = handle
        .create_document(
            b"value".to_vec(),
            std::num::NonZeroUsize::new(8).expect("limit"),
        )
        .expect("document");
    let port = document.bind().expect("port");
    let mut first = port.read().expect("8 bytes reserved");
    let second = port.read().expect("8 more bytes reserved");
    run.run_owner_turn();
    assert!(
        matches!(
            port.read(),
            Err(flui_app::DocumentSendError {
                reason: DocumentRefusal::Full,
                ..
            })
        ),
        "unobserved answers lost their byte reservations"
    );
    let Some(DocumentOutcome::Current(snapshot)) = first.try_result() else {
        panic!("first answered");
    };
    drop(second);
    let oversized = vec![0; 9];
    let refused = port
        .replace(snapshot.revision(), oversized.clone())
        .expect_err("allocation too large");
    assert_eq!(refused.reason, DocumentRefusal::Oversize);
    assert_eq!(refused.rejected, Some(oversized));
    let mut accepted = port
        .replace(snapshot.revision(), b"new".to_vec())
        .expect("budget released");
    // Teardown refuses this accepted but unserved command, retaining its edit.
    handle
        .request_quit()
        .expect("reliable control is independent of bytes");
    assert_eq!(run.join().expect("run completed").code(), 0);
    match accepted
        .try_result()
        .expect("shutdown settles accepted command")
    {
        DocumentOutcome::Refused(error) => {
            assert_eq!(error.reason, DocumentRefusal::OwnerGone);
            assert_eq!(error.rejected.as_deref(), Some(b"new".as_slice()));
        }
        other => panic!("unexpected shutdown result: {other:?}"),
    }
}

enum OwnerPolicy {}
struct RetirementPanic;
struct FactoryPayload {
    retired: mpsc::Sender<ThreadId>,
}
impl Drop for FactoryPayload {
    fn drop(&mut self) {
        self.retired
            .send(std::thread::current().id())
            .expect("observer alive");
    }
}

#[test]
fn factory_panic_payload_retires_on_its_owner_for_both_launch_paths() {
    let (retired, events) = mpsc::channel();
    let run = HeadlessRun::start(Application::new(move |_| -> flui_widgets::SizedBox {
        std::panic::panic_any(FactoryPayload {
            retired: retired.clone(),
        });
    }));
    run.run_owner_turn();
    assert!(matches!(
        run.join(),
        Err(flui_app::AppRunError::InitialWindow(_))
    ));
    assert_eq!(
        events
            .recv_timeout(Duration::from_secs(1))
            .expect("main factory payload was destroyed"),
        std::thread::current().id()
    );

    let run = HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    let (retired, events) = mpsc::channel();
    let mut launch =
        open_dedicated_window(move || -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
            std::panic::panic_any(FactoryPayload { retired });
        })
        .expect("dedicated admitted");
    assert!(matches!(
        completed(&mut launch),
        Err(flui_app::AppWindowError::FactoryPanicked { .. })
    ));
    assert_ne!(
        events
            .recv_timeout(Duration::from_secs(1))
            .expect("dedicated factory payload was destroyed"),
        std::thread::current().id()
    );
    assert_eq!(run.join().expect("run completed").code(), 0);
}

impl Drop for RetirementPanic {
    fn drop(&mut self) {
        panic!("published owner retirement failure");
    }
}
#[derive(Clone, StatelessView)]
struct RetirementRoot {
    retained: Rc<RetirementPanic>,
}
impl StatelessView for RetirementRoot {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        assert!(Rc::strong_count(&self.retained) > 0);
        flui_widgets::SizedBox::square(10.0)
    }
}

#[test]
fn published_owner_failure_is_retained_until_application_result() {
    let run = HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    let mut launch = open_dedicated_window(|| {
        (
            RetirementRoot {
                retained: Rc::new(RetirementPanic),
            },
            RuntimeWindowConfig::new(),
        )
    })
    .expect("admitted");
    completed(&mut launch).expect("published before failure");
    let error = run
        .join()
        .expect_err("successful publication must not swallow retirement failure");
    assert!(
        matches!(error, flui_app::AppRunError::DedicatedOwner(_)),
        "{error:?}"
    );
}

#[test]
fn last_owner_retirement_rechecks_windowless_application_exit_policy() {
    let run = HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    run.run_owner_turn();
    let (release, held) = mpsc::channel();
    let (entered, ready) = mpsc::channel();
    let mut launch =
        open_dedicated_window(move || -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
            entered.send(()).expect("observer");
            held.recv().expect("released");
            panic!("last starting owner fails");
        })
        .expect("worker admitted");
    ready
        .recv_timeout(Duration::from_secs(10))
        .expect("worker started");
    assert!(run.open_windows()[0].simulate_close());
    run.run_owner_turn();
    assert!(run.is_running(), "initial loop outlives a starting worker");
    release.send(()).expect("release worker");
    assert!(completed(&mut launch).is_err());
    let deadline = Instant::now() + Duration::from_secs(2);
    while run.is_running() && Instant::now() < deadline {
        run.run_owner_turn();
        std::thread::yield_now();
    }
    assert!(
        !run.is_running(),
        "retirement did not re-evaluate the windowless application's exit policy"
    );
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[test]
fn completed_launch_receipts_keep_their_admission_budget_until_observed() {
    let limits = flui_app::OwnerLimits::default().with_launches(std::num::NonZeroUsize::MIN);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0))
            .with_config(flui_app::AppConfig::new().with_owner_limits(limits)),
    );
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let (release, held) = mpsc::channel();
    let (finished, observed) = mpsc::channel();
    let mut first = open_dedicated_window(move || {
        held.recv().expect("factory released after receipt polling");
        (
            flui_widgets::SizedBox::square(10.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("first admitted");
    let waker = Waker::from(Arc::new(CompletionNotice(finished)));
    assert!(matches!(
        Pin::new(&mut first).poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    release.send(()).expect("release");
    observed
        .recv_timeout(Duration::from_secs(10))
        .expect("completion committed before notification");
    assert!(matches!(
        open_dedicated_window(|| (
            flui_widgets::SizedBox::square(1.0),
            RuntimeWindowConfig::new()
        )),
        Err(flui_app::AppWindowError::LaunchCapacity { capacity: 1 })
    ));
    completed(&mut first).expect("first observed");
    let mut second = open_dedicated_window(|| {
        (
            flui_widgets::SizedBox::square(1.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("observation frees capacity");
    completed(&mut second).expect("second installed");
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[test]
fn completed_main_receipt_keeps_the_shared_command_budget_until_observed() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let saved = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&saved);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0))
            .with_config(flui_app::AppConfig::new().with_owner_limits(
                flui_app::OwnerLimits::default().with_commands(std::num::NonZeroUsize::MIN),
            ))
            .on_ready(move |handle| {
                ready.replace(Some(handle.clone()));
            }),
    );
    let handle = saved.borrow().as_ref().expect("ready").clone();
    let document = handle
        .create_document(b"v".to_vec(), std::num::NonZeroUsize::MIN)
        .expect("document");
    let port = document.bind().expect("bound");
    let mut receipt = handle.request_show_main_window().expect("admitted");
    let (finished, observed) = mpsc::channel();
    let waker = Waker::from(Arc::new(CompletionNotice(finished)));
    assert!(matches!(
        Pin::new(&mut receipt).poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        run.run_owner_turn();
        if observed.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "main receipt did not complete");
    }
    assert!(
        matches!(
            port.read(),
            Err(flui_app::DocumentSendError {
                reason: flui_app::DocumentRefusal::Full,
                ..
            })
        ),
        "unread main receipt must retain its command slot"
    );
    assert!(matches!(
        handle.request_show_main_window(),
        Err(flui_app::AppControlError::Capacity { .. })
    ));
    assert!(receipt.try_result().expect("completed").is_ok());
    let unread = port.read().expect("observing releases the slot");
    handle
        .request_quit()
        .expect("reserved quit bypasses ordinary saturation");
    assert_eq!(run.join().expect("run completed").code(), 0);
    drop(unread);
}

struct ThreadRetirement {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Drop for ThreadRetirement {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv_timeout(Duration::from_secs(10));
    }
}

#[test]
fn owner_capacity_includes_thread_local_retirement() {
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0)).with_config(
            flui_app::AppConfig::new().with_owner_limits(
                flui_app::OwnerLimits::default().with_owners(std::num::NonZeroUsize::MIN),
            ),
        ),
    );
    let (entered, retiring) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let mut failed = open_dedicated_window(move || -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
        std::thread_local! {
            static RETIREMENT: std::cell::RefCell<Option<ThreadRetirement>> = const { std::cell::RefCell::new(None) };
        }
        RETIREMENT.with(|slot| {
            slot.replace(Some(ThreadRetirement { entered, release: held }));
        });
        panic!("retire after failed construction");
    }).expect("admitted");
    assert!(completed(&mut failed).is_err());
    retiring
        .recv_timeout(Duration::from_secs(10))
        .expect("TLS retirement began");
    let concurrent = open_dedicated_window(|| {
        (
            flui_widgets::SizedBox::square(20.0),
            RuntimeWindowConfig::new(),
        )
    });
    // Release before asserting so a failing control cannot strand the run's join.
    let _ = release.send(());
    let refused = matches!(
        concurrent,
        Err(flui_app::AppWindowError::OwnerCapacity { capacity: 1 })
    );
    drop(concurrent);
    assert_eq!(run.join().expect("run completed").code(), 0);
    assert!(
        refused,
        "owner slot was released before its thread completed"
    );
}

#[test]
fn global_quit_keeps_initial_loop_until_full_thread_retirement() {
    let saved = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&saved);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0)).on_ready(move |handle| {
            ready.replace(Some(handle.clone()));
        }),
    );
    let (finish_factory, factory_held) = mpsc::channel();
    let (entered, retiring) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let mut request = open_dedicated_window(move || -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
        std::thread_local! {
            static RETIREMENT: std::cell::RefCell<Option<ThreadRetirement>> = const { std::cell::RefCell::new(None) };
        }
        RETIREMENT.with(|slot| { slot.replace(Some(ThreadRetirement { entered, release: held })); });
        factory_held.recv().expect("release factory");
        panic!("fail a launch after quit consultation started");
    }).expect("admitted");
    saved
        .borrow()
        .as_ref()
        .expect("ready")
        .request_quit()
        .expect("quit admitted");
    run.run_owner_turn();
    finish_factory.send(()).expect("release factory");
    assert!(completed(&mut request).is_err());
    retiring
        .recv_timeout(Duration::from_secs(10))
        .expect("TLS retirement began");
    for _ in 0..4 {
        run.run_owner_turn();
    }
    if let Some(window) = run.open_windows().first() {
        let _ = window.simulate_close();
    }
    run.run_owner_turn();
    let initial_alive = run.is_running();
    let _ = release.send(());
    assert_eq!(run.join().expect("run completed").code(), 0);
    assert!(
        initial_alive,
        "initial loop retired before the owner thread completed"
    );
}

struct CompletionNotice(mpsc::Sender<()>);
impl std::task::Wake for CompletionNotice {
    fn wake(self: Arc<Self>) {
        self.0.send(()).expect("observer alive");
    }
}

struct ReentrantDocumentWake {
    port: flui_app::DocumentPort,
    admitted: Mutex<Vec<flui_app::DocumentRequest>>,
}
impl std::task::Wake for ReentrantDocumentWake {
    fn wake(self: Arc<Self>) {
        let request = self
            .port
            .read()
            .expect("reentry observes committed state without a lock");
        self.admitted.lock().expect("receipts").push(request);
        panic!("receiver wake panicked after accepting another request");
    }
}

#[test]
fn document_completion_reentry_and_panicking_wake_preserve_all_accepted_work() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let saved = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&saved);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0)).on_ready(move |handle| {
            ready.replace(Some(handle.clone()));
        }),
    );
    let document = saved
        .borrow()
        .as_ref()
        .expect("ready")
        .create_document(
            b"v".to_vec(),
            std::num::NonZeroUsize::new(8).expect("bound"),
        )
        .expect("document");
    let port = document.bind().expect("bound");
    let observer = Arc::new(ReentrantDocumentWake {
        port: port.clone(),
        admitted: Mutex::new(Vec::new()),
    });
    let waker = Waker::from(Arc::clone(&observer));
    let mut cx = Context::from_waker(&waker);
    let mut first = port.read().expect("first admitted");
    assert!(matches!(Pin::new(&mut first).poll(&mut cx), Poll::Pending));
    let mut sibling = port.read().expect("sibling admitted");
    run.run_owner_turn();
    assert!(matches!(
        first.try_result(),
        Some(flui_app::DocumentOutcome::Current(_))
    ));
    assert!(matches!(
        sibling.try_result(),
        Some(flui_app::DocumentOutcome::Current(_))
    ));
    run.run_owner_turn();
    let mut reentered = observer
        .admitted
        .lock()
        .expect("receipts")
        .pop()
        .expect("wake reentered");
    assert!(matches!(
        reentered.try_result(),
        Some(flui_app::DocumentOutcome::Current(_))
    ));
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[test]
fn failed_owner_ports_never_retarget_a_survivor_or_a_new_application() {
    let saved = Rc::new(std::cell::RefCell::new(None::<AppHandle>));
    let ready = Rc::clone(&saved);
    let run = HeadlessRun::start(
        Application::new(|_| flui_widgets::SizedBox::square(10.0)).on_ready(move |handle| {
            ready.replace(Some(handle.clone()));
        }),
    );
    let document = saved
        .borrow()
        .as_ref()
        .expect("ready")
        .create_document(b"v".to_vec(), std::num::NonZeroUsize::MIN)
        .expect("document");
    let local = document.bind().expect("local port");
    let child_document = document.clone();
    let (send, receive) = mpsc::channel();
    let mut failed =
        open_dedicated_window(move || -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
            send.send(child_document.bind().expect("destination bound"))
                .expect("observer");
            panic!("installation failed after binding");
        })
        .expect("admitted");
    assert!(completed(&mut failed).is_err());
    let remote = receive
        .recv_timeout(Duration::from_secs(10))
        .expect("remote port");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match remote.read() {
            Err(error) if error.reason == flui_app::DocumentRefusal::OwnerGone => break,
            Ok(request) => {
                drop(request);
                run.run_owner_turn();
            }
            other => panic!("unexpected retired port result: {other:?}"),
        }
        assert!(Instant::now() < deadline, "failed owner did not retire");
    }
    let mut survivor = local.read().expect("initial owner still accepts");
    run.run_owner_turn();
    assert!(matches!(
        survivor.try_result(),
        Some(flui_app::DocumentOutcome::Current(_))
    ));
    assert_eq!(run.join().expect("run completed").code(), 0);
    let replacement =
        HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    assert_eq!(
        document
            .bind()
            .expect_err("document cannot bind another run"),
        flui_app::DocumentRefusal::WrongRun
    );
    assert_eq!(
        remote.read().expect_err("old owner cannot retarget").reason,
        flui_app::DocumentRefusal::OwnerGone
    );
    assert_eq!(replacement.join().expect("replacement completed").code(), 0);
}

#[test]
fn registered_fonts_are_shared_across_owner_retirement() {
    let run = HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    let mut first = open_dedicated_window(|| -> (flui_widgets::SizedBox, RuntimeWindowConfig) {
        flui_app::register_font(include_bytes!(
            "../../flui-painting/assets/fonts/probe-variable-wght.ttf"
        ))
        .expect("font registered by first owner");
        panic!("retire the registering owner");
    })
    .expect("first admitted");
    assert!(completed(&mut first).is_err());
    assert!(matches!(
        flui_app::register_font(include_bytes!(
            "../../flui-painting/assets/fonts/probe-variable-wght.ttf"
        )),
        Err(flui_app::FontRegistrationError::AlreadyRegistered)
    ));
    let mut next = open_dedicated_window(|| {
        assert!(matches!(
            flui_app::register_font(include_bytes!(
                "../../flui-painting/assets/fonts/probe-variable-wght.ttf"
            )),
            Err(flui_app::FontRegistrationError::AlreadyRegistered)
        ));
        (
            flui_widgets::SizedBox::square(10.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("next admitted");
    completed(&mut next).expect("shared fonts survived first owner's retirement");
    assert_eq!(run.join().expect("run completed").code(), 0);
    let replacement =
        HeadlessRun::start(Application::new(|_| flui_widgets::SizedBox::square(10.0)));
    flui_app::register_font(include_bytes!(
        "../../flui-painting/assets/fonts/probe-variable-wght.ttf"
    ))
    .expect("registration state is scoped to a run, not its thread");
    assert_eq!(replacement.join().expect("replacement completed").code(), 0);
}

impl flui_platform::capability::Capability for OwnerPolicy {
    type Handle = Rc<String>;
    const NAME: &'static str = "multi-owner-policy";
}
enum OptionalPolicy {}
impl flui_platform::capability::Capability for OptionalPolicy {
    type Handle = ();
    const NAME: &'static str = "multi-owner-optional";
}

#[derive(Clone, StatefulView)]
struct PolicyRoot {
    observed: mpsc::Sender<(String, ThreadId)>,
}
struct PolicyState {
    observed: mpsc::Sender<(String, ThreadId)>,
}
impl StatefulView for PolicyRoot {
    type State = PolicyState;
    fn create_state(&self) -> Self::State {
        PolicyState {
            observed: self.observed.clone(),
        }
    }
}
impl ViewState<PolicyRoot> for PolicyState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        let value = cx
            .capability::<OwnerPolicy>()
            .expect("reconstructed override");
        assert!(
            cx.capability::<OptionalPolicy>().is_err(),
            "optional provider refusal changed"
        );
        self.observed
            .send((value.as_ref().clone(), std::thread::current().id()))
            .expect("observer");
    }
    fn build(&self, _: &PolicyRoot, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::SizedBox::square(12.0)
    }
}

fn optional_policy(
    _: &flui_platform::capability::ProviderContext<'_>,
) -> Result<(), flui_platform::Unsupported> {
    Err(flui_platform::Unsupported::of::<OptionalPolicy>(
        flui_platform::UnsupportedReason::Unavailable,
    ))
}

#[test]
fn dedicated_provider_policy_is_explicit_and_optional_refusal_is_preserved() {
    use flui_platform::capability::ProviderContext;
    let (observed, events) = mpsc::channel();
    let main_events = observed.clone();
    let run = HeadlessRun::start(
        Application::new(move |_| PolicyRoot {
            observed: main_events.clone(),
        })
        .capability::<OwnerPolicy>(|_: &ProviderContext<'_>| Ok(Rc::new("override".to_owned())))
        .capability::<OptionalPolicy>(optional_policy),
    );
    run.run_owner_turn();
    let main = events
        .recv_timeout(Duration::from_secs(10))
        .expect("initial policy acquired");
    assert_eq!(main, ("override".to_owned(), std::thread::current().id()));
    let mut missing = open_dedicated_window(|| {
        (
            flui_widgets::SizedBox::square(10.0),
            RuntimeWindowConfig::new(),
        )
    })
    .expect("factory admitted");
    assert!(matches!(
        completed(&mut missing),
        Err(flui_app::AppWindowError::MissingCapability { .. })
    ));
    let mut reconstructed = open_dedicated_window(move || {
        let local = Rc::new("override".to_owned());
        let capabilities = flui_app::AppCapabilities::new()
            .capability::<OwnerPolicy>(move |_: &ProviderContext<'_>| Ok(Rc::clone(&local)))
            .capability::<OptionalPolicy>(optional_policy);
        (
            PolicyRoot { observed },
            RuntimeWindowConfig::new().with_capabilities(capabilities),
        )
    })
    .expect("reconstruction admitted");
    completed(&mut reconstructed).expect("optional unsupported did not abort launch");
    let dedicated = events
        .recv_timeout(Duration::from_secs(10))
        .expect("dedicated policy acquired");
    assert_eq!(dedicated.0, "override");
    assert_ne!(dedicated.1, main.1);
    assert_eq!(run.join().expect("run completed").code(), 0);
}

#[cfg(windows)]
#[test]
#[ignore = "opens actual Win32 windows and GPU surfaces; run explicitly for native qualification"]
fn native_dedicated_factory_builds_on_two_win32_owners_and_retires() {
    let (events, observed) = mpsc::channel();
    let initial_events = events.clone();
    let (finished, result) = mpsc::channel();
    let application = Application::new(move |_| LocalRoot {
        capture: Rc::new(LocalCapture {
            created: std::thread::current().id(),
            builds: Cell::new(0),
            events: initial_events.clone(),
        }),
    })
    .with_config(
        flui_app::AppConfig::new()
            .with_title("FLUI initial owner qualification")
            .with_size(320, 180),
    )
    .on_ready(move |handle| {
        let mut request = open_dedicated_window(move || {
            let created = std::thread::current().id();
            (
                LocalRoot {
                    capture: Rc::new(LocalCapture {
                        created,
                        builds: Cell::new(0),
                        events,
                    }),
                },
                RuntimeWindowConfig::new()
                    .with_title("FLUI dedicated owner qualification")
                    .with_size(320, 180),
            )
        })
        .expect("qualification admission");
        let handle = handle.clone();
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completed(&mut request).expect("native dedicated installation");
                let mut owners = std::collections::HashSet::new();
                let deadline = Instant::now() + Duration::from_secs(15);
                while owners.len() != 2 {
                    let (kind, owner) = observed
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("both native roots build");
                    if kind == "build" {
                        owners.insert(owner);
                    }
                }
            }));
            let _ = handle.request_quit();
            finished.send(outcome.is_ok()).expect("native observer");
        });
    });
    flui_app::testing::run_native_multi_owner(application).expect("native run and local teardown");
    assert!(
        result
            .recv_timeout(Duration::from_secs(20))
            .expect("native observations")
    );
}
