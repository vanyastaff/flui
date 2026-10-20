//! Behavior values: platform presets, their fallback chain, the resolution order
//! over host observations and subtree overrides, and accessibility floors
//! (ADR-0212).

use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use flui_interaction::{
    GestureArena, GestureArenaMember as _, GestureRecognizer, GestureSettings,
    GestureSettingsProvider, GestureSettingsSource, LongPressGestureRecognizer, ManualClock,
    PointerId,
    events::{PointerEventExt as _, PointerKind},
    gesture::RemainderHandoff,
    pointer::PointerDispatch,
};
use flui_platform_api::{GesturePreferences, TargetPlatform, pointer::PenTool};

const PEN: PointerKind = PointerKind::Pen { tool: PenTool::Tip };

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// A kind with its own row, a kind falling to its class row, and a platform
/// with neither. Expected numbers are the cited sources, not the code's rows.
fn preset_fallback_chain() {
    // Android's own pen row: Views apply ViewConfiguration.TOUCH_SLOP (8 dp)
    // to every contact, stylus included, although a pen is a fine pointer.
    let pen = GestureSettings::preset(TargetPlatform::Android, PEN);
    assert_eq!(pen.touch_slop(), 8.0, "Android pen keeps TOUCH_SLOP");
    // Android mouse has no kind row: it falls to the fine class row, whose
    // timings stay AOSP's while distances are the precise-pointer tier.
    let mouse = GestureSettings::preset(TargetPlatform::Android, PointerKind::Mouse);
    assert_eq!(mouse.touch_slop(), 1.0, "fine class row");
    assert_eq!(mouse.long_press_timeout(), ms(400), "AOSP long press");
    // Touch is coarse: the coarse class row carries AOSP's touch numbers.
    let touch = GestureSettings::preset(TargetPlatform::Android, PointerKind::Touch);
    assert_eq!(
        (touch.touch_slop(), touch.pan_slop(), touch.min_target()),
        (8.0, 16.0, 48.0)
    );
    assert_eq!(touch.double_tap_timeout(), ms(300));
    // Fuchsia publishes neither kind nor class rows: every kind reaches the
    // platform row, Android's numbers.
    for kind in [PointerKind::Mouse, PointerKind::Touch, PEN] {
        let fallback = GestureSettings::preset(TargetPlatform::Fuchsia, kind);
        assert_eq!(
            fallback.touch_slop(),
            8.0,
            "{kind:?} reaches the platform row"
        );
    }
    // iOS touch: UIKit's documented allowableMovement (10 pt) and
    // minimumPressDuration (0.5 s); HIG's 44 pt target.
    let ios = GestureSettings::preset(TargetPlatform::iOS, PointerKind::Touch);
    assert_eq!(
        (ios.touch_slop(), ios.long_press_timeout(), ios.min_target()),
        (10.0, ms(500), 44.0)
    );
    // Windows mouse: GetDoubleClickTime's documented 500 ms default.
    let windows = GestureSettings::preset(TargetPlatform::Windows, PointerKind::Mouse);
    assert_eq!(windows.double_tap_timeout(), ms(500));
    // Every preset row carries the remaining plain values.
    assert_eq!(touch.remainder_handoff(), RemainderHandoff::NextSequence);
    assert_eq!(touch.haptic_frame_deadline_factor().get(), 2);
    assert!(
        touch.max_resolution_delay() >= touch.long_press_timeout() + touch.double_tap_timeout()
    );
}

/// An authored value below, at and above the OS floor. Shorter values are
/// raised, never refused; longer ones are kept.
fn accessibility_timing_floors_hold_under_overrides() {
    let preferences = GesturePreferences::default()
        .with_long_press_timeout(ms(900))
        .with_double_tap_interval(ms(700))
        .with_double_click_interval(ms(800));
    let host = GestureSettings::resolve_preferences(
        &GestureSettings::preset(TargetPlatform::Android, PointerKind::Touch),
        &preferences,
        None,
    )
    .expect("timing only");
    for (authored, expected) in [(200, 900), (900, 900), (1200, 1200)] {
        let overridden = host.clone().with_long_press_timeout(ms(authored));
        assert_eq!(
            overridden.long_press_timeout(),
            ms(expected),
            "long press authored {authored}ms"
        );
    }
    let shorter = host.clone().with_double_tap_timeout(ms(100));
    assert_eq!(shorter.double_tap_timeout_for(PointerKind::Touch), ms(700));
    assert_eq!(shorter.double_tap_timeout_for(PointerKind::Mouse), ms(800));
    let longer = host.clone().with_double_tap_timeout(ms(1000));
    assert_eq!(longer.double_tap_timeout_for(PointerKind::Touch), ms(1000));

    // A subtree override authored without the host still meets its floor
    // when it is layered over the live presentation provider.
    let source = GestureSettingsSource::new(host);
    let authored = GestureSettings::preset(TargetPlatform::Android, PointerKind::Touch)
        .with_long_press_timeout(ms(100));
    let scoped = source.provider().overridden_by(authored.clone());
    assert_eq!(
        scoped.snapshot_for(PointerKind::Touch).long_press_timeout(),
        ms(900)
    );
    // The floor follows the host: an accepted observation that removes it
    // lets the same override's authored value through for new admissions.
    source.replace(GestureSettings::preset(
        TargetPlatform::Android,
        PointerKind::Touch,
    ));
    assert_eq!(
        scoped.snapshot_for(PointerKind::Touch).long_press_timeout(),
        ms(100)
    );
    // An override not layered over a host has no floor to meet.
    let fixed: GestureSettingsProvider = authored.into();
    assert_eq!(
        fixed.snapshot_for(PointerKind::Touch).long_press_timeout(),
        ms(100)
    );

    // The recognizer arms the floored deadline, not the authored one.
    let clock = Arc::new(ManualClock::new());
    let arena = GestureArena::binding_driven(clock.clone());
    let floored = GestureSettings::resolve_preferences(
        &GestureSettings::preset(TargetPlatform::Android, PointerKind::Touch),
        &GesturePreferences::default().with_long_press_timeout(ms(900)),
        None,
    )
    .expect("timing only")
    .with_long_press_timeout(ms(100));
    let fired = Rc::new(Cell::new(false));
    let output = fired.clone();
    let press = LongPressGestureRecognizer::builder(arena.clone())
        .settings(floored)
        .on_long_press(move || output.set(true))
        .build();
    let down = flui_interaction::testing::input::pointer_down(
        flui_foundation::geometry::Offset::new(0.0, 0.0),
        PointerKind::Touch,
    )
    .expect("down");
    press.add_pointer(PointerDispatch::at_root(&down));
    let pointer: PointerId = down.pointer_id().expect("contact event");
    arena.close(pointer);
    arena.drain_deferred_resolutions();
    clock.advance(ms(500));
    press.poll_deadline(super::at(&clock));
    assert!(!fired.get(), "authored 100ms is raised to the 900ms floor");
    clock.advance(ms(401));
    press.poll_deadline(super::at(&clock));
    assert!(fired.get());
    press.cancel();
}

/// OS observation over the preset; an explicit override over both.
fn behavior_values_resolve_os_then_preset_then_override() {
    let preset = GestureSettings::preset(TargetPlatform::Windows, PointerKind::Mouse);
    let unobserved =
        GestureSettings::resolve_preferences(&preset, &GesturePreferences::default(), None)
            .expect("no observation");
    assert_eq!(
        unobserved.double_tap_timeout_for(PointerKind::Mouse),
        ms(500),
        "preset when unobserved"
    );
    let observed = GestureSettings::resolve_preferences(
        &preset,
        &GesturePreferences::default().with_double_click_interval(ms(350)),
        None,
    )
    .expect("timing");
    assert_eq!(
        observed.double_tap_timeout_for(PointerKind::Mouse),
        ms(350),
        "an OS observation replaces the preset, even when shorter"
    );
    let overridden = observed.with_double_tap_timeout(ms(600));
    assert_eq!(
        overridden.double_tap_timeout_for(PointerKind::Mouse),
        ms(600),
        "an explicit override replaces both"
    );
}

#[test]
fn gesture_presets_contract() {
    let cases: &[(&str, fn())] = &[
        ("preset_fallback_chain", preset_fallback_chain),
        (
            "accessibility_timing_floors_hold_under_overrides",
            accessibility_timing_floors_hold_under_overrides,
        ),
        (
            "behavior_values_resolve_os_then_preset_then_override",
            behavior_values_resolve_os_then_preset_then_override,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            eprintln!("gesture preset case {name} failed");
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "gesture preset cases failed: {failures:?}"
    );
}
