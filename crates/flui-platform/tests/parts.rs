//! The parts types every serialization of a snapshot goes through: taking a
//! value apart and putting it back together gives the same value, every
//! field of it.

use std::num::NonZeroU64;
use std::time::Duration;

use flui_foundation::geometry::{Bounds, DeviceSize, EdgeInsets, Point, Size, WindowMetrics};
use flui_platform::{
    Availability, CapabilitiesOwner, CapabilityAbsence, ColorGamut, DisplayColor, DisplayInfo,
    DisplayKey, DisplayRegistry, GesturePreferences, Locale, MotionPreference, SizeConstraints,
    SystemPreferences, TextWeightPreference, WheelPreferences, WheelStep, WindowAppearance,
    WindowCapabilities, WindowCommand, WindowExecutionState, WindowGeometry, WindowMaterial,
    WindowMode, WindowState,
};

fn preferences_round_trip_through_their_parts() {
    let preferences = SystemPreferences::default()
        .with_text_scale(1.5)
        .expect("in range")
        .with_motion(MotionPreference::Reduce)
        .with_high_contrast(true)
        .with_haptics(false)
        .with_text_weight(TextWeightPreference::Bold)
        .with_locales(vec![Locale::en_us()])
        .with_gestures(
            GesturePreferences::default()
                .with_double_click_interval(Duration::from_millis(400))
                .with_long_press_timeout(Duration::from_millis(500)),
        )
        .with_wheel(
            WheelPreferences::default()
                .with_vertical(WheelStep::Page)
                .with_horizontal_characters(4),
        );
    let parts = preferences.clone().into_parts();
    assert_eq!(parts.text_scale, Some(1.5));
    assert_eq!(parts.try_into_preferences(), Ok(preferences));
}

fn preference_parts_validate_as_the_builder_does() {
    let mut parts = SystemPreferences::default().into_parts();
    parts.text_scale = Some(f64::NAN);
    assert!(
        parts.try_into_preferences().is_err(),
        "a text scale the builder refuses is refused from parts too"
    );
}

fn a_display_round_trips_through_its_parts() {
    let registry = DisplayRegistry::new();
    let id = registry.mint(&DisplayKey::Injected(9)).expect("id");
    let display = DisplayInfo::new(
        id,
        "Studio Display",
        Bounds::new(Point::new(-1920, 0), Size::new(5120, 2880)),
        2.0,
    )
    .with_usable_bounds(Bounds::new(Point::new(-1920, 40), Size::new(5120, 2840)))
    .with_primary(true)
    .with_refresh_period(Duration::from_micros(16_667))
    .with_persistent_key("studio")
    .with_color(Ok(DisplayColor::high(
        ColorGamut::DisplayP3,
        Err(CapabilityAbsence::NotOnThisDevice),
    )));
    let (taken, parts) = display.clone().into_parts();
    assert_eq!(taken, id);
    assert_eq!(parts.into_info(id), display);
}

fn a_window_state_round_trips_through_its_parts() {
    let material = WindowCommand::SetMaterial(WindowMaterial::System);
    let capabilities = WindowCapabilities::ALL
        .unavailable(&material, CapabilityAbsence::NotYetOnThisHost)
        .without_presentation_feedback(CapabilityAbsence::NotOnThisDevice);
    let metrics = WindowMetrics::try_new(DeviceSize::new(1600, 1200), 2.0).expect("valid");
    let state = WindowState::new(
        WindowGeometry::new(Some(Point::new(10.0, 20.0)), Size::new(820.0, 640.0)),
        Size::new(800.0, 600.0),
    )
    .with_mode(WindowMode::Maximized)
    .with_normal_geometry(Some(WindowGeometry::new(None, Size::new(400.0, 300.0))))
    .with_visible(true)
    .with_occluded(false)
    .with_focused(true)
    .with_title(Some("Notes".to_owned()))
    .with_constraints(SizeConstraints::new(Some(Size::new(200.0, 100.0)), None))
    .with_resizable(false)
    .with_decorated(true)
    .with_metrics(Some(metrics))
    .with_appearance(WindowAppearance::Dark)
    .with_safe_area(EdgeInsets {
        top: 24.0,
        right: 0.0,
        bottom: 34.0,
        left: 0.0,
    })
    .with_execution(WindowExecutionState::Suspended)
    .with_hovered(true)
    .with_active(true)
    .with_capabilities(capabilities);
    let parts = state.clone().into_parts();
    assert_eq!(
        parts.capabilities.material,
        Err(CapabilityAbsence::NotYetOnThisHost)
    );
    assert_eq!(
        parts.capabilities.reports_presentation,
        Err(CapabilityAbsence::NotOnThisDevice)
    );
    assert_eq!(parts.capabilities.sets_title, Ok(Availability::Native));
    assert_eq!(parts.into_state(), state);
}

fn parts_leave_the_revision_and_owner_to_the_publishing_window() {
    let owner = CapabilitiesOwner::new(NonZeroU64::MIN);
    let first = WindowState::new(
        WindowGeometry::new(None, Size::new(1.0, 1.0)),
        Size::new(1.0, 1.0),
    )
    .with_capabilities(WindowCapabilities::ALL.owned_by(owner));
    let published = first
        .successor(first.clone().with_focused(true))
        .expect("a revision is left");
    let sample = published.clone().into_parts().into_state();
    assert_eq!(sample.revision.get(), 0, "a sample carries no revision");
    let again = published.successor(sample).expect("a revision is left");
    assert_eq!(
        again.revision, published.revision,
        "the sample of a published snapshot is no change to it"
    );
}

#[test]
fn parts_table() {
    crate::run_table(
        "parts_table",
        &[
            (
                "preferences_round_trip_through_their_parts",
                preferences_round_trip_through_their_parts as fn(),
            ),
            (
                "preference_parts_validate_as_the_builder_does",
                preference_parts_validate_as_the_builder_does as fn(),
            ),
            (
                "a_display_round_trips_through_its_parts",
                a_display_round_trips_through_its_parts as fn(),
            ),
            (
                "a_window_state_round_trips_through_its_parts",
                a_window_state_round_trips_through_its_parts as fn(),
            ),
            (
                "parts_leave_the_revision_and_owner_to_the_publishing_window",
                parts_leave_the_revision_and_owner_to_the_publishing_window as fn(),
            ),
        ],
    );
}
