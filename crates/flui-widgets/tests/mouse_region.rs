//! `MouseRegion` widget coverage over `RenderMouseRegion`.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::common::{lay_out, loose, size, tight};
use flui_widgets::{MouseRegion, SizedBox};

#[test]
fn mouse_region_childless_fills_parent_and_mounts_render_object() {
    let laid = lay_out(MouseRegion::new(), tight(80.0, 40.0));

    let root = laid.root();
    assert_eq!(laid.find_by_render_type("RenderMouseRegion"), root);
    assert_eq!(
        laid.size(root),
        size(80.0, 40.0),
        "childless MouseRegion must grow to the incoming biggest constraints",
    );
}

#[test]
fn mouse_region_with_child_sizes_to_child() {
    let laid = lay_out(
        MouseRegion::new().child(SizedBox::new(30.0, 20.0)),
        loose(80.0),
    );

    assert_eq!(laid.size(laid.root()), size(30.0, 20.0));
}

#[test]
fn mouse_region_hover_callback_fires_on_hover_move() {
    let hovers = Arc::new(AtomicUsize::new(0));
    let in_callback = Arc::clone(&hovers);
    let laid = lay_out(
        MouseRegion::new()
            .on_hover(move |_cx, _device, _position| {
                in_callback.fetch_add(1, Ordering::SeqCst);
            })
            .child(SizedBox::new(60.0, 30.0)),
        tight(60.0, 30.0),
    );

    laid.dispatch_pointer_hover(10.0, 10.0);
    assert_eq!(
        hovers.load(Ordering::SeqCst),
        1,
        "MouseRegion::on_hover must route through RenderMouseRegion's hit entry",
    );
}

/// Event context (ADR-0086): the region takes the owner's writer source from
/// its render-object context and opens one write per enter, hover or exit.
mod event_cx {
    use crate::common::{ProbeSignals, SignalProbe, lay_out, tight};
    use flui_view::SignalWriteExt;
    use flui_widgets::{MouseRegion, SizedBox};

    #[test]
    fn an_enter_writes_a_signal_and_rebuilds_its_reader() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            MouseRegion::new()
                .on_enter(move |cx, _device, position| count.set(cx, position.dx.get() as u32))
                .child(SizedBox::new(60.0, 30.0))
        });
        let mut app = lay_out(probe.view(), tight(60.0, 30.0));

        app.dispatch_pointer_hover(12.0, 10.0);
        assert_eq!(probe.value(), Ok(12), "the enter carried its position");
        app.tick();

        assert_eq!(probe.reads(), [0, 12], "the reader rebuilt once");
    }

    #[test]
    fn a_refused_write_in_an_enter_is_reported_not_panicked() {
        let probe = SignalProbe::new(|ProbeSignals { released, .. }| {
            MouseRegion::new()
                .on_enter(move |cx, _device, _position| released.set(cx, 1))
                .child(SizedBox::new(60.0, 30.0))
        });
        let app = lay_out(probe.view(), tight(60.0, 30.0));

        let ((), log) =
            flui_testing::log_capture::capture(|| app.dispatch_pointer_hover(12.0, 10.0));

        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        assert_eq!(probe.value(), Ok(0));
    }
}
