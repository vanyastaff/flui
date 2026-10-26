//! `WindowCommand::validate`, `WindowCommand::admit` and `WindowState::successor`:
//! the pure rules every backend and every queue in front of one relies on.

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    SizeConstraints, Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand,
    WindowCommandError, WindowGeometry, WindowMode, WindowRevision, WindowRevisionExhausted,
    WindowState,
};

fn size(width: f64, height: f64) -> Size<f64> {
    Size::new(width, height)
}

fn constraints(min: Option<Size<f64>>, max: Option<Size<f64>>) -> WindowCommand {
    WindowCommand::SetConstraints(SizeConstraints::new(min, max))
}

fn invalid(command: &WindowCommand) {
    assert_eq!(
        command.validate(),
        Err(WindowCommandError::InvalidGeometry),
        "{command:?} must be refused before any OS call"
    );
}

fn valid(command: &WindowCommand) {
    assert_eq!(command.validate(), Ok(()), "{command:?} describes a window");
}

fn a_window_opens_with_balanced_input_unless_it_asks_for_low_latency() {
    use flui_platform::{InputLatency, WindowOptions};
    assert_eq!(
        WindowOptions::default().input_latency,
        InputLatency::Balanced
    );
    assert_eq!(
        WindowOptions::default()
            .with_input_latency(InputLatency::Low)
            .input_latency,
        InputLatency::Low
    );
}

fn position_must_be_finite() {
    invalid(&WindowCommand::SetPosition(Point::new(f64::NAN, 0.0)));
    invalid(&WindowCommand::SetPosition(Point::new(0.0, f64::INFINITY)));
    invalid(&WindowCommand::SetPosition(Point::new(
        f64::NEG_INFINITY,
        1.0,
    )));
    valid(&WindowCommand::SetPosition(Point::new(-1920.0, -40.0)));
}

fn size_must_be_finite_and_positive() {
    invalid(&WindowCommand::SetSize(size(0.0, 100.0)));
    invalid(&WindowCommand::SetSize(size(100.0, -1.0)));
    invalid(&WindowCommand::SetSize(size(f64::NAN, 100.0)));
    invalid(&WindowCommand::SetSize(size(100.0, f64::INFINITY)));
    valid(&WindowCommand::SetSize(size(1.0, 1.0)));
}

fn constraints_with_a_bad_bound_are_refused() {
    invalid(&constraints(Some(size(0.0, 10.0)), None));
    invalid(&constraints(None, Some(size(10.0, f64::NAN))));
    invalid(&constraints(
        Some(size(-5.0, 10.0)),
        Some(size(100.0, 100.0)),
    ));
}

fn minimum_above_maximum_in_either_dimension_is_refused() {
    invalid(&constraints(
        Some(size(200.0, 50.0)),
        Some(size(100.0, 100.0)),
    ));
    invalid(&constraints(
        Some(size(50.0, 200.0)),
        Some(size(100.0, 100.0)),
    ));
    valid(&constraints(
        Some(size(100.0, 100.0)),
        Some(size(100.0, 100.0)),
    ));
    valid(&constraints(
        Some(size(50.0, 50.0)),
        Some(size(100.0, 100.0)),
    ));
}

fn open_or_absent_constraints_are_valid() {
    valid(&constraints(None, None));
    valid(&constraints(Some(size(50.0, 50.0)), None));
    valid(&constraints(None, Some(size(500.0, 500.0))));
}

fn commands_without_numbers_are_always_valid() {
    for command in [
        WindowCommand::SetTitle(String::new()),
        WindowCommand::SetResizable(false),
        WindowCommand::SetDecorated(false),
        WindowCommand::SetMode(WindowMode::Fullscreen),
        WindowCommand::Show,
        WindowCommand::Hide,
        WindowCommand::Activate,
    ] {
        valid(&command);
    }
}

type Capability = fn(&mut WindowCapabilities) -> &mut bool;

fn admit_refuses_exactly_when_the_capability_is_false() {
    let rows: [(WindowCommand, Capability); 12] = [
        (WindowCommand::SetTitle("t".into()), |c| &mut c.sets_title),
        (WindowCommand::SetPosition(Point::new(1.0, 1.0)), |c| {
            &mut c.sets_position
        }),
        (WindowCommand::SetSize(size(10.0, 10.0)), |c| {
            &mut c.sets_size
        }),
        (constraints(None, None), |c| &mut c.sets_constraints),
        (WindowCommand::SetResizable(true), |c| &mut c.sets_resizable),
        (WindowCommand::SetDecorated(true), |c| &mut c.sets_decorated),
        (WindowCommand::SetMode(WindowMode::Minimized), |c| {
            &mut c.minimizes
        }),
        (WindowCommand::SetMode(WindowMode::Maximized), |c| {
            &mut c.maximizes
        }),
        (WindowCommand::SetMode(WindowMode::Fullscreen), |c| {
            &mut c.enters_fullscreen
        }),
        (WindowCommand::Show, |c| &mut c.shows),
        (WindowCommand::Hide, |c| &mut c.hides),
        (WindowCommand::Activate, |c| &mut c.activates),
    ];
    for (command, capability) in rows {
        let mut all = WindowCapabilities::ALL;
        assert_eq!(command.admit(&all), Ok(()), "{command:?}");
        *capability(&mut all) = false;
        assert_eq!(
            command.admit(&all),
            Err(WindowCommandError::Unsupported(Unsupported::new(
                UnsupportedReason::NotOnThisPlatform
            ))),
            "{command:?} without its capability"
        );
        assert_eq!(
            command.admit(&WindowCapabilities::NONE),
            Err(WindowCommandError::Unsupported(Unsupported::new(
                UnsupportedReason::NotOnThisPlatform
            ))),
            "{command:?} on a window that honours nothing"
        );
    }
}

fn restoring_normal_needs_no_capability() {
    assert_eq!(
        WindowCommand::SetMode(WindowMode::Normal).admit(&WindowCapabilities::NONE),
        Ok(())
    );
}

fn admit_checks_numbers_before_capabilities() {
    let bad = WindowCommand::SetSize(size(-1.0, 10.0));
    assert_eq!(
        bad.admit(&WindowCapabilities::NONE),
        Err(WindowCommandError::InvalidGeometry)
    );
}

fn base_state() -> WindowState {
    WindowState::new(
        WindowGeometry::new(Some(Point::new(10.0, 20.0)), size(800.0, 600.0)),
        size(784.0, 561.0),
    )
}

fn equal_sample_keeps_the_revision() {
    let published = base_state()
        .successor(base_state().with_title(Some("a".into())))
        .expect("a change publishes");
    assert_eq!(published.revision.get(), 1);
    let again = published
        .successor(base_state().with_title(Some("a".into())))
        .expect("an equal sample publishes nothing");
    assert_eq!(again, published);
    assert_eq!(again.revision, published.revision);
}

fn a_difference_advances_the_revision_by_one() {
    let first = base_state();
    let second = first
        .successor(base_state().with_mode(WindowMode::Maximized))
        .expect("a change publishes");
    assert_eq!(second.revision.get(), first.revision.get() + 1);
    assert_eq!(second.mode, WindowMode::Maximized);
}

fn the_revision_a_sample_carries_is_ignored() {
    let mut sample = base_state();
    sample.revision = WindowRevision::LAST;
    let next = base_state()
        .successor(sample)
        .expect("equal but for the revision");
    assert_eq!(next.revision, WindowRevision::ZERO);
}

fn the_last_revision_refuses_instead_of_wrapping() {
    let mut at_last = base_state();
    at_last.revision = WindowRevision::LAST;
    assert_eq!(
        at_last.successor(base_state().with_visible(false)),
        Err(WindowRevisionExhausted)
    );
    assert_eq!(
        at_last.successor(base_state().with_visible(false)),
        Err(WindowRevisionExhausted),
        "exhaustion is permanent"
    );
    let unchanged = at_last
        .successor(base_state())
        .expect("an equal sample needs no new revision");
    assert_eq!(unchanged.revision, WindowRevision::LAST);
}

#[test]
fn window_command_validate() {
    crate::run_table(
        "window_command_validate",
        &[
            (
                "a_window_opens_with_balanced_input_unless_it_asks_for_low_latency",
                a_window_opens_with_balanced_input_unless_it_asks_for_low_latency,
            ),
            ("position_must_be_finite", position_must_be_finite),
            (
                "size_must_be_finite_and_positive",
                size_must_be_finite_and_positive,
            ),
            (
                "constraints_with_a_bad_bound_are_refused",
                constraints_with_a_bad_bound_are_refused,
            ),
            (
                "minimum_above_maximum_in_either_dimension_is_refused",
                minimum_above_maximum_in_either_dimension_is_refused,
            ),
            (
                "open_or_absent_constraints_are_valid",
                open_or_absent_constraints_are_valid,
            ),
            (
                "commands_without_numbers_are_always_valid",
                commands_without_numbers_are_always_valid,
            ),
            (
                "admit_refuses_exactly_when_the_capability_is_false",
                admit_refuses_exactly_when_the_capability_is_false,
            ),
            (
                "restoring_normal_needs_no_capability",
                restoring_normal_needs_no_capability,
            ),
            (
                "admit_checks_numbers_before_capabilities",
                admit_checks_numbers_before_capabilities,
            ),
            (
                "equal_sample_keeps_the_revision",
                equal_sample_keeps_the_revision,
            ),
            (
                "a_difference_advances_the_revision_by_one",
                a_difference_advances_the_revision_by_one,
            ),
            (
                "the_revision_a_sample_carries_is_ignored",
                the_revision_a_sample_carries_is_ignored,
            ),
            (
                "the_last_revision_refuses_instead_of_wrapping",
                the_last_revision_refuses_instead_of_wrapping,
            ),
        ],
    );
}
