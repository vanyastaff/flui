use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

use super::{OwnerSignal, OwnerTurnSlot};

/// Reading the owner's platform first drops what slots dropped on other
/// threads left with the owner, and such a destructor may ask the signal
/// again: the read holds no lock of the signal's while it does.
fn an_orphan_dropped_while_the_owner_reads_its_platform_may_ask_the_signal_again() {
    struct AsksAgain(std::sync::Weak<OwnerSignal>);
    impl Drop for AsksAgain {
        fn drop(&mut self) {
            if let Some(signal) = self.0.upgrade() {
                let _ = signal.request_exit_policy_reevaluation();
            }
        }
    }
    let (answered, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let signal = OwnerSignal::new(Arc::new(|| Ok(())));
        let platform: std::rc::Rc<dyn crate::platform::Platform> =
            std::rc::Rc::new(crate::testing::HeadlessPlatform::new());
        signal.serve_platform(&platform);
        let orphan = crate::owner::slot::OwnerSlot::new();
        assert!(
            orphan.replace(AsksAgain(Arc::downgrade(&signal))).is_ok(),
            "the owner stores the value"
        );
        std::thread::spawn(move || drop(orphan))
            .join()
            .expect("the slot is dropped on another thread");
        let _ = answered.send(signal.request_exit_policy_reevaluation().is_ok());
    });
    assert_eq!(
        answer.recv_timeout(std::time::Duration::from_secs(10)),
        Ok(true),
        "the owner asked its platform without deadlocking on its own signal"
    );
}

fn worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn() {
    let posts = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&posts);
    let slot = std::rc::Rc::new(OwnerTurnSlot::default());
    let signal = OwnerSignal::new(Arc::new(move || {
        recorded.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&calls);
    let weak = Arc::downgrade(&signal);
    let weak_slot = std::rc::Rc::downgrade(&slot);
    signal
        .register_in(
            &slot,
            Box::new(move || {
                if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                    let signal = weak.upgrade().expect("live");
                    let slot = weak_slot.upgrade().expect("live");
                    signal.wake().expect("reentrant wake");
                    assert!(!signal.drive_in(&slot));
                }
            }),
        )
        .expect("register");
    signal.start().expect("start");
    let worker = Arc::clone(&signal);
    std::thread::spawn(move || {
        for _ in 0..100 {
            worker.wake().expect("wake");
        }
    })
    .join()
    .expect("worker");
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    assert!(!signal.drive_in(&slot));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(posts.load(Ordering::SeqCst), 2);
    assert!(!signal.drive_in(&slot));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(posts.load(Ordering::SeqCst), 2);
    signal.close_in(&slot);
}

/// A loop that reads what wakes asked of it begins the turn, reads, then
/// drives. A wake that lands while it reads posts the next turn, and the
/// loop reads again there: a change recorded during the first read is not
/// left for an unrelated later wake.
fn a_wake_while_the_loop_reads_gets_a_turn_that_reads_again() {
    let posts = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&posts);
    let slot = OwnerTurnSlot::default();
    let signal = OwnerSignal::new(Arc::new(move || {
        recorded.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    signal
        .register_in(
            &slot,
            Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .expect("register");
    signal.start().expect("start");
    // What a background source records for the loop to read, and wakes
    // the owner for.
    let changes = Arc::new(Mutex::new(Vec::<u32>::new()));
    changes.lock().push(1);
    signal.wake().expect("wake");
    assert_eq!(posts.load(Ordering::SeqCst), 1);

    let mut read = Vec::new();
    let mut turns = 0;
    // The loop: one pass per posted turn.
    while turns < posts.load(Ordering::SeqCst) {
        turns += 1;
        let turn = signal.begin_turn();
        read.extend(std::mem::take(&mut *changes.lock()));
        if turns == 1 {
            // The source records another change while the loop reads.
            changes.lock().push(2);
            signal.wake().expect("wake");
        }
        assert!(!turn.drive_in(&slot));
    }
    assert_eq!(
        read,
        [1, 2],
        "the change made during the read was never read"
    );
    assert_eq!(turns, 2);
    // The wake during the read was the first turn's to run: the second
    // turn only read.
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    signal.close_in(&slot);
}

/// A turn begun and dropped before it was driven (a read that unwinds, a
/// loop that returns early) posts the next turn for the work still owed,
/// so the application's turn never waits for an unrelated wake.
fn an_undriven_turn_posts_the_next_one() {
    let posts = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::clone(&posts);
    let slot = OwnerTurnSlot::default();
    let signal = OwnerSignal::new(Arc::new(move || {
        recorded.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    signal
        .register_in(
            &slot,
            Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .expect("register");
    signal.start().expect("start");
    signal.wake().expect("wake");
    assert_eq!(posts.load(Ordering::SeqCst), 1);

    let unwound = catch_unwind(AssertUnwindSafe(|| {
        let _turn = signal.begin_turn();
        panic!("a read panics before the turn is driven");
    }));
    assert!(unwound.is_err());
    assert_eq!(
        posts.load(Ordering::SeqCst),
        2,
        "the undriven turn posted no turn for the work it left owed"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!signal.drive_in(&slot));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the posted turn runs it");
    assert_eq!(posts.load(Ordering::SeqCst), 2);
    signal.close_in(&slot);
}

/// Work that reaches code the owner does not own runs once the turn released
/// its state, as a message taken from the queue would: a modal loop it opens
/// (a turn it drives) still runs the owner's work.
fn work_outside_the_turn_runs_after_it_released_its_state() {
    let signal = OwnerSignal::new(Arc::new(|| Ok(())));
    signal.start().expect("start");
    let nested = Arc::new(AtomicUsize::new(0));
    let (weak, seen) = (Arc::downgrade(&signal), Arc::clone(&nested));
    signal.run_outside_turn(Arc::new(move || {
        let Some(signal) = weak.upgrade() else {
            return;
        };
        let counted = Arc::clone(&seen);
        signal.run_for_window(Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        // The modal loop's own turn.
        let _quit = signal.drive_in(&OwnerTurnSlot::default());
    }));
    assert!(!signal.drive_in(&OwnerTurnSlot::default()));
    assert_eq!(
        nested.load(Ordering::SeqCst),
        1,
        "a turn nested in the work ran the owner's work"
    );
}

/// The owner signal's contracts: an orphan dropped while the owner reads its
/// platform may ask the signal again; a burst coalesces and a re-entrant wake
/// gets a later turn; a wake that lands while the loop reads gets a turn that
/// reads again; a begun turn dropped undriven posts the next one.
#[test]
fn the_owner_signal_honours_its_contracts() {
    crate::table_test::run_table(
        "owner signal",
        &[
            (
                "an orphan that asks the signal while the owner reads its platform",
                an_orphan_dropped_while_the_owner_reads_its_platform_may_ask_the_signal_again,
            ),
            (
                "worker burst and reentrant wake",
                worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn,
            ),
            (
                "a wake while the loop reads",
                a_wake_while_the_loop_reads_gets_a_turn_that_reads_again,
            ),
            ("an undriven turn", an_undriven_turn_posts_the_next_one),
            (
                "work outside the turn",
                work_outside_the_turn_runs_after_it_released_its_state,
            ),
        ],
    );
}
