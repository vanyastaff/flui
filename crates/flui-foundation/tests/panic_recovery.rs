//! Failure provenance and payload custody through the owner recovery interface.

use flui_foundation::Notifier;
use flui_foundation::panic::{FailureClass, PanicRecovery};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn a_nested_delivery_keeps_the_first_failure_and_later_computation() {
    let notifier = Notifier::<()>::new();
    notifier.add(Rc::new(|_| panic!("first delivery")));
    let tail = Rc::new(Cell::new(false));
    let delivered = Rc::clone(&tail);
    notifier.add(Rc::new(move |_| delivered.set(true)));
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

fn opaque_receipt_drop_preserves_custody() {
    let drops = Arc::new(AtomicUsize::new(0));
    let payload_drops = Arc::clone(&drops);
    let mut recovery = PanicRecovery::new();
    recovery
        .scope()
        .run_classified(FailureClass::Delivery, |_| {
            std::panic::panic_any(HostileDrop(payload_drops));
        });
    let failure = recovery.into_failure().expect("opaque failure receipt");
    drop(failure);
    assert_eq!(
        drops.load(Ordering::Relaxed),
        0,
        "opaque payload is retained"
    );
    let mut next = PanicRecovery::new();
    assert!(next.scope().run_classified(FailureClass::Delivery, |_| {}));
    assert!(
        next.into_failure().is_none(),
        "the next operation remains usable"
    );
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
    notifier.add(Rc::new(|_| panic!("first channel failure")));
    let reenter = Rc::new(Cell::new(true));
    let nested = Rc::downgrade(&notifier);
    notifier.add_with_recovery(Rc::new(move |_, scope| {
        if reenter.replace(false) {
            nested
                .upgrade()
                .expect("notification owner")
                .notify_with_recovery(&(), scope);
        }
    }));
    let delivered = Rc::new(Cell::new(0));
    let tail = Rc::clone(&delivered);
    notifier.add(Rc::new(move |_| tail.set(tail.get() + 1)));
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
            "opaque receipt custody",
            opaque_receipt_drop_preserves_custody,
        ),
        (
            "unproved retirement",
            unproved_retirement_does_not_inherit_a_delivery_claim,
        ),
        (
            "inherited local annotation",
            an_inherited_failure_keeps_local_annotation_without_rethrowing_it,
        ),
        (
            "nested channel authority",
            a_nested_channel_latch_does_not_demote_its_owners_original_failure,
        ),
    ]);
}
