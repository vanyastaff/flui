//! Actual Win32/GPU qualification through the application factory and runners.
mod accessibility;
use flui_app::testing::{NativeFrame, NativeFrames};
use flui_app::{Application, RuntimeWindowConfig, open_dedicated_window};
use flui_foundation::geometry::{Point, Size};
use flui_painting::styling::Color;
use flui_view::prelude::*;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

enum Command {
    Resize(Size<f64>),
    Mode(flui_platform::WindowMode),
    Hold {
        entered: mpsc::SyncSender<()>,
        release: mpsc::Receiver<()>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Initial,
    Sibling,
}
struct Model {
    color: Color,
    command: Option<Command>,
    panic_on_dispose: bool,
}
#[derive(Clone)]
struct Client {
    role: Role,
    model: Arc<Mutex<Model>>,
    rebuild: RebuildHandle,
    owner: ThreadId,
}
impl Client {
    fn color(&self, color: Color) {
        self.model.lock().expect("model").color = color;
        self.rebuild.schedule(RebuildReason::StateChange);
    }
    fn command(&self, command: Command) {
        assert!(
            self.model
                .lock()
                .expect("model")
                .command
                .replace(command)
                .is_none(),
            "one outstanding command"
        );
        self.rebuild.schedule(RebuildReason::StateChange);
    }
}
#[derive(Clone, StatefulView)]
struct Root {
    role: Role,
    model: Arc<Mutex<Model>>,
    ready: mpsc::Sender<Client>,
}
struct State {
    role: Role,
    model: Arc<Mutex<Model>>,
    ready: mpsc::Sender<Client>,
    window: Option<flui_view::WindowHandle>,
}
impl StatefulView for Root {
    type State = State;
    fn create_state(&self) -> State {
        State {
            role: self.role,
            model: Arc::clone(&self.model),
            ready: self.ready.clone(),
            window: None,
        }
    }
}
impl ViewState<Root> for State {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.window = cx.window();
        self.ready
            .send(Client {
                role: self.role,
                model: Arc::clone(&self.model),
                rebuild: cx.rebuild_handle(),
                owner: std::thread::current().id(),
            })
            .expect("observer");
    }
    fn build(&self, _: &Root, _: &dyn BuildContext) -> impl IntoView {
        let (color, command) = {
            let mut model = self.model.lock().expect("model");
            (model.color, model.command.take())
        };
        match command {
            Some(Command::Mode(mode)) => self
                .window
                .as_ref()
                .expect("window capability")
                .set_mode(mode)
                .expect("mode admitted"),
            Some(Command::Resize(size)) => self
                .window
                .as_ref()
                .expect("window capability")
                .set_size(size)
                .expect("resize admitted"),
            Some(Command::Hold { entered, release }) => {
                entered.send(()).expect("hold observer");
                release
                    .recv_timeout(Duration::from_secs(10))
                    .expect("bounded owner release");
            }
            None => {}
        }
        flui_widgets::ColoredBox::new(color)
    }

    fn dispose(&mut self) {
        let fail = self.model.lock().expect("model").panic_on_dispose;
        assert!(!fail, "qualification application dispose failure");
    }
}
fn root(role: Role, color: Color, ready: mpsc::Sender<Client>) -> Root {
    Root {
        role,
        model: Arc::new(Mutex::new(Model {
            color,
            command: None,
            panic_on_dispose: false,
        })),
        ready,
    }
}
fn until<T>(reason: &str, mut observe: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(value) = observe() {
            return value;
        }
        assert!(Instant::now() < deadline, "{reason}");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn pixels(frame: &NativeFrame, expected: [u8; 4]) -> (u32, u32) {
    let captured = until("actual swapchain capture did not arrive", || {
        frame.take_capture()
    })
    .expect("native readback supported");
    for sample in &captured.pixels {
        for (actual, expected) in sample.rgba.iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 3,
                "wrong surface pixel: {sample:?}, expected {expected}"
            );
        }
    }
    captured.size
}
struct Release(mpsc::Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[derive(Clone, Copy, Debug)]
enum Placement {
    SharedHeavyFirst,
    SharedLightFirst,
    Dedicated,
}

#[test]
#[ignore = "opens actual Win32 GPU windows; compares shared and dedicated native owner placement"]
fn native_multi_owner_placement_controls_preserve_independent_progress() {
    for placement in [
        Placement::SharedHeavyFirst,
        Placement::SharedLightFirst,
        Placement::Dedicated,
    ] {
        for hold_initial in [true, false] {
            placement_control(placement, hold_initial);
        }
    }
}

fn placement_control(placement: Placement, hold_initial: bool) {
    let apartment_before = flui_native::testing::windows::current_thread_has_sta_apartment();
    let frames = NativeFrames::default();
    let observed = frames.clone();
    let (ready, clients) = mpsc::channel();
    let initial = root(Role::Initial, Color::RED, ready.clone());
    let (done, result) = mpsc::channel();
    let application = Application::new(move |_| initial.clone())
        .with_config(flui_app::AppConfig::new().with_title("FLUI placement initial").with_size(320, 240))
        .on_ready(move |app| {
            let mut launched = match placement {
                Placement::Dedicated => open_dedicated_window(move || (
                    root(Role::Sibling, Color::GREEN, ready),
                    RuntimeWindowConfig::new().with_title("FLUI placement sibling").with_size(320, 240),
                )),
                Placement::SharedHeavyFirst | Placement::SharedLightFirst => flui_app::open_window(
                    flui_app::AppConfig::new().with_title("FLUI placement sibling").with_size(320, 240),
                    flui_app::WindowPolicy::Isolated,
                    root(Role::Sibling, Color::GREEN, ready),
                ),
            }.expect("sibling admission");
            let app = app.clone();
            std::thread::spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let address = until("sibling installation", || launched.try_result()).expect("sibling mounted");
                    let sibling = until("renderer bound to installed presentation", || observed.for_presentation(address));
                    let main = until("initial renderer with a distinct presentation", || {
                        let windows = observed.windows();
                        if windows.len() != 2 { return None; }
                        windows.into_iter().find(|frame| frame.snapshot().presentation.is_some_and(|identity| identity != address))
                    });
                    assert_ne!(main.snapshot().presentation, sibling.snapshot().presentation);
                    let one = clients.recv_timeout(Duration::from_secs(8)).expect("first client");
                    let two = clients.recv_timeout(Duration::from_secs(8)).expect("second client");
                    let (initial, second) = if one.role == Role::Initial { (one, two) } else { (two, one) };
                    assert!(initial.role == Role::Initial && second.role == Role::Sibling);
                    assert_eq!(main.snapshot().owner, initial.owner);
                    assert_eq!(sibling.snapshot().owner, second.owner);
                    assert_eq!(initial.owner != second.owner, matches!(placement, Placement::Dedicated));
                    pixels(&main, [255, 0, 0, 255]);
                    pixels(&sibling, [0, 255, 0, 255]);
                    let (heavy, heavy_frame, light, light_frame) = if hold_initial {
                        (&initial, &main, &second, &sibling)
                    } else {
                        (&second, &sibling, &initial, &main)
                    };
                    if matches!(placement, Placement::SharedLightFirst) {
                        assert!(light_frame.capture_next());
                        light.color(Color::YELLOW);
                        pixels(light_frame, [255, 255, 0, 255]);
                    }
                    // Queue the changed heavy scene before entering its build;
                    // it cannot be submitted until the latch is released.
                    heavy.model.lock().expect("model").color = Color::BLUE;
                    assert!(heavy_frame.capture_next());
                    let (entered, held) = mpsc::sync_channel(1);
                    let (release, released) = mpsc::channel();
                    let release = Release(release);
                    heavy.command(Command::Hold { entered, release: released });
                    held.recv_timeout(Duration::from_secs(8)).expect("heavy owner entered build");
                    let before = heavy_frame.snapshot().presented;
                    let request = Instant::now();
                    assert!(light_frame.capture_next());
                    light.color(Color::CYAN);
                    if matches!(placement, Placement::Dedicated) {
                        pixels(light_frame, [0, 255, 255, 255]);
                    } else {
                        // This is a causal latch control, not a latency estimate:
                        // the only thread able to paint light is inside heavy.
                        assert!(light_frame.take_capture().is_none());
                    }
                    assert!(heavy_frame.take_capture().is_none(), "held heavy scene cannot render");
                    assert_eq!(heavy_frame.snapshot().presented, before);
                    println!("placement={placement:?} hold_initial={hold_initial} light_before_release={} observed_us={}",
                        matches!(placement, Placement::Dedicated), request.elapsed().as_micros());
                    drop(release);
                    pixels(heavy_frame, [0, 0, 255, 255]);
                    if !matches!(placement, Placement::Dedicated) {
                        pixels(light_frame, [0, 255, 255, 255]);
                    }
                    vec![main, sibling]
                }));
                let _ = app.request_quit();
                done.send(outcome).expect("placement result observer");
            });
        });
    frames
        .run(application)
        .expect("placement application retires");
    assert_eq!(
        flui_native::testing::windows::current_thread_has_sta_apartment(),
        apartment_before
    );
    let windows = result
        .recv_timeout(Duration::from_secs(10))
        .expect("placement result")
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
    assert!(windows.iter().all(|frame| frame.snapshot().released));
}

#[test]
#[ignore = "opens an actual Win32 window through the ordinary production entry point"]
fn native_multi_owner_production_admission_remains_closed() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let apartment_before = flui_native::testing::windows::current_thread_has_sta_apartment();
    let refused = Arc::new(AtomicBool::new(false));
    let observed_refusal = Arc::clone(&refused);
    let factory_ran = Arc::new(AtomicBool::new(false));
    let observed_factory = Arc::clone(&factory_ran);
    Application::new(|_| flui_widgets::ColoredBox::new(Color::BLUE))
        .with_config(
            flui_app::AppConfig::new()
                .with_title("FLUI admission check")
                .with_size(320, 180),
        )
        .on_ready(move |app| {
            let result = open_dedicated_window(move || {
                observed_factory.store(true, Ordering::Release);
                (
                    flui_widgets::ColoredBox::new(Color::RED),
                    RuntimeWindowConfig::new(),
                )
            });
            observed_refusal.store(
                matches!(
                    result,
                    Err(flui_app::AppWindowError::UnsupportedPolicy { .. })
                ),
                Ordering::Release,
            );
            // Also retire a mistakenly admitted owner before asserting, so the
            // negative control cannot leave a native thread/window behind.
            let _ = app.request_quit();
        })
        .run()
        .expect("ordinary native application retires");
    assert_eq!(
        flui_native::testing::windows::current_thread_has_sta_apartment(),
        apartment_before,
        "production refusal does not retain an STA",
    );
    assert!(
        refused.load(Ordering::Acquire),
        "production dedicated admission must refuse"
    );
    assert!(
        !factory_ran.load(Ordering::Acquire),
        "refusal precedes the factory"
    );
}

#[test]
#[ignore = "opens actual Win32 windows and reads real GPU swapchains; explicit native qualification"]
fn native_multi_owner_frames_survive_recovery_and_resize() {
    native_frames(Scenario::Recovery);
}

#[test]
#[ignore = "opens actual Win32 windows; explicit native qualification"]
fn native_multi_owner_idle_quit_retires_both_loops() {
    native_frames(Scenario::IdleQuit);
}

#[test]
#[ignore = "opens actual Win32 windows, minimizes/restores and reads real GPU swapchains"]
fn native_multi_owner_minimize_restore_preserves_geometry_and_pixels() {
    native_frames(Scenario::Visibility);
}

#[test]
#[ignore = "opens actual Win32 GPU windows and injects an application disposal panic"]
fn native_multi_owner_dispose_failure_retires_both_loops_and_reports_failure() {
    native_frames(Scenario::RetirementPanic);
}

#[test]
#[ignore = "opens actual Win32 GPU windows and waits for real recovery backoff"]
fn native_multi_owner_recovery_exhaustion_and_retry() {
    native_frames(Scenario::Retry);
}

#[test]
#[ignore = "opens actual Win32 GPU windows and holds a dedicated owner's TLS destructor"]
fn native_multi_owner_resource_retirement_returns_to_baseline() {
    for _ in 0..3 {
        native_frames(Scenario::RetirementProof);
    }
}

#[derive(Default)]
struct CompletionCount(std::sync::atomic::AtomicUsize);
impl std::task::Wake for CompletionCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[test]
#[ignore = "opens actual Win32 GPU windows and saturates both owners' document lanes"]
fn native_multi_owner_saturation_preserves_control_and_settlement() {
    use flui_app::{DocumentOutcome, DocumentRefusal, OwnerLimits};
    use std::{
        future::Future,
        num::NonZeroUsize,
        pin::Pin,
        task::{Context, Poll, Waker},
    };

    for (case, limits) in [
        OwnerLimits::default()
            .with_commands(NonZeroUsize::new(2).expect("command limit"))
            .with_payload_bytes(NonZeroUsize::new(64).expect("byte limit")),
        OwnerLimits::default()
            .with_commands(NonZeroUsize::new(64).expect("command limit"))
            .with_payload_bytes(NonZeroUsize::new(16).expect("byte limit")),
    ]
    .into_iter()
    .enumerate()
    {
        let frames = NativeFrames::default();
        let observed = frames.clone();
        let (ready, clients) = mpsc::channel();
        let initial = root(Role::Initial, Color::RED, ready.clone());
        let (done, result) = mpsc::channel();
        let application = Application::new(move |_| initial.clone())
            .with_config(
                flui_app::AppConfig::new()
                    .with_owner_limits(limits)
                    .with_size(320, 240),
            )
            .on_ready(move |app| {
                let document = app
                    .create_document(
                        b"old".to_vec(),
                        NonZeroUsize::new(8).expect("document limit"),
                    )
                    .expect("document");
                let first_port = document.bind().expect("initial owner bound");
                let (port_tx, port_rx) = mpsc::channel();
                let mut launch = open_dedicated_window(move || {
                    port_tx
                        .send(document.bind().expect("dedicated owner bound"))
                        .expect("observer");
                    (
                        root(Role::Sibling, Color::GREEN, ready),
                        RuntimeWindowConfig::new().with_size(320, 240),
                    )
                })
                .expect("dedicated launch");
                let app = app.clone();
                std::thread::spawn(move || {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        until("dedicated installed", || launch.try_result()).expect("installed");
                        let second_port = port_rx
                            .recv_timeout(Duration::from_secs(8))
                            .expect("dedicated port");
                        let one = clients
                            .recv_timeout(Duration::from_secs(8))
                            .expect("first client");
                        let two = clients
                            .recv_timeout(Duration::from_secs(8))
                            .expect("second client");
                        let (first, second) = if one.role == Role::Initial {
                            (one, two)
                        } else {
                            (two, one)
                        };
                        assert_ne!(first.owner, second.owner);
                        let windows = until("two native renderers", || {
                            let windows = observed.windows();
                            (windows.len() == 2).then_some(windows)
                        });
                        let sibling = windows
                            .iter()
                            .find(|frame| frame.snapshot().owner == second.owner)
                            .expect("sibling renderer");
                        // Consume initial readback before requesting a distinguishable frame.
                        for frame in &windows {
                            let _ = until("initial readback", || frame.take_capture())
                                .expect("readback");
                        }
                        let (entered_tx, entered) = mpsc::sync_channel(1);
                        let (release_tx, release_rx) = mpsc::channel();
                        let release = Release(release_tx);
                        first.command(Command::Hold {
                            entered: entered_tx,
                            release: release_rx,
                        });
                        entered
                            .recv_timeout(Duration::from_secs(8))
                            .expect("initial owner held");
                        let ports = [first_port, second_port];
                        let mut pending = ports
                            .iter()
                            .flat_map(|port| {
                                [
                                    port.read().expect("first admitted"),
                                    port.read().expect("second admitted"),
                                ]
                            })
                            .collect::<Vec<_>>();
                        assert_eq!(flui_app::testing::retained_owner_payload_bytes(&app), 32);
                        for port in &ports {
                            assert_eq!(
                                port.read().expect_err("third refused").reason,
                                DocumentRefusal::Full
                            );
                        }
                        if case == 0 {
                            assert!(matches!(
                                app.request_show_main_window(),
                                Err(flui_app::AppControlError::Capacity { .. })
                            ));
                        }
                        let old = Arc::new(CompletionCount::default());
                        let latest = Arc::new(CompletionCount::default());
                        let old_waker = Waker::from(Arc::clone(&old));
                        let latest_waker = Waker::from(Arc::clone(&latest));
                        for request in &mut pending {
                            assert!(matches!(
                                Pin::new(&mut *request).poll(&mut Context::from_waker(&old_waker)),
                                Poll::Pending
                            ));
                            assert!(matches!(
                                Pin::new(request).poll(&mut Context::from_waker(&latest_waker)),
                                Poll::Pending
                            ));
                        }
                        assert!(sibling.capture_next());
                        second.color(Color::BLUE);
                        pixels(sibling, [0, 0, 255, 255]);
                        drop(release);
                        until("all accepted completions wake current observers", || {
                            (latest.0.load(std::sync::atomic::Ordering::SeqCst) == 4).then_some(())
                        });
                        assert_eq!(old.0.load(std::sync::atomic::Ordering::SeqCst), 0);
                        assert_eq!(
                            flui_app::testing::retained_owner_payload_bytes(&app),
                            32,
                            "unread completions retain reservations"
                        );
                        for port in &ports {
                            assert_eq!(
                                port.read()
                                    .expect_err("unread answers retain capacity")
                                    .reason,
                                DocumentRefusal::Full
                            );
                        }
                        let mut snapshots = Vec::new();
                        for request in &mut pending {
                            match request.try_result().expect("completed") {
                                DocumentOutcome::Current(snapshot) => snapshots.push(snapshot),
                                other => panic!("read outcome: {other:?}"),
                            }
                        }
                        assert_eq!(flui_app::testing::retained_owner_payload_bytes(&app), 0);
                        let mut write = ports[0]
                            .replace(snapshots[0].revision(), b"new".to_vec())
                            .expect("write admitted");
                        let written = match until("write completed", || write.try_result()) {
                            DocumentOutcome::Current(snapshot) => snapshot,
                            other => panic!("write outcome: {other:?}"),
                        };
                        let mut late = ports[1]
                            .replace(snapshots[2].revision(), b"late".to_vec())
                            .expect("late write admitted");
                        match until("late write answered", || late.try_result()) {
                            DocumentOutcome::Conflict { current, rejected } => {
                                assert_eq!(current.bytes(), b"new");
                                assert_eq!(rejected, b"late");
                            }
                            other => panic!("late reply overwrote a newer revision: {other:?}"),
                        }
                        let (entered_tx, entered) = mpsc::sync_channel(1);
                        let (release_tx, release_rx) = mpsc::channel();
                        let release = Release(release_tx);
                        first.command(Command::Hold {
                            entered: entered_tx,
                            release: release_rx,
                        });
                        entered
                            .recv_timeout(Duration::from_secs(8))
                            .expect("held for quit admission");
                        let accepted = ports
                            .iter()
                            .map(|port| {
                                port.replace(written.revision(), b"quit".to_vec())
                                    .expect("accepted before quit")
                            })
                            .collect::<Vec<_>>();
                        let unread = ports
                            .iter()
                            .filter_map(|port| port.read().ok())
                            .collect::<Vec<_>>();
                        for port in &ports {
                            assert_eq!(
                                port.read().expect_err("saturated before quit").reason,
                                DocumentRefusal::Full
                            );
                        }
                        app.request_quit()
                            .expect("reserved control bypasses ordinary saturation");
                        drop(release);
                        (ports, accepted, unread, windows)
                    }));
                    let _ = app.request_quit();
                    done.send(outcome).expect("test result");
                });
            });
        frames
            .run(application)
            .expect("native owners retire after saturated quit");
        let (ports, mut accepted, unread, windows) = result
            .recv_timeout(Duration::from_secs(10))
            .expect("observer finished")
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        for request in &mut accepted {
            match request.try_result().expect("accepted edit must settle") {
                DocumentOutcome::Current(snapshot) => assert_eq!(snapshot.bytes(), b"quit"),
                DocumentOutcome::Conflict { current, rejected } => {
                    assert_eq!(current.bytes(), b"quit");
                    assert_eq!(rejected, b"quit");
                }
                DocumentOutcome::Refused(error) => {
                    assert_eq!(error.reason, DocumentRefusal::OwnerGone);
                    assert_eq!(error.rejected.as_deref(), Some(b"quit".as_slice()));
                }
            }
        }
        drop(unread);
        for port in ports {
            assert_eq!(
                port.read().expect_err("retired identity refuses").reason,
                DocumentRefusal::OwnerGone
            );
        }
        assert!(windows.iter().all(|frame| frame.snapshot().released));
    }
}

struct ThreadRetirement {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
    finished: mpsc::SyncSender<(ThreadId, bool, Instant)>,
}
thread_local! {
    static RETIREMENT: std::cell::RefCell<Option<ThreadRetirement>> = const { std::cell::RefCell::new(None) };
}
impl Drop for ThreadRetirement {
    fn drop(&mut self) {
        let sta = flui_native::testing::windows::current_thread_has_sta_apartment();
        self.entered.send(()).expect("TLS retirement observer");
        self.release
            .recv_timeout(Duration::from_secs(8))
            .expect("bounded TLS retirement release");
        self.finished
            .send((std::thread::current().id(), sta, Instant::now()))
            .expect("retirement completion observer");
    }
}

#[derive(Clone, Copy)]
enum Scenario {
    Recovery,
    IdleQuit,
    Retry,
    Visibility,
    RetirementPanic,
    RetirementProof,
}

fn failures(
    role: Role,
    sender: mpsc::SyncSender<(Role, ThreadId, u32)>,
) -> flui_app::FrameFailureHandler {
    flui_app::FrameFailureHandler::new(move |report| {
        if let flui_app::FrameFailureKind::DeviceLost {
            recovery: flui_app::DeviceRecovery::Abandoned { attempts },
            ..
        } = report.kind
        {
            sender
                .try_send((role, std::thread::current().id(), attempts))
                .expect("one abandonment per tested owner");
        }
    })
}

fn native_frames(scenario: Scenario) {
    let apartment_before = flui_native::testing::windows::current_thread_has_sta_apartment();
    let frames = NativeFrames::default();
    let observed_frames = frames.clone();
    let (ready, clients) = mpsc::channel();
    let initial = root(Role::Initial, Color::RED, ready.clone());
    let (done, result) = mpsc::channel();
    let (failures_tx, failures_rx) = mpsc::sync_channel(4);
    let initial_failures = failures(Role::Initial, failures_tx.clone());
    let (retiring_tx, retiring) = mpsc::sync_channel(1);
    let (release_tls, tls_release) = mpsc::channel();
    let (retired_tx, retired) = mpsc::sync_channel(1);
    let application = Application::new(move |_| initial.clone())
        .with_config(
            flui_app::AppConfig::new()
                .with_title("FLUI native frame initial")
                .with_frame_failure_handler(initial_failures)
                .with_size(320, 240)
                .with_position(Point::new(80.0, 80.0)),
        )
        .on_ready(move |app| {
            let mut launched = open_dedicated_window(move || {
                if matches!(scenario, Scenario::RetirementProof) {
                    RETIREMENT.with(|slot| {
                        assert!(slot.replace(Some(ThreadRetirement {
                            entered: retiring_tx, release: tls_release, finished: retired_tx,
                        })).is_none());
                    });
                }
                (
                    root(Role::Sibling, Color::GREEN, ready),
                    RuntimeWindowConfig::new()
                        .with_frame_failure_handler(failures(Role::Sibling, failures_tx))
                        .with_title("FLUI native frame dedicated")
                        .with_size(320, 240)
                        .with_position(Point::new(500.0, 80.0)),
                )
            })
            .expect("dedicated launch");
            let app = app.clone();
            std::thread::spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    until("dedicated installation", || launched.try_result())
                        .expect("dedicated mounted");
                    let one = clients
                        .recv_timeout(Duration::from_secs(8))
                        .expect("initial client");
                    let two = clients
                        .recv_timeout(Duration::from_secs(8))
                        .expect("dedicated client");
                    let (first, second) = if one.role == Role::Initial {
                        (one, two)
                    } else {
                        (two, one)
                    };
                    assert!(first.role == Role::Initial && second.role == Role::Sibling);
                    assert_ne!(first.owner, second.owner);
                    let windows = until("two actual renderers", || {
                        let windows = observed_frames.windows();
                        (windows.len() == 2).then_some(windows)
                    });
                    let main = windows
                        .iter()
                        .find(|frame| frame.snapshot().owner == first.owner)
                        .expect("initial renderer");
                    let dedicated = windows
                        .iter()
                        .find(|frame| frame.snapshot().owner == second.owner)
                        .expect("dedicated renderer");
                    pixels(main, [255, 0, 0, 255]);
                    pixels(dedicated, [0, 255, 0, 255]);
                    if matches!(scenario, Scenario::RetirementPanic) {
                        second.model.lock().expect("model").panic_on_dispose = true;
                        return windows;
                    }
                    if matches!(scenario, Scenario::IdleQuit | Scenario::RetirementProof) {
                        // Stop issuing frame/input wakes before requesting global quit.
                        std::thread::sleep(Duration::from_millis(250));
                        return windows;
                    }
                    if matches!(scenario, Scenario::Visibility) {
                        for (client, frame, sibling, healthy, color, expected) in [
                            (&first, main, &second, dedicated, Color::BLUE, [0, 0, 255, 255]),
                            (&second, dedicated, &first, main, Color::YELLOW, [255, 255, 0, 255]),
                        ] {
                            let window = frame.snapshot().window;
                            let before = flui_native::testing::windows::window_geometry(window)
                                .expect("live HWND");
                            assert!(!before.minimized);
                            client.command(Command::Mode(flui_platform::WindowMode::Minimized));
                            until("native minimize was not applied", || {
                                let state = flui_native::testing::windows::window_geometry(window).expect("live HWND");
                                state.minimized.then_some(())
                            });
                            assert!(healthy.capture_next());
                            sibling.color(color);
                            pixels(healthy, expected);
                            // Minimized trees suspend build/layout. Restore must
                            // enter through the native system-command route,
                            // rather than asking a suspended build to run it.
                            flui_native::testing::windows::request_window_restore(window)
                                .expect("restore system command queued");
                            let mut previous_geometry = None;
                            let restored = until("native restore did not settle at its original geometry", || {
                                let state = flui_native::testing::windows::window_geometry(window).expect("live HWND");
                                if previous_geometry != Some(state) {
                                    println!("native restore owner={:?} observed={state:?}", client.owner);
                                    previous_geometry = Some(state);
                                }
                                (!state.minimized && state.outer == before.outer && state.client == before.client).then_some(state)
                            });
                            assert_eq!(restored.outer, before.outer, "restored screen position and extent");
                            assert_eq!(restored.client, before.client, "restored client size");
                            assert!(frame.capture_next());
                            client.color(color);
                            pixels(frame, expected);
                            println!("native visibility owner={:?} before={before:?} restored={restored:?}", client.owner);
                        }
                        return windows;
                    }
                    if matches!(scenario, Scenario::Retry) {
                        for (
                            client,
                            frame,
                            sibling,
                            healthy,
                            color,
                            expected,
                            other_color,
                            other_pixels,
                        ) in [
                            (
                                &first,
                                main,
                                &second,
                                dedicated,
                                Color::BLUE,
                                [0, 0, 255, 255],
                                Color::YELLOW,
                                [255, 255, 0, 255],
                            ),
                            (
                                &second,
                                dedicated,
                                &first,
                                main,
                                Color::GREEN,
                                [0, 255, 0, 255],
                                Color::RED,
                                [255, 0, 0, 255],
                            ),
                        ] {
                            let healthy_before = healthy.snapshot();
                            frame.lose_after_present(u32::MAX);
                            client.color(color);
                            until("lost device attempted recovery", || {
                                (frame.snapshot().recovery_attempts > 0).then_some(())
                            });
                            assert!(healthy.capture_next());
                            sibling.color(other_color);
                            pixels(healthy, other_pixels);
                            let (role, owner, attempts) = failures_rx
                                .recv_timeout(Duration::from_secs(20))
                                .expect("real backoff eventually reports abandonment");
                            assert!(role == client.role);
                            assert_eq!(
                                owner, client.owner,
                                "failure delivered on its source owner"
                            );
                            assert_eq!(attempts, frame.snapshot().recovery_attempts);
                            let abandoned = frame.snapshot();
                            assert!(
                                attempts > 10,
                                "actual exponential backoff preceded the ceiling"
                            );
                            frame.allow_recovery();
                            std::thread::sleep(Duration::from_millis(1200));
                            assert_eq!(
                                frame.snapshot().recovery_attempts,
                                abandoned.recovery_attempts,
                                "exhaustion stays parked until the application explicitly retries"
                            );
                            app.retry_render_device().expect("public retry accepted");
                            until("public retry rebuilt the actual GPU stack", || {
                                (frame.snapshot().recovered == 1).then_some(())
                            });
                            pixels(frame, expected);
                            until("recovered frame was presented", || {
                                (frame.snapshot().presented > abandoned.presented).then_some(())
                            });
                            assert_eq!(
                                healthy.snapshot().recovered,
                                healthy_before.recovered,
                                "global retry must not rebuild a healthy sibling"
                            );
                        }
                        return windows;
                    }
                    for (client, frame, color, expected) in [
                        (&first, main, Color::BLUE, [0, 0, 255, 255]),
                        (&second, dedicated, Color::YELLOW, [255, 255, 0, 255]),
                    ] {
                        let before = frame.snapshot();
                        frame.lose_after_present(0);
                        client.color(color);
                        until("actual device stack rebuilt", || {
                            (frame.snapshot().recovered == 1).then_some(())
                        });
                        pixels(frame, expected);
                        until(
                            "new surface received no fresh present without new input",
                            || (frame.snapshot().presented >= before.presented + 2).then_some(()),
                        );
                        assert!(frame.capture_next());
                        client.command(Command::Resize(Size::new(420.0, 270.0)));
                        let size = pixels(frame, expected);
                        assert_ne!(size, before.size, "native resize changed surface extent");
                        assert_eq!(size, frame.snapshot().size);
                    }
                    for (held, sibling, frame, color, expected) in [
                        (&first, &second, dedicated, Color::BLUE, [0, 0, 255, 255]),
                        (&second, &first, main, Color::GREEN, [0, 255, 0, 255]),
                    ] {
                        let (entered, held_here) = mpsc::sync_channel(1);
                        let (release, released) = mpsc::channel();
                        let release = Release(release);
                        held.command(Command::Hold {
                            entered,
                            release: released,
                        });
                        held_here
                            .recv_timeout(Duration::from_secs(8))
                            .expect("owner held in build");
                        assert!(frame.capture_next());
                        sibling.color(color);
                        pixels(frame, expected);
                        drop(release);
                    }
                    windows
                }));
                let _ = app.request_quit();
                if matches!(scenario, Scenario::RetirementProof) {
                    retiring.recv_timeout(Duration::from_secs(8)).expect("dedicated owner reached TLS retirement");
                    // An early run return must be distinguishable even if the
                    // observer result is collected later. Capture return time
                    // immediately below, before waiting for this observer.
                    std::thread::sleep(Duration::from_millis(150));
                    release_tls.send(()).expect("held TLS still belongs to its owner");
                }
                done.send(outcome).expect("native result observer");
            });
        });
    let run = frames.run(application);
    let returned = Instant::now();
    if matches!(scenario, Scenario::RetirementPanic) {
        assert!(
            matches!(run, Err(flui_app::AppRunError::DedicatedOwner(_))),
            "application disposal failure must reach the caller: {run:?}"
        );
    } else {
        run.expect("native application retires cleanly");
    }
    assert_eq!(
        flui_native::testing::windows::current_thread_has_sta_apartment(),
        apartment_before,
        "the initial owner released its COM apartment before returning, not during TLS destruction"
    );
    let windows = result
        .recv_timeout(Duration::from_secs(10))
        .expect("qualification result")
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
    assert!(
        windows.iter().all(|frame| frame.snapshot().released),
        "both GPU renderers retired on their owners"
    );
    if matches!(scenario, Scenario::RetirementProof) {
        let (owner, sta, finished) = retired
            .recv_timeout(Duration::from_secs(8))
            .expect("TLS completion");
        assert!(!sta, "dedicated STA released before TLS destruction");
        assert!(
            finished <= returned,
            "Application::run returned before dedicated TLS retirement completed"
        );
        assert!(windows.iter().any(|frame| frame.snapshot().owner == owner));
        for frame in &windows {
            assert!(
                flui_native::testing::windows::window_geometry(frame.snapshot().window).is_err(),
                "retired native HWND is no longer live"
            );
        }
        println!(
            "native retirement owner={owner:?} tls_before_run_return=true sta=false hwnd_live=0"
        );
    }
}
