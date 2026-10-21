//! How the display a window reports is merged while its delivery is pending
//! and applied to the presentation (ADR-0236).

use std::cell::Cell;
use std::rc::Rc;

use flui_platform::{DisplayKey, DisplayRegistry, RefreshPeriod, WindowDisplay};
use flui_runtime::owner::{DisplayUpdate, WindowObservation};

use crate::HostInputExt as _;

fn display(registry: &DisplayRegistry, key: u64, period_ms: Option<u64>) -> WindowDisplay {
    WindowDisplay::new(
        registry.mint(&DisplayKey::Injected(key)).expect("id"),
        period_ms.and_then(|ms| RefreshPeriod::try_from(std::time::Duration::from_millis(ms)).ok()),
    )
}

/// Admit `updates` while a native callback has spent the delivery budget, so
/// they are all pending when the owner turn runs, then return what the
/// presentation holds afterwards.
fn applied_after_pending(updates: &[Option<WindowDisplay>]) -> Option<WindowDisplay> {
    let (owner, address, effects) = crate::owner_input::fixture(true);
    let frame = owner.frame_dispatcher(address).expect("frame");
    {
        let _callback = owner.begin_callback(&effects).expect("native callback");
        for _ in 0..32 {
            frame
                .deliver(crate::VSYNC, &effects)
                .expect("consume callback budget");
        }
        for update in updates {
            effects
                .input
                .observe(
                    WindowObservation::Display(DisplayUpdate::new(*update)),
                    &effects,
                )
                .expect("admit a pending display update");
        }
    }
    owner.continue_work(&effects);
    let seen = Rc::new(Cell::new(None));
    let observed = Rc::clone(&seen);
    effects
        .input
        .test_callback(
            Box::new(move |runtime| {
                observed.set(Some(runtime.window_display(address.presentation_id)));
            }),
            &effects,
        )
        .expect("read the presentation");
    owner.shutdown(&effects);
    seen.get().expect("the presentation was read")
}

fn several_changes_before_the_turn_deliver_the_last() {
    let registry = DisplayRegistry::new();
    let (a, b, c) = (
        display(&registry, 1, Some(16)),
        display(&registry, 2, Some(8)),
        display(&registry, 3, Some(7)),
    );
    assert_eq!(
        applied_after_pending(&[Some(a), Some(b), Some(c)]),
        Some(c),
        "an older pending display replaced the newest"
    );
}

fn newer_unknown_display_replaces_a_pending_known_one() {
    let registry = DisplayRegistry::new();
    let known = display(&registry, 1, Some(8));
    assert_eq!(
        applied_after_pending(&[Some(known), None]),
        None,
        "a window that moved to an unknown display kept the older known period"
    );
    // The same holds when the known display arrives later: newest wins both ways.
    assert_eq!(applied_after_pending(&[None, Some(known)]), Some(known));
}

fn a_display_without_a_period_is_applied_as_known_display_with_unknown_period() {
    let registry = DisplayRegistry::new();
    let no_period = display(&registry, 1, None);
    assert_eq!(
        applied_after_pending(&[Some(no_period)]),
        Some(no_period),
        "a known display with no refresh period is not an unknown display"
    );
}

#[test]
fn window_display_merge_contract() {
    crate::table_test::run_table(
        "window_display_merge_contract",
        &[
            (
                "several_changes_before_the_turn_deliver_the_last",
                several_changes_before_the_turn_deliver_the_last as fn(),
            ),
            (
                "newer_unknown_display_replaces_a_pending_known_one",
                newer_unknown_display_replaces_a_pending_known_one as fn(),
            ),
            (
                "a_display_without_a_period_is_applied_as_known_display_with_unknown_period",
                a_display_without_a_period_is_applied_as_known_display_with_unknown_period
                    as fn(),
            ),
        ],
    );
}
