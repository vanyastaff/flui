//! The raster share of a frame (ADR-0305 §10): on a grid scene, how much of
//! one frame's wall-clock time is the raster executor's turn and how much is
//! the owner's thread.
//!
//! The scene is a grid of N × N cells, each a coloured box with a short text
//! label, at N = 10, 30 and 60. Six frames are measured for each N:
//!
//! - **first**: a freshly mounted tree's first frame, drawn in full, on a
//!   device that an earlier mount already warmed: pipelines built, and the
//!   glyphs in the atlas, since every mount shares one font collection;
//! - **update**: a steady-state frame in which one cell, under a repaint
//!   boundary of its own, changes colour; the bench fails unless the window
//!   slot draws it in region passes (the engine's partial-damage path);
//! - **update, no boundary**: the same change to a cell that paints into the
//!   grid's content without a boundary of its own: the content repaints, every
//!   other cell keeps its chunk (ADR-0304 §2), and the slot draws the changed
//!   cell in region passes too;
//! - **update, two far cells**: two cells near opposite corners, each under a
//!   repaint boundary of its own, change colour together, so the damage is
//!   two disjoint rects whose bounding box covers most of the surface;
//! - **update, eight cells**: eight cells spread over two rows near the top
//!   and the bottom, each under a boundary of its own, change together: a
//!   region of eight rects, the most it holds;
//! - **update, eight packed cells**: eight cells around the middle, one cell
//!   apart in two rows two apart, change together: eight rects that cover
//!   more than a third of their bounding box, large ones at N = 10.
//!
//! Each row also gives the share of the surface the frame redrew (the
//! region's bounding box when the slot drew it in one pass, its rects when it
//! drew one pass per rect) and the chunks the frame recorded and replayed
//! from the engine's recording cache (ADR-0304 §9).
//!
//! The frame is the UI runtime's frame transaction (`UiRuntime::pump`) over a
//! sink that hands each publication to an inline [`RasterExecutor`] on the
//! application's [`GpuContext`], with one window slot on an X11 window, the
//! way flui-app's raster host does on Linux. The split is by wall clock:
//!
//! - **raster** is the sink's `submit` (the runtime's `flui.frame` `submit`
//!   span): the snapshot sent, the executor's turn (walking the retained
//!   version, encoding, submitting, presenting) and the settlement drained;
//! - **owner** is the rest of the pump: build, layout, paint, compositing,
//!   the scene patch and publish, and the frame's begin and end;
//! - **present** is the part of raster from the window's `pre_present_notify`
//!   (which the slot calls after its submit) to the end of the turn. On a
//!   software adapter the surface's present waits for the frame's rendering
//!   before it copies the image to the window, so this is where the GPU wait
//!   lands; it is left out of `raster cpu`.
//!
//! Each column is the median of that column's per-frame values. Frames are
//! spaced apart, so the previous frame's GPU work has retired when the next
//! one starts and the swapchain acquire does not wait; every frame presents
//! once.
//!
//! Linux only, under an X server: `xvfb-run -a cargo bench -p flui-app
//! --bench raster_share`. Skipped when `DISPLAY` is unset or no adapter
//! answers; any other failure panics. `WGPU_BACKEND` and `VK_ICD_FILENAMES`
//! pick the adapter (lavapipe:
//! `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`). It writes no
//! Criterion samples: a run with `--save-baseline` measures nothing.

fn main() {
    // Cargo passes `--bench`, and a benchmark run passes Criterion's flags; this
    // harness takes none of them, and a baseline run has nothing to save.
    if std::env::args().any(|arg| arg == "--save-baseline") {
        println!("raster_share writes no Criterion samples; run it alone to measure");
        return;
    }
    #[cfg(target_os = "linux")]
    grid::main();
    #[cfg(not(target_os = "linux"))]
    println!("skipping raster_share: it opens an X11 window");
}

#[cfg(target_os = "linux")]
mod grid {
    use std::rc::Rc;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use flui_engine::{
        EngineError, GpuContext, RasterCommand, RasterExecutor, RasterSender, RasterVerdict,
        SendReceipt, SlotChange, SlotConfig, SlotId, SlotSettlement, SlotTarget,
    };
    use flui_foundation::geometry::{DeviceSize, Size};
    use flui_foundation::{FrameEpoch, FrameStamp, PresentationAddress, SurfaceGeneration};
    use flui_layer::{DamageMode, DamageRegion, FullReason, RasterBase, SceneSnapshot, SurfaceKey};
    use flui_native::{Platform as _, X11Platform};
    use flui_painting::FontCollection;
    use flui_painting::styling::Color;
    use flui_painting::typography::TextStyle;
    use flui_platform::{PlatformWindow, SurfaceTarget, SwapChainStatistics, WindowOptions};
    use flui_runtime::pump::SampledClock;
    use flui_runtime::sink::{FrameSink, SubmitVerdict};
    use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
    use flui_testing::widgets::SharedSignal;
    use flui_testing::window_handler::RecordingWindowHandler;
    use flui_widgets::prelude::*;
    use flui_widgets::{Center, ColoredBox, Column, Expanded, RepaintBoundary, Row, Text};
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    };

    /// The surface, in device pixels (scale factor 1): a cell is 120, 40 and
    /// 20 pixels wide at N = 10, 30 and 60.
    const SIDE: u32 = 1200;
    const SIZES: [usize; 3] = [10, 30, 60];
    /// Fresh mounts measured for the first frame, after one that warms the device.
    const MOUNTS: usize = 5;
    /// Update frames run before the measured ones, and measured.
    const WARM_UPDATES: usize = 10;
    const UPDATES: usize = 30;
    /// The pause between frames, so one frame's GPU work never overlaps the next.
    const SETTLE: Duration = Duration::from_millis(30);
    /// How long the X server may take to give the window its size.
    const CONFIGURE_PATIENCE: Duration = Duration::from_secs(5);

    const TONES: [Color; 4] = [
        Color::rgb(0x3b, 0x6e, 0xa8),
        Color::rgb(0x5a, 0x9e, 0x6f),
        Color::rgb(0xc2, 0x8a, 0x3a),
        Color::rgb(0x8e, 0x5a, 0xa8),
    ];
    const CHANGED: [Color; 2] = [Color::rgb(0xe0, 0x40, 0x40), Color::rgb(0x20, 0x20, 0x20)];

    pub(super) fn main() {
        if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
            println!("skipping raster_share: DISPLAY names no X server");
            return;
        }
        let platform = X11Platform::try_new().expect("the X server DISPLAY names answers");
        let window = platform
            .open_window(
                WindowOptions::default().size(
                    flui_platform::WindowExtent::try_new(Size::new(
                        f64::from(SIDE),
                        f64::from(SIDE),
                    ))
                    .expect("a side is an extent"),
                ),
                RecordingWindowHandler::new(),
            )
            .expect("an X11 window opens");
        await_size(&platform, &*window);
        let target = Arc::new(TimedTarget {
            inner: window.surface_target(),
            pre_presents: Mutex::new(Vec::new()),
        });
        let bootstrap = match pollster::block_on(GpuContext::bootstrap(
            platform.display_connection(),
            Some(Arc::clone(&target) as Arc<dyn SurfaceTarget>),
        )) {
            Ok(bootstrap) => bootstrap,
            Err(error @ EngineError::NoSupportedGpu { .. }) => {
                println!("skipping raster_share: {error}");
                return;
            }
            Err(error) => panic!("the GPU context over the bench's window: {error}"),
        };
        let counts = Arc::new(Mutex::new(None));
        tracing::subscriber::set_global_default(FrameCountsSubscriber {
            last: Arc::clone(&counts),
        })
        .expect("BUG: the bench installs the only subscriber");
        let mut sink = GpuSink::attach(bootstrap, target, counts);
        let fonts = FontCollection::new();

        // The first mount builds the pipelines and fills the glyph atlas.
        let cold = first_frame(&mut sink, &fonts, 60);
        println!(
            "cold first frame, N = 60 (pipelines built, glyphs rasterized): owner {:.2} ms, raster {:.2} ms",
            ms(cold.owner),
            ms(cold.raster)
        );
        println!();
        println!(
            "| N | frame | damage | redrawn | recorded | replayed | owner ms | raster ms | raster cpu ms | present ms | raster share % | cpu share % |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
        for n in SIZES {
            let firsts: Vec<Split> = (0..MOUNTS)
                .map(|_| first_frame(&mut sink, &fonts, n))
                .collect();
            report(n, "first", Drawn::Full, &firsts);
            let grid = Grid::new(n, Isolation::Boundary, Cells::Middle);
            report(
                n,
                "update",
                Drawn::Partial,
                &update_frames(&mut sink, &fonts, grid),
            );
            let grid = Grid::new(n, Isolation::None, Cells::Middle);
            let updates = update_frames(&mut sink, &fonts, grid);
            report(n, "update, no boundary", Drawn::Partial, &updates);
            let grid = Grid::new(n, Isolation::Boundary, Cells::FarApart);
            let updates = update_frames(&mut sink, &fonts, grid);
            report(n, "update, two far cells", Drawn::Partial, &updates);
            let grid = Grid::new(n, Isolation::Boundary, Cells::Eight);
            let updates = update_frames(&mut sink, &fonts, grid);
            report(n, "update, eight cells", Drawn::Partial, &updates);
            let grid = Grid::new(n, Isolation::Boundary, Cells::Packed);
            let updates = update_frames(&mut sink, &fonts, grid);
            report(n, "update, eight packed cells", Drawn::Partial, &updates);
        }
        println!();
        println!(
            "medians of {MOUNTS} mounts and {UPDATES} update frames; VK_ICD_FILENAMES = {:?}",
            std::env::var("VK_ICD_FILENAMES").ok()
        );
        drop(window);
    }

    /// Turn the X11 loop until the server reports the window at the size the
    /// bench measures at, so the surface is never configured at another size
    /// and presented twice; panics when a window manager keeps another.
    fn await_size(platform: &X11Platform, window: &dyn PlatformWindow) {
        let wanted = Size::new(f64::from(SIDE), f64::from(SIDE));
        let started = Instant::now();
        loop {
            flui_native::testing::x11::run_for(platform, Duration::from_millis(50))
                .expect("the X11 loop turns");
            let state = window.state();
            if state.content_size.get() == wanted {
                return;
            }
            assert!(
                started.elapsed() < CONFIGURE_PATIENCE,
                "the window holds {:?}, not the {SIDE} × {SIDE} the bench measures at",
                state.content_size
            );
        }
    }

    /// One frame's wall-clock split.
    #[derive(Debug, Clone, Copy)]
    struct Split {
        owner: Duration,
        raster: Duration,
        present: Duration,
        damage: Damaged,
        /// What the window slot's frame protocol counted for the frame.
        counts: FrameCounts,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Damaged {
        Full(FullReason),
        /// The fractions of the surface the region's rects and its bounding
        /// box cover.
        Region {
            rects: f64,
            bounds: f64,
        },
        Unchanged,
    }

    /// How a row's frames must have been drawn.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Drawn {
        Full,
        Partial,
    }

    fn first_frame(sink: &mut GpuSink, fonts: &FontCollection, n: usize) -> Split {
        let grid = Grid::new(n, Isolation::None, Cells::Middle);
        let mut ui_runtime = mount(&grid, fonts);
        let split = frame(&mut ui_runtime, sink);
        drop(ui_runtime);
        std::thread::sleep(SETTLE);
        split
    }

    fn update_frames(sink: &mut GpuSink, fonts: &FontCollection, grid: Grid) -> Vec<Split> {
        let mut ui_runtime = mount(&grid, fonts);
        frame(&mut ui_runtime, sink);
        std::thread::sleep(SETTLE);
        (0..WARM_UPDATES + UPDATES)
            .map(|index| {
                for changed in &grid.changed {
                    changed.set(CHANGED[index % CHANGED.len()]);
                }
                let split = frame(&mut ui_runtime, sink);
                std::thread::sleep(SETTLE);
                split
            })
            .skip(WARM_UPDATES)
            .collect()
    }

    fn frame(ui_runtime: &mut UiRuntime, sink: &mut GpuSink) -> Split {
        let started = Instant::now();
        let outcome = ui_runtime.pump(&mut SampledClock(web_time::Instant::now()), sink);
        let total = started.elapsed();
        assert!(outcome.presented(), "every measured frame presents");
        let raster = sink
            .last
            .take()
            .expect("BUG: a presented frame went through the sink");
        Split {
            owner: total.saturating_sub(raster.raster),
            ..raster
        }
    }

    fn mount(grid: &Grid, fonts: &FontCollection) -> UiRuntime {
        let window = flui_native::headless_platform()
            .open_window(
                WindowOptions::default().size(
                    flui_platform::WindowExtent::try_new(Size::new(
                        f64::from(SIDE),
                        f64::from(SIDE),
                    ))
                    .expect("a side is an extent"),
                ),
                RecordingWindowHandler::new(),
            )
            .expect("a headless window opens");
        let accessibility = window.accessibility();
        let ui_runtime = UiRuntime::try_new(
            flui_runtime::presentation::PresentationWindow::new(window, accessibility),
            1.0,
            RuntimeHostServices::new(
                Arc::new(|| {}),
                Arc::new(AtomicBool::new(false)),
                fonts,
                flui_scheduler::ClockSource::Platform,
            ),
        )
        .expect("a UI runtime");
        ui_runtime
            .attach_root_widget(grid)
            .expect("the grid mounts");
        ui_runtime
    }

    fn report(n: usize, kind: &str, drawn: Drawn, frames: &[Split]) {
        for split in frames {
            let passes = split.counts.region_passes;
            let partial = matches!(split.damage, Damaged::Region { .. }) && passes > 0;
            let full = matches!(split.damage, Damaged::Full(_)) && passes == 0;
            match drawn {
                Drawn::Partial => assert!(
                    partial,
                    "a {kind} frame at N = {n} must take the partial path: {split:?}"
                ),
                Drawn::Full => assert!(
                    full,
                    "a {kind} frame at N = {n} must draw in full: {split:?}"
                ),
            }
        }
        assert!(
            frames.iter().all(|split| split.damage == frames[0].damage),
            "every {kind} frame at N = {n} draws the same damage: {frames:?}"
        );
        let damage = match frames[0].damage {
            Damaged::Full(reason) => format!("full ({reason:?})"),
            Damaged::Region { rects, .. } => format!("{:.2} %", rects * 100.0),
            Damaged::Unchanged => "none".to_owned(),
        };
        // A frame drawn in one pass redraws the region's bounding box, one
        // drawn in a pass per rect only its rects.
        let redrawn = |split: &Split| match split.damage {
            Damaged::Full(_) => 1.0,
            Damaged::Region { bounds, .. } if split.counts.region_passes == 1 => bounds,
            Damaged::Region { rects, .. } => rects,
            Damaged::Unchanged => 0.0,
        };
        let cpu = |split: &Split| split.raster.saturating_sub(split.present);
        let share = |part: Duration, owner: Duration| {
            100.0 * part.as_secs_f64() / (part + owner).as_secs_f64()
        };
        let count = |value: fn(&FrameCounts) -> u64| {
            median(frames.iter().map(|split| value(&split.counts) as f64))
        };
        println!(
            "| {n} | {kind} | {damage} | {:.2} % | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1} | {:.1} |",
            median(frames.iter().map(|split| redrawn(split) * 100.0)),
            count(|counts| counts.chunks_recorded),
            count(|counts| counts.recording_cache_hits),
            median(frames.iter().map(|split| ms(split.owner))),
            median(frames.iter().map(|split| ms(split.raster))),
            median(frames.iter().map(|split| ms(cpu(split)))),
            median(frames.iter().map(|split| ms(split.present))),
            median(frames.iter().map(|split| share(split.raster, split.owner))),
            median(frames.iter().map(|split| share(cpu(split), split.owner))),
        );
    }

    fn median(values: impl Iterator<Item = f64>) -> f64 {
        let mut values: Vec<f64> = values.collect();
        values.sort_unstable_by(f64::total_cmp);
        values[values.len() / 2]
    }

    fn ms(duration: Duration) -> f64 {
        duration.as_secs_f64() * 1000.0
    }

    /// Whether the changing cell paints in a repaint boundary of its own.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Isolation {
        /// It does: a change repaints the cell alone and damages its bounds.
        Boundary,
        /// It paints into the grid's one picture, which a change repaints whole.
        None,
    }

    /// Which cells a frame changes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Cells {
        /// The middle cell.
        Middle,
        /// The cell one in from the top-left corner and the one one in from
        /// the bottom-right corner.
        FarApart,
        /// Eight cells spread over the grid, one per region rect.
        Eight,
        /// Eight cells packed around the middle, one cell apart, in two rows
        /// two apart: eight rects that cover more than a third of their
        /// bounding box.
        Packed,
    }

    impl Cells {
        /// The indices of the cells a frame changes.
        fn indices(self, n: usize) -> Vec<usize> {
            let middle = (n / 2) * n + n / 2;
            match self {
                Self::Middle => vec![middle],
                Self::FarApart => vec![n + 1, (n - 2) * n + n - 2],
                Self::Eight => (0..8)
                    .map(|k| {
                        let row = 1 + (k / 4) * (n - 3);
                        let column = 1 + (k % 4) * (n - 3) / 3;
                        row * n + column
                    })
                    .collect::<Vec<usize>>(),
                Self::Packed => (0..8)
                    .map(|k| {
                        let row = n / 2 - 1 + (k / 4) * 2;
                        let column = n / 2 - 3 + (k % 4) * 2;
                        row * n + column
                    })
                    .collect::<Vec<usize>>(),
            }
        }

        /// Which of the grid's signals colours the cell at `index`, if any.
        fn signal(self, n: usize, index: usize) -> Option<usize> {
            self.indices(n).iter().position(|&cell| cell == index)
        }
    }

    /// The grid: N rows of N cells, the ones `cells` names each coloured by
    /// its signal of `changed` (a signal binds one view).
    #[derive(Clone, StatelessView)]
    struct Grid {
        n: usize,
        isolation: Isolation,
        cells: Cells,
        changed: Vec<SharedSignal<Color>>,
    }

    impl Grid {
        fn new(n: usize, isolation: Isolation, cells: Cells) -> Self {
            Self {
                n,
                isolation,
                cells,
                changed: (0..cells.indices(n).len())
                    .map(|_| SharedSignal::new(CHANGED[1]))
                    .collect::<Vec<SharedSignal<Color>>>(),
            }
        }
    }

    impl StatelessView for Grid {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            let n = self.n;
            let font_size = (f64::from(SIDE) / n as f64 * 0.4).clamp(8.0, 16.0);
            let rows = (0..n)
                .map(|row| {
                    let cells = (0..n)
                        .map(|column| {
                            let index = row * n + column;
                            let label = Text::new(index.to_string())
                                .style(
                                    TextStyle::default()
                                        .with_font_size(font_size)
                                        .with_color(Color::WHITE),
                                )
                                .max_lines(1);
                            let cell = if let Some(signal) = self.cells.signal(n, index) {
                                let changing = Changing {
                                    color: self.changed[signal].clone(),
                                    label,
                                };
                                match self.isolation {
                                    Isolation::Boundary => {
                                        RepaintBoundary::new().child(changing).boxed()
                                    }
                                    Isolation::None => changing.boxed(),
                                }
                            } else {
                                ColoredBox::new(TONES[index % TONES.len()])
                                    .child(Center::new().child(label))
                                    .boxed()
                            };
                            Expanded::new(cell).boxed()
                        })
                        .collect::<Vec<BoxedView>>();
                    Expanded::new(Row::new(cells)).boxed()
                })
                .collect::<Vec<BoxedView>>();
            Column::new(rows)
        }
    }

    /// The one cell whose colour a frame changes.
    #[derive(Clone, StatefulView)]
    struct Changing {
        color: SharedSignal<Color>,
        label: Text,
    }

    struct ChangingState {
        color: SharedSignal<Color>,
    }

    impl StatefulView for Changing {
        type State = ChangingState;
        fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
            ChangingState {
                color: self.color.clone(),
            }
        }
    }

    impl ViewState<Changing> for ChangingState {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            self.color.bind(ctx);
        }
        fn build(&self, view: &Changing, ctx: &dyn BuildContext) -> impl IntoView {
            ColoredBox::new(self.color.get(ctx)).child(Center::new().child(view.label.clone()))
        }
    }

    /// The X11 window's surface target, noting each time the window slot calls
    /// `pre_present_notify`: after its frame is submitted, before the present.
    struct TimedTarget {
        inner: Arc<dyn SurfaceTarget>,
        /// `SurfaceTarget` is `Send + Sync`; only the owner thread writes it.
        pre_presents: Mutex<Vec<Instant>>,
    }

    impl TimedTarget {
        fn take_pre_presents(&self) -> Vec<Instant> {
            std::mem::take(
                &mut *self
                    .pre_presents
                    .lock()
                    .expect("BUG: nothing panics holding the notes"),
            )
        }
    }

    impl HasWindowHandle for TimedTarget {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            self.inner.window_handle()
        }
    }

    impl HasDisplayHandle for TimedTarget {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            self.inner.display_handle()
        }
    }

    impl SurfaceTarget for TimedTarget {
        fn pre_present_notify(&self, frame: Option<FrameStamp>, statistics: SwapChainStatistics) {
            self.pre_presents
                .lock()
                .expect("BUG: nothing panics holding the notes")
                .push(Instant::now());
            self.inner.pre_present_notify(frame, statistics);
        }

        fn keep_past_close(&self) {
            self.inner.keep_past_close();
        }
    }

    /// What the window slot's frame protocol counted for one frame: the
    /// region passes it drew the frame in (`0` for a frame drawn in full,
    /// the engine's own record of whether it took the partial path), the
    /// chunks it recorded and the chunks it replayed from the recording cache.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    struct FrameCounts {
        region_passes: u64,
        chunks_recorded: u64,
        recording_cache_hits: u64,
    }

    /// A subscriber that hears one event alone: the window slot's frame
    /// protocol reporting its [`FrameCounts`]. Every other callsite is never
    /// enabled, so the frames it watches pay for nothing else.
    struct FrameCountsSubscriber {
        /// `Subscriber` is `Send + Sync`; only the owner thread writes it.
        last: Arc<Mutex<Option<FrameCounts>>>,
    }

    impl FrameCountsSubscriber {
        const TARGET: &'static str = "flui_engine::frame_protocol";
        const FIELD: &'static str = "region_passes";

        fn wanted(metadata: &tracing::Metadata<'_>) -> bool {
            metadata.is_event()
                && metadata.target() == Self::TARGET
                && metadata.fields().field(Self::FIELD).is_some()
        }
    }

    impl tracing::Subscriber for FrameCountsSubscriber {
        fn register_callsite(
            &self,
            metadata: &'static tracing::Metadata<'static>,
        ) -> tracing::subscriber::Interest {
            if Self::wanted(metadata) {
                tracing::subscriber::Interest::always()
            } else {
                tracing::subscriber::Interest::never()
            }
        }

        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            Self::wanted(metadata)
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            // No span is ever enabled, so none is ever made.
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            let mut counts = CountFields::default();
            event.record(&mut counts);
            if counts.passes_seen {
                *self
                    .last
                    .lock()
                    .expect("BUG: nothing panics holding the counts") = Some(counts.counts);
            }
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    /// The fields of the frame protocol's event that [`FrameCounts`] holds.
    #[derive(Default)]
    struct CountFields {
        counts: FrameCounts,
        passes_seen: bool,
    }

    impl CountFields {
        fn set(&mut self, field: &tracing::field::Field, value: u64) {
            match field.name() {
                FrameCountsSubscriber::FIELD => {
                    self.counts.region_passes = value;
                    self.passes_seen = true;
                }
                "chunks_recorded" => self.counts.chunks_recorded = value,
                "recording_cache_hits" => self.counts.recording_cache_hits = value,
                _ => {}
            }
        }
    }

    impl tracing::field::Visit for CountFields {
        fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
            self.set(field, value);
        }

        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if let Ok(value) = format!("{value:?}").parse::<u64>() {
                self.set(field, value);
            }
        }
    }

    /// The one presentation every mount's frames are stamped as: the slot
    /// serves one window, whichever UI runtime drew the frame.
    fn address() -> PresentationAddress {
        PresentationAddress {
            ui_runtime_id: flui_foundation::UiRuntimeId::new(1),
            presentation_id: flui_foundation::PresentationId::new(1),
        }
    }

    /// A frame sink over an inline executor with one window slot, as
    /// flui-app's raster host submits on its owner thread.
    struct GpuSink {
        executor: RasterExecutor,
        sender: RasterSender,
        slot: SlotId,
        generation: SurfaceGeneration,
        base: RasterBase,
        target: Arc<TimedTarget>,
        counts: Arc<Mutex<Option<FrameCounts>>>,
        settlements: Vec<SlotSettlement>,
        /// The slot's own sequence: each mount's UI runtime numbers its frames
        /// from the start, and a slot's stamps only grow.
        epoch: FrameEpoch,
        /// The last submit's split, without the owner's part.
        last: Option<Split>,
    }

    impl GpuSink {
        fn attach(
            bootstrap: flui_engine::GpuBootstrap,
            target: Arc<TimedTarget>,
            counts: Arc<Mutex<Option<FrameCounts>>>,
        ) -> Self {
            let first = bootstrap.first.expect("the window's surface was made");
            let (mut executor, sender) = RasterExecutor::new(
                bootstrap.context,
                Rc::new(flui_foundation::SystemClock),
                || {},
                || {},
            );
            let slot = sender.mint_slot().expect("a slot id is left");
            sender
                .send(RasterCommand::Attach {
                    slot,
                    address: address(),
                    target: SlotTarget::Window(first),
                    config: SlotConfig::new(),
                })
                .expect("the attach is accepted");
            let side = i32::try_from(SIDE).expect("BUG: the side fits an i32");
            let receipt = sender
                .send(RasterCommand::Configure {
                    slot,
                    change: SlotChange::Size(DeviceSize::new(side, side)),
                })
                .expect("the size is accepted");
            executor.turn();
            let generation = sender
                .record(slot)
                .map_or(SurfaceGeneration::ZERO, |record| record.required_generation);
            assert!(
                matches!(receipt, SendReceipt::Configured(_))
                    && generation != SurfaceGeneration::ZERO,
                "the window slot is configured: {receipt:?}"
            );
            Self {
                executor,
                sender,
                slot,
                generation,
                base: RasterBase::new(DamageMode::default()),
                target,
                counts,
                settlements: Vec::new(),
                epoch: FrameEpoch::ZERO,
                last: None,
            }
        }
    }

    impl FrameSink for GpuSink {
        fn surface_size(&mut self) -> (u32, u32) {
            (SIDE, SIDE)
        }

        fn submit(
            &mut self,
            _epoch: FrameEpoch,
            publication: flui_layer::ScenePublication,
        ) -> SubmitVerdict {
            let started = Instant::now();
            self.epoch = self.epoch.next();
            let epoch = self.epoch;
            let key = SurfaceKey {
                size: (SIDE, SIDE),
                generation: self.generation,
            };
            let damage = self.base.damage_for(&publication, key);
            let damaged = match &damage {
                DamageRegion::Full(reason) => Damaged::Full(*reason),
                DamageRegion::Region(region) => {
                    let surface = f64::from(SIDE * SIDE);
                    Damaged::Region {
                        rects: region.area() as f64 / surface,
                        bounds: region.bounds().area() as f64 / surface,
                    }
                }
                DamageRegion::Unchanged => Damaged::Unchanged,
            };
            let shown = publication.scene.id();
            let snapshot = SceneSnapshot::of_version(
                FrameStamp::new(address(), epoch, self.generation),
                publication.base,
                damage,
                publication.scene,
            );
            self.sender
                .send(RasterCommand::Frame {
                    slot: self.slot,
                    snapshot,
                })
                .expect("the frame is accepted");
            self.executor.turn();
            self.settlements.clear();
            self.sender
                .drain_settlements(self.slot, &mut self.settlements);
            let raster = started.elapsed();
            let pre_presents = self.target.take_pre_presents();
            let counts = self
                .counts
                .lock()
                .expect("BUG: nothing panics holding the counts")
                .take()
                .expect("the window slot reports what it counted for the frame");
            let verdict = self
                .settlements
                .iter()
                .find(|settlement| settlement.epoch == epoch)
                .map(|settlement| settlement.verdict);
            assert_eq!(
                verdict,
                Some(RasterVerdict::Presented),
                "the frame presents"
            );
            let [presented_at] = pre_presents[..] else {
                panic!("a frame presents once, not {} times", pre_presents.len());
            };
            self.base.shown(shown, key);
            self.last = Some(Split {
                owner: Duration::ZERO,
                raster,
                present: (started + raster) - presented_at,
                damage: damaged,
                counts,
            });
            SubmitVerdict::Presented
        }
    }
}
