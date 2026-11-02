//! Repeatable headless benchmark: mounted CPU-frame interference between UI runtimes.
//! No native windows, GPU, user-input dispatch or vsync is measured here.
//! Each runtime is constructed, used and dropped on its own owner thread.

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;
use std::time::Instant;

use flui_painting::display_list::DrawOp;
use flui_testing::widgets::{LaidOut, lay_out, tight};
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{Column, Text};

fn text(epoch: usize) -> String {
    format!("Revision {epoch:06}: The quick brown fox jumps over the lazy dog. 0123456789")
}

fn root(count: usize, epoch: usize) -> Column {
    let value = text(epoch);
    let children: Vec<BoxedView> = (0..count)
        .map(|_| Text::new(value.clone()).boxed())
        .collect();
    Column::new(children)
}

struct Panel {
    laid: LaidOut,
    count: usize,
}

impl Panel {
    fn new(count: usize) -> Self {
        let height = f64::from(u32::try_from(count).expect("bounded row count")) * 32.0;
        let laid = lay_out(root(count, 0), tight(1200.0, height));
        let panel = Self { laid, count };
        panel.verify(0);
        panel
    }

    fn verify(&self, epoch: usize) {
        assert_eq!(self.laid.find_all_text(&text(epoch)).len(), self.count);
        let paragraphs = self
            .laid
            .draw_ops()
            .iter()
            .filter(|command| matches!(command.op, DrawOp::Paragraph { .. }))
            .count();
        assert_eq!(paragraphs, self.count, "every text row must be painted");
    }

    fn step(&mut self, epoch: usize, requested: Instant) -> Sample {
        let before = self.laid.painted_frame_count();
        let start = Instant::now();
        self.laid.pump_widget(root(self.count, epoch));
        let finished = Instant::now();
        let submitted = self.laid.painted_frame_count() - before;
        assert_eq!(submitted, 1, "one updated scene per request");
        Sample {
            service_ms: finished.duration_since(start).as_secs_f64() * 1000.0,
            latency_ms: finished.duration_since(requested).as_secs_f64() * 1000.0,
            submitted,
        }
    }
}

#[derive(Clone, Copy)]
struct Sample {
    service_ms: f64,
    latency_ms: f64,
    submitted: u64,
}

enum Request {
    Frame { epoch: usize, requested: Instant },
    Stop { epoch: usize },
}

struct Worker {
    requests: SyncSender<Request>,
    results: Receiver<Sample>,
    thread: thread::JoinHandle<()>,
}

impl Worker {
    fn start(count: usize) -> Self {
        let (request_tx, request_rx) = sync_channel(1);
        let (result_tx, result_rx) = sync_channel(1);
        let (ready_tx, ready_rx) = sync_channel(0);
        let thread = thread::spawn(move || {
            let mut panel = Panel::new(count);
            ready_tx
                .send(())
                .expect("coordinator waits for initialization");
            while let Ok(request) = request_rx.recv() {
                match request {
                    Request::Frame { epoch, requested } => result_tx
                        .send(panel.step(epoch, requested))
                        .expect("coordinator receives every frame"),
                    Request::Stop { epoch } => {
                        panel.verify(epoch);
                        break;
                    }
                }
            }
        });
        ready_rx.recv().expect("owner initializes its runtime");
        drop(ready_rx);
        Self {
            requests: request_tx,
            results: result_rx,
            thread,
        }
    }

    fn submit(&self, epoch: usize, requested: Instant) {
        self.requests
            .send(Request::Frame { epoch, requested })
            .expect("owner accepts frame");
    }

    fn receive(&self) -> Sample {
        self.results.recv().expect("owner completes frame")
    }

    fn stop(self, epoch: usize) {
        self.requests
            .send(Request::Stop { epoch })
            .expect("owner accepts stop");
        self.thread.join().expect("owner shuts down without panic");
    }
}

fn emit(mode: &str, index: usize, heavy: Sample, small: Sample, round_ms: f64) {
    println!(
        "{mode},{index},{:.6},{:.6},{:.6},{:.6},{round_ms:.6},{},{}",
        heavy.service_ms,
        heavy.latency_ms,
        small.service_ms,
        small.latency_ms,
        heavy.submitted,
        small.submitted,
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map_or("serial", String::as_str);
    let heavy_count = args
        .get(2)
        .map_or(1000, |x| x.parse::<usize>().expect("row count"));
    let samples = args
        .get(3)
        .map_or(30, |x| x.parse::<usize>().expect("sample count"));
    let warmup = args
        .get(4)
        .map_or(5, |x| x.parse::<usize>().expect("warmup count"));
    let small_count = args
        .get(5)
        .map_or(1, |x| x.parse::<usize>().expect("second panel row count"));
    assert!((1..=10_000).contains(&heavy_count));
    assert!((1..=10_000).contains(&small_count));
    assert!((1..=10_000).contains(&samples));
    assert!(warmup <= 10_000);
    eprintln!(
        "flui=workspace; mode={mode}; heavy_rows={heavy_count}; small_rows={small_count}; samples={samples}; warmup={warmup}; CPU frames only, no GPU/OS/vsync; semantics disabled"
    );
    println!(
        "mode,sample,heavy_service_ms,heavy_latency_ms,small_service_ms,small_latency_ms,round_ms,heavy_submitted,small_submitted"
    );
    match mode {
        "serial" | "serial-reverse" => {
            let mut heavy = Panel::new(heavy_count);
            let mut small = Panel::new(small_count);
            for epoch in 1..=warmup + samples {
                let requested = Instant::now();
                let (h, s) = if mode == "serial" {
                    let h = heavy.step(epoch, requested);
                    (h, small.step(epoch, requested))
                } else {
                    let s = small.step(epoch, requested);
                    (heavy.step(epoch, requested), s)
                };
                let round_ms = requested.elapsed().as_secs_f64() * 1000.0;
                if epoch > warmup {
                    emit(mode, epoch - warmup, h, s, round_ms);
                }
            }
            heavy.verify(warmup + samples);
            small.verify(warmup + samples);
        }
        "separate" => {
            let heavy = Worker::start(heavy_count);
            let small = Worker::start(small_count);
            for epoch in 1..=warmup + samples {
                let requested = Instant::now();
                heavy.submit(epoch, requested);
                small.submit(epoch, requested);
                let h = heavy.receive();
                let s = small.receive();
                let round_ms = requested.elapsed().as_secs_f64() * 1000.0;
                if epoch > warmup {
                    emit(mode, epoch - warmup, h, s, round_ms);
                }
            }
            heavy.stop(warmup + samples);
            small.stop(warmup + samples);
        }
        _ => panic!("mode must be serial, serial-reverse or separate"),
    }
}
