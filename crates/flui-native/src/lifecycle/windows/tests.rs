//! The lifecycle subscriber on a real broadcast receiver, with its pool-thread
//! wait and timer stood in for.

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::{MemoryPressureLevel, SystemNotice};

use super::HostEvents;
use super::host_events::LOW_MEMORY;
use crate::lifecycle::notice::SystemNotices;
use crate::owner::app_delivery::testing::{AppLog, Bound, bound};
use crate::owner::signal::{OwnerSignal, OwnerTurnSlot};
use crate::owner::windows::broadcast::{Receiver, Subscriber};
use crate::owner::windows::control::{OwnerQueue, testing::FullQueue};

/// A receiver wired to a host, with the notices it delivers heard by a
/// recording application handler.
fn hosted_receiver() -> (Receiver, Rc<HostEvents>, Rc<AppLog>, Bound) {
    hosted_receiver_of(&OwnerSignal::new(Arc::new(|| Ok(()))))
}

/// [`hosted_receiver`] on the owner thread of `signal`, whose turns run what
/// the thread's queue refused.
fn hosted_receiver_of(signal: &Arc<OwnerSignal>) -> (Receiver, Rc<HostEvents>, Rc<AppLog>, Bound) {
    let app = AppLog::new();
    let run = bound(app.clone());
    let notices = Arc::new(SystemNotices::new(Arc::clone(&run.delivery)));
    let host = Rc::new(HostEvents::new(
        Arc::clone(&run.delivery),
        notices,
        OwnerQueue::new(Arc::downgrade(signal)),
    ));
    let receiver = Receiver::try_new([Rc::clone(&host) as Rc<dyn Subscriber>]).expect("receiver");
    (receiver, host, app, run)
}

fn low_memory_is_reported_once_per_hold() {
    let (receiver, host, heard, _run) = hosted_receiver();
    let (low, high) = host.low_memory_parameters_for_test();
    let report = || receiver.send_with_for_test(LOW_MEMORY, low, high);
    let critical = SystemNotice::MemoryPressure(MemoryPressureLevel::Critical);

    assert!(
        host.signal_low_memory_for_test(),
        "the first signal wakes the owner"
    );
    report();
    assert_eq!(heard.notices(), [critical]);

    // The event stays signaled while memory is low: more signals during the
    // hold are the same episode, whatever reaches the owner's queue.
    assert!(!host.signal_low_memory_for_test());
    report();
    assert_eq!(heard.notices().len(), 1);

    // The hold over, the wait is registered again and a new episode reports.
    host.end_low_memory_hold_for_test(receiver.hwnd_for_test());
    assert!(host.signal_low_memory_for_test());
    report();
    assert_eq!(heard.notices(), [critical, critical]);
}

fn a_report_the_queue_refuses_is_sent_from_the_owners_turn() {
    let signal = OwnerSignal::new(Arc::new(|| Ok(())));
    signal.start().expect("the owner starts its turns");
    let (_receiver, host, heard, _run) = hosted_receiver_of(&signal);
    {
        // The thread's queue holds its posted-message quota: the pool
        // thread's post of the report is refused.
        let _full = FullQueue::fill();
        host.fire_low_memory_for_test();
    }
    assert_eq!(heard.notices(), [], "no message carried the report");
    assert!(!signal.drive_in(&OwnerTurnSlot::default()));
    assert_eq!(
        heard.notices(),
        [SystemNotice::MemoryPressure(MemoryPressureLevel::Critical)],
        "the owner's turn sent the report the queue refused"
    );
}

fn a_refused_rearm_timer_is_set_again_by_the_next_message() {
    let (receiver, host, _heard, _run) = hosted_receiver();
    host.owe_rearm_timer_for_test();
    assert!(host.rearm_timer_owed_for_test());
    // Any message the receiver gets pays the debt.
    receiver.send_with_for_test(windows::Win32::UI::WindowsAndMessaging::WM_NULL, 0, 0);
    assert!(!host.rearm_timer_owed_for_test());
}

#[test]
fn host_event_sources() {
    crate::table_test::run_table(
        "host_event_sources",
        &[
            (
                "low_memory_is_reported_once_per_hold",
                low_memory_is_reported_once_per_hold as fn(),
            ),
            (
                "a_report_the_queue_refuses_is_sent_from_the_owners_turn",
                a_report_the_queue_refuses_is_sent_from_the_owners_turn as fn(),
            ),
            (
                "a_refused_rearm_timer_is_set_again_by_the_next_message",
                a_refused_rearm_timer_is_set_again_by_the_next_message as fn(),
            ),
        ],
    );
}
