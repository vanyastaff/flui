//! Failure provenance and payload custody through the owner recovery interface.

use flui_foundation::Notifier;
use flui_foundation::panic::{FailureClass, PanicRecovery};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn a_nested_delivery_keeps_the_first_failure_and_later_computation() {
    let notifier = Notifier::<()>::new();
    notifier.add(Rc::new(|()| panic!("first delivery")));
    let tail = Rc::new(Cell::new(false));
    let delivered = Rc::clone(&tail);
    notifier.add(Rc::new(move |()| delivered.set(true)));
    let mut recovery = PanicRecovery::new();
    assert!(
        recovery
            .scope()
            .run_classified(FailureClass::Delivery, |scope| {
                notifier.notify_with_recovery(&(), scope);
            })
    );
    assert!(tail.get(), "the live notification tail completes");
    assert!(
        !recovery
            .scope()
            .run_classified(FailureClass::Computation, |_| {
                panic!("later computation");
            })
    );
    assert_eq!(recovery.first_failure_class(), Some(FailureClass::Delivery));
    assert_eq!(
        recovery.blocking_failure_class(),
        Some(FailureClass::Computation)
    );
    let failure = recovery
        .into_failure()
        .expect("the owner receives its failure");
    assert_eq!(
        failure.payload().downcast_ref::<&str>(),
        Some(&"first delivery")
    );
    assert_eq!(failure.class(), FailureClass::Delivery);
    assert_eq!(failure.blocking_class(), Some(FailureClass::Computation));
    drop(failure);
}

fn computation_remains_authoritative_before_delivery() {
    let mut recovery = PanicRecovery::new();
    recovery
        .scope()
        .run_classified(FailureClass::Computation, |_| {
            panic!("first computation");
        });
    recovery
        .scope()
        .run_classified(FailureClass::Delivery, |_| {
            panic!("later delivery");
        });
    let failure = recovery.into_failure().expect("first failure");
    assert_eq!(failure.class(), FailureClass::Computation);
    assert_eq!(failure.blocking_class(), Some(FailureClass::Computation));
    assert_eq!(
        failure.payload().downcast_ref::<&str>(),
        Some(&"first computation")
    );
}

struct HostileDrop(Arc<AtomicUsize>);

impl Drop for HostileDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("opaque ownership retirement");
    }
}

/// An opaque panic payload that counts its destruction.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn unproved_retirement_does_not_inherit_a_delivery_claim() {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut recovery = PanicRecovery::new();
    recovery
        .scope()
        .run_classified(FailureClass::Delivery, |scope| {
            scope.retire(HostileDrop(Arc::clone(&drops)));
        });
    let failure = recovery.into_failure().expect("retirement failure");
    assert_eq!(failure.class(), FailureClass::Unknown);
    assert_eq!(failure.blocking_class(), Some(FailureClass::Unknown));
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn notification_capture_retirement(class: FailureClass) {
    let notifier = Rc::new(Notifier::<()>::new());
    let identity = Rc::new(Cell::new(None));
    let callback_identity = Rc::clone(&identity);
    let callback_owner = Rc::downgrade(&notifier);
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = HostileDrop(Arc::clone(&drops));
    identity.set(Some(notifier.add(Rc::new(move |()| {
        let _capture = &capture;
        callback_owner
            .upgrade()
            .expect("the notification owner remains live")
            .remove(callback_identity.take().expect("live callback identity"));
    }))));
    let delivered = Rc::new(Cell::new(0));
    let tail = Rc::clone(&delivered);
    notifier.add(Rc::new(move |()| tail.set(tail.get() + 1)));
    let mut recovery = PanicRecovery::new();
    assert!(recovery.scope().run_classified(class, |scope| {
        notifier.notify_with_recovery(&(), scope);
    }));
    assert_eq!(delivered.get(), 1, "the live notification tail delivers");
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    let receipt = recovery.into_failure().expect("capture retirement failure");
    assert_eq!(receipt.class(), class);
    assert_eq!(
        receipt.blocking_class(),
        (class != FailureClass::Delivery).then_some(class)
    );
    assert_eq!(
        receipt.payload().downcast_ref::<&str>(),
        Some(&"opaque ownership retirement")
    );
    drop(receipt);
    notifier.notify(&());
    assert_eq!(delivered.get(), 2, "the next notification remains usable");
}

fn committed_notification_retirement_preserves_delivery_proof() {
    notification_capture_retirement(FailureClass::Delivery);
}

fn computation_notification_retirement_preserves_computation_proof() {
    notification_capture_retirement(FailureClass::Computation);
}

fn unclassified_notification_retirement_stays_unknown() {
    notification_capture_retirement(FailureClass::Unknown);
}

fn an_inherited_failure_keeps_local_annotation_without_rethrowing_it() {
    let mut recovery = PanicRecovery::new();
    recovery.inherit_failure();
    assert!(
        !recovery.scope().run_classified(FailureClass::Unknown, |_| {
            panic!("local query failure");
        })
    );
    let receipt = recovery
        .into_failure()
        .expect("local query remains diagnosable");
    assert_eq!(receipt.class(), FailureClass::Unknown);
    assert_eq!(
        receipt.payload().downcast_ref::<&str>(),
        Some(&"local query failure")
    );
    drop(receipt);

    let mut inherited = PanicRecovery::new();
    inherited.inherit_failure();
    inherited
        .scope()
        .run_classified(FailureClass::Unknown, |_| {
            panic!("secondary annotation");
        });
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inherited.finish())).is_ok(),
        "finishing must preserve the enclosing failure's authority"
    );
}

fn a_nested_channel_latch_does_not_demote_its_owners_original_failure() {
    let notifier = Rc::new(Notifier::<()>::new());
    notifier.add(Rc::new(|()| panic!("first channel failure")));
    let reenter = Rc::new(Cell::new(true));
    let nested = Rc::downgrade(&notifier);
    notifier.add_with_recovery(Rc::new(move |(), scope| {
        if reenter.replace(false) {
            nested
                .upgrade()
                .expect("notification owner")
                .notify_with_recovery(&(), scope);
        }
    }));
    let delivered = Rc::new(Cell::new(0));
    let tail = Rc::clone(&delivered);
    notifier.add(Rc::new(move |()| tail.set(tail.get() + 1)));
    let mut recovery = PanicRecovery::new();
    recovery
        .scope()
        .run_classified(FailureClass::Delivery, |scope| {
            notifier.notify_with_recovery(&(), scope);
        });
    assert_eq!(
        delivered.get(),
        2,
        "the nested and outer live tails deliver"
    );
    let original = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recovery.finish()))
        .expect_err("the owner's original failure remains authoritative");
    assert_eq!(
        original.downcast_ref::<&str>(),
        Some(&"first channel failure")
    );
}

fn forwarded_receipt_authority(source_inherited: bool, receiver_inherited: bool) {
    let mut source = PanicRecovery::new();
    if source_inherited {
        source.inherit_failure();
    }
    source.scope().run_classified(FailureClass::Unknown, |_| {
        panic!("relayed captured failure");
    });
    let receipt = source.into_failure().expect("captured relay receipt");
    let mut receiver = PanicRecovery::new();
    if receiver_inherited {
        receiver.inherit_failure();
    }
    receiver.scope().capture_failure(receipt);
    assert_eq!(receiver.first_failure_class(), Some(FailureClass::Unknown));
    assert_eq!(
        receiver.blocking_failure_class(),
        Some(FailureClass::Unknown)
    );
    // A subsequent channel's failure signal cannot change earlier authority.
    receiver.inherit_failure();
    receiver
        .scope()
        .run_classified(FailureClass::Delivery, |_| {
            panic!("later callback delivery");
        });
    let finished = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| receiver.finish()));
    if source_inherited || receiver_inherited {
        assert!(
            finished.is_ok(),
            "an enclosing earlier failure stays authoritative"
        );
    } else {
        let original = finished.expect_err("the relayed original remains authoritative");
        assert_eq!(
            original.downcast_ref::<&str>(),
            Some(&"relayed captured failure")
        );
    }
}

fn an_authoritative_receipt_precedes_a_later_callback_failure() {
    forwarded_receipt_authority(false, false);
}

fn an_inherited_receipt_never_acquires_standalone_authority() {
    forwarded_receipt_authority(true, false);
}

fn a_receiver_with_an_earlier_inherited_failure_keeps_outer_authority() {
    forwarded_receipt_authority(false, true);
}

fn a_receipt_keeps_blocking_evidence_separate_from_its_first_payload() {
    let mut source = PanicRecovery::new();
    source
        .scope()
        .run_classified(FailureClass::Delivery, |_| panic!("relayed first notice"));
    source.scope().run_classified(FailureClass::Unknown, |_| {
        panic!("relayed later retirement")
    });
    let receipt = source.into_failure().expect("mixed relay receipt");
    let mut receiver = PanicRecovery::new();
    receiver.scope().capture_failure(receipt);
    receiver
        .scope()
        .run_classified(FailureClass::Computation, |_| {
            panic!("receiver later computation");
        });
    assert_eq!(receiver.first_failure_class(), Some(FailureClass::Delivery));
    assert_eq!(
        receiver.blocking_failure_class(),
        Some(FailureClass::Unknown)
    );
    let original = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| receiver.finish()))
        .expect_err("the first notice retains its original payload");
    assert_eq!(
        original.downcast_ref::<&str>(),
        Some(&"relayed first notice")
    );
}

fn a_later_opaque_receipt_keeps_the_first_cause_and_adds_its_blocker() {
    let drops = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&drops);
    let mut source = PanicRecovery::new();
    source.scope().run_classified(FailureClass::Unknown, |_| {
        std::panic::panic_any(Counted(captured));
    });
    let receipt = source.into_failure().expect("opaque relay receipt");
    let mut receiver = PanicRecovery::new();
    receiver
        .scope()
        .run_classified(FailureClass::Delivery, |_| panic!("earlier owner notice"));
    receiver.scope().capture_failure(receipt);
    let original = receiver.into_failure().expect("the earlier owner failure");
    assert_eq!(original.class(), FailureClass::Delivery);
    assert_eq!(original.blocking_class(), Some(FailureClass::Unknown));
    assert_eq!(
        original.payload().downcast_ref::<&str>(),
        Some(&"earlier owner notice")
    );
    drop(original);
    assert_eq!(
        drops.load(Ordering::Relaxed),
        1,
        "the later opaque payload is dropped, not retained"
    );
}

fn an_inherited_opaque_receipt_finishes_without_a_new_unwind() {
    let drops = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&drops);
    let mut source = PanicRecovery::new();
    source.inherit_failure();
    source.scope().run_classified(FailureClass::Unknown, |_| {
        std::panic::panic_any(Counted(captured));
    });
    let receipt = source.into_failure().expect("inherited opaque receipt");
    let mut receiver = PanicRecovery::new();
    receiver.scope().capture_failure(receipt);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| receiver.finish())).is_ok(),
        "inherited ownership cannot become a new unwind"
    );
    assert_eq!(
        drops.load(Ordering::Relaxed),
        1,
        "the inherited opaque payload is dropped, not retained"
    );
}

struct CountedRetirement(Arc<AtomicUsize>);

impl Drop for CountedRetirement {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// ADR-0281 §3: after a caught failure a retired value is destroyed, not
/// leaked, and a destructor that panics there competes without replacing
/// the first failure.
fn a_value_retired_after_a_caught_failure_is_destroyed_under_containment() {
    let healthy = Arc::new(AtomicUsize::new(0));
    let hostile = Arc::new(AtomicUsize::new(0));
    let mut recovery = PanicRecovery::new();
    recovery.run(|| panic!("first failure"));
    assert!(recovery.has_failure());
    recovery.retire(CountedRetirement(Arc::clone(&healthy)));
    assert_eq!(
        healthy.load(Ordering::Relaxed),
        1,
        "a value retired after a caught failure is dropped"
    );
    recovery.retire(HostileDrop(Arc::clone(&hostile)));
    assert_eq!(
        hostile.load(Ordering::Relaxed),
        1,
        "its destructor ran under containment"
    );
    let resumed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recovery.finish()))
        .expect_err("the first failure resumes");
    assert_eq!(
        resumed.downcast_ref::<&str>(),
        Some(&"first failure"),
        "the competing destructor panic does not replace the first failure"
    );
}

#[test]
fn recovery_preserves_provenance_and_owned_failure() {
    crate::run_table(&[
        (
            "nested delivery then computation",
            a_nested_delivery_keeps_the_first_failure_and_later_computation,
        ),
        (
            "computation then delivery",
            computation_remains_authoritative_before_delivery,
        ),
        (
            "retirement after a caught failure",
            a_value_retired_after_a_caught_failure_is_destroyed_under_containment,
        ),
        (
            "unproved retirement",
            unproved_retirement_does_not_inherit_a_delivery_claim,
        ),
        (
            "committed notification retirement",
            committed_notification_retirement_preserves_delivery_proof,
        ),
        (
            "computation notification retirement",
            computation_notification_retirement_preserves_computation_proof,
        ),
        (
            "unclassified notification retirement",
            unclassified_notification_retirement_stays_unknown,
        ),
        (
            "inherited local annotation",
            an_inherited_failure_keeps_local_annotation_without_rethrowing_it,
        ),
        (
            "nested channel authority",
            a_nested_channel_latch_does_not_demote_its_owners_original_failure,
        ),
        (
            "authoritative receipt chronology",
            an_authoritative_receipt_precedes_a_later_callback_failure,
        ),
        (
            "inherited receipt authority",
            an_inherited_receipt_never_acquires_standalone_authority,
        ),
        (
            "receiving inherited authority",
            a_receiver_with_an_earlier_inherited_failure_keeps_outer_authority,
        ),
        (
            "receipt blocking evidence",
            a_receipt_keeps_blocking_evidence_separate_from_its_first_payload,
        ),
        (
            "later opaque receipt",
            a_later_opaque_receipt_keeps_the_first_cause_and_adds_its_blocker,
        ),
        (
            "inherited opaque receipt",
            an_inherited_opaque_receipt_finishes_without_a_new_unwind,
        ),
    ]);
}
