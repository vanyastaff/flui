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

/// A hot reload renews the scene of every presentation: each one starts a new
/// lineage with no base the raster side could apply a region to, and the next
/// frame repaints from the root and is published in full.
pub(crate) fn a_hot_reload_renews_every_scene_lineage() {
    let mut ui_runtime = new_runtime(Arc::new(|| {})).expect("runtime");
    ui_runtime
        .attach_root_widget_to_for_test(ui_runtime.presentation_id(), &SizedBox::square(20.0))
        .expect("root");
    let window: Rc<dyn PlatformWindow> = Rc::new(crate::testing::TestWindow::new());
    let assembled = ui_runtime.assemble_presentation(Rc::clone(&window));
    let secondary = ui_runtime.install_presentation(assembled);
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
    let shown = seen.lock().pop().expect("the first frame is published");
    assert!(
        matches!(shown.2, flui_layer::DamageRegion::Full(_)),
        "the first frame is drawn in full"
    );
    let lineage_of = |ui_runtime: &UiRuntime, id| {
        ui_runtime
            .presentations
            .get(id)
            .expect("presentation")
            .scene
            .publication()
            .expect("no frame holds the scene")
            .scene
            .id()
            .lineage()
    };
    let primary = ui_runtime.presentation_id();
    let before = [
        lineage_of(&ui_runtime, primary),
        lineage_of(&ui_runtime, secondary),
    ];
    assert_eq!(before[0], shown.0.lineage());

    ui_runtime
        .command_sender()
        .request_hot_reload(crate::reload::ReloadTier::Reassemble)
        .expect("reload accepted");
    let _ = ui_runtime.drain_commands();
    for (id, old) in [primary, secondary].into_iter().zip(before) {
        let publication = ui_runtime
            .presentations
            .get(id)
            .expect("presentation")
            .scene
            .publication()
            .expect("no frame holds the scene");
        assert_ne!(
            publication.scene.id().lineage(),
            old,
            "the reload starts a new lineage for every presentation"
        );
        assert_eq!(publication.base, None, "nothing on screen is a base of it");
    }

    ui_runtime.render_frame(&mut sink);
    let (id, base, damage) = seen.lock().pop().expect("the reloaded tree is published");
    assert_ne!(id.lineage(), shown.0.lineage());
    assert_eq!(base, None);
    assert_eq!(
        damage,
        flui_layer::DamageRegion::Full(flui_layer::FullReason::Resync(
            flui_layer::ResyncReason::Rebuilt
        ))
    );
}
