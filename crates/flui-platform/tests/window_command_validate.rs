//! `WindowCommand::validate`, `WindowCommand::admit` and `WindowState::successor`:
//! the pure rules every backend and every queue in front of one relies on.

use flui_foundation::geometry::{EdgeInsets, Point, Size, WindowMetrics};
use flui_platform::{
    AdmissionError, AdmittedCommand, AppTitleBar, ApplyError, CapabilityAbsence, CapabilityParts,
    ControlsOffset, InvalidControlsOffset, InvalidGeometry, KeyboardInset, SizeConstraints,
    SystemControls, Unsupported, UnsupportedReason, WindowAppearance, WindowCapabilities,
    WindowCapability, WindowCommand, WindowExecutionState, WindowExtent, WindowFrame,
    WindowGeometry, WindowInsets, WindowMaterial, WindowMode, WindowOption, WindowOptions,
    WindowRevision, WindowRevisionExhausted, WindowState,
};

fn size(width: f64, height: f64) -> Size<f64> {
    Size::new(width, height)
}

fn constraints(min: Option<Size<f64>>, max: Option<Size<f64>>) -> WindowCommand {
    WindowCommand::SetConstraints(SizeConstraints::try_new(min, max).expect("finite limits"))
}

/// `command` is refused before any OS call, and the refusal names the numbers.
fn invalid(command: &WindowCommand, refused: InvalidGeometry) {
    assert_eq!(
        command.validate(),
        Err(AdmissionError::InvalidGeometry(refused)),
        "{command:?} must be refused before any OS call"
    );
}

fn extent(width: f64, height: f64) -> WindowExtent {
    WindowExtent::try_new(size(width, height)).expect("an extent")
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
            .input_latency(InputLatency::Low)
            .input_latency,
        InputLatency::Low
    );
}

fn position_must_be_finite() {
    for (x, y) in [
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 1.0),
    ] {
        invalid(
            &WindowCommand::SetPosition(Point::new(x, y)),
            InvalidGeometry::Position { x, y },
        );
    }
    valid(&WindowCommand::SetPosition(Point::new(-1920.0, -40.0)));
}

fn size_must_be_finite_and_positive() {
    invalid(
        &WindowCommand::SetSize(size(0.0, 100.0)),
        InvalidGeometry::Empty {
            width: 0.0,
            height: 100.0,
        },
    );
    for (width, height) in [(100.0, -1.0), (f64::NAN, 100.0), (100.0, f64::INFINITY)] {
        invalid(
            &WindowCommand::SetSize(size(width, height)),
            InvalidGeometry::Extent { width, height },
        );
    }
    valid(&WindowCommand::SetSize(size(1.0, 1.0)));
}

/// A limit with no area is refused at admission; one that is no extent at all
/// cannot be put in a command.
fn constraints_with_a_bad_bound_are_refused() {
    invalid(
        &constraints(Some(size(0.0, 10.0)), None),
        InvalidGeometry::Empty {
            width: 0.0,
            height: 10.0,
        },
    );
    invalid(
        &constraints(None, Some(size(10.0, 0.0))),
        InvalidGeometry::Empty {
            width: 10.0,
            height: 0.0,
        },
    );
    for bad in [
        size(10.0, f64::NAN),
        size(-5.0, 10.0),
        size(f64::INFINITY, 10.0),
    ] {
        assert!(
            SizeConstraints::try_new(None, Some(bad)).is_err(),
            "{bad:?}"
        );
        assert!(
            SizeConstraints::try_new(Some(bad), Some(size(100.0, 100.0))).is_err(),
            "{bad:?}"
        );
    }
}

fn minimum_above_maximum_in_either_dimension_is_refused() {
    for min in [extent(200.0, 50.0), extent(50.0, 200.0)] {
        let max = extent(100.0, 100.0);
        invalid(
            &WindowCommand::SetConstraints(SizeConstraints::new(Some(min), Some(max))),
            InvalidGeometry::Limits { min, max },
        );
    }
    valid(&constraints(
        Some(size(100.0, 100.0)),
        Some(size(100.0, 100.0)),
    ));
    valid(&constraints(
        Some(size(50.0, 50.0)),
        Some(size(100.0, 100.0)),
    ));
}

/// An opening asks for its size, limits and position as the commands do, and
/// is refused for the same numbers, the error naming the option: nothing a
/// command could not ask for reaches a host at open.
fn an_opening_is_refused_for_what_a_command_is_refused_for() {
    let refusal = |command: WindowCommand| match command.validate() {
        Ok(()) => None,
        Err(AdmissionError::InvalidGeometry(refused)) => Some(refused),
        Err(other) => panic!("{other:?} is no geometry refusal"),
    };
    let named = |options: &WindowOptions| {
        options
            .geometry()
            .err()
            .map(|refused| (refused.option, refused.refused))
    };
    let limits = [
        (
            Some(extent(800.0, 600.0)),
            Some(extent(400.0, 300.0)),
            WindowOption::Limits,
        ),
        (
            Some(extent(800.0, 600.0)),
            Some(extent(900.0, 300.0)),
            WindowOption::Limits,
        ),
        (Some(WindowExtent::ZERO), None, WindowOption::MinSize),
        (None, Some(extent(0.0, 300.0)), WindowOption::MaxSize),
        (
            Some(extent(400.0, 300.0)),
            Some(extent(800.0, 600.0)),
            WindowOption::Limits,
        ),
        (None, None, WindowOption::Limits),
    ];
    for (min, max, option) in limits {
        let options = WindowOptions::default().min_size(min).max_size(max);
        let command = WindowCommand::SetConstraints(SizeConstraints::new(min, max));
        assert_eq!(
            named(&options),
            refusal(command).map(|refused| (option, refused)),
            "{min:?} {max:?}"
        );
    }
    for asked in [
        size(0.0, 600.0),
        size(f64::NAN, 600.0),
        size(800.0, -1.0),
        size(800.0, 600.0),
    ] {
        let options = WindowOptions::default().size(asked);
        assert_eq!(
            named(&options),
            refusal(WindowCommand::SetSize(asked)).map(|refused| (WindowOption::Size, refused)),
            "{asked:?}"
        );
    }
    for at in [
        Point::new(f64::NAN, 0.0),
        Point::new(0.0, f64::INFINITY),
        Point::new(-1920.0, 40.0),
    ] {
        let options = WindowOptions::default().position(Some(at));
        assert_eq!(
            named(&options),
            refusal(WindowCommand::SetPosition(at))
                .map(|refused| (WindowOption::Position, refused)),
            "{at:?}"
        );
    }
}

fn open_or_absent_constraints_are_valid() {
    valid(&constraints(None, None));
    valid(&constraints(Some(size(50.0, 50.0)), None));
    valid(&constraints(None, Some(size(500.0, 500.0))));
}

fn a_window_opens_with_the_system_frame() {
    assert_eq!(WindowOptions::default().frame, WindowFrame::System);
    assert!(WindowFrame::System.has_system_title_bar());
    assert!(!WindowFrame::AppTitleBar(AppTitleBar::new()).has_system_title_bar());
    assert!(!WindowFrame::None.has_system_title_bar());
    assert_eq!(
        AppTitleBar::new().system_controls(),
        SystemControls::Shown,
        "the system's own controls stay where the system puts them"
    );
}

fn a_controls_offset_is_finite_and_non_negative() {
    for (along, down) in [
        (0.0, 0.0),
        (0.0, 6.5),
        (12.0, 0.0),
        (12.0, 6.5),
        (f64::MAX, f64::MAX),
    ] {
        let offset = ControlsOffset::try_new(along, down).expect("a finite, non-negative offset");
        assert_eq!((offset.along(), offset.down()), (along, down));
    }
    for (along, down) in [
        (-1.0, 0.0),
        (0.0, -0.5),
        (f64::NAN, 0.0),
        (0.0, f64::NAN),
        (f64::INFINITY, 0.0),
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 0.0),
        (0.0, f64::NEG_INFINITY),
    ] {
        assert_eq!(
            ControlsOffset::try_new(along, down),
            Err(InvalidControlsOffset),
            "({along}, {down})"
        );
    }
}

fn a_window_that_cannot_take_the_app_title_bar_opens_with_the_system_frame() {
    let bar = WindowFrame::AppTitleBar(AppTitleBar::new());
    let mut takes_it = WindowCapabilities::NONE.into_parts();
    takes_it.app_title_bar = WindowCapability::Native;
    let takes_it = takes_it.into_capabilities();
    assert_eq!(bar.at_open(&takes_it), bar);
    assert_eq!(bar.at_open(&WindowCapabilities::NONE), WindowFrame::System);
    for frame in [WindowFrame::System, WindowFrame::None] {
        assert_eq!(frame.at_open(&WindowCapabilities::NONE), frame);
    }
}

fn commands_without_numbers_are_always_valid() {
    for command in [
        WindowCommand::SetTitle(String::new()),
        WindowCommand::SetResizable(false),
        WindowCommand::SetFrame(WindowFrame::None),
        WindowCommand::SetFrame(WindowFrame::AppTitleBar(AppTitleBar::new())),
        WindowCommand::SetMode(WindowMode::Fullscreen),
        WindowCommand::Show,
        WindowCommand::Hide,
        WindowCommand::Activate,
    ] {
        valid(&command);
    }
}

type Capability = fn(&mut CapabilityParts) -> &mut WindowCapability;

/// What admitting `command` against `capabilities` answers, the token aside.
fn admits(
    command: &WindowCommand,
    capabilities: &WindowCapabilities,
) -> Result<(), AdmissionError> {
    command.clone().admit(capabilities).map(|_admitted| ())
}

fn admit_refuses_exactly_when_the_capability_is_unavailable() {
    let rows: [(WindowCommand, Capability); 16] = [
        (WindowCommand::SetTitle("t".into()), |c| &mut c.sets_title),
        (WindowCommand::SetPosition(Point::new(1.0, 1.0)), |c| {
            &mut c.sets_position
        }),
        (WindowCommand::SetSize(size(10.0, 10.0)), |c| {
            &mut c.sets_size
        }),
        (constraints(None, None), |c| &mut c.sets_constraints),
        (WindowCommand::SetResizable(true), |c| &mut c.sets_resizable),
        (WindowCommand::SetFrame(WindowFrame::System), |c| {
            &mut c.sets_frame
        }),
        (WindowCommand::SetFrame(WindowFrame::None), |c| {
            &mut c.sets_frame
        }),
        (
            WindowCommand::SetFrame(WindowFrame::AppTitleBar(AppTitleBar::new())),
            |c| &mut c.app_title_bar,
        ),
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
        let mut all = WindowCapabilities::ALL.into_parts();
        assert_eq!(
            admits(&command, &all.into_capabilities()),
            Ok(()),
            "{command:?}"
        );
        *capability(&mut all) = WindowCapability::Emulated;
        assert_eq!(
            admits(&command, &all.into_capabilities()),
            Ok(()),
            "{command:?} emulated"
        );
        *capability(&mut all) = WindowCapability::NOT_ON_THIS_PLATFORM;
        assert_eq!(
            admits(&command, &all.into_capabilities()),
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
        // A capability this device lacks where the platform has it, and one
        // the operating system has that the host does not implement yet:
        // admission answers the reason the host marked, not the platform's.
        for (absence, reason) in [
            (
                CapabilityAbsence::NOT_ON_THIS_DEVICE,
                UnsupportedReason::NotOnThisDevice,
            ),
            (
                CapabilityAbsence::NOT_YET_ON_THIS_HOST,
                UnsupportedReason::NotYetOnThisHost,
            ),
        ] {
            let marked = WindowCapabilities::ALL.unavailable(&command, absence);
            assert_eq!(
                marked.capability(&command),
                WindowCapability::Absent(absence),
                "{command:?} taken away, {absence:?}"
            );
            assert_eq!(
                *capability(&mut marked.into_parts()),
                WindowCapability::Absent(absence),
                "{command:?} is admitted against its own field"
            );
            assert!(!marked.permits(&command), "{command:?}");
            assert_eq!(
                admits(&command, &marked),
                Err(AdmissionError::Unsupported(Unsupported::new(reason))),
                "{command:?} without its capability, {absence:?}"
            );
            assert_ne!(marked, WindowCapabilities::ALL, "{command:?}");
        }
        assert_ne!(
            WindowCapabilities::ALL.unavailable(&command, CapabilityAbsence::NOT_ON_THIS_DEVICE),
            WindowCapabilities::ALL.unavailable(&command, CapabilityAbsence::NOT_YET_ON_THIS_HOST),
            "{command:?}: the absence is part of what the window honours"
        );
    }
}

/// A host that makes a capability available again publishes the same
/// capabilities as one that never marked it absent, and the revision does not
/// move: the absence went with the value that held it.
fn a_capability_restored_is_the_one_never_taken_away() {
    let material = WindowCommand::SetMaterial(WindowMaterial::System);
    let mut restored = WindowCapabilities::ALL
        .unavailable(&material, CapabilityAbsence::NOT_YET_ON_THIS_HOST)
        .into_parts();
    restored.material = WindowCapability::Native;
    let restored = restored.into_capabilities();
    assert_eq!(restored, WindowCapabilities::ALL);
    assert_eq!(
        hash_of(&restored),
        hash_of(&WindowCapabilities::ALL),
        "equal capabilities hash alike"
    );

    let published = base_state()
        .successor(base_state().with_capabilities(WindowCapabilities::ALL))
        .expect("capabilities change");
    let next = published
        .successor(base_state().with_capabilities(restored))
        .expect("an equal sample");
    assert_eq!(
        next.revision, published.revision,
        "a capability restored moved the revision"
    );
    assert_eq!(
        next.capabilities.revision(),
        published.capabilities.revision()
    );
}

fn hash_of(capabilities: &WindowCapabilities) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    BuildHasherDefault::<DefaultHasher>::default().hash_one(capabilities)
}

/// A capability carries only a reason no retry changes: converting refuses
/// every reason that asks to be tried again (and those about a declaration, a
/// window or a registration), and each one it accepts reads back as itself.
/// A capability stays `Unavailable` until the host publishes it again.
///
/// The list is every [`UnsupportedReason`]; a reason added to the enum fails
/// to compile in the conversion until it is classed there.
fn no_absence_is_retryable() {
    let every_reason = [
        UnsupportedReason::NotOnThisPlatform,
        UnsupportedReason::NotOnThisDevice,
        UnsupportedReason::NotYetOnThisHost,
        UnsupportedReason::NotDeclared { what: "a key" },
        UnsupportedReason::Unavailable,
        UnsupportedReason::NotRegistered,
        UnsupportedReason::NoWindow,
    ];
    let mut lasting = Vec::new();
    for reason in every_reason {
        match CapabilityAbsence::try_from(reason) {
            Ok(absence) => {
                assert_eq!(absence.reason(), reason);
                assert_eq!(UnsupportedReason::from(absence), reason);
                assert_eq!(Unsupported::from(absence), Unsupported::new(reason));
                assert!(
                    !Unsupported::from(absence).is_retryable(),
                    "{reason:?} asks to be retried"
                );
                lasting.push(absence);
            }
            Err(refused) => assert_eq!(refused, reason),
        }
        if Unsupported::new(reason).is_retryable() {
            assert_eq!(CapabilityAbsence::try_from(reason), Err(reason));
        }
    }
    assert_eq!(
        lasting,
        [
            CapabilityAbsence::NOT_ON_THIS_PLATFORM,
            CapabilityAbsence::NOT_ON_THIS_DEVICE,
            CapabilityAbsence::NOT_YET_ON_THIS_HOST,
        ]
    );
    assert_eq!(
        CapabilityAbsence::default(),
        CapabilityAbsence::NOT_ON_THIS_PLATFORM
    );
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
        Err(AdmissionError::InvalidGeometry(InvalidGeometry::Extent {
            width: -1.0,
            height: 10.0
        }))
    );
}

fn base_state() -> WindowState {
    WindowState::new(
        WindowGeometry::try_new(Some(Point::new(10.0, 20.0)), size(800.0, 600.0))
            .expect("a finite rectangle"),
        WindowExtent::try_new(size(784.0, 561.0)).expect("a finite extent"),
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
    WindowMetrics::try_new(Size::new(width, height), scale).expect("a drawable area")
}

/// Every fact the snapshot holds is a difference the revision moves for.
fn each_window_fact_advances_the_revision() {
    let mut honours_title = WindowCapabilities::NONE.into_parts();
    honours_title.sets_title = WindowCapability::Native;
    let honours_title = honours_title.into_capabilities();
    let facts: [(&str, WindowState); 12] = [
        ("focus", base_state().with_focused(true)),
        ("visibility", base_state().with_visible(false)),
        ("occlusion", base_state().with_occluded(true)),
        ("hover", base_state().with_hovered(true)),
        ("frame", base_state().with_frame(WindowFrame::None)),
        ("activation", base_state().with_active(true)),
        (
            "appearance",
            base_state().with_appearance(WindowAppearance::Dark),
        ),
        (
            "insets",
            base_state().with_insets(
                WindowInsets::ZERO
                    .try_with_safe_area(EdgeInsets::new(24.0, 0.0, 48.0, 0.0))
                    .expect("a finite safe area"),
            ),
        ),
        (
            "keyboard",
            base_state().with_insets(
                WindowInsets::ZERO.with_keyboard(
                    KeyboardInset::try_at_rest(EdgeInsets::new(0.0, 0.0, 300.0, 0.0))
                        .expect("a finite keyboard inset"),
                ),
            ),
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
        WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO).metrics,
        None,
        "nothing drawable reported yet"
    );
}

/// Shown, minimized and occluded are separate facts that `is_seen` reads
/// together: minimizing or covering a window leaves it shown, and an
/// occlusion is published only for a shown window in another mode than
/// minimized, whatever the window system said of the others.
fn shown_minimized_and_occluded_are_three_facts() {
    use WindowMode::{Maximized, Minimized, Normal};

    // (shown, mode, occlusion reported) → (occlusion published, seen)
    for (visible, mode, reported, occluded, seen) in [
        (true, Normal, false, false, true),
        (true, Maximized, false, false, true),
        (true, Normal, true, true, false),
        (true, Maximized, true, true, false),
        (true, Minimized, false, false, false),
        (true, Minimized, true, false, false),
        (false, Normal, false, false, false),
        (false, Normal, true, false, false),
    ] {
        let published = base_state()
            .successor(
                base_state()
                    .with_visible(visible)
                    .with_mode(mode)
                    .with_occluded(reported),
            )
            .expect("a revision");
        let case = format!("shown {visible}, {mode:?}, occlusion reported {reported}");
        assert_eq!(published.visible, visible, "{case}: shown is kept");
        assert_eq!(published.mode, mode, "{case}: the mode is kept");
        assert_eq!(published.occluded, occluded, "{case}: occlusion");
        assert_eq!(published.is_seen(), seen, "{case}: seen");
    }
    let minimized = base_state()
        .successor(base_state().with_mode(Minimized))
        .expect("minimized");
    assert_eq!(
        minimized.successor(base_state().with_mode(Minimized).with_occluded(true)),
        Ok(minimized),
        "an occlusion of a minimized window is no change"
    );
}

/// The capabilities' revision is the snapshot revision at which what the
/// window honours last changed: a change of another fact leaves it, a change
/// of a capability moves it to the new snapshot's, and a sample's own is
/// ignored.
fn the_capabilities_revision_moves_only_with_the_capabilities() {
    let mut honours_title = WindowCapabilities::ALL.into_parts();
    honours_title.sets_title = WindowCapability::NOT_ON_THIS_PLATFORM;
    let honours_title = honours_title.into_capabilities();
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

/// `capabilities` as a window's first snapshot publishes them.
fn as_published(capabilities: WindowCapabilities) -> WindowCapabilities {
    capabilities
        .publish()
        .expect("the process has publications left")
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

/// Capabilities taken apart, changed and put back together are no window's,
/// whatever window's they were: a command a window lacks, admitted against
/// its published capabilities with that capability switched on, opens not on
/// the window but `Superseded`. So does one admitted against the capabilities
/// marked absent once more, and publishing equal capabilities again is
/// another publication.
fn an_edit_of_published_capabilities_belongs_to_no_window() {
    let material = WindowCommand::SetMaterial(WindowMaterial::System);
    let published = as_published(WindowCapabilities::NONE);
    let mut switched_on = published.into_parts();
    switched_on.material = WindowCapability::Native;
    let switched_on = switched_on.into_capabilities();
    let admitted = material
        .clone()
        .admit(&switched_on)
        .expect("the edited copy honours the material");
    assert_eq!(
        admitted.against(&published),
        Err(ApplyError::Superseded {
            command: material.clone()
        }),
        "the window's own capabilities do not take the material"
    );

    let window = as_published(WindowCapabilities::ALL);
    let narrowed = window.unavailable(&material, CapabilityAbsence::NOT_YET_ON_THIS_HOST);
    let hide = |capabilities: &WindowCapabilities| {
        WindowCommand::Hide
            .admit(capabilities)
            .expect("every one honours hiding")
    };
    assert_eq!(opens(hide(&narrowed), &window), Err(()), "an edit");
    assert_eq!(
        opens(hide(&as_published(WindowCapabilities::ALL)), &window),
        Err(()),
        "published again, equal capabilities are another publication"
    );
    assert_eq!(
        opens(hide(&window), &window),
        Ok(()),
        "a copy is the publication"
    );
}

/// The successor a caller computes from a window's snapshot is a publication
/// of its own: a token admitted against it does not open under the window's
/// next capabilities, even when the window publishes its next capabilities at
/// that very revision.
fn a_successor_a_caller_computes_opens_nothing() {
    let material = WindowCommand::SetMaterial(WindowMaterial::System);
    let first = base_state().with_capabilities(as_published(WindowCapabilities::NONE));
    let computed = first
        .successor(base_state().with_capabilities(WindowCapabilities::ALL))
        .expect("a capability changes");
    let window = first
        .successor(base_state().with_capabilities(
            WindowCapabilities::ALL.unavailable(&material, CapabilityAbsence::NOT_ON_THIS_DEVICE),
        ))
        .expect("a capability changes");
    assert_eq!(
        computed.capabilities.revision(),
        window.capabilities.revision(),
        "the same revision"
    );
    assert_eq!(
        opens(
            material
                .admit(&computed.capabilities)
                .expect("the computed capabilities take the material"),
            &window.capabilities
        ),
        Err(()),
        "the window's capabilities do not take the material"
    );
    assert_eq!(
        opens(
            WindowCommand::Hide
                .admit(&computed.capabilities)
                .expect("the computed capabilities take hiding"),
            &window.capabilities
        ),
        Err(()),
        "nor anything else: the token is bound to the publication, not the revision"
    );
}

/// A token opens only under the publication it was admitted against: the
/// same publication lets the command through; the window's next one answers
/// `Superseded` and gives the command back, even for a command the new
/// capabilities still honour; admitting it again against the new
/// capabilities either opens or is refused by `admit`.
fn a_token_opens_only_under_the_capabilities_it_was_admitted_against() {
    let published = base_state().with_capabilities(as_published(WindowCapabilities::ALL));
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

    let mut without_fullscreen = WindowCapabilities::ALL.into_parts();
    without_fullscreen.enters_fullscreen = WindowCapability::NOT_ON_THIS_PLATFORM;
    let without_fullscreen = without_fullscreen.into_capabilities();
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
    let first = as_published(WindowCapabilities::ALL);
    let second = as_published(WindowCapabilities::ALL);
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
                "an_opening_is_refused_for_what_a_command_is_refused_for",
                an_opening_is_refused_for_what_a_command_is_refused_for,
            ),
            (
                "open_or_absent_constraints_are_valid",
                open_or_absent_constraints_are_valid,
            ),
            (
                "a_window_opens_with_the_system_frame",
                a_window_opens_with_the_system_frame,
            ),
            (
                "a_window_that_cannot_take_the_app_title_bar_opens_with_the_system_frame",
                a_window_that_cannot_take_the_app_title_bar_opens_with_the_system_frame,
            ),
            (
                "a_controls_offset_is_finite_and_non_negative",
                a_controls_offset_is_finite_and_non_negative,
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
                "shown_minimized_and_occluded_are_three_facts",
                shown_minimized_and_occluded_are_three_facts,
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
            (
                "an_edit_of_published_capabilities_belongs_to_no_window",
                an_edit_of_published_capabilities_belongs_to_no_window,
            ),
            (
                "a_successor_a_caller_computes_opens_nothing",
                a_successor_a_caller_computes_opens_nothing,
            ),
            (
                "a_capability_restored_is_the_one_never_taken_away",
                a_capability_restored_is_the_one_never_taken_away,
            ),
            ("no_absence_is_retryable", no_absence_is_retryable),
            (
                "the_presentation_report_answers_why_it_is_missing",
                the_presentation_report_answers_why_it_is_missing,
            ),
            (
                "a_refusal_answers_the_reason_the_capability_holds",
                a_refusal_answers_the_reason_the_capability_holds,
            ),
        ],
    );
}

/// A window that reports no presentation says why, the reason is part of what
/// it honours, and one left behind once it reports again is no difference.
fn the_presentation_report_answers_why_it_is_missing() {
    assert_eq!(
        WindowCapabilities::ALL.reports_presentation,
        WindowCapability::Native
    );
    assert_eq!(
        WindowCapabilities::NONE.reports_presentation,
        WindowCapability::NOT_ON_THIS_PLATFORM,
        "a window that reports nothing: the platform's"
    );
    let mut host = WindowCapabilities::ALL.into_parts();
    host.reports_presentation = WindowCapability::Absent(CapabilityAbsence::NOT_YET_ON_THIS_HOST);
    let host = host.into_capabilities();
    assert!(!host.reports_presentation.is_available());
    let mut device = WindowCapabilities::ALL.into_parts();
    device.reports_presentation = WindowCapability::Absent(CapabilityAbsence::NOT_ON_THIS_DEVICE);
    let device = device.into_capabilities();
    assert_ne!(
        host, device,
        "the absence is part of what the window honours"
    );
    let shown = flui_platform::FramePresented::new(flui_platform::EventTime::from_nanos(1));
    assert_eq!(
        shown.with_refresh(std::time::Duration::ZERO).refresh,
        None,
        "a zero refresh, a variable rate, is no interval"
    );
    let mut restored = host.into_parts();
    restored.reports_presentation = WindowCapability::Native;
    let restored = restored.into_capabilities();
    assert_eq!(restored, WindowCapabilities::ALL);
    assert_eq!(hash_of(&restored), hash_of(&WindowCapabilities::ALL));
}

/// A capability is one value, so the reason a refusal answers is the one the
/// field holds now: marking a capability absent a second time, by its command
/// or by its field, leaves no earlier reason behind to answer in its place.
fn a_refusal_answers_the_reason_the_capability_holds() {
    let material = WindowCommand::SetMaterial(WindowMaterial::System);
    let mut parts = WindowCapabilities::ALL
        .unavailable(&material, CapabilityAbsence::NOT_YET_ON_THIS_HOST)
        .into_parts();
    parts.material = WindowCapability::Native;
    assert_eq!(admits(&material, &parts.into_capabilities()), Ok(()));
    parts.material = WindowCapability::NOT_ON_THIS_PLATFORM;
    let capabilities = parts.into_capabilities();
    assert_eq!(
        admits(&material, &capabilities),
        Err(AdmissionError::Unsupported(Unsupported::new(
            UnsupportedReason::NotOnThisPlatform
        ))),
        "the reason set last, not the one marked first"
    );
    assert_eq!(
        capabilities.capability(&material),
        capabilities.material,
        "a command reads the field it is admitted against"
    );
    let parts = capabilities.into_parts();
    assert_eq!(parts.material, capabilities.material);
    assert_eq!(parts.into_capabilities(), capabilities);
}
