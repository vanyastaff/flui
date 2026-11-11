//! Real widget reassembly delivers a frame request; a failed host delivery
//! must not strand an already accepted command behind the reload.
use super::*;
use flui_protocol::{ErrorCode, ReadParams, method};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn reload_failure(failures: usize, fail_reassemble: bool) {
    let failures_left = Arc::new(AtomicUsize::new(0));
    let failures_in_wake = Arc::clone(&failures_left);
    let delivered = Arc::new(AtomicUsize::new(0));
    let delivered_in_wake = Arc::clone(&delivered);
    let ui_runtime = new_runtime(Arc::new(move || {
        let remaining = failures_in_wake.load(Ordering::Relaxed);
        if remaining != 0 {
            failures_in_wake.store(remaining - 1, Ordering::Relaxed);
            assert!(remaining != failures, "reload wake first");
            panic!("reload wake secondary");
        }
        delivered_in_wake.fetch_add(1, Ordering::Relaxed);
    }))
    .expect("runtime");
    ui_runtime
        .attach_root_widget_to_for_test(ui_runtime.presentation_id(), &SizedBox::square(20.0))
        .expect("root");
    let window = ui_runtime
        .dev_agent_window(ui_runtime.presentation_id())
        .expect("window");
    let sender = ui_runtime.command_sender();
    let frame_sender = sender.clone();
    ui_runtime
        .presentation_widgets_for_test(ui_runtime.presentation_id())
        .set_on_need_frame(move || {
            assert!(!fail_reassemble, "reassemble first");
            frame_sender.request_redraw();
        });
    sender
        .request_hot_reload(crate::reload::ReloadTier::Reassemble)
        .expect("reload accepted");
    let mut tail = window
        .call::<method::SemanticsRead>(ReadParams::new(window.id()))
        .expect("tail accepted");
    let _ = ui_runtime.take_redraw_request();
    let before = delivered.load(Ordering::Relaxed);
    failures_left.store(failures, Ordering::Relaxed);
    let payload = catch_unwind(AssertUnwindSafe(|| ui_runtime.drain_commands()))
        .expect_err("reload wake failed");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if fail_reassemble {
            "reassemble first"
        } else {
            "reload wake first"
        })
    );
    flui_foundation::panic::discard_payload(payload);
    assert!(
        tail.try_take().is_none(),
        "the failure leaves the accepted FIFO tail queued"
    );
    assert!(
        ui_runtime.take_redraw_request(),
        "partially reassembled state remains dirty"
    );
    if failures == 0 || (failures == 1 && !fail_reassemble) {
        assert_eq!(
            delivered.load(Ordering::Relaxed),
            before + 1,
            "rearm delivered a new owner turn without ingress"
        );
    } else {
        assert_eq!(
            delivered.load(Ordering::Relaxed),
            before,
            "no owner delivery succeeded"
        );
    }
    let report = ui_runtime.drain_commands();
    assert_eq!(report.invoked, 1);
    let reply = tail
        .try_take()
        .expect("accepted read answered at the next turn");
    assert_eq!(
        reply.expect_err("no semantics frame yet").code,
        ErrorCode::Busy
    );
    assert_eq!(
        delivered.load(Ordering::Relaxed),
        before + 1,
        "later owner opportunity pays remaining wake debt once"
    );
    assert_eq!(ui_runtime.drain_commands(), DrainReport::default());
    assert_eq!(delivered.load(Ordering::Relaxed), before + 1);
}

pub(crate) fn failed_reload_wake_rearms_the_accepted_tail() {
    reload_failure(1, false);
}
pub(crate) fn competing_reload_wakes_preserve_the_first_failure_and_retry() {
    reload_failure(2, false);
}

pub(crate) fn reassemble_failure_keeps_priority_over_a_failed_rearm() {
    reload_failure(1, true);
}

pub(crate) fn reassemble_failure_rearms_the_accepted_tail() {
    reload_failure(0, true);
}

/// A hot reload renews the presentation's scene: the next frame repaints
/// from the root into a new lineage and is published in full, with no base
/// the raster side could apply a region to.
pub(crate) fn a_hot_reload_publishes_the_next_frame_in_full() {
    let ui_runtime = new_runtime(Arc::new(|| {})).expect("runtime");
    ui_runtime
        .attach_root_widget_to_for_test(ui_runtime.presentation_id(), &SizedBox::square(20.0))
        .expect("root");
    let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut sink = {
        let seen = Arc::clone(&seen);
        crate::testing::ScriptedSink::new(move |_, publication| {
            seen.lock()
                .push((publication.scene.id(), publication.base, publication.damage));
            crate::sink::SubmitVerdict::Presented
        })
    };
    ui_runtime.render_frame(&mut sink);
    ui_runtime
        .command_sender()
        .request_hot_reload(crate::reload::ReloadTier::Reassemble)
        .expect("reload accepted");
    let _ = ui_runtime.drain_commands();
    ui_runtime.render_frame(&mut sink);

    let seen = seen.lock();
    assert_eq!(seen.len(), 2, "the reloaded tree is painted");
    assert_ne!(
        seen[1].0.lineage(),
        seen[0].0.lineage(),
        "the reload starts a new lineage"
    );
    assert_eq!(seen[1].1, None, "nothing on screen is a base of it");
    assert_eq!(
        seen[1].2,
        flui_layer::DamageRegion::Full(flui_layer::FullReason::Resync(
            flui_layer::ResyncReason::Rebuilt
        ))
    );
}
