//! FLUI's pointer and keyboard vocabulary refuses what a platform must not publish
//! (non-finite geometry, out-of-range sensors) with a typed error, keeps "no sensor" apart
//! from a reading, and keeps a scroll's unit.

use core::f64::consts::{FRAC_PI_2, PI, TAU};
use core::time::Duration;

use flui_foundation::geometry::{Offset, Point, Size};
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, NamedKey};
use flui_platform_api::pointer::{
    ContactSize, InputValueError, PanZoomTransform, PenOrientation, PointerButton, PointerButtons,
    PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, Pressure, Quantity, ScrollDelta, ScrollEvent,
    ScrollPhase, ScrollUnit, TangentialPressure, Twist,
};

use crate::run_table;

const T0: EventTime = EventTime::from_nanos(1_000);

fn mouse() -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(1_u64).expect("non-zero"),
        PointerKind::Mouse,
    )
    .with_role(PointerRole::Primary)
}

fn position(x: f64, y: f64) -> PointerPosition {
    PointerPosition::try_new(Point::new(x, y)).expect("a finite position")
}

fn sampled_at(nanos: u64) -> PointerSample {
    PointerSample::new(EventTime::from_nanos(nanos), position(1.0, 2.0))
}

fn non_finite(quantity: Quantity) -> Result<(), InputValueError> {
    Err(InputValueError::NonFinite { quantity })
}

fn a_non_finite_position_is_refused() {
    for (x, y) in [
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 1.0),
    ] {
        assert_eq!(
            PointerPosition::try_new(Point::new(x, y)).map(drop),
            non_finite(Quantity::Position),
            "({x}, {y})"
        );
    }
}

fn a_device_without_a_pressure_sensor_reads_none_not_a_default() {
    let mouse_reading = PointerSample::new(T0, position(1.0, 2.0));
    assert_eq!(mouse_reading.pressure, None);
    let zero = Pressure::try_new(0.0).expect("in range");
    assert_eq!(
        mouse_reading
            .with_pressure(zero)
            .pressure
            .map(Pressure::get),
        Some(0.0)
    );
}

fn a_non_finite_sensor_reading_is_refused() {
    assert_eq!(
        Pressure::try_new(f32::NAN).map(drop),
        non_finite(Quantity::Pressure)
    );
    assert_eq!(
        Pressure::saturating(f32::INFINITY).map(drop),
        non_finite(Quantity::Pressure)
    );
    assert_eq!(
        TangentialPressure::saturating(f32::NAN).map(drop),
        non_finite(Quantity::TangentialPressure)
    );
    assert_eq!(
        Twist::try_new(f64::NAN).map(drop),
        non_finite(Quantity::Twist)
    );
    assert_eq!(
        PenOrientation::try_new(0.5, f64::INFINITY).map(drop),
        non_finite(Quantity::Azimuth)
    );
}

fn an_out_of_range_reading_is_refused_or_saturated() {
    assert!(matches!(
        Pressure::try_new(1.5),
        Err(InputValueError::OutOfRange {
            quantity: Quantity::Pressure,
            ..
        })
    ));
    assert_eq!(Pressure::saturating(1.5).map(Pressure::get), Ok(1.0));
    assert_eq!(Pressure::saturating(-0.5).map(Pressure::get), Ok(0.0));
    assert!(TangentialPressure::try_new(-3.0).is_err());
    assert_eq!(
        TangentialPressure::saturating(-3.0).map(TangentialPressure::get),
        Ok(-1.0)
    );
    assert!(matches!(
        PenOrientation::try_new(2.0 * PI, 0.0),
        Err(InputValueError::OutOfRange {
            quantity: Quantity::Altitude,
            ..
        })
    ));
}

fn periodic_angles_are_wrapped() {
    let orientation = PenOrientation::try_new(FRAC_PI_2, -FRAC_PI_2).expect("valid");
    assert!((orientation.azimuth() - 3.0 * FRAC_PI_2).abs() < 1e-12);
    let twist = Twist::try_new(-1e-300).expect("finite").radians();
    assert!((0.0..TAU).contains(&twist), "{twist}");
}

fn a_negative_or_non_finite_contact_size_is_refused() {
    assert!(ContactSize::try_new(Size::new(-1.0, 2.0)).is_err());
    assert_eq!(
        ContactSize::try_new(Size::new(f64::NAN, 2.0)).map(drop),
        non_finite(Quantity::ContactSize)
    );
    assert_eq!(
        ContactSize::try_from(Size::new(12.0, 8.0)).map(ContactSize::get),
        Ok(Size::new(12.0, 8.0))
    );
}

fn a_button_event_holds_the_button_after_a_press_and_not_after_a_release() {
    let held = PointerButtons::only(PointerButton::PRIMARY);
    let sample = PointerSample::new(T0, position(0.0, 0.0));
    let press = PointerPress::new(mouse(), PointerButton::SECONDARY, held, sample);
    assert_eq!(press.buttons(), held.with(PointerButton::SECONDARY));
    let release = PointerRelease::new(mouse(), PointerButton::PRIMARY, held, sample);
    assert!(release.buttons().is_empty());
}

fn button_numbers_skip_the_eraser_slot() {
    for refused in [0, 6, 33] {
        assert!(PointerButton::try_from(refused).is_err(), "{refused}");
    }
    assert_eq!(
        PointerButton::try_from(7).map(|button| button.number().get()),
        Ok(7)
    );
    assert_eq!(PointerButton::try_from(5), Ok(PointerButton::FORWARD));
}

fn coalesced_and_predicted_readings_are_ordered_around_the_current_one() {
    let moved = PointerMove::new(mouse(), PointerButtons::NONE, sampled_at(50))
        .with_coalesced(vec![sampled_at(40), sampled_at(10), sampled_at(60)])
        .with_predicted(vec![sampled_at(70), sampled_at(20), sampled_at(55)]);
    let times = |samples: &[PointerSample]| {
        samples
            .iter()
            .map(|sample| sample.time.as_nanos())
            .collect::<Vec<_>>()
    };
    assert_eq!(times(moved.coalesced()), [10, 40]);
    assert_eq!(times(moved.predicted()), [55, 70]);

    // An exact copy of `current` in the history is dropped; a distinct reading at the same
    // time is kept.
    let current = sampled_at(50);
    let elsewhere = PointerSample::new(EventTime::from_nanos(50), position(9.0, 9.0));
    let moved = PointerMove::new(mouse(), PointerButtons::NONE, current)
        .with_coalesced(vec![current, elsewhere]);
    assert_eq!(moved.coalesced(), [elsewhere].as_slice());
}

fn a_scroll_keeps_its_unit() {
    for unit in [ScrollUnit::Lines, ScrollUnit::Pages, ScrollUnit::Pixels] {
        let delta = ScrollDelta::try_new(unit, 4.5, -2.25).expect("finite");
        let scroll = ScrollEvent::new(mouse(), T0, position(5.0, 5.0), delta);
        assert_eq!(
            (scroll.delta.unit(), scroll.delta.x(), scroll.delta.y()),
            (unit, 4.5, -2.25)
        );
    }
}

fn a_non_finite_scroll_delta_is_refused_and_zero_keeps_the_phase() {
    assert_eq!(
        ScrollDelta::try_new(ScrollUnit::Pixels, f64::NAN, 0.0).map(drop),
        non_finite(Quantity::ScrollDelta)
    );
    let ended = ScrollEvent::new(
        mouse(),
        T0,
        position(5.0, 5.0),
        ScrollDelta::zero(ScrollUnit::Pixels),
    )
    .with_phase(ScrollPhase::Ended);
    assert_eq!(ended.phase, Some(ScrollPhase::Ended));
}

fn a_pan_zoom_transform_is_finite_with_a_positive_scale() {
    let zero = Offset::new(0.0, 0.0);
    for scale in [0.0, -2.0] {
        assert!(matches!(
            PanZoomTransform::try_new(zero, scale, 0.0),
            Err(InputValueError::OutOfRange {
                quantity: Quantity::Scale,
                ..
            })
        ));
    }
    assert_eq!(
        PanZoomTransform::try_new(zero, f64::NAN, 0.0).map(drop),
        non_finite(Quantity::Scale)
    );
    assert_eq!(
        PanZoomTransform::try_new(zero, 1.0, f64::INFINITY).map(drop),
        non_finite(Quantity::Rotation)
    );
    assert_eq!(
        PanZoomTransform::try_new(Offset::new(f64::NAN, 0.0), 1.0, 0.0).map(drop),
        non_finite(Quantity::Pan)
    );
    let transform = PanZoomTransform::try_new(Offset::new(3.0, 4.0), 1.5, 0.25).expect("valid");
    assert_eq!(
        (transform.pan(), transform.scale(), transform.rotation()),
        (Offset::new(3.0, 4.0), 1.5, 0.25)
    );
    assert_eq!(PanZoomTransform::default(), PanZoomTransform::IDENTITY);
    // Every finite positive scale is valid, a subnormal one included.
    let tiny = f64::MIN_POSITIVE / 4.0;
    assert!(PanZoomTransform::try_new(zero, tiny, 0.0).is_ok());
}

fn a_key_event_is_a_repeat_only_while_down() {
    let up = KeyEvent::new(KeyState::Up, Key::Named(NamedKey::Enter), Code::Enter, T0)
        .with_repeat(KeyRepeat::AutoRepeat);
    assert_eq!(up.repeat(), KeyRepeat::First);
    let down = KeyEvent::new(KeyState::Down, Key::character("a"), Code::KeyA, T0)
        .with_repeat(KeyRepeat::AutoRepeat);
    assert_eq!(down.repeat(), KeyRepeat::AutoRepeat);
}

fn an_empty_character_is_an_unidentified_key() {
    assert_eq!(Key::character(""), Key::Named(NamedKey::Unidentified));
    assert_eq!(Key::from(NamedKey::Tab), Key::Named(NamedKey::Tab));
}

fn every_generated_key_is_found_by_its_w3c_spelling() {
    for key in NamedKey::ALL {
        assert_eq!(NamedKey::from_w3c(key.as_str()), Some(*key), "{key:?}");
    }
    for code in Code::ALL {
        assert_eq!(Code::from_w3c(code.as_str()), Some(*code), "{code:?}");
    }
}

fn event_times_subtract_without_underflow() {
    let later = EventTime::from_nanos(5_000);
    assert_eq!(
        later.saturating_duration_since(T0),
        Duration::from_nanos(4_000)
    );
    assert_eq!(T0.saturating_duration_since(later), Duration::ZERO);
}

fn coalescing_preserves_the_latest_dispatch_and_real_history() {
    use flui_platform_api::keyboard::Modifiers;
    let old_current = sampled_at(10);
    let changed_sensor = old_current.with_pressure(Pressure::try_new(0.2).expect("valid pressure"));
    let older = PointerMove::new(mouse(), PointerButtons::NONE, old_current)
        .with_coalesced(vec![sampled_at(0), changed_sensor, sampled_at(5)])
        .with_predicted(vec![sampled_at(15)]);
    let mut newer = PointerMove::new(
        mouse(),
        PointerButtons::only(PointerButton::PRIMARY),
        sampled_at(20),
    )
    .with_modifiers(Modifiers::SHIFT)
    .with_coalesced(vec![old_current, changed_sensor, sampled_at(18)])
    .with_predicted(vec![sampled_at(25)]);
    let accepted_older = older.clone();
    newer.try_coalesce(&older).expect("same pointer metadata");
    assert_eq!(
        older, accepted_older,
        "the accepted earlier dispatch remains owned"
    );
    assert_eq!(*newer.current(), sampled_at(20));
    assert_eq!(newer.predicted(), &[sampled_at(25)]);
    assert_eq!(newer.buttons, PointerButtons::only(PointerButton::PRIMARY));
    assert_eq!(newer.modifiers, Modifiers::SHIFT);
    assert_eq!(
        newer.coalesced(),
        &[
            sampled_at(0),
            sampled_at(5),
            changed_sensor,
            old_current,
            old_current,
            changed_sensor,
            sampled_at(18)
        ]
    );
}

fn coalescing_refuses_every_pointer_metadata_mismatch_without_mutation() {
    use flui_platform_api::pointer::{DeviceId, MismatchedPointerInfo};
    let base = mouse();
    for other in [
        PointerInfo::new(PointerId::try_from(2_u64).expect("nonzero"), base.kind)
            .with_role(base.role),
        base.with_device(DeviceId::try_from(3_u64).expect("nonzero")),
        PointerInfo::new(base.id, PointerKind::Touch).with_role(base.role),
        base.with_role(PointerRole::Additional),
    ] {
        let older = PointerMove::new(other, PointerButtons::NONE, sampled_at(0));
        let mut newer = PointerMove::new(base, PointerButtons::NONE, sampled_at(20));
        let before = newer.clone();
        assert_eq!(newer.try_coalesce(&older), Err(MismatchedPointerInfo));
        assert_eq!(newer, before);
        assert_eq!(*older.current(), sampled_at(0));
        assert_eq!(older.pointer, other);
    }
}

fn coalescing_keeps_distinct_current_time_readings_and_excludes_future_history() {
    let current = sampled_at(20);
    let same_time_sensor = current.with_pressure(Pressure::try_new(0.5).expect("valid pressure"));
    let older = PointerMove::new(mouse(), PointerButtons::NONE, sampled_at(30))
        .with_coalesced(vec![sampled_at(10), current, same_time_sensor]);
    let mut newer = PointerMove::new(mouse(), PointerButtons::NONE, current)
        .with_coalesced(vec![sampled_at(10)]);
    newer.try_coalesce(&older).expect("same pointer metadata");
    assert_eq!(
        newer.coalesced(),
        &[sampled_at(10), sampled_at(10), same_time_sensor]
    );
    assert_eq!(*newer.current(), current);
}

#[test]
fn input_vocabulary_contract() {
    run_table(
        "input_vocabulary_contract",
        &[
            (
                "a_non_finite_position_is_refused",
                a_non_finite_position_is_refused,
            ),
            (
                "a_device_without_a_pressure_sensor_reads_none_not_a_default",
                a_device_without_a_pressure_sensor_reads_none_not_a_default,
            ),
            (
                "a_non_finite_sensor_reading_is_refused",
                a_non_finite_sensor_reading_is_refused,
            ),
            (
                "an_out_of_range_reading_is_refused_or_saturated",
                an_out_of_range_reading_is_refused_or_saturated,
            ),
            ("periodic_angles_are_wrapped", periodic_angles_are_wrapped),
            (
                "a_negative_or_non_finite_contact_size_is_refused",
                a_negative_or_non_finite_contact_size_is_refused,
            ),
            (
                "a_button_event_holds_the_button_after_a_press_and_not_after_a_release",
                a_button_event_holds_the_button_after_a_press_and_not_after_a_release,
            ),
            (
                "button_numbers_skip_the_eraser_slot",
                button_numbers_skip_the_eraser_slot,
            ),
            (
                "coalesced_and_predicted_readings_are_ordered_around_the_current_one",
                coalesced_and_predicted_readings_are_ordered_around_the_current_one,
            ),
            ("a_scroll_keeps_its_unit", a_scroll_keeps_its_unit),
            (
                "a_non_finite_scroll_delta_is_refused_and_zero_keeps_the_phase",
                a_non_finite_scroll_delta_is_refused_and_zero_keeps_the_phase,
            ),
            (
                "a_pan_zoom_transform_is_finite_with_a_positive_scale",
                a_pan_zoom_transform_is_finite_with_a_positive_scale,
            ),
            (
                "a_pan_zoom_event_is_always_a_trackpad_gesture",
                a_pan_zoom_event_is_always_a_trackpad_gesture,
            ),
            (
                "a_key_event_is_a_repeat_only_while_down",
                a_key_event_is_a_repeat_only_while_down,
            ),
            (
                "an_empty_character_is_an_unidentified_key",
                an_empty_character_is_an_unidentified_key,
            ),
            (
                "every_generated_key_is_found_by_its_w3c_spelling",
                every_generated_key_is_found_by_its_w3c_spelling,
            ),
            (
                "event_times_subtract_without_underflow",
                event_times_subtract_without_underflow,
            ),
            (
                "coalescing_preserves_the_latest_dispatch_and_real_history",
                coalescing_preserves_the_latest_dispatch_and_real_history,
            ),
            (
                "coalescing_refuses_every_pointer_metadata_mismatch_without_mutation",
                coalescing_refuses_every_pointer_metadata_mismatch_without_mutation,
            ),
            (
                "coalescing_keeps_distinct_current_time_readings_and_excludes_future_history",
                coalescing_keeps_distinct_current_time_readings_and_excludes_future_history,
            ),
        ],
    );
}

/// A pan/zoom event is a trackpad's, whatever pointer info it was built from.
fn a_pan_zoom_event_is_always_a_trackpad_gesture() {
    use flui_platform_api::pointer::{PanZoomEvent, PanZoomPhase};
    let event = PanZoomEvent::new(mouse(), T0, position(1.0, 1.0), PanZoomPhase::Start);
    assert_eq!(event.pointer().kind, PointerKind::Trackpad);
}
