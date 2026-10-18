//! How system notices queued behind a busy owner are merged before they are
//! delivered (ADR-0237).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_foundation::memory::{MemoryPressureLevel, MemoryTrim};
use flui_platform_api::HostNotice;
use flui_runtime::owner::RuntimeOperation;
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

struct Trims(Arc<Mutex<Vec<MemoryPressureLevel>>>, Arc<AtomicUsize>);

impl MemoryTrim for Trims {
    fn trim(&self, level: MemoryPressureLevel) {
        self.0.lock().expect("log").push(level);
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

/// The trims the host's receiver was asked for after `notices` were all
/// pending when the owner turn ran.
fn trims_after_pending(notices: &[HostNotice]) -> Vec<MemoryPressureLevel> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        )
        .with_memory_trim(Arc::new(Trims(Arc::clone(&log), Arc::default()))),
    )
    .expect("runtime");
    let (owner, address, effects) = crate::owner_input::fixture_over(runtime, false);
    let frame = owner.frame_dispatcher(address).expect("frame");
    let dispatcher = owner
        .runtime_dispatcher(address.ui_runtime_id)
        .expect("runtime authority");
    {
        // A native callback has spent the delivery budget, so every notice is
        // pending when the owner turn runs.
        let _callback = owner.begin_callback(&effects).expect("native callback");
        for _ in 0..32 {
            frame.deliver(&effects).expect("consume callback budget");
        }
        for notice in notices {
            dispatcher
                .deliver(RuntimeOperation::HostNotice(notice.clone()), &effects)
                .expect("admit a pending notice");
        }
    }
    owner.continue_work(&effects);
    owner.shutdown(&effects);
    log.lock().expect("log").clone()
}

fn pressure(level: MemoryPressureLevel) -> HostNotice {
    HostNotice::MemoryPressure(level)
}

fn the_same_level_queued_twice_is_trimmed_once() {
    use MemoryPressureLevel::{Critical, Moderate};
    assert_eq!(
        trims_after_pending(&[pressure(Critical), pressure(Critical), pressure(Critical)]),
        [Critical]
    );
    assert_eq!(
        trims_after_pending(&[pressure(Moderate), pressure(Moderate)]),
        [Moderate]
    );
}

fn different_levels_are_each_delivered_in_order() {
    use MemoryPressureLevel::{Critical, Moderate};
    assert_eq!(
        trims_after_pending(&[pressure(Moderate), pressure(Critical)]),
        [Moderate, Critical],
        "a stronger notice does not swallow the weaker one queued before it"
    );
}

fn another_notice_between_two_pressures_keeps_both() {
    use MemoryPressureLevel::Critical;
    assert_eq!(
        trims_after_pending(&[pressure(Critical), HostNotice::Wake, pressure(Critical)]),
        [Critical, Critical],
        "only a repeat directly behind its twin folds into it"
    );
}

#[test]
fn host_notice_coalescing() {
    crate::table_test::run_table(
        "host_notice_coalescing",
        &[
            (
                "the_same_level_queued_twice_is_trimmed_once",
                the_same_level_queued_twice_is_trimmed_once as fn(),
            ),
            (
                "different_levels_are_each_delivered_in_order",
                different_levels_are_each_delivered_in_order as fn(),
            ),
            (
                "another_notice_between_two_pressures_keeps_both",
                another_notice_between_two_pressures_keeps_both as fn(),
            ),
        ],
    );
}
