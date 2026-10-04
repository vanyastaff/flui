//! Real widget reassembly delivers a frame request; a failed host delivery
//! must not strand an already accepted command behind the reload.
use super::*;
use flui_protocol::{ErrorCode, ReadQuery};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn reload_failure(failures: usize, fail_reassemble: bool) {
    let failures_left = Arc::new(AtomicUsize::new(0));
    let failures_in_wake = Arc::clone(&failures_left);
    let delivered = Arc::new(AtomicUsize::new(0));
    let delivered_in_wake = Arc::clone(&delivered);
    let realm = new_runtime(Arc::new(move || {
        let remaining = failures_in_wake.load(Ordering::Relaxed);
        if remaining != 0 {
            failures_in_wake.store(remaining - 1, Ordering::Relaxed);
            assert!(remaining != failures, "reload wake first");
            panic!("reload wake secondary");
        }
        delivered_in_wake.fetch_add(1, Ordering::Relaxed);
    }))
    .expect("runtime");
    realm
        .attach_root_widget_to_for_test(realm.presentation_id(), &SizedBox::square(20.0))
        .expect("root");
    let agent = realm
        .semantics_agent(realm.presentation_id())
        .expect("agent");
    let sender = realm.command_sender();
    let frame_sender = sender.clone();
    realm
        .presentation_widgets_for_test(realm.presentation_id())
        .set_on_need_frame(move || {
            assert!(!fail_reassemble, "reassemble first");
            frame_sender.request_redraw();
        });
    sender
        .request_hot_reload(crate::reload::ReloadTier::Reassemble)
        .expect("reload accepted");
    let mut tail = agent.read(ReadQuery::new()).expect("tail accepted");
    let _ = realm.take_redraw_request();
    let before = delivered.load(Ordering::Relaxed);
    failures_left.store(failures, Ordering::Relaxed);
    let payload =
        catch_unwind(AssertUnwindSafe(|| realm.drain_commands())).expect_err("reload wake failed");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&if fail_reassemble {
            "reassemble first"
        } else {
            "reload wake first"
        })
    );
    flui_foundation::panic::retain_opaque_payload(payload);
    assert!(
        tail.try_take().is_none(),
        "the failure leaves the accepted FIFO tail queued"
    );
    assert!(
        realm.take_redraw_request(),
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
    let report = realm.drain_commands();
    assert_eq!(report.invoked, 1);
    let reply = tail
        .try_take()
        .expect("accepted read answered at the next turn");
    assert_eq!(
        reply.expect_err("no semantics frame yet").code(),
        ErrorCode::Busy
    );
    assert_eq!(
        delivered.load(Ordering::Relaxed),
        before + 1,
        "later owner opportunity pays remaining wake debt once"
    );
    assert_eq!(realm.drain_commands(), DrainReport::default());
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
