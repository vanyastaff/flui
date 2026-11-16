//! The window state contract (ADR-0237), driven through the public
//! `PlatformWindow` surface of the headless platform: every command changes the
//! snapshot and advances its revision by one, a repeat changes nothing, the
//! host's clamps and refusals show in the snapshot, admission refuses a
//! command whose capability is `Absent` with `Unsupported` and no change,
//! and a command admitted before the capabilities changed is `Superseded`.

use flui_platform::CommandRefusal;
use flui_platform::{AppTitleBar, ControlsOffset, SystemControls, WindowFrame};
use flui_testing::window_commands::AdmitAndApply as _;
use std::rc::Rc;
use std::sync::Arc;

use crate::Opened;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::{DevicePixelRatio, Point, Size};
use flui_native::{
    DisplaySpec, HeadlessHostWindow, HeadlessPlatform, HeadlessWindowPolicy, HostWindow,
    Platform as _,
};
use flui_platform::{
    AdmissionError, ApplyError, WindowCapabilities, WindowCommand, WindowMode, WindowOptions,
    WindowState,
};

/// What a window answers for a command its capabilities leave out.
const NOT_HERE: flui_platform::Unsupported =
    flui_platform::Unsupported::new(flui_platform::UnsupportedReason::NotOnThisPlatform);

fn open(platform: &HeadlessPlatform) -> Opened {
    Opened::on(platform, WindowOptions::default()).expect("a headless window opens")
}

fn mock(window: &Rc<dyn HostWindow>) -> &HeadlessHostWindow {
    window
        .as_any()
        .downcast_ref::<HeadlessHostWindow>()
        .expect("the headless platform opens headless windows")
}

/// Counts the handler's `state_changed` calls from here on.
fn count_changes(window: &Opened) -> Arc<AtomicUsize> {
    let changes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&changes);
    window.handler.react_to_state_change(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    changes
}

fn applied(window: &Rc<dyn HostWindow>, command: WindowCommand) -> (WindowState, WindowState) {
    let before = window.state();
    window
        .admit_and_apply(command.clone())
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
            |state| state.outer.position() == Some(Point::new(120.0, 80.0)),
        ),
        (
            "set_size",
            WindowCommand::SetSize(Size::new(640.0, 480.0)),
            |state| {
                state.content_size == Size::new(640.0, 480.0)
                    && state.outer.size() == Size::new(640.0, 480.0)
            },
        ),
        (
            "set_constraints",
            WindowCommand::SetConstraints(limits(
                Some(Size::new(300.0, 200.0)),
                Some(Size::new(1200.0, 900.0)),
            )),
            |state| {
                state.constraints.min.map(flui_platform::WindowExtent::get)
                    == Some(Size::new(300.0, 200.0))
            },
        ),
        (
            "set_resizable",
            WindowCommand::SetResizable(false),
            |state| !state.resizable,
        ),
        (
            "set_frame",
            WindowCommand::SetFrame(WindowFrame::None),
            |state| !state.frame.has_system_title_bar(),
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
            |state| state.mode == WindowMode::Fullscreen && !state.frame.has_system_title_bar(),
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
    window.admit_and_apply(WindowCommand::Hide).expect("hide");
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
        maximized.outer.size(),
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
    let platform = HeadlessPlatform::new()
        .with_displays(vec![DisplaySpec::new("Wide", 2560, 1440).scale_factor(
            DevicePixelRatio::try_new(2.0).expect("a positive ratio"),
        )]);
    let window = open(&platform);
    let (_, fullscreen) = applied(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
    assert_eq!(fullscreen.mode, WindowMode::Fullscreen);
    assert_eq!(
        fullscreen.outer.size(),
        Size::new(1280.0, 720.0),
        "the display's size in its own logical pixels"
    );
    assert_eq!(
        fullscreen.frame,
        WindowFrame::None,
        "fullscreen has no frame"
    );
}

fn app_title_bar_controls_survive_open_commands_and_fullscreen() {
    for controls in [
        SystemControls::Shown,
        SystemControls::Hidden,
        SystemControls::Offset(ControlsOffset::try_new(17.0, 9.0).expect("finite offset")),
    ] {
        let frame = WindowFrame::AppTitleBar(AppTitleBar::new().with_system_controls(controls));
        let platform = HeadlessPlatform::new();
        let window = Opened::on(&platform, WindowOptions::default().frame(frame))
            .expect("a headless window opens with an app title bar");
        assert_eq!(window.state().frame, frame, "opening keeps {controls:?}");
        applied(&window, WindowCommand::SetFrame(WindowFrame::System));
        let (before, changed) = applied(&window, WindowCommand::SetFrame(frame));
        assert_eq!(changed.frame, frame, "a command keeps {controls:?}");
        assert_eq!(changed.revision.get(), before.revision.get() + 1);
        let (_, repeated) = applied(&window, WindowCommand::SetFrame(frame));
        assert_eq!(repeated, changed, "the same payload is not a change");

        applied(&window, WindowCommand::SetFrame(WindowFrame::System));
        applied(&window, WindowCommand::SetMode(WindowMode::Fullscreen));
        assert_eq!(window.state().frame, WindowFrame::None);
        applied(&window, WindowCommand::SetFrame(frame));
        assert_eq!(window.state().frame, WindowFrame::None);
        let (_, restored) = applied(&window, WindowCommand::SetMode(WindowMode::Normal));
        assert_eq!(
            restored.frame, frame,
            "restoring keeps the latest {controls:?}"
        );
    }
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
    let platform = HeadlessPlatform::new().with_window_policy(
        HeadlessWindowPolicy::default().with_max_window_size(Size::new(800.0, 600.0)),
    );
    let window = open(&platform);
    let (_, after) = applied(&window, WindowCommand::SetSize(Size::new(2000.0, 1500.0)));
    assert_eq!(
        after.content_size,
        Size::new(800.0, 600.0),
        "the snapshot describes what the host did, not what was asked"
    );
}

/// A size past the largest surface a renderer draws is held at it, at the
/// window's scale, as every native host clamps a window extent; one under it
/// is taken as asked.
fn a_size_past_the_largest_surface_is_held_at_it() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    let largest = f64::from(flui_platform::MAX_SURFACE_EXTENT);
    // (scale, asked width, published width)
    for (scale, asked, published) in [
        (1.0, largest - 1.0, largest - 1.0),
        (1.0, largest, largest),
        (1.0, largest + 1.0, largest),
        (1.0, 1.0e10, largest),
        (2.0, largest, largest / 2.0),
    ] {
        mock(&window).simulate_scale_factor_change(scale);
        let (_, after) = applied(&window, WindowCommand::SetSize(Size::new(asked, 100.0)));
        assert_eq!(
            after.content_size,
            Size::new(published, 100.0),
            "{asked} at scale {scale}"
        );
        let physical = after.metrics.expect("a drawable window").physical();
        assert!(
            u32::try_from(physical.width)
                .is_ok_and(|width| width <= flui_platform::MAX_SURFACE_EXTENT),
            "{asked} at scale {scale}: {physical:?}"
        );
    }
}

fn a_refused_maximize_is_accepted_and_changes_nothing() {
    let platform = HeadlessPlatform::new()
        .with_window_policy(HeadlessWindowPolicy::default().refusing_maximize());
    let window = open(&platform);
    let changes = count_changes(&window);
    let (before, after) = applied(&window, WindowCommand::SetMode(WindowMode::Maximized));
    assert_eq!(after.mode, WindowMode::Normal);
    assert_eq!(before, after);
    assert_eq!(changes.load(Ordering::SeqCst), 0);
}

fn a_false_capability_is_unsupported_and_changes_nothing() {
    for (name, command, _) in commands() {
        let platform = HeadlessPlatform::new().with_window_capabilities(WindowCapabilities::NONE);
        let window = open(&platform);
        let changes = count_changes(&window);
        let before = window.state();
        assert_eq!(
            window.admit_and_apply(command),
            Err(CommandRefusal::Admission(AdmissionError::Unsupported(
                NOT_HERE
            ))),
            "{name}"
        );
        assert_eq!(window.state(), before, "{name}: no change");
        assert_eq!(changes.load(Ordering::SeqCst), 0, "{name}: no notification");
    }
}

fn a_true_capability_is_honoured() {
    let mut only_title = WindowCapabilities::NONE.into_parts();
    only_title.sets_title = flui_platform::WindowCapability::Native;
    let only_title = only_title.into_capabilities();
    let platform = HeadlessPlatform::new().with_window_capabilities(only_title);
    let window = open(&platform);
    assert_eq!(window.state().capabilities, only_title);
    window
        .admit_and_apply(WindowCommand::SetTitle("Allowed".into()))
        .expect("the one capability the window has");
    assert_eq!(window.state().title.as_deref(), Some("Allowed"));
    assert_eq!(
        window.admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized)),
        Err(CommandRefusal::Admission(AdmissionError::Unsupported(
            NOT_HERE
        )))
    );
}

fn restoring_normal_is_always_accepted() {
    let platform = HeadlessPlatform::new().with_window_capabilities(WindowCapabilities::NONE);
    let window = open(&platform);
    window
        .admit_and_apply(WindowCommand::SetMode(WindowMode::Normal))
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
        WindowCommand::SetConstraints(limits(
            Some(Size::new(500.0, 500.0)),
            Some(Size::new(100.0, 100.0)),
        )),
    ] {
        let refused = command
            .validate()
            .expect_err("numbers that describe no window");
        assert_eq!(
            window.admit_and_apply(command.clone()),
            Err(CommandRefusal::Admission(refused)),
            "{command:?}"
        );
        assert!(
            matches!(refused, AdmissionError::InvalidGeometry(_)),
            "{command:?}"
        );
    }
    assert_eq!(window.state(), before);
}

/// A report whose geometry describes no window costs the window its geometry
/// and nothing else: a focus change and a close the window system reports
/// while it stands are published, and the close's `state_changed` reaches the
/// handler before `closed`.
fn a_bad_report_keeps_the_geometry_and_publishes_the_rest() {
    use flui_testing::window_handler::HandlerCall;

    let platform = HeadlessPlatform::new();
    let opened = open(&platform);
    let host = mock(&opened.window);
    let before = opened.window.state();
    host.simulate_resize(f64::NAN, 100.0);
    host.simulate_focus(!before.focused);
    let focused = opened.window.state();
    assert_eq!(focused.focused, !before.focused, "the focus is published");
    assert_eq!(
        focused.content_size, before.content_size,
        "the geometry stands"
    );
    assert_eq!(focused.outer, before.outer);
    let calls = opened.handler.calls().len();
    host.simulate_close();
    let closed = opened.window.state();
    assert!(!closed.visible, "the close is published");
    assert_eq!(
        closed.execution,
        flui_platform::WindowExecutionState::Detached
    );
    let after: Vec<HandlerCall> = opened.handler.calls().split_off(calls);
    let changed = after
        .iter()
        .position(|call| *call == HandlerCall::StateChanged);
    let ended = after.iter().position(|call| *call == HandlerCall::Closed);
    assert!(
        matches!((changed, ended), (Some(changed), Some(ended)) if changed < ended),
        "the close's state change, then closed: {after:?}"
    );
}
/// An opening asks for its size, limits and position as the commands do:
/// options a command would be refused for open no window, and the error names
/// the option and its numbers.
fn options_that_describe_no_window_open_none() {
    use flui_platform::{InvalidGeometry, WindowExtent, WindowOption};

    let platform = HeadlessPlatform::new();
    let extent =
        |width, height| WindowExtent::try_new(Size::new(width, height)).expect("an extent");
    let rows = [
        (
            WindowOptions::default()
                .min_size(Some(extent(800.0, 600.0)))
                .max_size(Some(extent(400.0, 300.0))),
            WindowOption::Limits,
            InvalidGeometry::Limits {
                min: extent(800.0, 600.0),
                max: extent(400.0, 300.0),
            },
        ),
        (
            WindowOptions::default().min_size(Some(WindowExtent::ZERO)),
            WindowOption::MinSize,
            InvalidGeometry::Empty {
                width: 0.0,
                height: 0.0,
            },
        ),
        (
            WindowOptions::default().size(Size::new(f64::NAN, 600.0)),
            WindowOption::Size,
            InvalidGeometry::Extent {
                width: f64::NAN,
                height: 600.0,
            },
        ),
        (
            WindowOptions::default().position(Some(Point::new(f64::INFINITY, 0.0))),
            WindowOption::Position,
            InvalidGeometry::Position {
                x: f64::INFINITY,
                y: 0.0,
            },
        ),
    ];
    for (options, option, refused) in rows {
        let opened = Opened::on(&platform, options);
        assert!(
            matches!(
                opened,
                Err(flui_native::OpenWindowError::InvalidOption(error))
                    if error.option == option && error.refused == refused
            ),
            "{refused}: {:?}",
            opened.err()
        );
    }
}

fn a_closed_window_refuses_commands() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    window
        .admit_and_apply(flui_platform::WindowCommand::Close)
        .expect("a headless window closes");
    assert_eq!(
        window.admit_and_apply(WindowCommand::SetTitle("late".into())),
        Err(CommandRefusal::Apply(ApplyError::Closed))
    );
}

/// A command admitted at one capabilities revision and applied after the
/// window system revised what the window honours answers `Superseded`, not a
/// failure of the OS, and changes nothing; admitted again it either applies
/// or is refused by `admit`. The revision of the capabilities moves with the
/// change, one `WindowHandler::state_changed` announces it, and a fact apart from the
/// capabilities leaves their revision where it was.
fn a_command_admitted_before_a_capability_change_is_superseded() {
    let platform = HeadlessPlatform::new();
    let window = open(&platform);
    let changes = count_changes(&window);
    let opened = window.state();
    let title = WindowCommand::SetTitle("admitted before".into());
    let maximize = WindowCommand::SetMode(WindowMode::Maximized);
    let early_title = title
        .clone()
        .admit(&opened.capabilities)
        .expect("the window takes titles");
    let early_maximize = maximize
        .clone()
        .admit(&opened.capabilities)
        .expect("the window maximizes");

    mock(&window).simulate_focus(false);
    let unfocused = window.state();
    assert_eq!(
        unfocused.capabilities.revision(),
        opened.capabilities.revision(),
        "focus is not a capability"
    );

    let mut revised = WindowCapabilities::ALL.into_parts();
    revised.maximizes = flui_platform::WindowCapability::NOT_ON_THIS_PLATFORM;
    let revised = revised.into_capabilities();
    mock(&window).simulate_capabilities(revised);
    let after = window.state();
    assert_eq!(after.revision.get(), unfocused.revision.get() + 1);
    assert_eq!(after.capabilities.revision(), after.revision);
    assert_eq!(
        changes.load(Ordering::SeqCst),
        2,
        "focus, then the capabilities"
    );

    assert_eq!(
        window.apply(early_title),
        Err(ApplyError::Superseded {
            command: title.clone()
        })
    );
    assert_eq!(
        window.apply(early_maximize),
        Err(ApplyError::Superseded {
            command: maximize.clone()
        })
    );
    assert_eq!(
        window.state(),
        after,
        "a superseded command changes nothing"
    );

    window
        .admit_and_apply(title)
        .expect("admitted again, the title applies");
    assert_eq!(window.state().title.as_deref(), Some("admitted before"));
    assert_eq!(
        window.admit_and_apply(maximize),
        Err(CommandRefusal::Admission(AdmissionError::Unsupported(
            NOT_HERE
        ))),
        "admitted again, what the window lost is refused"
    );
}

/// A command admitted for one window is never applied to another, even
/// though both opened with the same capabilities at the same revision, and a
/// command admitted against capabilities built by hand opens on no window.
fn a_command_admitted_for_another_window_is_superseded() {
    let platform = HeadlessPlatform::new();
    let first = open(&platform);
    let second = open(&platform);
    assert_eq!(
        first.state().capabilities.revision(),
        second.state().capabilities.revision()
    );
    let title = WindowCommand::SetTitle("for the first".into());
    let for_first = title
        .clone()
        .admit(&first.state().capabilities)
        .expect("the first window takes titles");
    assert_eq!(
        second.apply(for_first),
        Err(ApplyError::Superseded {
            command: title.clone()
        })
    );
    assert_eq!(
        title.admit(&WindowCapabilities::ALL),
        Err(AdmissionError::Unpublished),
        "capabilities built by hand admit nothing"
    );
    assert_ne!(first.state().title.as_deref(), Some("for the first"));
    assert_ne!(second.state().title.as_deref(), Some("for the first"));
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
    assert_eq!(moved.outer.position(), Some(Point::new(30.0, 40.0)));
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
    let mut caps = WindowCapabilities::ALL.into_parts();
    caps.observes_position = flui_platform::Availability::Unavailable;
    let caps = caps.into_capabilities();
    let platform = HeadlessPlatform::new().with_window_capabilities(caps);
    let window = open(&platform);
    assert_eq!(window.state().outer.position(), None);
}

fn opening_options_are_reflected_in_the_first_snapshot() {
    let platform = HeadlessPlatform::new();
    let normal = geometry(Some(Point::new(50.0, 60.0)), Size::new(700.0, 500.0));
    let options = WindowOptions::default()
        .position(Some(Point::new(50.0, 60.0)))
        .mode(WindowMode::Maximized)
        .normal_geometry(Some(normal));
    let window: Rc<dyn HostWindow> = platform
        .open_window(
            options,
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("window");
    let state = window.state();
    assert_eq!(state.mode, WindowMode::Maximized);
    assert_eq!(state.revision.get(), 0, "the first snapshot is revision 0");
    assert_eq!(state.normal_geometry, Some(normal));
}

/// Record the snapshot read inside every `state_changed` of `window`'s handler
/// from here on.
fn listen(window: &Opened) -> Arc<std::sync::Mutex<Vec<WindowState>>> {
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = Rc::downgrade(&window.window);
    let log = Arc::clone(&heard);
    window.handler.react_to_state_change(move || {
        if let Some(window) = state.upgrade() {
            let snapshot = window.state();
            log.lock().expect("log").push(snapshot);
        }
    });
    heard
}

/// Each window fact (ADR-0254 §1) arrives only in the snapshot: its change
/// publishes one revision, announced by exactly one `WindowHandler::state_changed` that
/// reads it; the same report again publishes nothing; a report that is not a
/// drawable area publishes nothing.
fn window_facts_arrive_only_in_the_snapshot() {
    use flui_foundation::geometry::{EdgeInsets, WindowMetrics};
    use flui_platform::{WindowAppearance, WindowExecutionState};

    let platform = HeadlessPlatform::new().with_displays(vec![
        DisplaySpec::new("Left", 1920, 1080),
        DisplaySpec::new("Right", 2560, 1440),
    ]);
    let right = platform.display_snapshot().expect("displays").displays()[1].id;
    let window = open(&platform);
    let mock = mock(&window).clone();
    let insets = flui_platform::WindowInsets::ZERO
        .try_with_safe_area(EdgeInsets::new(24.0, 0.0, 48.0, 0.0))
        .expect("a finite safe area")
        .with_keyboard(
            flui_platform::KeyboardInset::try_at_rest(EdgeInsets::new(0.0, 0.0, 300.0, 0.0))
                .expect("a finite keyboard"),
        );
    type Report = Box<dyn Fn(&HeadlessHostWindow)>;
    type Holds = Box<dyn Fn(&WindowState) -> bool>;
    let rows: Vec<(&str, Report, Holds)> = vec![
        (
            "focus",
            Box::new(|mock: &HeadlessHostWindow| mock.simulate_focus(false)),
            Box::new(|state: &WindowState| !state.focused && !state.active),
        ),
        (
            "occlusion",
            Box::new(|mock: &HeadlessHostWindow| mock.simulate_occlusion(true)),
            Box::new(|state: &WindowState| state.visible && state.occluded && !state.is_seen()),
        ),
        (
            "hover",
            Box::new(|mock: &HeadlessHostWindow| mock.simulate_hover(true)),
            Box::new(|state: &WindowState| state.hovered),
        ),
        (
            "appearance",
            Box::new(|mock: &HeadlessHostWindow| mock.simulate_appearance(WindowAppearance::Dark)),
            Box::new(|state: &WindowState| state.appearance == WindowAppearance::Dark),
        ),
        (
            "insets",
            Box::new(move |mock: &HeadlessHostWindow| mock.simulate_insets(insets)),
            Box::new(move |state: &WindowState| state.insets == insets),
        ),
        (
            "execution",
            Box::new(|mock: &HeadlessHostWindow| {
                mock.simulate_execution_state(WindowExecutionState::Suspended);
            }),
            Box::new(|state: &WindowState| state.execution == WindowExecutionState::Suspended),
        ),
        (
            "display",
            Box::new(move |mock: &HeadlessHostWindow| {
                mock.simulate_move_to(right).expect("a held display");
            }),
            Box::new(move |state: &WindowState| state.display == Some(right)),
        ),
        (
            "scale",
            Box::new(|mock: &HeadlessHostWindow| mock.simulate_scale_factor_change(2.0)),
            Box::new(|state: &WindowState| {
                state.metrics.map(|metrics| metrics.scale().get()) == Some(2.0)
                    && state.metrics.map(WindowMetrics::physical) == Some(Size::new(1600, 1200))
            }),
        ),
        (
            "size",
            Box::new(|mock: &HeadlessHostWindow| mock.simulate_resize(1000.0, 500.0)),
            Box::new(|state: &WindowState| {
                state.metrics.map(WindowMetrics::physical) == Some(Size::new(2000, 1000))
            }),
        ),
    ];
    let mut failures = Vec::new();
    for (name, report, holds) in &rows {
        let heard = listen(&window);
        let before = window.state();
        report(&mock);
        // What the report itself announced, before any read could sample
        // the window again.
        let log = heard.lock().expect("log").clone();
        let after = window.state();
        if !holds(&after) || after.revision.get() != before.revision.get() + 1 {
            failures.push(format!("{name}: the snapshot is {after:?}"));
        }
        if log != [after.clone()] {
            failures.push(format!("{name}: heard {log:?}"));
        }
        heard.lock().expect("log").clear();
        report(&mock);
        if window.state() != after || !heard.lock().expect("log").is_empty() {
            failures.push(format!("{name}: the same report published again"));
        }
    }
    // A scale that is not finite is read by the hosts' one rule, as the base
    // ratio: the window keeps a scale and still draws.
    mock.simulate_scale_factor_change(f64::NAN);
    if window.state().metrics.map(|metrics| metrics.scale().get()) != Some(1.0) {
        failures.push("a NaN scale was not read as the base ratio".to_owned());
    }
    // Nothing drawable: a zero size.
    let heard = listen(&window);
    mock.simulate_scale_factor_change(2.0);
    heard.lock().expect("log").clear();
    let before_empty = window.state();
    mock.simulate_resize(0.0, 0.0);
    let empty = window.state();
    if empty.metrics != before_empty.metrics {
        failures.push(format!("an empty area reached the metrics: {empty:?}"));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn window_state_contract() {
    crate::run_table(
        "window_state_contract",
        &[
            (
                "app_title_bar_controls_survive_open_commands_and_fullscreen",
                app_title_bar_controls_survive_open_commands_and_fullscreen as fn(),
            ),
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
                "a_size_past_the_largest_surface_is_held_at_it",
                a_size_past_the_largest_surface_is_held_at_it as fn(),
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
                "options_that_describe_no_window_open_none",
                options_that_describe_no_window_open_none as fn(),
            ),
            (
                "a_bad_report_keeps_the_geometry_and_publishes_the_rest",
                a_bad_report_keeps_the_geometry_and_publishes_the_rest as fn(),
            ),
            (
                "a_closed_window_refuses_commands",
                a_closed_window_refuses_commands as fn(),
            ),
            (
                "a_command_admitted_before_a_capability_change_is_superseded",
                a_command_admitted_before_a_capability_change_is_superseded as fn(),
            ),
            (
                "a_command_admitted_for_another_window_is_superseded",
                a_command_admitted_for_another_window_is_superseded as fn(),
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
            (
                "window_facts_arrive_only_in_the_snapshot",
                window_facts_arrive_only_in_the_snapshot as fn(),
            ),
        ],
    );
}

/// Limits with these bounds.
fn limits(
    min: Option<flui_foundation::geometry::Size<f64>>,
    max: Option<flui_foundation::geometry::Size<f64>>,
) -> flui_platform::SizeConstraints {
    flui_platform::SizeConstraints::try_new(min, max).expect("finite limits")
}

/// A rectangle with this position and size.
fn geometry(
    position: Option<flui_foundation::geometry::Point<f64>>,
    size: flui_foundation::geometry::Size<f64>,
) -> flui_platform::WindowGeometry {
    flui_platform::WindowGeometry::try_new(position, size).expect("a finite rectangle")
}
