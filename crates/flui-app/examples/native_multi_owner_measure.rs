//! Native release measurement harness. Requires `testing`; it grants no production support.
//! Arguments: unloaded|shared-heavy-first|shared-light-first|dedicated callback|text|throughput off|on samples.
#[cfg(all(windows, feature = "testing"))]
mod native {
    use flui_app::testing::{NativeFrame, NativeFrameSample, NativeFrames};
    use flui_app::{AppConfig, Application, RuntimeWindowConfig, WindowPolicy};
    use flui_foundation::geometry::Point;
    use flui_painting::styling::Color;
    use flui_platform::EventTime;
    use flui_view::prelude::*;
    use flui_widgets::{ColoredBox, Column, SingleChildScrollView, Text, WidgetsApp};
    use std::num::NonZeroU64;
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Mode {
        Unloaded,
        HeavyFirst,
        LightFirst,
        Dedicated,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Work {
        Callback,
        Text,
        Throughput,
    }
    struct Job {
        ticket: NonZeroU64,
        requested: EventTime,
        frame: NativeFrame,
        busy: bool,
    }
    struct Model {
        revision: u64,
        job: Option<Job>,
    }
    struct Completion {
        ticket: NonZeroU64,
        semantics: bool,
        cpu_ns: u128,
        wall_ns: u128,
    }
    #[derive(Clone)]
    struct Client {
        heavy: bool,
        model: Arc<Mutex<Model>>,
        rebuild: RebuildHandle,
    }
    impl Client {
        fn request(&self, ticket: u64, frame: &NativeFrame, busy: bool) {
            let requested = EventTime::now();
            {
                let mut model = self.model.lock().expect("model");
                model.revision = ticket;
                assert!(
                    model
                        .job
                        .replace(Job {
                            ticket: NonZeroU64::new(ticket).expect("nonzero ticket"),
                            requested,
                            frame: frame.clone(),
                            busy
                        })
                        .is_none()
                );
            }
            self.rebuild.schedule(RebuildReason::StateChange);
        }
    }
    #[derive(Clone, StatefulView)]
    struct Root {
        heavy: bool,
        rows: usize,
        model: Arc<Mutex<Model>>,
        ready: mpsc::SyncSender<Client>,
        started: mpsc::SyncSender<NonZeroU64>,
        completed: mpsc::SyncSender<Completion>,
    }
    struct State {
        root: Root,
        post: Option<flui_view::PostFrameHandle>,
        pipeline: Option<flui_rendering::pipeline::PipelineCell>,
    }
    impl StatefulView for Root {
        type State = State;
        fn create_state(&self) -> State {
            State {
                root: self.clone(),
                post: None,
                pipeline: None,
            }
        }
    }
    impl ViewState<Root> for State {
        fn init_state(&mut self, cx: &dyn LifecycleContext) {
            self.post = cx.post_frame_handle();
            self.pipeline = cx.pipeline_owner();
            self.root
                .ready
                .try_send(Client {
                    heavy: self.root.heavy,
                    model: Arc::clone(&self.root.model),
                    rebuild: cx.rebuild_handle(),
                })
                .expect("two clients");
        }
        fn build(&self, root: &Root, _: &dyn BuildContext) -> impl IntoView {
            let (revision, job) = {
                let mut model = root.model.lock().expect("model");
                (model.revision, model.job.take())
            };
            if let Some(job) = job {
                assert!(
                    job.frame.begin_sample(job.ticket, job.requested),
                    "one sample per exact request"
                );
                root.started
                    .try_send(job.ticket)
                    .expect("one pending build notification");
                let start = Instant::now();
                let cpu_start =
                    flui_native::testing::windows::current_thread_cpu_time().expect("thread CPU");
                if job.busy {
                    let mut value = revision;
                    loop {
                        for _ in 0..4096 {
                            value = std::hint::black_box(
                                value
                                    .wrapping_mul(6_364_136_223_846_793_005)
                                    .wrapping_add(1_442_695_040_888_963_407),
                            );
                        }
                        if flui_native::testing::windows::current_thread_cpu_time()
                            .expect("thread CPU")
                            .saturating_sub(cpu_start)
                            >= Duration::from_millis(200)
                        {
                            break;
                        }
                        assert!(
                            start.elapsed() < Duration::from_secs(5),
                            "CPU turn watchdog"
                        );
                    }
                    std::hint::black_box(value);
                }
                let cpu_ns = flui_native::testing::windows::current_thread_cpu_time()
                    .expect("thread CPU")
                    .saturating_sub(cpu_start)
                    .as_nanos();
                let wall_ns = start.elapsed().as_nanos();
                let pipeline = self.pipeline.clone().expect("mounted pipeline");
                let completed = root.completed.clone();
                self.post
                    .as_ref()
                    .expect("post frame")
                    .schedule(move |_| {
                        let semantics = pipeline.with(|owner| owner.semantics_owner().is_some());
                        let sent = completed.try_send(Completion {
                            ticket: job.ticket,
                            semantics,
                            cpu_ns,
                            wall_ns,
                        });
                        assert!(
                            !matches!(sent, Err(mpsc::TrySendError::Full(_))),
                            "one pending completion"
                        );
                    })
                    .expect("post frame admitted");
            }
            let rows: Vec<BoxedView> = (0..root.rows).map(|row| Text::new(format!("Revision {revision:06} row {row:04}: native text layout and paint 0123456789")).boxed()).collect();
            let [red, green, blue, _] = pixel_color(revision);
            WidgetsApp::new(
                ColoredBox::new(Color::rgb(red, green, blue))
                    .child(SingleChildScrollView::new().child(Column::new(rows))),
            )
        }
    }
    fn root(
        heavy: bool,
        rows: usize,
        ready: mpsc::SyncSender<Client>,
        started: mpsc::SyncSender<NonZeroU64>,
        completed: mpsc::SyncSender<Completion>,
    ) -> Root {
        Root {
            heavy,
            rows,
            ready,
            started,
            completed,
            model: Arc::new(Mutex::new(Model {
                revision: 0,
                job: None,
            })),
        }
    }
    fn until<T>(mut read: impl FnMut() -> Option<T>) -> T {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(value) = read() {
                return value;
            }
            assert!(Instant::now() < end, "measurement watchdog");
            std::thread::sleep(Duration::from_micros(250));
        }
    }
    fn collect(
        frame: &NativeFrame,
        completed: &mpsc::Receiver<Completion>,
        ticket: u64,
        semantics: bool,
    ) -> (NativeFrameSample, Completion) {
        let sample = until(|| frame.take_sample());
        complete(sample, completed, ticket, semantics)
    }
    fn complete(
        sample: NativeFrameSample,
        completed: &mpsc::Receiver<Completion>,
        ticket: u64,
        semantics: bool,
    ) -> (NativeFrameSample, Completion) {
        let done = completed
            .recv_timeout(Duration::from_secs(8))
            .expect("post-frame completion");
        assert_eq!(sample.ticket.get(), ticket);
        assert_eq!(done.ticket, sample.ticket);
        assert!(sample.requested <= sample.build_started);
        assert!(sample.native_received <= sample.transaction_started);
        assert!(sample.transaction_started <= sample.build_started);
        assert!(sample.build_started <= sample.scene_submitted.expect("scene timestamp"));
        assert!(sample.scene_submitted <= sample.present_returned);
        assert!(sample.present_returned <= sample.transaction_finished);
        assert_eq!(done.semantics, semantics, "actual semantics population");
        assert!(
            sample.scene_submitted.is_some() && sample.present_returned.is_some(),
            "changed scene actually presented"
        );
        (sample, done)
    }
    fn emit(role: &str, phase: &str, sample: &NativeFrameSample, done: &Completion) {
        println!(
            "sample,{role},{phase},{},{},{},{},{},{},{},{},{},{},{}",
            sample.ticket,
            sample.requested.as_nanos(),
            sample.native_received.as_nanos(),
            sample.transaction_started.as_nanos(),
            sample.build_started.as_nanos(),
            sample.scene_submitted.expect("scene").as_nanos(),
            sample.present_returned.expect("present").as_nanos(),
            sample.transaction_finished.expect("finish").as_nanos(),
            sample.transaction_cpu.expect("Win32 CPU").as_nanos(),
            done.cpu_ns,
            done.wall_ns
        );
    }
    fn pixels(role: &str, frame: &NativeFrame, ticket: u64) {
        let image = until(|| frame.take_capture()).expect("native readback");
        let expected = pixel_color(ticket);
        assert!(
            image.pixels.iter().any(|pixel| pixel.rgba == expected),
            "changed background reached actual surface"
        );
        println!(
            "pixels,role={role},ticket={ticket},size={:?},samples={:?}",
            image.size, image.pixels
        );
    }
    fn pixel_color(ticket: u64) -> [u8; 4] {
        let code = ticket.wrapping_mul(73);
        [
            u8::try_from(code & 255).expect("byte"),
            u8::try_from((code >> 8) & 255).expect("byte"),
            193,
            255,
        ]
    }
    fn idle_marker(edge: &str) {
        let before = EventTime::now();
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("UTC after epoch");
        let after = EventTime::now();
        println!(
            "idle,edge={edge},event_before_ns={},unix_ns={},event_after_ns={}",
            before.as_nanos(),
            unix.as_nanos(),
            after.as_nanos()
        );
    }
    pub fn run() -> anyhow::Result<()> {
        if cfg!(debug_assertions) {
            anyhow::bail!("measure only optimized release builds");
        }
        let args = std::env::args().collect::<Vec<_>>();
        let mode = match args.get(1).map(String::as_str) {
            Some("unloaded") => Mode::Unloaded,
            Some("shared-heavy-first") => Mode::HeavyFirst,
            Some("shared-light-first") => Mode::LightFirst,
            Some("dedicated") => Mode::Dedicated,
            _ => anyhow::bail!("invalid placement"),
        };
        let work = match args.get(2).map(String::as_str) {
            Some("callback") => Work::Callback,
            Some("text") => Work::Text,
            Some("throughput") => Work::Throughput,
            _ => anyhow::bail!("invalid workload"),
        };
        let semantics = match args.get(3).map(String::as_str) {
            Some("on") => true,
            Some("off") => false,
            _ => anyhow::bail!("invalid semantics mode"),
        };
        let samples = args
            .get(4)
            .ok_or_else(|| anyhow::anyhow!("sample count required"))?
            .parse::<u64>()?;
        anyhow::ensure!((1..=10000).contains(&samples), "bounded sample count");
        println!(
            "protocol,version=1,mode={mode:?},work={work:?},semantics={semantics},pid={},samples={samples},warmup=30,clock=process_event_ns,physical_latency=unavailable",
            std::process::id()
        );
        let observed_resources = args.get(5).is_some_and(|arg| arg == "observe-resources");
        if observed_resources {
            println!("resources,stage=before_application");
            let mut command = String::new();
            std::io::stdin().read_line(&mut command)?;
            anyhow::ensure!(
                command.trim() == "start",
                "resource observer start handshake"
            );
        }
        let start = Instant::now();
        let frames = NativeFrames::default().with_semantics(semantics);
        let observed = frames.clone();
        let (ready, clients) = mpsc::sync_channel(2);
        let (heavy_started_tx, heavy_started) = mpsc::sync_channel(1);
        let (light_started_tx, light_started) = mpsc::sync_channel(1);
        let (heavy_done_tx, heavy_done) = mpsc::sync_channel(1);
        let (light_done_tx, light_done) = mpsc::sync_channel(1);
        let initial = root(
            true,
            if work == Work::Callback { 1 } else { 1000 },
            ready.clone(),
            heavy_started_tx,
            heavy_done_tx,
        );
        let (done, result) = mpsc::sync_channel(1);
        let application = Application::new(move |_| initial.clone())
            .with_config(
                AppConfig::new()
                    .with_title("FLUI measure heavy")
                    .with_size(640, 480)
                    .with_position(Point::new(60.0, 80.0)),
            )
            .on_ready(move |app| {
                let factory = move || {
                    root(
                        false,
                        if work == Work::Throughput { 1000 } else { 1 },
                        ready,
                        light_started_tx,
                        light_done_tx,
                    )
                };
                let mut request = if mode == Mode::Dedicated {
                    flui_app::open_dedicated_window(move || {
                        (
                            factory(),
                            RuntimeWindowConfig::new()
                                .with_title("FLUI measure light")
                                .with_size(640, 480)
                                .with_position(Point::new(760.0, 80.0)),
                        )
                    })
                } else {
                    flui_app::open_window(
                        AppConfig::new()
                            .with_title("FLUI measure light")
                            .with_size(640, 480)
                            .with_position(Point::new(760.0, 80.0)),
                        WindowPolicy::Isolated,
                        factory(),
                    )
                }
                .expect("measurement sibling admitted");
                let app = app.clone();
                std::thread::spawn(move || {
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let address = until(|| request.try_result()).expect("sibling mounted");
                        let light_frame = until(|| observed.for_presentation(address));
                        let heavy_frame = until(|| {
                            observed.windows().into_iter().find(|frame| {
                                frame
                                    .snapshot()
                                    .presentation
                                    .is_some_and(|id| id != address)
                            })
                        });
                        let one = clients
                            .recv_timeout(Duration::from_secs(8))
                            .expect("client");
                        let two = clients
                            .recv_timeout(Duration::from_secs(8))
                            .expect("client");
                        let (heavy, light) = if one.heavy { (one, two) } else { (two, one) };
                        assert_eq!(
                            heavy_frame.snapshot().owner != light_frame.snapshot().owner,
                            mode == Mode::Dedicated
                        );
                        for frame in [&heavy_frame, &light_frame] {
                            until(|| frame.take_capture()).expect("initial native pixels");
                        }
                        println!(
                            "startup,ns={},heavy={:?},light={:?}",
                            start.elapsed().as_nanos(),
                            heavy_frame.snapshot(),
                            light_frame.snapshot()
                        );
                        idle_marker("start");
                        std::thread::sleep(Duration::from_secs(2));
                        idle_marker("end");
                        let mut next_heavy = EventTime::now();
                        let mut pending_heavy = None;
                        for ticket in 1..=samples + 30 {
                            let phase = if ticket <= 30 { "warmup" } else { "measured" };
                            if ticket == 31 {
                                if let Some(previous) = pending_heavy.take() {
                                    let (sample, done) =
                                        collect(&heavy_frame, &heavy_done, previous, semantics);
                                    emit("heavy", "warmup", &sample, &done);
                                    pixels("heavy", &heavy_frame, previous);
                                }
                                next_heavy = EventTime::now();
                            }
                            let round_start = Instant::now();
                            if let Some(previous) = pending_heavy
                                && let Some(sample) = heavy_frame.take_sample()
                            {
                                let (sample, done) =
                                    complete(sample, &heavy_done, previous, semantics);
                                emit(
                                    "heavy",
                                    if previous <= 30 { "warmup" } else { "measured" },
                                    &sample,
                                    &done,
                                );
                                pixels("heavy", &heavy_frame, previous);
                                pending_heavy = None;
                            }
                            let heavy_due = pending_heavy.is_none()
                                && mode != Mode::Unloaded
                                && (work != Work::Callback || EventTime::now() >= next_heavy);
                            if heavy_due && mode != Mode::LightFirst {
                                println!(
                                    "schedule,ticket={ticket},nominal_ns={},admitted_ns={}",
                                    next_heavy.as_nanos(),
                                    EventTime::now().as_nanos()
                                );
                                assert!(heavy_frame.capture_next());
                                heavy.request(ticket, &heavy_frame, work == Work::Callback);
                                assert_eq!(
                                    heavy_started
                                        .recv_timeout(Duration::from_secs(8))
                                        .expect("heavy started")
                                        .get(),
                                    ticket
                                );
                                pending_heavy = Some(ticket);
                                next_heavy = next_heavy
                                    .checked_add(Duration::from_millis(500))
                                    .expect("bounded measurement clock");
                            }
                            let capture = ticket >= 31 && (ticket - 31).is_multiple_of(49);
                            if capture {
                                assert!(light_frame.capture_next());
                            }
                            light.request(ticket, &light_frame, false);
                            assert_eq!(
                                light_started
                                    .recv_timeout(Duration::from_secs(8))
                                    .expect("light started")
                                    .get(),
                                ticket
                            );
                            let (sample, done) =
                                collect(&light_frame, &light_done, ticket, semantics);
                            emit("light", phase, &sample, &done);
                            if capture {
                                pixels("light", &light_frame, ticket);
                            }
                            if heavy_due && mode == Mode::LightFirst {
                                println!(
                                    "schedule,ticket={ticket},nominal_ns={},admitted_ns={}",
                                    next_heavy.as_nanos(),
                                    EventTime::now().as_nanos()
                                );
                                assert!(heavy_frame.capture_next());
                                heavy.request(ticket, &heavy_frame, work == Work::Callback);
                                assert_eq!(
                                    heavy_started
                                        .recv_timeout(Duration::from_secs(8))
                                        .expect("heavy started")
                                        .get(),
                                    ticket
                                );
                                pending_heavy = Some(ticket);
                                next_heavy = next_heavy
                                    .checked_add(Duration::from_millis(500))
                                    .expect("bounded measurement clock");
                            }
                            if work == Work::Throughput {
                                if let Some(previous) = pending_heavy.take() {
                                    let (sample, done) =
                                        collect(&heavy_frame, &heavy_done, previous, semantics);
                                    emit("heavy", phase, &sample, &done);
                                    pixels("heavy", &heavy_frame, previous);
                                }
                                println!(
                                    "round,{phase},ticket={ticket},wall_ns={}",
                                    round_start.elapsed().as_nanos()
                                );
                            }
                            println!(
                                "transport,ticket={ticket},retained_payload_bytes={}",
                                flui_app::testing::retained_owner_payload_bytes(&app)
                            );
                            // Request cadence is not simulated CPU work. The heavy
                            // callback above uses charged CPU time and never sleeps.
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        if let Some(ticket) = pending_heavy {
                            let (sample, done) =
                                collect(&heavy_frame, &heavy_done, ticket, semantics);
                            emit("heavy", "measured", &sample, &done);
                            pixels("heavy", &heavy_frame, ticket);
                        }
                        println!(
                            "finished,heavy={:?},light={:?}",
                            heavy_frame.snapshot(),
                            light_frame.snapshot()
                        );
                        (heavy_frame, light_frame, Instant::now())
                    }));
                    let _ = app.request_quit();
                    done.send(outcome).expect("run result");
                });
            });
        frames.run(application)?;
        let (heavy, light, shutdown) = result
            .recv_timeout(Duration::from_secs(15))
            .expect("run result")
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
        assert!(heavy.snapshot().released && light.snapshot().released);
        println!("shutdown,ns={},retired=true", shutdown.elapsed().as_nanos());
        if observed_resources {
            println!(
                "resources,stage=after_application,sta={}",
                flui_native::testing::windows::current_thread_has_sta_apartment()
            );
            let mut command = String::new();
            std::io::stdin().read_line(&mut command)?;
            anyhow::ensure!(
                command.trim() == "exit",
                "resource observer cleanup handshake"
            );
        }
        Ok(())
    }
}
#[cfg(all(windows, feature = "testing"))]
fn main() -> anyhow::Result<()> {
    native::run()
}
#[cfg(not(all(windows, feature = "testing")))]
fn main() {
    eprintln!("Windows and testing required; production dedicated support remains unavailable");
}
