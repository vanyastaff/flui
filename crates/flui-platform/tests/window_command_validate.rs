//! `WindowCommand::validate`, `WindowCommand::admit` and `WindowState::successor`:
//! the pure rules every backend and every queue in front of one relies on.

use flui_foundation::geometry::{EdgeInsets, Point, Size, WindowMetrics};
use flui_platform::{
    AdmissionError, AdmittedCommand, ApplyError, Availability, CapabilitiesOwner, SizeConstraints,
    Unsupported, UnsupportedReason, WindowAppearance, WindowCapabilities, WindowCommand,
    WindowExecutionState, WindowGeometry, WindowMaterial, WindowMode, WindowRevision,
    WindowRevisionExhausted, WindowState,
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
        Err(AdmissionError::InvalidGeometry),
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

type Capability = fn(&mut WindowCapabilities) -> &mut Availability;

/// What admitting `command` against `capabilities` answers, the token aside.
fn admits(
    command: &WindowCommand,
    capabilities: &WindowCapabilities,
) -> Result<(), AdmissionError> {
    command.clone().admit(capabilities).map(|_admitted| ())
}

fn admit_refuses_exactly_when_the_capability_is_unavailable() {
    let rows: [(WindowCommand, Capability); 14] = [
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
        (WindowCommand::Close, |c| &mut c.closes),
        (WindowCommand::SetMaterial(WindowMaterial::System), |c| {
            &mut c.material
        }),
    ];
    for (command, capability) in rows {
        let mut all = WindowCapabilities::ALL;
        assert_eq!(admits(&command, &all), Ok(()), "{command:?}");
        *capability(&mut all) = Availability::Emulated;
        assert_eq!(admits(&command, &all), Ok(()), "{command:?} emulated");
        *capability(&mut all) = Availability::Unavailable;
        assert_eq!(
            admits(&command, &all),
            Err(AdmissionError::Unsupported(Unsupported::new(
                UnsupportedReason::NotOnThisPlatform
            ))),
            "{command:?} without its capability"
        );
        assert_eq!(
            admits(&command, &WindowCapabilities::NONE),
            Err(AdmissionError::Unsupported(Unsupported::new(
                UnsupportedReason::NotOnThisPlatform
            ))),
            "{command:?} on a window that honours nothing"
        );
        // A capability this device lacks, where the platform has it.
        let on_this_device = WindowCapabilities::ALL.unavailable_on_this_device(&command);
        assert_eq!(
            on_this_device.availability(&command),
            Availability::Unavailable,
            "{command:?} taken away on this device"
        );
        assert_eq!(
            admits(&command, &on_this_device),
            Err(AdmissionError::Unsupported(Unsupported::new(
                UnsupportedReason::NotOnThisDevice
            ))),
            "{command:?} without its capability on this device"
        );
        assert_ne!(on_this_device, WindowCapabilities::ALL, "{command:?}");
    }
}

fn restoring_normal_needs_no_capability() {
    assert_eq!(
        admits(
            &WindowCommand::SetMode(WindowMode::Normal),
            &WindowCapabilities::NONE
        ),
        Ok(())
    );
}

fn admit_checks_numbers_before_capabilities() {
    let bad = WindowCommand::SetSize(size(-1.0, 10.0));
    assert_eq!(
        admits(&bad, &WindowCapabilities::NONE),
        Err(AdmissionError::InvalidGeometry)
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

fn metrics(width: i32, height: i32, scale: f64) -> WindowMetrics {
    WindowMetrics::new(Size::new(width, height), scale).expect("a drawable area")
}

/// Every fact the snapshot holds is a difference the revision moves for.
fn each_window_fact_advances_the_revision() {
    let mut honours_title = WindowCapabilities::NONE;
    honours_title.sets_title = Availability::Native;
    let facts: [(&str, WindowState); 9] = [
        ("focus", base_state().with_focused(true)),
        ("visibility", base_state().with_visible(false)),
        ("hover", base_state().with_hovered(true)),
        ("activation", base_state().with_active(true)),
        (
            "appearance",
            base_state().with_appearance(WindowAppearance::Dark),
        ),
        (
            "safe area",
            base_state().with_safe_area(EdgeInsets::new(24.0, 0.0, 48.0, 0.0)),
        ),
        (
            "execution",
            base_state().with_execution(WindowExecutionState::Suspended),
        ),
        (
            "metrics",
            base_state().with_metrics(Some(metrics(1568, 1122, 2.0))),
        ),
        (
            "capabilities",
            base_state().with_capabilities(honours_title),
        ),
    ];
    for (fact, sample) in facts {
        let next = base_state()
            .successor(sample.clone())
            .expect("a change publishes");
        assert_eq!(next.revision.get(), 1, "{fact}");
        assert_eq!(
            next.successor(sample).expect("equal").revision.get(),
            1,
            "{fact}: the same report again publishes nothing"
        );
    }
}

/// A report that is not a drawable area keeps the metrics before it, and
/// publishes nothing when nothing else changed.
fn a_report_that_is_not_drawable_keeps_the_metrics() {
    let drawn = base_state()
        .successor(base_state().with_metrics(Some(metrics(800, 600, 1.0))))
        .expect("first metrics");
    assert_eq!(drawn.revision.get(), 1);
    let minimized = drawn
        .successor(base_state().with_metrics(None))
        .expect("nothing to publish");
    assert_eq!(minimized, drawn, "no revision for an empty report");
    let with_mode = drawn
        .successor(
            base_state()
                .with_metrics(None)
                .with_mode(WindowMode::Minimized),
        )
        .expect("the mode changes");
    assert_eq!(with_mode.revision.get(), 2);
    assert_eq!(with_mode.metrics, Some(metrics(800, 600, 1.0)));
    assert_eq!(
        WindowState::new(WindowGeometry::new(None, size(0.0, 0.0)), size(0.0, 0.0)).metrics,
        None,
        "nothing drawable reported yet"
    );
}

/// The capabilities' revision is the snapshot revision at which what the
/// window honours last changed: a change of another fact leaves it, a change
/// of a capability moves it to the new snapshot's, and a sample's own is
/// ignored.
fn the_capabilities_revision_moves_only_with_the_capabilities() {
    let mut honours_title = WindowCapabilities::ALL;
    honours_title.sets_title = Availability::Unavailable;
    let first = base_state()
        .successor(base_state().with_capabilities(WindowCapabilities::ALL))
        .expect("capabilities change");
    assert_eq!(first.capabilities.revision(), first.revision);
    let focused = first
        .successor(
            base_state()
                .with_capabilities(WindowCapabilities::ALL)
                .with_focused(true),
        )
        .expect("focus changes");
    assert_eq!(focused.revision.get(), first.revision.get() + 1);
    assert_eq!(
        focused.capabilities.revision(),
        first.revision,
        "focus is not a capability"
    );
    let revised = focused
        .successor(
            base_state()
                .with_capabilities(honours_title)
                .with_focused(true),
        )
        .expect("a capability changes");
    assert_eq!(revised.capabilities.revision(), revised.revision);
    let again = revised
        .successor(
            base_state()
                .with_capabilities(honours_title)
                .with_focused(true),
        )
        .expect("an equal sample");
    assert_eq!(
        again, revised,
        "the sample's own capabilities revision is ignored"
    );
}

fn owner(number: u64) -> CapabilitiesOwner {
    CapabilitiesOwner::new(std::num::NonZeroU64::new(number).expect("a non-zero owner"))
}

/// What opening `admitted` under `capabilities` answers, as the command or
/// the fact that it was superseded.
fn opens(admitted: AdmittedCommand, capabilities: &WindowCapabilities) -> Result<(), ()> {
    match admitted.against(capabilities) {
        Ok(_) => Ok(()),
        Err(ApplyError::Superseded { .. }) => Err(()),
        Err(other) => panic!("`against` answers only Superseded, not {other:?}"),
    }
}

/// A token opens only under the capabilities it was admitted against: the
/// same window at the same revision lets the command through; a moved
/// revision answers `Superseded` and gives the command back, even for a
/// command the new capabilities still honour; admitting it again against the
/// new capabilities either opens or is refused by `admit`.
fn a_token_opens_only_under_the_capabilities_it_was_admitted_against() {
    let mut published = base_state().with_capabilities(WindowCapabilities::ALL);
    published.capabilities = published.capabilities.owned_by(owner(1));
    let title = WindowCommand::SetTitle("t".into());
    let admitted = title
        .clone()
        .admit(&published.capabilities)
        .expect("the window honours titles");
    assert_eq!(admitted.command(), &title);
    assert_eq!(
        admitted.against(&published.capabilities),
        Ok(title.clone()),
        "under the capabilities it was admitted against"
    );

    let mut without_fullscreen = WindowCapabilities::ALL;
    without_fullscreen.enters_fullscreen = Availability::Unavailable;
    let revised = published
        .successor(base_state().with_capabilities(without_fullscreen))
        .expect("a capability changes");
    let stale = title
        .clone()
        .admit(&published.capabilities)
        .expect("admitted before the change");
    assert_eq!(
        stale.against(&revised.capabilities),
        Err(ApplyError::Superseded {
            command: title.clone()
        }),
        "a title is still honoured, but it was checked against other capabilities"
    );
    assert_eq!(
        title
            .clone()
            .admit(&revised.capabilities)
            .expect("admitted again")
            .against(&revised.capabilities),
        Ok(title)
    );
    let fullscreen = WindowCommand::SetMode(WindowMode::Fullscreen);
    assert_eq!(
        opens(
            fullscreen
                .clone()
                .admit(&published.capabilities)
                .expect("admitted before the change"),
            &revised.capabilities
        ),
        Err(())
    );
    assert!(
        matches!(
            fullscreen.admit(&revised.capabilities),
            Err(AdmissionError::Unsupported(_))
        ),
        "admitted again, the command the window lost is refused"
    );
}

/// A token is bound to its window: at the same revision, another window's
/// capabilities, or capabilities built by hand, do not open it, and a token
/// admitted against hand-built capabilities opens on no window.
fn a_token_opens_only_on_the_window_it_was_admitted_for() {
    let first = WindowCapabilities::ALL.owned_by(owner(1));
    let second = WindowCapabilities::ALL.owned_by(owner(2));
    assert_eq!(first.revision(), second.revision(), "the same revision");
    let admit = |capabilities: &WindowCapabilities| {
        WindowCommand::Hide
            .admit(capabilities)
            .expect("both honour hiding")
    };
    assert_eq!(opens(admit(&first), &first), Ok(()));
    assert_eq!(opens(admit(&first), &second), Err(()), "another window");
    assert_eq!(
        opens(admit(&WindowCapabilities::ALL), &first),
        Err(()),
        "capabilities built by hand belong to no window"
    );
    assert_eq!(
        opens(admit(&first), &WindowCapabilities::ALL),
        Err(()),
        "nor does a window's token open under them"
    );
    assert_eq!(first, second, "equal in what they honour all the same");
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
                "admit_refuses_exactly_when_the_capability_is_unavailable",
                admit_refuses_exactly_when_the_capability_is_unavailable,
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
            (
                "each_window_fact_advances_the_revision",
                each_window_fact_advances_the_revision,
            ),
            (
                "a_report_that_is_not_drawable_keeps_the_metrics",
                a_report_that_is_not_drawable_keeps_the_metrics,
            ),
            (
                "the_capabilities_revision_moves_only_with_the_capabilities",
                the_capabilities_revision_moves_only_with_the_capabilities,
            ),
            (
                "a_token_opens_only_under_the_capabilities_it_was_admitted_against",
                a_token_opens_only_under_the_capabilities_it_was_admitted_against,
            ),
            (
                "a_token_opens_only_on_the_window_it_was_admitted_for",
                a_token_opens_only_on_the_window_it_was_admitted_for,
            ),
        ],
    );
}
