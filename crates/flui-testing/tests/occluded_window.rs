//! A window the window system keeps off the screen draws no frame while its
//! content changes. Where the occlusion may be a covering, the window draws
//! one owed frame once it has been quiet a while, so the surface the window
//! system shows on uncovering holds the content of then, not of before the
//! covering; elsewhere (a lock, a background app) it draws nothing until it is
//! seen (ADR-0357).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_foundation::geometry::Size;
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, Leaf, PaintContext, RenderBox};
use flui_testing::widgets::SharedSignal;
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_view::prelude::*;
use flui_view::{RenderView, StatefulView, View};

const FRAME: Duration = Duration::from_millis(16);

/// A 40 × 40 leaf that records the number it carries each time it paints.
#[derive(Debug)]
struct NumberBox {
    number: u32,
    painted: Arc<AtomicU32>,
}

impl flui_foundation::inspect::Inspect for NumberBox {
    fn kind(&self) -> &'static str {
        "NumberBox"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderBox for NumberBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::new(40.0, 40.0))
    }

    fn paint(&self, _ctx: &mut PaintContext<'_, Leaf>) {
        self.painted.store(self.number, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct PaintedNumber {
    number: u32,
    painted: Arc<AtomicU32>,
}

impl RenderView for PaintedNumber {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = NumberBox;

    fn create_render_object(&self, _ctx: &flui_view::RenderObjectContext<'_>) -> NumberBox {
        NumberBox {
            number: self.number,
            painted: Arc::clone(&self.painted),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut NumberBox,
    ) -> flui_rendering::RenderUpdateImpact {
        if render_object.number == self.number {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        render_object.number = self.number;
        flui_rendering::RenderUpdateImpact::PAINT
    }
}

impl View for PaintedNumber {
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// The content: the number a test sets, painted by a [`NumberBox`].
#[derive(Clone, StatefulView)]
struct Content {
    number: SharedSignal<u32>,
    painted: Arc<AtomicU32>,
}

struct ContentState {
    number: SharedSignal<u32>,
}

impl StatefulView for Content {
    type State = ContentState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ContentState {
            number: self.number.clone(),
        }
    }
}

impl ViewState<Content> for ContentState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.number.bind(ctx);
    }

    fn build(&self, view: &Content, ctx: &dyn BuildContext) -> impl IntoView {
        PaintedNumber {
            number: self.number.get(ctx),
            painted: Arc::clone(&view.painted),
        }
    }
}

/// How the window comes back into sight.
#[derive(Clone, Copy, Debug)]
enum Uncovering {
    /// After the runtime's next wake: the quiet period, where there is one.
    AfterTheNextWake,
    /// One frame after the change, before any quiet period ends.
    AtOnce,
}

struct Case {
    name: &'static str,
    /// Whether the window system tells a covering, so the occlusion may be one.
    reports_covering: bool,
    uncovering: Uncovering,
    /// Whether the content changes while the window is occluded.
    changes: bool,
    /// Frames drawn while still occluded.
    frames_while_occluded: u64,
    /// Frames drawn once the window is seen again.
    frames_once_seen: u64,
}

/// The cases differ in whether the occlusion may be a covering, whether the
/// content changes while the window is occluded and when the window is
/// uncovered. Once seen, the window draws exactly one frame, with the content
/// of then: the only window coming back into sight turns frames back on, and
/// that redraws it in full.
///
/// The first row fails without the owed frame: the covered window draws
/// nothing until it is uncovered, so its uncovering shows the frame of before
/// the covering until the frame drawn then reaches the screen; and the loop is
/// asked for a frame at once and on every wake while covered, each drawing
/// nothing.
#[test]
fn an_occluded_window_draws_its_change_in_exactly_one_frame() {
    let cases = [
        Case {
            name: "a covering draws one owed frame once quiet",
            reports_covering: true,
            uncovering: Uncovering::AfterTheNextWake,
            changes: true,
            frames_while_occluded: 1,
            frames_once_seen: 1,
        },
        Case {
            name: "a covering lifted before the quiet period ends draws one frame then",
            reports_covering: true,
            uncovering: Uncovering::AtOnce,
            changes: true,
            frames_while_occluded: 0,
            frames_once_seen: 1,
        },
        Case {
            name: "an occlusion that is no covering draws nothing until seen",
            reports_covering: false,
            uncovering: Uncovering::AfterTheNextWake,
            changes: true,
            frames_while_occluded: 0,
            frames_once_seen: 1,
        },
        Case {
            name: "a covering with nothing changed draws nothing while covered",
            reports_covering: true,
            uncovering: Uncovering::AfterTheNextWake,
            changes: false,
            frames_while_occluded: 0,
            frames_once_seen: 1,
        },
    ];
    let failed: Vec<&str> = cases
        .iter()
        .filter(|case| std::panic::catch_unwind(|| run(case)).is_err())
        .map(|case| case.name)
        .collect();
    assert!(failed.is_empty(), "failing cases: {failed:?}");
}

fn run(case: &Case) {
    let name = case.name;
    let window = HeadlessWindow::new(40, 40);
    let window = if case.reports_covering {
        window.with_occlusion_reports()
    } else {
        window
    };
    let mut host = HeadlessHost::new(window);
    let number = SharedSignal::new(1);
    let painted = Arc::new(AtomicU32::new(0));
    host.attach(&Content {
        number: number.clone(),
        painted: Arc::clone(&painted),
    })
    .expect("a fresh host has no root");
    let _ = host.pump(FRAME);
    assert_eq!(painted.load(Ordering::SeqCst), 1, "{name}: the first frame");
    let primary = host.primary_window();

    host.set_window_occluded(primary, true);
    let _ = host.pump(FRAME);
    let before = host.sink().submits();
    let shown = if case.changes {
        number.set(2);
        2
    } else {
        1
    };
    let _ = host.pump(FRAME);
    assert_eq!(
        host.sink().submits(),
        before,
        "{name}: the change draws no frame of its own while occluded"
    );

    if matches!(case.uncovering, Uncovering::AfterTheNextWake) {
        match host.time_to_next_frame() {
            Some(wait) => {
                assert!(
                    wait > FRAME,
                    "{name}: an occluded window waits out a quiet period, not a frame: {wait:?}"
                );
                let _ = host.pump(wait);
            }
            None => assert_eq!(
                case.frames_while_occluded, 0,
                "{name}: an owed frame needs a wake"
            ),
        }
        let _ = host.pump(FRAME);
    }
    let drawn_occluded = host.sink().submits() - before;
    assert_eq!(
        drawn_occluded, case.frames_while_occluded,
        "{name}: frames while occluded"
    );
    if drawn_occluded > 0 {
        assert_eq!(
            painted.load(Ordering::SeqCst),
            shown,
            "{name}: the owed frame paints the new content"
        );
    }

    host.set_window_occluded(primary, false);
    let seen = host.sink().submits();
    let _ = host.pump(FRAME);
    let _ = host.pump(FRAME);
    assert_eq!(
        host.sink().submits() - seen,
        case.frames_once_seen,
        "{name}: frames once seen"
    );
    assert_eq!(
        painted.load(Ordering::SeqCst),
        shown,
        "{name}: the window shows its content once seen"
    );
}
