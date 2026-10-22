use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::WindowInstallation;
use crate::PlatformProxy;
use crate::error::PlatformError;
use crate::owner::signal::{OwnerSignal, SignalTransport};

/// A running owner on this thread whose next wake fails once `fail` is set,
/// and the proxy that wakes it.
fn owner(fail: &Arc<AtomicBool>) -> (Arc<OwnerSignal>, PlatformProxy) {
    let failure = Arc::clone(fail);
    let signal = OwnerSignal::new(Arc::new(move || {
        if failure.swap(false, Ordering::AcqRel) {
            Err(PlatformError::EventLoop {
                message: "injected owner wake failure".into(),
            })
        } else {
            Ok(())
        }
    }));
    signal.start().expect("the owner starts");
    let proxy = PlatformProxy::new(Arc::new(SignalTransport::new(&signal)));
    (signal, proxy)
}

fn acknowledgement_and_cancellation_are_committed_before_notification() {
    let (_signal, proxy) = owner(&Arc::default());
    let (ready, pending) = WindowInstallation::channel(proxy.clone());
    assert!(pending.outcome().is_none());
    ready.complete().expect("notify readiness");
    assert_eq!(pending.outcome(), Some(Ok(())));
    assert_eq!(
        pending.outcome(),
        Some(Ok(())),
        "reading cannot consume completion"
    );

    let (cancelled, pending) = WindowInstallation::channel(proxy.clone());
    drop(cancelled);
    assert!(pending.outcome().expect("cancellation committed").is_err());

    let (late, retired) = WindowInstallation::channel(proxy);
    drop(retired);
    late.complete()
        .expect("retired attachment ignores late acknowledgement");
}

#[test]
fn window_installation_contract() {
    crate::table_test::run_table(
        "window_installation_contract",
        &[
            (
                "acknowledgement_and_cancellation_are_committed_before_notification",
                acknowledgement_and_cancellation_are_committed_before_notification as fn(),
            ),
            (
                "failed_wake_does_not_undo_installation_acknowledgement",
                failed_wake_does_not_undo_installation_acknowledgement as fn(),
            ),
        ],
    );
}

fn failed_wake_does_not_undo_installation_acknowledgement() {
    let fail = Arc::new(AtomicBool::new(false));
    let (signal, proxy) = owner(&fail);
    signal.drive();
    let (ready, pending) = WindowInstallation::channel(proxy.clone());
    fail.store(true, Ordering::Release);
    assert!(
        ready.complete().is_err(),
        "native posting failure reaches caller"
    );
    assert_eq!(
        pending.outcome(),
        Some(Ok(())),
        "accepted completion survives a failed notification"
    );
    proxy.wake().expect("later owner opportunity");
    signal.drive();
    assert_eq!(
        pending.outcome(),
        Some(Ok(())),
        "retry cannot convert readiness to cancellation"
    );
}
