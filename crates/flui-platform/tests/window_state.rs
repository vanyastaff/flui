//! The window state contract (ADR-0237), driven through the public
//! `PlatformWindow` surface of the headless platform: every command changes the
//! snapshot and advances its revision by one, a repeat changes nothing, the
//! host's clamps and refusals show in the snapshot, and a capability that is
//! `false` refuses with `Unsupported` and no change.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    DisplaySpec, HeadlessPlatform, HostLimits, HostWindow, MockWindow, Platform as _,
    SizeConstraints, WindowCapabilities, WindowCommand, WindowCommandError, WindowMode,
    WindowOptions, WindowState,
};

fn open(platform: &HeadlessPlatform) -> Arc<dyn HostWindow> {
    let window: Arc<dyn HostWindow> = platform
        .open_window(WindowOptions::default())
        .expect("a headless window opens");
    window
}

fn mock(window: &Arc<dyn HostWindow>) -> &MockWindow {
    window
        .as_any()
        .downcast_ref::<MockWindow>()
        .expect("the headless platform opens mock windows")
}

/// Counts `on_state_changed` notifications.
fn count_changes(window: &Arc<dyn HostWindow>) -> Arc<AtomicUsize> {
    let changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&changes);
    window.on_state_changed(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    changes
}

fn applied(window: &Arc<dyn HostWindow>, command: WindowCommand) -> (WindowState, WindowState) {
    let before = window.state();
    window
        .apply(command.clone())
        .unwrap_or_else(|error| panic!("{command:?} is supported: {error}"));
    (before, window.state())
}

type Change = fn(&WindowState) -> bool;

/// Each row: the command, and what the snapshot must show afterwards.
fn commands() -> Vec<(&'static str, WindowCommand, Change)> {
    vec![
        (
            "set_title",
            WindowCommand::SetTitle("Renamed".into()),
            |state| state.title.as_deref() == Some("Renamed"),
        ),
        (
            "set_position",
            WindowCommand::SetPosition(Point::new(120.0, 80.0)),
            |state| state.outer.position == Some(Point::new(120.0, 80.0)),
        ),
        (
            "set_size",
            WindowCommand::SetSize(Size::new(640.0, 480.0)),
            |state| {
                state.content_size == Size::new(640.0, 480.0)
                    && state.outer.size == Size::new(640.0, 480.0)
            },
        ),
        (
            "set_constraints",
            WindowCommand::SetConstraints(SizeConstraints::new(
                Some(Size::new(300.0, 200.0)),
                Some(Size::new(1200.0, 900.0)),
            )),
            |state| state.constraints.min == Some(Size::new(300.0, 200.0)),
        ),
        (
            "set_resizable",
            WindowCommand::SetResizable(false),
            |state| !state.resizable,
        ),
        (
            "set_decorated",
            WindowCommand::SetDecorated(false),
            |state| !state.decorated,
        ),
        (
            "maximize",
            WindowCommand::SetMode(WindowMode::Maximized),
            |state| state.mode == WindowMode::Maximized && state.normal_geometry.is_some(),
        ),
        (
            "minimize",
            WindowCommand::SetMode(WindowMode::Minimized),
            |state| state.mode == WindowMode::Minimized && !state.focused,
        ),
        (
            "fullscreen",
            WindowCommand::SetMode(WindowMode::Fullscreen),
            |state| state.mode == WindowMode::Fullscreen && !state.decorated,
        ),
        ("hide", WindowCommand::Hide, |state| !state.visible),
    ]
}

fn every_command_changes_the_snapshot_and_advances_the_revision_by_one() {
    for (name, command, shows) in commands() {
        let platform = HeadlessPlatform::new();
        let window = open(&platform);
        let (before, after) = applied(&window, command);
        assert!(shows(&after), "{name}: the snapshot does not show it");
        assert_eq!(
            after.revision.get(),
            before.revision.get() + 1,
            "{name}: one change is one revision"
        );
    }
}

fn show_and_activate_change_what_hide_and_blur_left() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    window.apply(WindowCommand::Hide).expect("hide");
    let (before, after) = applied(&window, WindowCommand::Show);
    assert!(after.visible && after.focused, "show reveals and focuses");
    assert_eq!(after.revision.get(), before.revision.get() + 1);

    mock(&window).simulate_focus(false);
    let (before, after) = applied(&window, WindowCommand::Activate);
    assert!(after.focused);
    assert_eq!(after.revision.get(), before.revision.get() + 1);
}

fn restoring_returns_to_the_rectangle_before() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    let opened = window.state();
    let (_, maximized) = applied(&window, WindowCommand::SetMode(WindowMode::Maximized));
    assert_eq!(maximized.mode, WindowMode::Maximized);
    assert_eq!(
        maximized.outer.size,
        Size::new(1920.0, 1080.0),
        "maximized fills the display"
    );
    assert_eq!(
        maximized.normal_geometry,
        Some(opened.outer),
        "the way back is the rectangle before"
    );

    let (_, restored) = applied(&window, WindowCommand::SetMode(WindowMode::Normal));
    assert_eq!(restored.mode, WindowMode::Normal);
    assert_eq!(restored.outer, opened.outer);
    assert_eq!(restored.normal_geometry, None);
}

fn fullscreen_is_borderless_on_the_whole_display() {
    let platform = HeadlessPlatform::new().with_displays(vec![
        DisplaySpec::new("Wide", 2560, 1440).with_scale_factor(2.0),
    ]);
    let window = open(&platform);
    let (_, fullscreen) = applied(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert_eq!(fullscreen.mode, WindowMode::Fullscreen);
    assert_eq!(
        fullscreen.outer.size,
        Size::new(1280.0, 720.0),
        "the display's size in its own logical pixels"
    );
    assert!(!fullscreen.decorated, "fullscreen has no frame");
}

fn repeating_a_command_keeps_the_revision_and_stays_silent() {
    for (name, command, _) in commands() {
        let platform = HeadlessPlatform::new();
        let window = open(&platform);
        let changes = count_changes(&window);
        let (_, first) = applied(&window, command.clone());
        assert_eq!(
            changes.load(Ordering::SeqCst),
            1,
            "{name}: one notification"
        );
        let (before, second) = applied(&window, command);
        assert_eq!(first.revision, second.revision, "{name}: revision kept");
        assert_eq!(before, second, "{name}: nothing changed");
        assert_eq!(
            changes.load(Ordering::SeqCst),
            1,
            "{name}: a repeat is not a change"
        );
    }
}

fn the_host_clamp_is_visible_in_the_snapshot() {
    let platform = HeadlessPlatform::new()
        .with_host_limits(HostLimits::none().with_max_window_size(Size::new(800.0, 600.0)));
    let window = open(&platform);
    let (_, after) = applied(&window, WindowCommand::SetSize(Size::new(2000.0, 1500.0)));
    assert_eq!(
        after.content_size,
        Size::new(800.0, 600.0),
        "the snapshot describes what the host did, not what was asked"
    );
}

fn a_refused_maximize_is_accepted_and_changes_nothing() {
    let platform =
        HeadlessPlatform::new().with_host_limits(HostLimits::none().with_refused_maximize());
    let window = open(&platform);
    let changes = count_changes(&window);
    let (before, after) = applied(&window, WindowCommand::SetMode(WindowMode::Maximized));
    assert_eq!(after.mode, WindowMode::Normal);
    assert_eq!(before, after);
    assert_eq!(changes.load(Ordering::SeqCst), 0);
}

fn a_false_capability_is_unsupported_and_changes_nothing() {
    for (name, command, _) in commands() {
        let platform = HeadlessPlatform::new().with_window_capabilities(WindowCapabilities::none());
        let window = open(&platform);
        let changes = count_changes(&window);
        let before = window.state();
        assert_eq!(
            window.apply(command),
            Err(WindowCommandError::Unsupported),
            "{name}"
        );
        assert_eq!(window.state(), before, "{name}: no change");
        assert_eq!(changes.load(Ordering::SeqCst), 0, "{name}: no notification");
    }
}

fn a_true_capability_is_honoured() {
    let mut only_title = WindowCapabilities::none();
    only_title.set_title = true;
    let platform = HeadlessPlatform::new().with_window_capabilities(only_title);
    let window = open(&platform);
    assert_eq!(window.capabilities(), only_title);
    window
        .apply(WindowCommand::SetTitle("Allowed".into()))
        .expect("the one capability the window has");
    assert_eq!(window.state().title.as_deref(), Some("Allowed"));
    assert_eq!(
        window.apply(WindowCommand::SetMode(WindowMode::Maximized)),
        Err(WindowCommandError::Unsupported)
    );
}

fn restoring_normal_is_always_accepted() {
    let platform = HeadlessPlatform::new().with_window_capabilities(WindowCapabilities::none());
    let window = open(&platform);
    window
        .apply(WindowCommand::SetMode(WindowMode::Normal))
        .expect("every window is normal already");
}

fn invalid_geometry_is_refused_before_the_host() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    let before = window.state();
    for command in [
        WindowCommand::SetSize(Size::new(f64::NAN, 100.0)),
        WindowCommand::SetSize(Size::new(0.0, 100.0)),
        WindowCommand::SetPosition(Point::new(f64::INFINITY, 0.0)),
        WindowCommand::SetConstraints(SizeConstraints::new(
            Some(Size::new(500.0, 500.0)),
            Some(Size::new(100.0, 100.0)),
        )),
    ] {
        assert_eq!(
            window.apply(command.clone()),
            Err(WindowCommandError::InvalidGeometry),
            "{command:?}"
        );
    }
    assert_eq!(window.state(), before);
}

fn a_closed_window_refuses_commands() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    window.close();
    assert_eq!(
        window.apply(WindowCommand::SetTitle("late".into())),
        Err(WindowCommandError::Closed)
    );
}

fn the_user_resizing_and_moving_publish_too() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    let changes = count_changes(&window);
    let opened = window.state();

    mock(&window).simulate_resize(900.0, 700.0);
    let resized = window.state();
    assert_eq!(resized.content_size, Size::new(900.0, 700.0));
    assert_eq!(resized.revision.get(), opened.revision.get() + 1);

    mock(&window).simulate_move(Point::new(30.0, 40.0));
    let moved = window.state();
    assert_eq!(moved.outer.position, Some(Point::new(30.0, 40.0)));
    assert_eq!(moved.revision.get(), resized.revision.get() + 1);
    assert_eq!(changes.load(Ordering::SeqCst), 2);

    mock(&window).simulate_resize(900.0, 700.0);
    assert_eq!(
        window.state().revision,
        moved.revision,
        "the same size again is not a change"
    );
}

fn a_window_without_position_reports_none() {
    let mut caps = WindowCapabilities::all();
    caps.observes_position = false;
    let platform = HeadlessPlatform::new().with_window_capabilities(caps);
    let window = open(&platform);
    assert_eq!(window.state().outer.position, None);
}

fn opening_options_are_reflected_in_the_first_snapshot() {
    let platform = HeadlessPlatform::new();
    let normal =
        flui_platform::WindowGeometry::new(Some(Point::new(50.0, 60.0)), Size::new(700.0, 500.0));
    let options = WindowOptions {
        position: Some(Point::new(50.0, 60.0)),
        mode: WindowMode::Maximized,
        normal_geometry: Some(normal),
        ..WindowOptions::default()
    };
    let window: Arc<dyn HostWindow> = platform.open_window(options).expect("window");
    let state = window.state();
    assert_eq!(state.mode, WindowMode::Maximized);
    assert_eq!(state.revision.get(), 0, "the first snapshot is revision 0");
    assert_eq!(state.normal_geometry, Some(normal));
}

#[test]
fn window_state_contract() {
    crate::run_table(
        "window_state_contract",
        &[
            (
                "every_command_changes_the_snapshot_and_advances_the_revision_by_one",
                every_command_changes_the_snapshot_and_advances_the_revision_by_one as fn(),
            ),
            (
                "show_and_activate_change_what_hide_and_blur_left",
                show_and_activate_change_what_hide_and_blur_left as fn(),
            ),
            (
                "restoring_returns_to_the_rectangle_before",
                restoring_returns_to_the_rectangle_before as fn(),
            ),
            (
                "fullscreen_is_borderless_on_the_whole_display",
                fullscreen_is_borderless_on_the_whole_display as fn(),
            ),
            (
                "repeating_a_command_keeps_the_revision_and_stays_silent",
                repeating_a_command_keeps_the_revision_and_stays_silent as fn(),
            ),
            (
                "the_host_clamp_is_visible_in_the_snapshot",
                the_host_clamp_is_visible_in_the_snapshot as fn(),
            ),
            (
                "a_refused_maximize_is_accepted_and_changes_nothing",
                a_refused_maximize_is_accepted_and_changes_nothing as fn(),
            ),
            (
                "a_false_capability_is_unsupported_and_changes_nothing",
                a_false_capability_is_unsupported_and_changes_nothing as fn(),
            ),
            (
                "a_true_capability_is_honoured",
                a_true_capability_is_honoured as fn(),
            ),
            (
                "restoring_normal_is_always_accepted",
                restoring_normal_is_always_accepted as fn(),
            ),
            (
                "invalid_geometry_is_refused_before_the_host",
                invalid_geometry_is_refused_before_the_host as fn(),
            ),
            (
                "a_closed_window_refuses_commands",
                a_closed_window_refuses_commands as fn(),
            ),
            (
                "the_user_resizing_and_moving_publish_too",
                the_user_resizing_and_moving_publish_too as fn(),
            ),
            (
                "a_window_without_position_reports_none",
                a_window_without_position_reports_none as fn(),
            ),
            (
                "opening_options_are_reflected_in_the_first_snapshot",
                opening_options_are_reflected_in_the_first_snapshot as fn(),
            ),
        ],
    );
}
