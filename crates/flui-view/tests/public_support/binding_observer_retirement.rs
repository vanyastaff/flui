//! Public binding observer owners and snapshots, isolated for abort-capable rows.
#![expect(
    clippy::arc_with_non_send_sync,
    reason = "WidgetsBinding observers are owner-local Arc capabilities"
)]
use flui_view::{
    AppExitResponse, AppLifecycleState, RouteInformation, WidgetsBinding, WidgetsBindingObserver,
};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak as ArcWeak};
use std::task::{Context, Poll, Waker};

#[derive(Default)]
struct Healthy {
    metrics: Cell<usize>,
    memory: Cell<usize>,
    lifecycle: Cell<usize>,
}
impl WidgetsBindingObserver for Healthy {
    fn did_change_metrics(&self) {
        self.metrics.set(self.metrics.get() + 1);
    }
    fn did_have_memory_pressure(&self) {
        self.memory.set(self.memory.get() + 1);
    }
    fn did_change_app_lifecycle_state(&self, _: AppLifecycleState) {
        self.lifecycle.set(self.lifecycle.get() + 1);
    }
}

struct Observer {
    name: &'static str,
    drops: Rc<Cell<usize>>,
    calls: Rc<Cell<usize>>,
    fail_drop: bool,
    fail_body: bool,
    replace: bool,
    pending: bool,
    reenter_drop: bool,
    binding: Weak<WidgetsBinding>,
    members: Rc<RefCell<Vec<ArcWeak<dyn WidgetsBindingObserver>>>>,
    replacement: Arc<Healthy>,
    future_state: Arc<FutureState>,
}

#[derive(Default)]
struct FutureState {
    fail: AtomicBool,
    drops: AtomicUsize,
    order: Mutex<Vec<&'static str>>,
}
struct ResponseFuture<T> {
    pending: bool,
    value: Option<T>,
    state: Arc<FutureState>,
}
impl<T: Unpin> Future for ResponseFuture<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        let this = self.get_mut();
        if this.pending {
            Poll::Pending
        } else {
            Poll::Ready(this.value.take().expect("response polled once"))
        }
    }
}
impl<T> Drop for ResponseFuture<T> {
    fn drop(&mut self) {
        self.state.drops.fetch_add(1, Ordering::Relaxed);
        self.state.order.lock().expect("order log").push("future");
        assert!(
            !self.state.fail.load(Ordering::Relaxed),
            "pending observer future retirement"
        );
    }
}
impl Observer {
    fn notify(&self) {
        self.calls.set(self.calls.get() + 1);
        if self.replace {
            let binding = self
                .binding
                .upgrade()
                .expect("notification binding still alive");
            let members = self.members.borrow().clone();
            for member in members {
                if let Some(member) = member.upgrade() {
                    binding.remove_observer(&member);
                }
            }
            binding.add_observer(self.replacement.clone());
        }
        assert!(!self.fail_body, "first binding observer callback");
    }
}
impl Drop for Observer {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
        self.future_state
            .order
            .lock()
            .expect("order log")
            .push(self.name);
        if self.reenter_drop {
            self.binding
                .upgrade()
                .expect("snapshot retirement binding alive")
                .handle_memory_pressure();
        }
        assert!(!self.fail_drop, "{}", self.name);
    }
}
impl WidgetsBindingObserver for Observer {
    fn did_change_metrics(&self) {
        self.notify();
    }
    fn did_change_app_lifecycle_state(&self, _: AppLifecycleState) {
        self.notify();
    }
    fn did_pop_route(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        self.notify();
        Box::pin(ResponseFuture {
            pending: self.pending,
            value: Some(false),
            state: Arc::clone(&self.future_state),
        })
    }
    fn did_push_route_information(
        &self,
        _: &RouteInformation,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        self.did_pop_route()
    }
    fn did_request_app_exit(&self) -> Pin<Box<dyn Future<Output = AppExitResponse> + Send + '_>> {
        self.notify();
        Box::pin(ResponseFuture {
            pending: self.pending,
            value: Some(AppExitResponse::Exit),
            state: Arc::clone(&self.future_state),
        })
    }
}

struct Fixture {
    binding: Rc<WidgetsBinding>,
    drops: [Rc<Cell<usize>>; 2],
    calls: [Rc<Cell<usize>>; 2],
    replacement: Arc<Healthy>,
    future_state: Arc<FutureState>,
}
#[derive(Default)]
struct ObserverOptions {
    replace: bool,
    body: bool,
    wait: bool,
    reenter: bool,
}

fn fixture(fail: [bool; 2], options: ObserverOptions) -> Fixture {
    let ObserverOptions {
        replace,
        body,
        wait,
        reenter,
    } = options;
    let binding = Rc::new(WidgetsBinding::new());
    let drops = [Rc::default(), Rc::default()];
    let calls = [Rc::default(), Rc::default()];
    let members = Rc::new(RefCell::new(Vec::new()));
    let replacement = Arc::new(Healthy::default());
    let future_state = Arc::new(FutureState::default());
    for index in 0..2 {
        let observer: Arc<dyn WidgetsBindingObserver> = Arc::new(Observer {
            name: if index == 0 {
                "first observer retirement"
            } else {
                "second observer retirement"
            },
            drops: Rc::clone(&drops[index]),
            calls: Rc::clone(&calls[index]),
            fail_drop: fail[index],
            fail_body: body && index == 0,
            replace: replace && index == 0,
            pending: wait && index == 0,
            reenter_drop: reenter && index == 0,
            binding: Rc::downgrade(&binding),
            members: Rc::clone(&members),
            replacement: Arc::clone(&replacement),
            future_state: Arc::clone(&future_state),
        });
        members.borrow_mut().push(Arc::downgrade(&observer));
        binding.add_observer(observer);
    }
    Fixture {
        binding,
        drops,
        calls,
        replacement,
        future_state,
    }
}
fn counts(values: &[Rc<Cell<usize>>; 2]) -> [usize; 2] {
    [values[0].get(), values[1].get()]
}
fn catch(f: impl FnOnce()) -> Option<Box<dyn std::any::Any + Send>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).err()
}
fn assert_failure(payload: Option<Box<dyn std::any::Any + Send>>, expected: &'static str) {
    let payload = payload.expect("expected first failure");
    assert_eq!(
        flui_foundation::panic::payload_text(payload.as_ref()),
        Some(expected)
    );
}
fn cancel<F: Future>(future: F, incoming: bool) -> Option<Box<dyn std::any::Any + Send>> {
    let mut future = Box::pin(future);
    assert!(matches!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    if incoming {
        catch(move || {
            let _future = future;
            panic!("incoming cancellation failure");
        })
    } else {
        catch(move || drop(future))
    }
}

fn ready<F: Future>(future: F) -> F::Output {
    match Box::pin(future)
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("ready observer response must complete"),
    }
}

pub(crate) fn dispatch_child(case: &str) {
    match case {
        "async-delivery" => {
            struct Navigation {
                calls: Rc<Cell<usize>>,
                handled: bool,
            }
            impl WidgetsBindingObserver for Navigation {
                fn did_pop_route(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
                    self.calls.set(self.calls.get() + 1);
                    let handled = self.handled;
                    Box::pin(async move { handled })
                }
                fn did_push_route_information(
                    &self,
                    _: &RouteInformation,
                ) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
                    self.did_pop_route()
                }
                fn did_request_app_exit(
                    &self,
                ) -> Pin<Box<dyn Future<Output = AppExitResponse> + Send + '_>> {
                    self.calls.set(self.calls.get() + 1);
                    let cancel = self.handled;
                    Box::pin(async move {
                        if cancel {
                            AppExitResponse::Cancel
                        } else {
                            AppExitResponse::Exit
                        }
                    })
                }
            }
            let binding = WidgetsBinding::new();
            let calls = [Rc::new(Cell::new(0)), Rc::new(Cell::new(0))];
            for (index, handled) in [true, false].into_iter().enumerate() {
                binding.add_observer(Arc::new(Navigation {
                    calls: Rc::clone(&calls[index]),
                    handled,
                }));
            }
            assert!(ready(binding.handle_pop_route()));
            assert_eq!(counts(&calls), [1, 0]);
            assert!(ready(
                binding.handle_push_route(&RouteInformation::new("/ready"))
            ));
            assert_eq!(counts(&calls), [2, 0]);
            assert_eq!(
                ready(binding.handle_request_app_exit()),
                AppExitResponse::Cancel
            );
            assert_eq!(counts(&calls), [3, 1], "exit still asks every observer");
        }
        "physical-healthy" | "physical-a" | "physical-b" | "physical-pair"
        | "physical-incoming" => {
            let fail = [
                matches!(case, "physical-a" | "physical-pair" | "physical-incoming"),
                matches!(case, "physical-b" | "physical-pair" | "physical-incoming"),
            ];
            let Fixture { binding, drops, .. } = fixture(fail, ObserverOptions::default());
            let failure = if case == "physical-incoming" {
                catch(move || {
                    let _binding = binding;
                    panic!("incoming binding failure");
                })
            } else {
                catch(move || drop(binding))
            };
            match case {
                "physical-healthy" => {
                    assert!(failure.is_none());
                    assert_eq!(counts(&drops), [1, 1]);
                }
                "physical-a" | "physical-pair" => {
                    assert_failure(failure, "first observer retirement");
                    assert_eq!(counts(&drops), [1, 0]);
                }
                "physical-b" => {
                    assert_failure(failure, "second observer retirement");
                    assert_eq!(counts(&drops), [1, 1]);
                }
                _ => {
                    assert_failure(failure, "incoming binding failure");
                    assert_eq!(counts(&drops), [0, 0]);
                }
            }
            let fresh = WidgetsBinding::new();
            let healthy = Arc::new(Healthy::default());
            fresh.add_observer(healthy.clone());
            fresh.handle_metrics_changed();
            assert_eq!(healthy.metrics.get(), 1);
        }
        "physical-alias" => {
            let binding = WidgetsBinding::new();
            let drops = Rc::new(Cell::new(0));
            struct Aliased(Rc<Cell<usize>>);
            impl WidgetsBindingObserver for Aliased {}
            impl Drop for Aliased {
                fn drop(&mut self) {
                    self.0.set(self.0.get() + 1);
                }
            }
            let observer: Arc<dyn WidgetsBindingObserver> = Arc::new(Aliased(Rc::clone(&drops)));
            binding.add_observer(observer.clone());
            drop(binding);
            assert_eq!(drops.get(), 0);
            observer.did_change_metrics();
            drop(observer);
            assert_eq!(drops.get(), 1);
        }
        "snapshot-healthy" | "snapshot-a" | "snapshot-b" | "snapshot-pair" | "snapshot-body" => {
            let body = case == "snapshot-body";
            let fail = [
                matches!(case, "snapshot-a" | "snapshot-pair" | "snapshot-body"),
                matches!(case, "snapshot-b" | "snapshot-pair" | "snapshot-body"),
            ];
            let f = fixture(
                fail,
                ObserverOptions {
                    replace: true,
                    body,
                    reenter: !body,
                    ..ObserverOptions::default()
                },
            );
            let failure = catch(|| f.binding.handle_metrics_changed());
            if body {
                assert_failure(failure, "first binding observer callback");
                assert_eq!(counts(&f.drops), [0, 0]);
                assert_eq!(counts(&f.calls), [1, 0]);
            } else {
                if fail[0] {
                    assert_failure(failure, "first observer retirement");
                    assert_eq!(counts(&f.drops), [1, 0]);
                } else if fail[1] {
                    assert_failure(failure, "second observer retirement");
                    assert_eq!(counts(&f.drops), [1, 1]);
                } else {
                    assert!(failure.is_none());
                    assert_eq!(counts(&f.drops), [1, 1]);
                }
                assert_eq!(counts(&f.calls), [1, 1]);
                assert_eq!(
                    f.replacement.memory.get(),
                    1,
                    "destructor reentry reaches new registry"
                );
            }
            assert_eq!(f.binding.observer_count(), 1);
            f.binding.handle_metrics_changed();
            assert_eq!(
                f.replacement.metrics.get(),
                1,
                "next notification uses replacement registry"
            );
        }
        "lifecycle-compete" => {
            let f = fixture(
                [true, true],
                ObserverOptions {
                    replace: true,
                    body: true,
                    ..ObserverOptions::default()
                },
            );
            let source = f.binding.with_build_owner(|owner| {
                owner
                    .lifecycle_handle()
                    .expect("binding lifecycle capability")
            });
            let scoped = Rc::new(Cell::new(0));
            let armed = Rc::new(Cell::new(true));
            let seen = Rc::clone(&scoped);
            let fail = Rc::clone(&armed);
            let (_, subscription) = source
                .subscribe(move |_| {
                    seen.set(seen.get() + 1);
                    assert!(!fail.get(), "later scoped lifecycle failure");
                })
                .expect("open binding");
            let failure = catch(|| {
                f.binding
                    .handle_app_lifecycle_state_changed(AppLifecycleState::Resumed);
            });
            assert_failure(failure, "first binding observer callback");
            assert_eq!(counts(&f.drops), [0, 0]);
            assert_eq!(scoped.get(), 1, "scoped drain follows legacy failure");
            armed.set(false);
            f.binding
                .handle_app_lifecycle_state_changed(AppLifecycleState::Paused);
            assert_eq!(scoped.get(), 2);
            assert_eq!(f.replacement.lifecycle.get(), 1);
            drop(subscription);
        }
        "async-completed" => {
            let f = fixture(
                [false, false],
                ObserverOptions {
                    replace: true,
                    ..ObserverOptions::default()
                },
            );
            assert!(!ready(f.binding.handle_pop_route()));
            assert_eq!(f.future_state.drops.load(Ordering::Relaxed), 2);
            assert_eq!(counts(&f.drops), [1, 1]);
            assert_eq!(
                *f.future_state.order.lock().expect("order log"),
                [
                    "future",
                    "first observer retirement",
                    "future",
                    "second observer retirement"
                ],
                "completed futures retire before their current observer"
            );
            f.binding.handle_metrics_changed();
            assert_eq!(f.replacement.metrics.get(), 1);
        }
        _ if case.starts_with("async-") => {
            let parts: Vec<_> = case.splitn(3, '-').collect();
            assert_eq!(parts.len(), 3);
            let mode = parts[2];
            assert!(matches!(
                mode,
                "healthy"
                    | "a"
                    | "b"
                    | "pair"
                    | "incoming"
                    | "future"
                    | "future-pair"
                    | "future-incoming"
            ));
            let fail = [
                matches!(
                    mode,
                    "a" | "pair" | "incoming" | "future-pair" | "future-incoming"
                ),
                matches!(
                    mode,
                    "b" | "pair" | "incoming" | "future-pair" | "future-incoming"
                ),
            ];
            let f = fixture(
                fail,
                ObserverOptions {
                    replace: true,
                    wait: true,
                    ..ObserverOptions::default()
                },
            );
            f.future_state.fail.store(
                matches!(mode, "future" | "future-pair" | "future-incoming"),
                Ordering::Relaxed,
            );
            let incoming = matches!(mode, "incoming" | "future-incoming");
            let route = RouteInformation::new("/pending");
            let failure = match parts[1] {
                "pop" => cancel(f.binding.handle_pop_route(), incoming),
                "push" => cancel(f.binding.handle_push_route(&route), incoming),
                "exit" => cancel(f.binding.handle_request_app_exit(), incoming),
                _ => panic!("unknown async producer"),
            };
            let retired = counts(&f.drops);
            match mode {
                "healthy" => {
                    assert!(failure.is_none());
                    assert_eq!(retired, [1, 1]);
                    assert_eq!(
                        f.future_state.drops.load(Ordering::Relaxed),
                        1,
                        "healthy pending future retires normally"
                    );
                }
                "incoming" | "future-incoming" => {
                    assert_failure(failure, "incoming cancellation failure");
                    assert_eq!(retired, [0, 0]);
                    assert_eq!(
                        f.future_state.drops.load(Ordering::Relaxed),
                        0,
                        "independent future envelope retained during incoming unwind"
                    );
                }
                "future" => {
                    assert_failure(failure, "pending observer future retirement");
                    assert_eq!(f.future_state.drops.load(Ordering::Relaxed), 1);
                }
                "future-pair" => {
                    let failure = failure.expect("one independent cancellation failure");
                    let message = flui_foundation::panic::payload_text(failure.as_ref());
                    assert!(matches!(
                        message,
                        Some(
                            "first observer retirement"
                                | "second observer retirement"
                                | "pending observer future retirement"
                        )
                    ));
                    assert_eq!(
                        retired.iter().sum::<usize>()
                            + f.future_state.drops.load(Ordering::Relaxed),
                        1,
                        "first independent failure protects future and observer successors"
                    );
                }
                "pair" => {
                    let failure = failure.expect("one cancellation retirement failure");
                    let message = flui_foundation::panic::payload_text(failure.as_ref());
                    assert!(matches!(
                        message,
                        Some("first observer retirement" | "second observer retirement")
                    ));
                    assert_eq!(retired.iter().sum::<usize>(), 1);
                }
                "a" => {
                    assert_failure(failure, "first observer retirement");
                    assert_eq!(retired[0], 1);
                }
                _ => {
                    assert_failure(failure, "second observer retirement");
                    assert_eq!(retired[1], 1);
                }
            }
            assert_eq!(counts(&f.calls), [1, 0]);
            f.binding.handle_metrics_changed();
            assert_eq!(f.replacement.metrics.get(), 1);
        }
        _ => panic!("unknown binding observer row"),
    }
    println!("binding observer row completed: {case}");
}

pub(crate) fn binding_observer_ownership_and_notifications() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let cases = [
        "physical-healthy",
        "physical-a",
        "physical-b",
        "physical-pair",
        "physical-incoming",
        "physical-alias",
        "snapshot-healthy",
        "snapshot-a",
        "snapshot-b",
        "snapshot-pair",
        "snapshot-body",
        "lifecycle-compete",
        "async-delivery",
        "async-completed",
        "async-pop-healthy",
        "async-pop-a",
        "async-pop-b",
        "async-pop-pair",
        "async-pop-incoming",
        "async-pop-future",
        "async-pop-future-pair",
        "async-pop-future-incoming",
        "async-push-healthy",
        "async-push-a",
        "async-push-b",
        "async-push-pair",
        "async-push-incoming",
        "async-push-future",
        "async-push-future-pair",
        "async-push-future-incoming",
        "async-exit-healthy",
        "async-exit-a",
        "async-exit-b",
        "async-exit-pair",
        "async-exit-incoming",
        "async-exit-future",
        "async-exit-future-pair",
        "async-exit-future-incoming",
    ];
    let selected = std::env::var("FLUI_BINDING_OBSERVER_RETIREMENT_CONTROL").ok();
    if let Some(selected) = &selected {
        assert!(
            cases.contains(&selected.as_str()),
            "unknown binding observer control row"
        );
    }
    let mut failures = Vec::new();
    for case in cases {
        if selected.as_deref().is_some_and(|selected| selected != case) {
            continue;
        }
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "lifecycle_panic_containment_matrix",
                "--nocapture",
            ])
            .env("FLUI_BINDING_OBSERVER_RETIREMENT_CHILD", case)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binding observer child");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("stdout read");
            text
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("stderr read");
            text
        });
        let started = Instant::now();
        while child.try_wait().expect("status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                child.kill().expect("kill stalled child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("reap child");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if !status.success()
            || !stdout.contains("1 passed; 0 failed")
            || !stdout.contains(&format!("binding observer row completed: {case}"))
        {
            failures.push(format!("{case}: {status}\n{stdout}\n{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
