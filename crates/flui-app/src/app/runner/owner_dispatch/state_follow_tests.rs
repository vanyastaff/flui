//! How a window's snapshots become its presentation's events
//! (`RuntimeEvent::follow`, `RuntimeEvent::deliver_each`): each fact brings its
//! own event and nothing else, a snapshot without drawable metrics keeps the
//! last ones, a change before the seed brings nothing, and a panicking
//! delivery keeps no later fact from its presentation. Declared in
//! `owner_dispatch.rs` through `#[path]` because the events are private to the
//! runner.

use flui_foundation::geometry::{DeviceSize, EdgeInsets, Size, WindowMetrics};
use flui_platform::{
    WindowAppearance, WindowExecutionState, WindowGeometry, WindowMode, WindowState,
};

use super::RuntimeEvent;

/// An event as the table names it.
fn named(event: &RuntimeEvent) -> String {
    match event {
        RuntimeEvent::Resized(metrics) => format!("resized {:?}", metrics.physical()),
        RuntimeEvent::WindowFocus(focused) => format!("focus {focused}"),
        RuntimeEvent::WindowExecution(execution) => format!("execution {execution:?}"),
        RuntimeEvent::SafeAreaChanged(insets) => format!("safe area {}", insets.top),
        RuntimeEvent::WindowVisibility(seen) => format!("seen {seen}"),
        RuntimeEvent::AppearanceChanged(brightness) => format!("brightness {brightness:?}"),
        RuntimeEvent::DisplayChanged(_) => "display".to_owned(),
        RuntimeEvent::WindowStateChanged => "state".to_owned(),
        RuntimeEvent::WindowHover(inside) => format!("hover {inside}"),
        RuntimeEvent::PrimarySurfaceRestored => "surface restored".to_owned(),
    }
}

fn metrics(width: i32, scale: f64) -> WindowMetrics {
    WindowMetrics::try_new(DeviceSize::new(width, 600), scale).expect("a drawable area")
}

/// A shown, focused, running window at 800×600 and scale 1.
fn shown() -> WindowState {
    let size = Size::new(800.0, 600.0);
    WindowState::new(WindowGeometry::new(None, size), size)
        .with_visible(true)
        .with_focused(true)
        .with_metrics(Some(metrics(800, 1.0)))
}

/// The events each snapshot of `snapshots` brings, after the first as seed.
fn followed(snapshots: Vec<WindowState>) -> Vec<Vec<String>> {
    let mut seen = None;
    snapshots
        .into_iter()
        .map(|next| {
            RuntimeEvent::follow(&mut seen, next)
                .events
                .iter()
                .map(named)
                .collect()
        })
        .collect()
}

fn each_fact_brings_exactly_its_event() {
    let rows: Vec<(&str, WindowState, &[&str])> = vec![
        ("nothing", shown(), &["state"]),
        (
            "metrics",
            shown().with_metrics(Some(metrics(1000, 1.0))),
            &["state", "resized Size { width: 1000, height: 600 }"],
        ),
        (
            "safe area",
            shown().with_safe_area(EdgeInsets::new(24.0, 0.0, 0.0, 0.0)),
            &["state", "safe area 24"],
        ),
        (
            "appearance",
            shown().with_appearance(WindowAppearance::Dark),
            &["state", "brightness Dark"],
        ),
        (
            "execution",
            shown().with_execution(WindowExecutionState::Suspended),
            &["state", "execution Suspended"],
        ),
        (
            "focus",
            shown().with_focused(false),
            &["state", "focus false"],
        ),
        (
            "visibility",
            shown().with_visible(false),
            &["state", "seen false"],
        ),
        (
            "minimize",
            shown().with_mode(WindowMode::Minimized),
            &["state", "seen false"],
        ),
        (
            "hover",
            shown().with_hovered(true),
            &["state", "hover true"],
        ),
        ("activation alone", shown().with_active(false), &["state"]),
    ];
    let mut failures = Vec::new();
    for (name, next, expected) in rows {
        let heard = followed(vec![shown(), next]);
        if heard
            != [
                vec![],
                expected.iter().map(|&e| e.to_owned()).collect::<Vec<_>>(),
            ]
        {
            failures.push(format!("{name}: {heard:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A snapshot without drawable metrics keeps the last ones, so the same
/// metrics back again are no resize.
fn the_same_metrics_after_none_are_no_resize() {
    let heard = followed(vec![shown(), shown().with_metrics(None), shown()]);
    assert_eq!(heard, [vec![], vec!["state"], vec!["state"]]);
}

/// A minimized window is not seen, and its restore is seen again.
fn a_minimize_and_restore_are_seen_and_unseen() {
    let heard = followed(vec![
        shown(),
        shown().with_mode(WindowMode::Minimized),
        shown(),
    ]);
    assert_eq!(
        heard,
        [
            vec![],
            vec!["state", "seen false"],
            vec!["state", "seen true"]
        ]
    );
}

/// A change heard between the subscription and the seed's read brings
/// nothing: the seed, read after it, carries it; what follows the seed is
/// compared against the seed.
fn a_change_before_the_seed_brings_nothing() {
    let mut seen = None;
    let early = RuntimeEvent::follow(&mut seen, shown().with_focused(false));
    assert!(early.events.is_empty(), "a change before the seed");
    // The seed replaces whatever the early change left.
    seen = Some(shown().with_focused(false));
    let after = RuntimeEvent::follow(&mut seen, shown());
    assert_eq!(
        after.events.iter().map(named).collect::<Vec<_>>(),
        ["state", "focus true"]
    );
}

/// A move to another display, or a new scale on the same one, asks for a new
/// display sample; a resize at the same scale does not.
fn a_display_move_or_rescale_asks_for_a_sample() {
    let mut seen = Some(shown());
    assert!(
        !RuntimeEvent::follow(&mut seen, shown().with_metrics(Some(metrics(900, 1.0))))
            .display_moved
    );
    assert!(
        RuntimeEvent::follow(&mut seen, shown().with_metrics(Some(metrics(900, 2.0))))
            .display_moved
    );
}

/// A panic delivering one event keeps none after it from its presentation,
/// and the first panic resumes once every event was delivered.
fn a_panicking_delivery_keeps_the_later_facts() {
    let mut seen = Some(shown());
    let events = RuntimeEvent::follow(
        &mut seen,
        shown()
            .with_metrics(Some(metrics(1000, 1.0)))
            .with_focused(false)
            .with_visible(false),
    )
    .events;
    let delivered = std::cell::RefCell::new(Vec::new());
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        RuntimeEvent::deliver_each(events, |event| {
            let name = named(&event);
            let state = matches!(event, RuntimeEvent::WindowStateChanged);
            delivered.borrow_mut().push(name);
            assert!(!state, "a subscriber panics");
            assert!(
                !matches!(event, RuntimeEvent::WindowFocus(_)),
                "a second panic"
            );
        });
    }))
    .expect_err("the first panic resumes");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("a subscriber panics")
    );
    assert_eq!(
        *delivered.borrow(),
        [
            "state",
            "resized Size { width: 1000, height: 600 }",
            "focus false",
            "seen false"
        ]
    );
}

#[test]
fn window_state_follow_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "each fact brings exactly its event",
            each_fact_brings_exactly_its_event,
        ),
        (
            "the same metrics after none are no resize",
            the_same_metrics_after_none_are_no_resize,
        ),
        (
            "a minimize and restore are unseen and seen",
            a_minimize_and_restore_are_seen_and_unseen,
        ),
        (
            "a change before the seed brings nothing",
            a_change_before_the_seed_brings_nothing,
        ),
        (
            "a display move or rescale asks for a sample",
            a_display_move_or_rescale_asks_for_a_sample,
        ),
        (
            "a panicking delivery keeps the later facts",
            a_panicking_delivery_keeps_the_later_facts,
        ),
    ];
    crate::table_test::run_table("window_state_follow_contract", cases);
}
