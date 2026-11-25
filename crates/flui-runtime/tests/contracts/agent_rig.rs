//! A runtime of its own, on a manual clock, showing a view to its agent: what the agent tests that
//! the headless host cannot reach drive. Its wake panics while the test says so, each time with its
//! own number, and its window can answer the gesture geometry a host reports.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock};
use flui_protocol::input::{Journal, JournalParams};
use flui_protocol::method;
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_view::View;
use flui_view::dev_agent::{AgentWindow, InputLease};

/// Presents every frame on a surface of its size.
struct Discard {
    size: (u32, u32),
}

impl FrameSink for Discard {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn submit(
        &mut self,
        _epoch: flui_foundation::FrameEpoch,
        _scene: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

/// A view shown by a runtime of its own, its agent window and a lease on its input. While
/// `failing` is set, its wake panics with `wake <n>`, `n` counting the wakes that panicked.
pub(crate) struct Rig {
    pub(crate) runtime: UiRuntime,
    pub(crate) clock: ManualClock,
    failing: Arc<AtomicBool>,
    /// How many times the wake ran, failing or not.
    wakes: Arc<AtomicU32>,
    pub(crate) window: AgentWindow,
    pub(crate) lease: InputLease,
    sink: Discard,
    /// The platform window, kept alive: the presentation holds it weakly.
    _platform_window: Rc<dyn flui_platform::PlatformWindow>,
}

impl Rig {
    /// `view` in a headless window 200 px square, after its first four frames.
    pub(crate) fn showing(view: &(impl View + Clone + 'static)) -> Self {
        Self::on(crate::owner_publication::window(), (200, 200), view)
    }

    /// `view` in `window`, whose surface is `size`, after its first four frames.
    pub(crate) fn on(
        window: PresentationWindow,
        size: (u32, u32),
        view: &(impl View + Clone + 'static),
    ) -> Self {
        let platform_window = Rc::clone(window.window());
        let clock = ManualClock::new();
        let failing = Arc::new(AtomicBool::new(false));
        let wakes = Arc::new(AtomicU32::new(0));
        let wake = {
            let failing = Arc::clone(&failing);
            let failed = AtomicU32::new(0);
            let wakes = Arc::clone(&wakes);
            Arc::new(move || {
                wakes.fetch_add(1, Ordering::SeqCst);
                if failing.load(Ordering::SeqCst) {
                    let n = failed.fetch_add(1, Ordering::SeqCst) + 1;
                    std::panic::panic_any(format!("wake {n}"));
                }
            })
        };
        let runtime = UiRuntime::try_new(
            window,
            1.0,
            RuntimeHostServices::new(
                wake,
                Arc::new(AtomicBool::new(false)),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Manual(clock.clone()),
            ),
        )
        .expect("runtime");
        runtime.attach_root_widget(view).expect("mount");
        let window = runtime
            .dev_agent_window(runtime.presentation_id())
            .expect("the runtime hosts its primary presentation");
        let lease = window
            .input_lease()
            .expect("an open window leases its input");
        let mut rig = Self {
            runtime,
            clock,
            failing,
            wakes,
            window,
            lease,
            sink: Discard { size },
            _platform_window: platform_window,
        };
        for _ in 0..4 {
            rig.frame();
        }
        rig
    }

    /// A headless window, 800 × 600 logical pixels, answering `geometry` as the gesture geometry
    /// its host reports.
    pub(crate) fn window_reporting(geometry: flui_platform::GestureGeometry) -> PresentationWindow {
        let headless = crate::owner_publication::window();
        let window = flui_platform::testing::MockWindow::over(Rc::clone(headless.window()))
            .with_gesture_geometry(Ok(Some(geometry)));
        PresentationWindow::new(Rc::new(window), None)
    }

    /// A 16 ms frame.
    pub(crate) fn frame(&mut self) {
        self.clock.advance(Duration::from_millis(16));
        let _ = self.runtime.pump(
            &mut flui_runtime::pump::SampledClock(self.clock.now()),
            &mut self.sink,
        );
    }

    /// An owner turn with no frame whose wakes fail: the panic it continues with.
    pub(crate) fn failing_turn(&mut self) -> String {
        self.failing.store(true, Ordering::SeqCst);
        let turned = catch_unwind(AssertUnwindSafe(|| self.runtime.pump_background()));
        self.failing.store(false, Ordering::SeqCst);
        let payload = turned.expect_err("a panic continues out of the turn");
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "a payload of another type".to_owned())
    }

    /// A 16 ms frame whose wakes fail: the panic it continued with, if one did.
    pub(crate) fn failing_frame(&mut self) -> Option<String> {
        self.failing.store(true, Ordering::SeqCst);
        let framed = catch_unwind(AssertUnwindSafe(|| self.frame()));
        self.failing.store(false, Ordering::SeqCst);
        framed.err().map(|payload| {
            payload
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "a payload of another type".to_owned())
        })
    }

    /// The window's input journal, read within 30 frames.
    pub(crate) fn journal(&mut self) -> Journal {
        let mut read = self
            .window
            .call::<method::InputJournal>(JournalParams::new(self.window.id()))
            .expect("the inbox has room");
        for _ in 0..30 {
            if let Some(journal) = read.try_take() {
                return journal.expect("the journal is read");
            }
            self.frame();
        }
        panic!("the journal is not read within 30 frames");
    }
}

impl Rig {
    /// How many times the runtime's wake has run so far.
    pub(crate) fn wakes(&self) -> u32 {
        self.wakes.load(Ordering::SeqCst)
    }
}
