//! `PageView::on_page_changed` receives an `EventCx` (ADR-0086). The
//! controller's listener only records a change; the callback runs after the
//! frame on the local post-frame lane, outside any build, so its writes land
//! — every recorded page, in order, through whichever callback is current
//! when it runs.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_painting::styling::Color;
use flui_rendering::view::ViewportOffset as _;
use flui_view::BoxedView;
use flui_view::prelude::*;
use flui_widgets::{ColoredBox, PageController, PageView, SizedBox};

use crate::common::{LaidOut, ProbeSignals, SignalProbe, lay_out, tight};

const PAGE: f64 = 300.0;

fn pages() -> Vec<BoxedView> {
    (0..4_u8)
        .map(|index| {
            ColoredBox::new(Color::rgb(10 * index, 20, 30))
                .into_view()
                .boxed()
        })
        .collect()
}

fn page_view(controller: &PageController) -> PageView {
    PageView::new(pages()).controller(controller.clone())
}

fn mounted(probe: &SignalProbe) -> LaidOut {
    lay_out(probe.view(), tight(PAGE, PAGE))
}

#[test]
fn page_change_writes_a_signal_after_the_frame() {
    let controller = PageController::new();
    let probe = {
        let controller = controller.clone();
        SignalProbe::new(move |ProbeSignals { count, .. }| {
            page_view(&controller).on_page_changed(move |cx, page| count.set(cx, page as u32))
        })
    };
    let mut app = mounted(&probe);

    controller.jump_to_page(2);
    assert_eq!(
        probe.value(),
        Ok(0),
        "the listener records, it does not call"
    );

    let ((), log) = flui_testing::log_capture::capture(|| app.tick());
    assert_eq!(probe.value(), Ok(2), "delivered after the frame");
    assert!(
        !log.contains("refused"),
        "the write did not run inside a build: {log}"
    );

    app.tick();
    assert_eq!(probe.reads(), [0, 2], "and the reader rebuilt once");
}

#[test]
fn page_changes_in_one_frame_are_delivered_in_order() {
    let controller = PageController::new();
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let probe = {
        let controller = controller.clone();
        let delivered = Rc::clone(&delivered);
        SignalProbe::new(move |ProbeSignals { count, .. }| {
            let delivered = Rc::clone(&delivered);
            page_view(&controller).on_page_changed(move |cx, page| {
                delivered.borrow_mut().push(page);
                count.update(cx, |n| *n = *n * 10 + page as u32)
            })
        })
    };
    let mut app = mounted(&probe);

    controller.jump_to_page(1);
    controller.jump_to_page(2);
    app.tick();

    assert_eq!(*delivered.borrow(), [1, 2], "both pages, in order");
    assert_eq!(probe.value(), Ok(12), "each wrote through its own cx");
}

/// A change recorded but not yet delivered when the page view unmounts is
/// dropped, and the callback's captures are released with the state.
#[test]
fn a_disposed_page_view_drops_its_pending_change() {
    struct DropProbe(Rc<Cell<u32>>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let controller = PageController::new();
    let show = Rc::new(Cell::new(true));
    let drops = Rc::new(Cell::new(0));
    let probe = {
        let controller = controller.clone();
        let show = Rc::clone(&show);
        let drops = Rc::clone(&drops);
        SignalProbe::new(move |ProbeSignals { count, .. }| {
            if !show.get() {
                return SizedBox::shrink().into_view().boxed();
            }
            let held = DropProbe(Rc::clone(&drops));
            page_view(&controller)
                .on_page_changed(move |cx, page| {
                    let _held = &held;
                    count.set(cx, page as u32)
                })
                .into_view()
                .boxed()
        })
    };
    let mut app = mounted(&probe);
    let drops_while_mounted = drops.get();

    controller.jump_to_page(2);
    show.set(false);
    app.pump();
    app.tick();

    assert_eq!(
        probe.value(),
        Ok(0),
        "the pending change was never delivered"
    );
    assert!(
        drops.get() > drops_while_mounted,
        "the mounted callback's capture was released"
    );
}

/// A change recorded before a rebuild that installs a new callback reaches
/// the new one: delivery reads the callback when it runs.
#[test]
fn the_latest_callback_receives_a_queued_page_change() {
    let controller = PageController::new();
    let generation = Rc::new(Cell::new(1_u32));
    let probe = {
        let controller = controller.clone();
        let generation = Rc::clone(&generation);
        SignalProbe::new(move |ProbeSignals { count, .. }| {
            let base = generation.get() * 100;
            page_view(&controller)
                .on_page_changed(move |cx, page| count.set(cx, base + page as u32))
        })
    };
    let mut app = mounted(&probe);

    controller.jump_to_page(2);
    generation.set(2);
    app.pump();

    assert_eq!(
        probe.value(),
        Ok(202),
        "the rebuilt closure ran, not the old one"
    );
}

#[test]
fn a_refused_page_change_write_is_reported_not_panicked() {
    let controller = PageController::new();
    let probe = {
        let controller = controller.clone();
        SignalProbe::new(move |ProbeSignals { released, .. }| {
            page_view(&controller).on_page_changed(move |cx, _page| released.set(cx, 1))
        })
    };
    let mut app = mounted(&probe);

    controller.jump_to_page(1);
    let ((), log) = flui_testing::log_capture::capture(|| app.tick());

    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );
    app.tick();
    assert_eq!(probe.value(), Ok(0), "other state is intact");
    assert_eq!(
        controller.page().map(f64::round),
        Some(1.0),
        "and the page view kept its page"
    );
}

/// A drag that crosses the halfway point changes the page mid-drag; the
/// frame that delivers the change rebuilds the page view and its
/// `Scrollable`, and the drag must keep scrolling afterwards.
#[test]
fn a_page_change_mid_drag_keeps_the_drag() {
    let controller = PageController::new();
    let probe = {
        let controller = controller.clone();
        SignalProbe::new(move |ProbeSignals { count, .. }| {
            page_view(&controller).on_page_changed(move |cx, page| count.set(cx, page as u32))
        })
    };
    let mut app = mounted(&probe);
    let pixels = || controller.position().pixels();

    app.dispatch_pointer_down(280.0, 150.0);
    app.dispatch_pointer_move(250.0, 150.0);
    app.dispatch_pointer_move(200.0, 150.0);
    app.dispatch_pointer_move(100.0, 150.0);
    app.tick();
    assert_eq!(probe.value(), Ok(1), "premise: the page changed mid-drag");
    app.tick();

    let before = pixels();
    app.dispatch_pointer_move(60.0, 150.0);
    let after = pixels();
    assert!(
        (after - before - 40.0).abs() < 1e-6,
        "the drag still scrolls after the rebuild: {before} -> {after}"
    );
    app.dispatch_pointer_up(60.0, 150.0);
}

/// Each recorded page is its own post-frame entry, so a callback that
/// panics on one page does not take the pages after it with it: the lane
/// recovers and the next frame delivers the rest, in order.
#[test]
fn a_panicking_page_change_does_not_discard_the_later_pages() {
    let controller = PageController::new();
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let probe = {
        let controller = controller.clone();
        let delivered = Rc::clone(&delivered);
        SignalProbe::new(move |ProbeSignals { count, .. }| {
            let delivered = Rc::clone(&delivered);
            page_view(&controller).on_page_changed(move |cx, page| {
                assert_ne!(page, 1, "intentional page-change panic");
                delivered.borrow_mut().push(page);
                count.set(cx, page as u32)
            })
        })
    };
    let mut app = mounted(&probe);

    controller.jump_to_page(1);
    controller.jump_to_page(2);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.tick()));
    assert!(panicked.is_err(), "the callback's panic propagates");
    app.tick();

    assert_eq!(*delivered.borrow(), [2], "the later page still ran");
    assert_eq!(probe.value(), Ok(2));
}
