//! `AppConfig` becomes `WindowOptions` without losing a window field, and
//! without choosing the two things only the runner can promise: that the window
//! is visible and when it is revealed.

use flui_app::AppConfig;
use flui_foundation::geometry::{Point, Size};
use flui_platform::os_options::{OsWindowOptions, Win32WindowOptions, WindowsCornerPreference};
use flui_platform::{WindowMode, WindowOptions, WindowReveal};

fn options(config: &AppConfig) -> WindowOptions {
    WindowOptions::from(config)
}

fn the_title_is_kept() {
    assert_eq!(
        options(&AppConfig::new().with_title("Notes")).title,
        "Notes"
    );
}

fn the_size_is_kept() {
    assert_eq!(
        options(&AppConfig::new().with_size(900, 700)).size,
        Size::new(900.0, 700.0)
    );
}

fn the_limits_are_kept() {
    let converted = options(
        &AppConfig::new()
            .with_min_size(300, 200)
            .with_max_size(1600, 1200),
    );
    assert_eq!(converted.min_size, Some(Size::new(300.0, 200.0)));
    assert_eq!(converted.max_size, Some(Size::new(1600.0, 1200.0)));
}

fn the_frame_flags_are_kept() {
    let converted = options(
        &AppConfig::new()
            .with_resizable(false)
            .with_decorations(false),
    );
    assert!(!converted.resizable);
    assert!(!converted.decorated);
}

fn the_position_is_kept() {
    assert_eq!(
        options(&AppConfig::new().with_position(Point::new(-1200.0, 80.0))).position,
        Some(Point::new(-1200.0, 80.0))
    );
    assert_eq!(options(&AppConfig::new()).position, None);
}

fn the_mode_is_kept() {
    for mode in [
        WindowMode::Normal,
        WindowMode::Minimized,
        WindowMode::Maximized,
        WindowMode::Fullscreen,
    ] {
        assert_eq!(options(&AppConfig::new().with_mode(mode)).mode, mode);
    }
}

fn every_systems_options_are_kept() {
    let os_options = OsWindowOptions::default().with_windows(
        Win32WindowOptions::default().with_corner_preference(WindowsCornerPreference::DoNotRound),
    );
    assert_eq!(
        options(&AppConfig::new().with_os_options(os_options.clone())).os_options,
        os_options
    );
    assert_eq!(
        options(&AppConfig::new()).os_options,
        OsWindowOptions::default()
    );
}

fn visibility_and_reveal_stay_the_options_own() {
    let defaults = WindowOptions::default();
    let converted = options(
        &AppConfig::new()
            .with_title("x")
            .with_mode(WindowMode::Maximized),
    );
    assert_eq!(converted.visible, defaults.visible);
    assert_eq!(converted.reveal, defaults.reveal);
    assert_eq!(
        converted.reveal,
        WindowReveal::AtOpen,
        "only a runner that keeps the promise defers the reveal"
    );
}

#[test]
fn config_to_options_keeps_every_field() {
    crate::run_table(
        "config_to_options_keeps_every_field",
        &[
            ("the_title_is_kept", the_title_is_kept as fn()),
            ("the_size_is_kept", the_size_is_kept as fn()),
            ("the_limits_are_kept", the_limits_are_kept as fn()),
            ("the_frame_flags_are_kept", the_frame_flags_are_kept as fn()),
            ("the_position_is_kept", the_position_is_kept as fn()),
            ("the_mode_is_kept", the_mode_is_kept as fn()),
            (
                "every_systems_options_are_kept",
                every_systems_options_are_kept as fn(),
            ),
            (
                "visibility_and_reveal_stay_the_options_own",
                visibility_and_reveal_stay_the_options_own as fn(),
            ),
        ],
    );
}
