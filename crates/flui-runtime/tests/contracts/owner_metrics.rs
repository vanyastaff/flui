use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::{
    PresentationAddress,
    geometry::{EdgeInsets, Size},
};
use flui_runtime::owner::{OwnerEffects, OwnerHost, WindowObservation};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::UiRuntime;
use flui_view::prelude::*;

type ObservedMetrics = Rc<RefCell<Vec<(Size<f64>, f64)>>>;

#[derive(Clone, StatelessView)]
struct PreferenceReader(Rc<RefCell<Vec<f64>>>);

impl StatelessView for PreferenceReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let scale =
            flui_widgets::MediaQuery::text_scale_factor_of(ctx).expect("runtime root preference");
        self.0.borrow_mut().push(scale);
        flui_widgets::SizedBox::new(20.0 * scale, 20.0)
    }
}

#[derive(Clone, StatelessView)]
struct ContrastReader(Rc<RefCell<Vec<bool>>>);

impl StatelessView for ContrastReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.0
            .borrow_mut()
            .push(flui_widgets::MediaQuery::high_contrast_of(ctx).expect("runtime root contrast"));
        flui_widgets::SizedBox::square(10.0)
    }
}

#[derive(Clone, StatelessView)]
struct MetricsReader {
    observed: ObservedMetrics,
    contrast: Rc<RefCell<Vec<bool>>>,
    text_scale: Rc<RefCell<Vec<f64>>>,
    owner: OwnerHost,
    brightness: Rc<RefCell<Vec<flui_platform_api::Brightness>>>,
    padding: Rc<RefCell<Vec<EdgeInsets>>>,
}

impl StatelessView for MetricsReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        assert_eq!(
            self.owner.phase(),
            Ok(Some(flui_scheduler::SchedulerPhase::PersistentCallbacks))
        );
        assert_eq!(
            self.owner.next_wake(),
            Err(flui_runtime::owner::DispatchError::Busy)
        );
        let ids = self
            .owner
            .runtime_ids()
            .expect("membership while frame is leased");
        assert_eq!(ids.len(), 1);
        let status = self
            .owner
            .runtime_status(ids[0])
            .expect("active runtime status");
        assert_eq!(
            status.phase,
            flui_scheduler::SchedulerPhase::PersistentCallbacks
        );
        let media = flui_widgets::MediaQuery::of(ctx);
        self.contrast.borrow_mut().push(media.high_contrast);
        self.text_scale.borrow_mut().push(media.text_scale_factor);
        self.brightness.borrow_mut().push(media.platform_brightness);
        self.padding.borrow_mut().push(media.padding);
        self.observed
            .borrow_mut()
            .push((media.size, media.device_pixel_ratio));
        flui_widgets::SizedBox::new(media.size.width, media.size.height)
    }
}

struct Sink {
    size: Rc<Cell<(u32, u32)>>,
    submitted: usize,
}

impl FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size.get()
    }
    fn submit(&mut self, _: flui_layer::Scene) -> SubmitVerdict {
        self.submitted += 1;
        SubmitVerdict::Presented
    }
}

struct Effects {
    address: PresentationAddress,
    sink: RefCell<Sink>,
    size: Rc<Cell<(u32, u32)>>,
    frame_time: Cell<web_time::Instant>,
    trace: RefCell<Vec<&'static str>>,
    expects_present: Cell<Option<bool>>,
    owner: OwnerHost,
    burst: Cell<bool>,
    native_sizes: RefCell<Vec<(Size<f64>, f64)>>,
    fail_resize: Cell<bool>,
    fail_tail: Cell<bool>,
}

impl OwnerEffects for Effects {
    fn finish_install(
        &self,
        _: flui_foundation::PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
    fn frame(&self, address: PresentationAddress, runtime: &mut UiRuntime) {
        assert_eq!(address, self.address);
        self.trace.borrow_mut().push("frame");
        let now = self.frame_time.get() + std::time::Duration::from_millis(16);
        self.frame_time.set(now);
        let presented = runtime
            .pump(
                &mut flui_runtime::pump::SampledClock(now),
                &mut *self.sink.borrow_mut(),
            )
            .presented();
        if let Some(expected) = self.expects_present.get() {
            assert_eq!(presented, expected);
        }
        if self.burst.replace(false) {
            let target = self.owner.presentation_dispatcher(address).expect("window");
            let frame = self.owner.frame_dispatcher(address).expect("frame");
            for (width, brightness, text_scale) in [
                (200.0, flui_platform_api::Brightness::Dark, 2.0),
                (400.0, flui_platform_api::Brightness::Light, 1.0),
            ] {
                self.owner
                    .update_preferences(
                        flui_platform_api::SystemPreferences::default()
                            .with_text_scale(text_scale)
                            .expect("valid preference"),
                        self,
                    )
                    .expect("queued preferences");
                for observation in [
                    WindowObservation::Metrics {
                        size: Size::new(width - 10.0, 100.0),
                        scale_factor: 1.5,
                    },
                    WindowObservation::Brightness(brightness),
                    WindowObservation::Metrics {
                        size: Size::new(width, 100.0),
                        scale_factor: 2.0,
                    },
                ] {
                    target.observe(observation, self).expect("queued state");
                }
                frame.deliver(self).expect("frame observes this segment");
            }
        }
    }
    fn commit_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        panic!("no pending install");
    }
    fn retire_host(
        &self,
        _: &[flui_foundation::PresentationAddress],
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn cancel_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) {
        panic!("no pending install");
    }
    fn resize_surface(&self, address: PresentationAddress, size: Size<f64>, scale_factor: f64) {
        assert_eq!(address, self.address);
        assert!(!self.fail_resize.replace(false), "native resize failure");
        self.native_sizes.borrow_mut().push((size, scale_factor));
        self.trace.borrow_mut().push("native resize");
        self.size.set((
            (size.width * scale_factor) as u32,
            (size.height * scale_factor) as u32,
        ));
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {
        assert!(!self.fail_tail.replace(false), "owner completion failure");
    }
    fn request_continuation(&self) -> bool {
        true
    }
}

fn resize_and_surface_restore_reach_the_product_frame() {
    let owner = OwnerHost::new();
    let runtime = crate::owner_publication::runtime();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let text_scale = Rc::new(RefCell::new(Vec::new()));
    let contrast = Rc::default();
    runtime
        .attach_root_widget_with_size(
            &MetricsReader {
                observed: Rc::clone(&observed),
                contrast: Rc::clone(&contrast),
                text_scale: Rc::clone(&text_scale),
                owner: owner.clone(),
                brightness: Rc::default(),
                padding: Rc::default(),
            },
            800.0,
            600.0,
        )
        .expect("mount actual widget");
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let size = Rc::new(Cell::new((800, 600)));
    let effects = Effects {
        address,
        sink: RefCell::new(Sink {
            size: Rc::clone(&size),
            submitted: 0,
        }),
        size,
        frame_time: Cell::new(web_time::Instant::now()),
        trace: RefCell::new(Vec::new()),
        expects_present: Cell::new(Some(true)),
        owner: owner.clone(),
        burst: Cell::new(false),
        native_sizes: RefCell::new(Vec::new()),
        fail_resize: Cell::new(false),
        fail_tail: Cell::new(false),
    };
    let frames = owner.frame_dispatcher(address).expect("frame authority");
    frames.deliver(&effects).expect("initial frame");
    assert_eq!(
        observed.borrow().last(),
        Some(&(Size::new(800.0, 600.0), 1.0))
    );
    owner
        .presentation_dispatcher(address)
        .expect("window authority")
        .observe(
            WindowObservation::Metrics {
                size: Size::new(200.0, 100.0),
                scale_factor: 2.0,
            },
            &effects,
        )
        .expect("resize");
    frames.deliver(&effects).expect("resized frame");
    assert_eq!(
        observed.borrow().last(),
        Some(&(Size::new(200.0, 100.0), 2.0))
    );
    assert_eq!(effects.sink.borrow().submitted, 2);
    assert_eq!(*effects.trace.borrow(), ["frame", "native resize", "frame"]);
    assert_eq!(
        *effects.native_sizes.borrow(),
        [(Size::new(200.0, 100.0), 2.0)]
    );
    effects.expects_present.set(Some(false));
    frames.deliver(&effects).expect("settled idle frame");
    assert_eq!(effects.sink.borrow().submitted, 2);
    frames
        .surface_restored(&effects)
        .expect("surface restoration");
    effects.expects_present.set(Some(true));
    frames.deliver(&effects).expect("restored surface frame");
    assert_eq!(effects.sink.borrow().submitted, 3);
    // The direct host scale entry must keep inherited data in step with the
    // renderer, without a caller obtaining and mutating the root's source.
    owner
        .presentation_dispatcher(address)
        .expect("window authority")
        .test_callback(
            Box::new(move |runtime| {
                assert!(runtime.set_device_pixel_ratio_for(address.presentation_id, 1.0));
            }),
            &effects,
        )
        .expect("direct scale update");
    effects.size.set((200, 100));
    effects.expects_present.set(None);
    frames.deliver(&effects).expect("direct scale frame");
    assert_eq!(
        observed.borrow().last(),
        Some(&(Size::new(200.0, 100.0), 1.0)),
        "the host scale entry updates the real inherited consumer",
    );
    assert!(
        owner.next_wake().is_ok(),
        "deadline snapshot resumes after the frame"
    );
    assert_eq!(text_scale.borrow().last(), Some(&1.0));
    assert_eq!(contrast.borrow().last(), Some(&false));
    for preference in [Some(true), Some(false), Some(true), None] {
        let values = flui_platform_api::SystemPreferences::default();
        owner
            .update_preferences(
                preference.map_or_else(
                    || values.clone(),
                    |value| values.clone().with_high_contrast(value),
                ),
                &effects,
            )
            .expect("accept contrast observation");
        frames.deliver(&effects).expect("contrast frame");
        assert_eq!(
            contrast.borrow().last(),
            Some(&preference.unwrap_or(false)),
            "retained runtime subtree receives contrast, including unknown fallback"
        );
    }
    for scale in [2.0, 1.0] {
        owner
            .update_preferences(
                flui_platform_api::SystemPreferences::default()
                    .with_text_scale(scale)
                    .expect("valid observation"),
                &effects,
            )
            .expect("accept preference snapshot");
        frames.deliver(&effects).expect("preference frame");
        assert_eq!(
            text_scale.borrow().last(),
            Some(&scale),
            "preserved subtree observes the host preference"
        );
    }
    owner
        .update_preferences(
            flui_platform_api::SystemPreferences::default()
                .with_high_contrast(true)
                .with_text_scale(2.0)
                .expect("valid observation"),
            &effects,
        )
        .expect("late runtime seed");
    let fonts = flui_painting::FontCollection::new();
    let mut late = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        flui_runtime::ui_runtime::RuntimeHostServices::new(
            std::sync::Arc::new(|| {}),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::sync::Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &fonts,
            flui_scheduler::ClockSource::Platform,
        )
        .with_preferences(
            owner
                .preferences()
                .expect("live host")
                .expect("accepted observations"),
        ),
    )
    .expect("late runtime");
    let initial = Rc::new(RefCell::new(Vec::new()));
    let initial_contrast = Rc::default();
    late.attach_root_widget_with_size(
        &flui_widgets::Column::new(vec![
            PreferenceReader(Rc::clone(&initial)).boxed(),
            ContrastReader(Rc::clone(&initial_contrast)).boxed(),
        ]),
        800.0,
        600.0,
    )
    .expect("attach seeded root");
    let mut sink = Sink {
        size: Rc::new(Cell::new((800, 600))),
        submitted: 0,
    };
    assert!(
        late.pump(
            &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
            &mut sink,
        )
        .presented()
    );
    assert_eq!(
        *initial.borrow(),
        [2.0],
        "first build observes accepted preferences"
    );
    assert_eq!(
        *initial_contrast.borrow(),
        [true],
        "first build receives contrast seed"
    );
    let other = OwnerHost::new();
    let refused = other
        .publication(other.prepare_runtime(late))
        .expect_err("a seed cannot cross host ownership");
    assert_eq!(
        refused.error,
        flui_runtime::owner::PublicationError::ForeignOwner
    );
    owner.shutdown(&effects);
    assert_eq!(owner.phase(), Ok(None));
}

fn a_secondary_window_observation_does_not_present_the_primary() {
    let owner = OwnerHost::new();
    let runtime = crate::owner_publication::runtime();
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::new(80.0, 60.0), 800.0, 600.0)
        .expect("mount primary content");
    let primary = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish primary")
        .commit();
    let secondary = owner
        .publication(
            owner
                .prepare_presentation(primary, crate::owner_publication::window())
                .expect("prepare contentless secondary"),
        )
        .expect("publish secondary")
        .commit();
    let size = Rc::new(Cell::new((800, 600)));
    let effects = Effects {
        address: primary,
        sink: RefCell::new(Sink {
            size: Rc::clone(&size),
            submitted: 0,
        }),
        size,
        frame_time: Cell::new(web_time::Instant::now()),
        trace: RefCell::new(Vec::new()),
        expects_present: Cell::new(Some(true)),
        owner: owner.clone(),
        burst: Cell::new(false),
        native_sizes: RefCell::new(Vec::new()),
        fail_resize: Cell::new(false),
        fail_tail: Cell::new(false),
    };
    let frames = owner.frame_dispatcher(primary).expect("primary frames");
    frames.deliver(&effects).expect("initial frame");
    effects.expects_present.set(Some(false));
    frames
        .deliver(&effects)
        .expect("settled primary stays idle");
    owner
        .presentation_dispatcher(secondary)
        .expect("secondary authority")
        .observe(
            WindowObservation::Brightness(flui_platform_api::Brightness::Dark),
            &effects,
        )
        .expect("secondary appearance update");
    frames
        .deliver(&effects)
        .expect("secondary observation leaves primary idle");
    assert_eq!(effects.sink.borrow().submitted, 1);
    owner.shutdown(&effects);
}

#[test]
fn owner_metrics_contract() {
    crate::table_test::run_table(
        "owner_metrics_contract",
        &[
            (
                "wheel_preferences_reach_the_next_mounted_input",
                wheel_preferences_reach_the_next_mounted_input as fn(),
            ),
            (
                "gesture_preferences_are_captured_in_input_order_before_a_frame",
                gesture_preferences_are_captured_in_input_order_before_a_frame as fn(),
            ),
            (
                "preference_fanout_survives_a_failing_runtime",
                preference_fanout_survives_a_failing_runtime as fn(),
            ),
            (
                "a_secondary_window_observation_does_not_present_the_primary",
                a_secondary_window_observation_does_not_present_the_primary as fn(),
            ),
            (
                "resize_failure_preserves_other_batched_window_state",
                resize_failure_preserves_other_batched_window_state as fn(),
            ),
            (
                "pointer_stream_and_keyboard_survive_interleaved_resize",
                pointer_stream_and_keyboard_survive_interleaved_resize as fn(),
            ),
            (
                "queued_state_bursts_coalesce_between_observing_frames",
                queued_state_bursts_coalesce_between_observing_frames as fn(),
            ),
            (
                "resize_and_surface_restore_reach_the_product_frame",
                resize_and_surface_restore_reach_the_product_frame as fn(),
            ),
        ],
    );
}

fn wheel_preferences_reach_the_next_mounted_input() {
    use flui_platform_api::pointer::{
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerPosition, ScrollDelta,
        ScrollEvent, ScrollUnit,
    };
    use flui_platform_api::{
        EventTime, PlatformInput, SystemPreferences, WheelPreferences, WheelStep,
    };

    for authored_gestures in [false, true] {
        let owner = OwnerHost::new();
        let runtime = crate::owner_publication::runtime();
        let scroll = flui_widgets::ScrollController::new();
        runtime
            .attach_root_widget_with_size(
                &NestedWheelScroll {
                    authored_gestures,
                    child: flui_widgets::Scrollable::new()
                        .controller(scroll.clone())
                        .child(flui_widgets::SizedBox::new(800.0, 5000.0)),
                },
                800.0,
                600.0,
            )
            .expect("mount scroll consumer");
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish")
            .commit();
        let size = Rc::new(Cell::new((800, 600)));
        let effects = Effects {
            address,
            sink: RefCell::new(Sink {
                size: Rc::clone(&size),
                submitted: 0,
            }),
            size,
            frame_time: Cell::new(web_time::Instant::now()),
            trace: RefCell::new(Vec::new()),
            expects_present: Cell::new(None),
            owner: owner.clone(),
            burst: Cell::new(false),
            native_sizes: RefCell::new(Vec::new()),
            fail_resize: Cell::new(false),
            fail_tail: Cell::new(false),
        };
        owner
            .frame_dispatcher(address)
            .expect("frame")
            .deliver(&effects)
            .expect("mount frame");
        owner
            .update_preferences(
                SystemPreferences::default()
                    .with_wheel(WheelPreferences::default().with_vertical(WheelStep::Lines(3))),
                &effects,
            )
            .expect("accept wheel settings");
        let input = owner.presentation_dispatcher(address).expect("input");
        let dispatch = |unit, distance| {
            let event = PointerEvent::Scroll(ScrollEvent::new(
                PointerInfo::new(
                    PointerId::try_from(1_u64).expect("pointer"),
                    PointerKind::Mouse,
                ),
                EventTime::from_nanos(1),
                PointerPosition::try_new(flui_foundation::geometry::Point::new(100.0, 100.0))
                    .expect("point"),
                ScrollDelta::try_new(unit, 0.0, distance).expect("distance"),
            ));
            input
                .input(PlatformInput::Pointer(event), &effects)
                .expect("wheel input");
        };
        dispatch(ScrollUnit::Detents, 1.0);
        assert_eq!(
            scroll.pixels(),
            159.0,
            "three authored line distances before another frame"
        );
        for (label, preference, unit, distance, expected) in [
            (
                "disabled detent",
                WheelStep::Lines(0),
                ScrollUnit::Detents,
                1.0,
                0.0,
            ),
            (
                "translated lines bypass disable",
                WheelStep::Lines(0),
                ScrollUnit::Lines,
                2.0,
                106.0,
            ),
            (
                "translated lines bypass counts and page cap",
                WheelStep::Lines(50),
                ScrollUnit::Lines,
                20.0,
                1060.0,
            ),
            (
                "pixels bypass disable",
                WheelStep::Lines(0),
                ScrollUnit::Pixels,
                19.5,
                19.5,
            ),
            (
                "translated pages bypass disable",
                WheelStep::Lines(0),
                ScrollUnit::Pages,
                0.5,
                300.0,
            ),
            (
                "fractional line detent",
                WheelStep::Lines(4),
                ScrollUnit::Detents,
                0.25,
                53.0,
            ),
            (
                "fractional page detent",
                WheelStep::Page,
                ScrollUnit::Detents,
                0.25,
                150.0,
            ),
            (
                "large native line count uses page",
                WheelStep::Lines(50),
                ScrollUnit::Detents,
                1.0,
                600.0,
            ),
            (
                "fractional large native count",
                WheelStep::Lines(50),
                ScrollUnit::Detents,
                0.5,
                300.0,
            ),
            (
                "overflowing detent product refused",
                WheelStep::Lines(4),
                ScrollUnit::Detents,
                f64::MAX,
                0.0,
            ),
            (
                "healthy detent after refusal",
                WheelStep::Lines(4),
                ScrollUnit::Detents,
                0.25,
                53.0,
            ),
            (
                "overflowing translated line product refused",
                WheelStep::Lines(4),
                ScrollUnit::Lines,
                f64::MAX,
                0.0,
            ),
            (
                "healthy pixels after refusal",
                WheelStep::Lines(4),
                ScrollUnit::Pixels,
                12.0,
                12.0,
            ),
        ] {
            scroll.jump_to(0.0);
            owner
                .update_preferences(
                    SystemPreferences::default()
                        .with_wheel(WheelPreferences::default().with_vertical(preference)),
                    &effects,
                )
                .expect("accept next host preference");
            dispatch(unit, distance);
            assert_eq!(
                scroll.pixels(),
                expected,
                "{label}; authored gestures={authored_gestures}"
            );
        }
        scroll.jump_to(0.0);
        owner
            .update_preferences(SystemPreferences::default(), &effects)
            .expect("unknown observation");
        dispatch(ScrollUnit::Detents, 1.0);
        assert_eq!(
            scroll.pixels(),
            53.0,
            "unknown observation restores the authored fallback"
        );
        owner.shutdown(&effects);
    }
}

#[derive(Clone, StatelessView)]
struct NestedWheelScroll {
    authored_gestures: bool,
    child: flui_widgets::Scrollable,
}

impl StatelessView for NestedWheelScroll {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let scope = flui_widgets::GestureArenaScope::new(
            flui_widgets::GestureArenaScope::of(ctx),
            self.child.clone(),
        );
        if self.authored_gestures {
            scope.settings(flui_interaction::GestureSettings::default())
        } else {
            scope
        }
    }
}

#[derive(Clone, StatefulView)]
struct AuthoredGestureTiming {
    timeout: std::time::Duration,
    child: flui_widgets::GestureDetector,
}

struct AuthoredGestureTimingState {
    arena: Option<flui_interaction::GestureArena>,
}

impl flui_view::StatefulView for AuthoredGestureTiming {
    type State = AuthoredGestureTimingState;

    fn create_state(&self) -> Self::State {
        AuthoredGestureTimingState { arena: None }
    }
}

impl flui_view::ViewState<AuthoredGestureTiming> for AuthoredGestureTimingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.arena = Some(flui_widgets::GestureArenaScope::of(ctx));
    }

    fn build(&self, view: &AuthoredGestureTiming, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::GestureArenaScope::new(
            self.arena.as_ref().expect("mounted arena").clone(),
            view.child.clone(),
        )
        .settings(
            flui_interaction::GestureSettings::default().with_long_press_timeout(view.timeout),
        )
    }
}

fn gesture_preferences_are_captured_in_input_order_before_a_frame() {
    use flui_foundation::{ManualClock, MonotonicClock, geometry::Offset};
    use flui_interaction::{
        events::PointerKind,
        testing::input::{pointer_down, pointer_up},
    };
    use flui_platform_api::{GesturePreferences, PlatformInput, SystemPreferences};
    use std::{sync::Arc, time::Duration};

    for (preferences_first, authored_timeout) in [
        (true, None),
        (false, None),
        (true, Some(Duration::from_millis(700))),
    ] {
        let owner = OwnerHost::new();
        let clock = ManualClock::new();
        let runtime = UiRuntime::new(
            crate::owner_publication::window(),
            1.0,
            flui_runtime::ui_runtime::RuntimeHostServices::new(
                Arc::new(|| {}),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
                Arc::new(flui_platform_api::InMemoryClipboard::new()),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Manual(clock.clone()),
            ),
        )
        .expect("manual-clock runtime");
        let calls = Rc::new(Cell::new(0));
        let activated = Rc::clone(&calls);
        let detector = flui_widgets::GestureDetector::new()
            .on_long_press(move |_| activated.set(activated.get() + 1))
            .child(flui_widgets::ColoredBox::new(
                flui_painting::styling::Color::RED,
            ));
        let root = authored_timeout.map_or_else(
            || detector.clone().boxed(),
            |timeout| {
                AuthoredGestureTiming {
                    timeout,
                    child: detector.clone(),
                }
                .boxed()
            },
        );
        runtime
            .attach_root_widget_with_size(&root, 800.0, 600.0)
            .expect("attach detector");
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish")
            .commit();
        let size = Rc::new(Cell::new((800, 600)));
        let effects = Effects {
            address,
            sink: RefCell::new(Sink {
                size: Rc::clone(&size),
                submitted: 0,
            }),
            size,
            frame_time: Cell::new(clock.now()),
            trace: RefCell::new(Vec::new()),
            expects_present: Cell::new(None),
            owner: owner.clone(),
            burst: Cell::new(false),
            native_sizes: RefCell::new(Vec::new()),
            fail_resize: Cell::new(false),
            fail_tail: Cell::new(false),
        };
        let frames = owner.frame_dispatcher(address).expect("frame dispatcher");
        let frame = |duration| {
            clock.advance(duration);
            effects
                .frame_time
                .set(clock.now() - Duration::from_millis(16));
            frames.deliver(&effects).expect("frame");
        };
        frame(Duration::from_millis(16));
        let target = owner
            .presentation_dispatcher(address)
            .expect("pointer dispatcher");
        let down = || {
            target
                .input(
                    PlatformInput::Pointer(
                        pointer_down(Offset::new(40.0, 40.0), PointerKind::Touch).expect("down"),
                    ),
                    &effects,
                )
                .expect("deliver down")
        };
        let up = || {
            target
                .input(
                    PlatformInput::Pointer(
                        pointer_up(Offset::new(40.0, 40.0), PointerKind::Touch).expect("up"),
                    ),
                    &effects,
                )
                .expect("deliver up")
        };
        let preferences = SystemPreferences::default().with_gestures(
            GesturePreferences::default().with_long_press_timeout(Duration::from_millis(200)),
        );
        if !preferences_first {
            down();
        }
        owner
            .update_preferences(preferences, &effects)
            .expect("accepted gesture preference");
        if preferences_first {
            down();
        }
        // There is deliberately no frame between the preference and pointer commands.
        frame(Duration::from_millis(250));
        assert_eq!(
            calls.get(),
            usize::from(preferences_first && authored_timeout.is_none()),
            "new admission sees the preceding preference; existing admission and authored override retain policy"
        );
        frame(Duration::from_millis(500));
        assert_eq!(
            calls.get(),
            1,
            "the originally admitted hold eventually completes"
        );
        up();
        down();
        frame(Duration::from_millis(250));
        assert_eq!(
            calls.get(),
            if authored_timeout.is_some() { 1 } else { 2 },
            "fresh contact adopts the latest policy"
        );
        frame(Duration::from_millis(500));
        assert_eq!(calls.get(), 2);
        up();
        owner
            .update_preferences(SystemPreferences::default(), &effects)
            .expect("unknown observation");
        down();
        frame(Duration::from_millis(250));
        assert_eq!(
            calls.get(),
            2,
            "unknown observation restores the consumer baseline"
        );
        frame(Duration::from_millis(500));
        assert_eq!(calls.get(), 3);
        up();
        owner.shutdown(&effects);
    }
}

fn preference_fanout_survives_a_failing_runtime() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    for (wake_fails, tail_fails) in [(false, false), (true, false), (false, true), (true, true)] {
        let owner = OwnerHost::new();
        let fail_wake = Arc::new(AtomicBool::new(false));
        let mut recipients = Vec::new();
        for index in 0..2 {
            let gate = Arc::clone(&fail_wake);
            let wakes = Arc::new(AtomicUsize::new(0));
            let requested = Arc::clone(&wakes);
            let runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
                requested.fetch_add(1, Ordering::SeqCst);
                assert!(
                    !(index == 0 && gate.load(Ordering::Acquire)),
                    "preference wake failure"
                );
            }));
            let observed = Rc::default();
            runtime
                .attach_root_widget_with_size(&PreferenceReader(Rc::clone(&observed)), 800.0, 600.0)
                .expect("mount preference consumer");
            let address = owner
                .publication(owner.prepare_runtime(runtime))
                .expect("publish recipient")
                .commit();
            let size = Rc::new(Cell::new((800, 600)));
            let effects = Effects {
                address,
                sink: RefCell::new(Sink {
                    size: Rc::clone(&size),
                    submitted: 0,
                }),
                size,
                frame_time: Cell::new(web_time::Instant::now()),
                trace: RefCell::default(),
                expects_present: Cell::new(Some(true)),
                owner: owner.clone(),
                burst: Cell::new(false),
                native_sizes: RefCell::default(),
                fail_resize: Cell::new(false),
                fail_tail: Cell::new(false),
            };
            owner
                .frame_dispatcher(address)
                .expect("recipient frame")
                .deliver(&effects)
                .expect("initial consumer frame");
            assert_eq!(*observed.borrow(), [1.0]);
            wakes.store(0, Ordering::SeqCst);
            recipients.push((effects, observed, wakes));
        }
        fail_wake.store(wake_fails, Ordering::Release);
        recipients[0].0.fail_tail.set(tail_fails);
        let result = catch_unwind(AssertUnwindSafe(|| {
            owner
                .update_preferences(
                    flui_platform_api::SystemPreferences::default()
                        .with_text_scale(2.0)
                        .expect("valid preference"),
                    &recipients[0].0,
                )
                .expect("accept fanout");
        }));
        fail_wake.store(false, Ordering::Release);
        if wake_fails || tail_fails {
            let failure = result.expect_err("first failure remains authoritative");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&if wake_fails {
                    "preference wake failure"
                } else {
                    "owner completion failure"
                })
            );
        } else {
            result.expect("healthy preference fanout");
        }
        owner.continue_work(&recipients[0].0);
        for (effects, observed, wakes) in &recipients {
            assert!(
                wakes.load(Ordering::SeqCst) > 0,
                "preference publication must request a frame before the harness supplies one"
            );
            owner
                .frame_dispatcher(effects.address)
                .expect("live recipient")
                .deliver(effects)
                .expect("preference recovery frame");
            assert_eq!(
                observed.borrow().last(),
                Some(&2.0),
                "a failing sibling must not discard accepted preferences"
            );
        }
        owner
            .update_preferences(
                flui_platform_api::SystemPreferences::default()
                    .with_text_scale(1.0)
                    .expect("valid preference"),
                &recipients[0].0,
            )
            .expect("next preference update");
        for (effects, observed, _) in &recipients {
            owner
                .frame_dispatcher(effects.address)
                .expect("live recipient")
                .deliver(effects)
                .expect("next frame after recovery");
            assert_eq!(observed.borrow().last(), Some(&1.0));
        }
        owner.shutdown(&recipients[0].0);
    }
}

fn queued_state_bursts_coalesce_between_observing_frames() {
    let owner = OwnerHost::new();
    let runtime = crate::owner_publication::runtime();
    let observed = Rc::default();
    let brightness = Rc::default();
    let text_scale = Rc::default();
    runtime
        .attach_root_widget_with_size(
            &MetricsReader {
                observed: Rc::clone(&observed),
                owner: owner.clone(),
                contrast: Rc::default(),
                brightness: Rc::clone(&brightness),
                text_scale: Rc::clone(&text_scale),
                padding: Rc::default(),
            },
            800.0,
            600.0,
        )
        .expect("mount state consumer");
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let size = Rc::new(Cell::new((800, 600)));
    let effects = Effects {
        address,
        sink: RefCell::new(Sink {
            size: Rc::clone(&size),
            submitted: 0,
        }),
        size,
        frame_time: Cell::new(web_time::Instant::now()),
        trace: RefCell::new(Vec::new()),
        expects_present: Cell::new(Some(true)),
        owner: owner.clone(),
        burst: Cell::new(true),
        native_sizes: RefCell::new(Vec::new()),
        fail_resize: Cell::new(false),
        fail_tail: Cell::new(false),
    };
    owner
        .frame_dispatcher(address)
        .expect("frame")
        .deliver(&effects)
        .expect("deliver burst and frames");
    assert_eq!(
        *effects.native_sizes.borrow(),
        [
            (Size::new(200.0, 100.0), 2.0),
            (Size::new(400.0, 100.0), 2.0)
        ],
        "only latest metrics in each pending segment reach native resize"
    );
    assert_eq!(
        &observed.borrow()[1..],
        [
            (Size::new(200.0, 100.0), 2.0),
            (Size::new(400.0, 100.0), 2.0)
        ]
    );
    assert_eq!(
        &brightness.borrow()[1..],
        [
            flui_platform_api::Brightness::Dark,
            flui_platform_api::Brightness::Light
        ]
    );
    assert_eq!(
        &text_scale.borrow()[1..],
        [2.0, 1.0],
        "each frame observes its admitted preference snapshot, even when the host has already accepted a newer one"
    );
    owner.shutdown(&effects);
}

fn resize_failure_preserves_other_batched_window_state() {
    use flui_platform_api::Brightness;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    for (resize_fails, wake_fails, tail_fails) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
        (true, true, false),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let owner = OwnerHost::new();
        let fail_wake = Arc::new(AtomicBool::new(false));
        let failed_wakes = Arc::new(AtomicUsize::new(0));
        let (gate, attempts) = (Arc::clone(&fail_wake), Arc::clone(&failed_wakes));
        let runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
            if gate.load(Ordering::Acquire) {
                attempts.fetch_add(1, Ordering::SeqCst);
                panic!("window wake failure");
            }
        }));
        let observed = Rc::default();
        let brightness = Rc::default();
        let padding = Rc::default();
        let text_scale = Rc::default();
        runtime
            .attach_root_widget_with_size(
                &MetricsReader {
                    observed: Rc::clone(&observed),
                    owner: owner.clone(),
                    contrast: Rc::default(),
                    brightness: Rc::clone(&brightness),
                    text_scale: Rc::clone(&text_scale),
                    padding: Rc::clone(&padding),
                },
                800.0,
                600.0,
            )
            .expect("mount actual window-state consumer");
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish")
            .commit();
        let size = Rc::new(Cell::new((800, 600)));
        let effects = Effects {
            address,
            sink: RefCell::new(Sink {
                size: Rc::clone(&size),
                submitted: 0,
            }),
            size,
            frame_time: Cell::new(web_time::Instant::now()),
            trace: RefCell::new(Vec::new()),
            expects_present: Cell::new(Some(true)),
            owner: owner.clone(),
            burst: Cell::new(false),
            native_sizes: RefCell::new(Vec::new()),
            fail_resize: Cell::new(false),
            fail_tail: Cell::new(false),
        };
        let frame = owner.frame_dispatcher(address).expect("frame");
        let target = owner.presentation_dispatcher(address).expect("window");
        frame.deliver(&effects).expect("initial scene");
        effects.expects_present.set(Some(false));
        {
            let _callback = owner.begin_callback(&effects).expect("physical callback");
            for _ in 0..32 {
                frame.deliver(&effects).expect("spend callback budget");
            }
            for observation in [
                WindowObservation::Brightness(Brightness::Light),
                WindowObservation::SafeArea(EdgeInsets::all(3.0)),
                WindowObservation::Metrics {
                    size: Size::new(100.0, 100.0),
                    scale_factor: 1.5,
                },
                WindowObservation::Brightness(Brightness::Dark),
                WindowObservation::SafeArea(EdgeInsets::all(12.0)),
                WindowObservation::Metrics {
                    size: Size::new(200.0, 100.0),
                    scale_factor: 2.0,
                },
            ] {
                target
                    .observe(observation, &effects)
                    .expect("admit pending state");
            }
            owner
                .update_preferences(
                    flui_platform_api::SystemPreferences::default()
                        .with_text_scale(2.0)
                        .expect("valid preference"),
                    &effects,
                )
                .expect("admit preferences after window state");
        }
        effects.fail_resize.set(resize_fails);
        effects.fail_tail.set(tail_fails);
        fail_wake.store(wake_fails, Ordering::Release);
        let result = catch_unwind(AssertUnwindSafe(|| owner.continue_work(&effects)));
        fail_wake.store(false, Ordering::Release);
        assert_eq!(
            failed_wakes.load(Ordering::SeqCst) != 0,
            wake_fails,
            "the failing wake path was reached"
        );
        if resize_fails || wake_fails || tail_fails {
            let failure = result.expect_err("preserve the first failure");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&if resize_fails {
                    "native resize failure"
                } else if wake_fails {
                    "window wake failure"
                } else {
                    "owner completion failure"
                })
            );
        } else {
            result.expect("healthy state batch");
        }
        frame
            .surface_restored(&effects)
            .expect("next native opportunity");
        effects.expects_present.set(Some(true));
        frame.deliver(&effects).expect("recovery scene");
        assert_eq!(
            text_scale.borrow().last(),
            Some(&2.0),
            "accepted preferences remain deliverable after preceding window-state and completion failures"
        );
        assert_eq!(
            brightness.borrow().last(),
            Some(&Brightness::Dark),
            "accepted appearance survives native failure; resize={resize_fails}, tail={tail_fails}"
        );
        assert_eq!(
            padding.borrow().last(),
            Some(&EdgeInsets::all(12.0)),
            "accepted safe area survives native failure"
        );
        let metrics = if resize_fails {
            (Size::new(800.0, 600.0), 1.0)
        } else {
            (Size::new(200.0, 100.0), 2.0)
        };
        assert_eq!(
            observed.borrow().last(),
            Some(&metrics),
            "logical geometry is not published when native resize failed"
        );
        target
            .observe(
                WindowObservation::Metrics {
                    size: Size::new(300.0, 100.0),
                    scale_factor: 1.5,
                },
                &effects,
            )
            .expect("a later resize still works");
        frame
            .deliver(&effects)
            .expect("scene after subsequent resize");
        assert_eq!(
            observed.borrow().last(),
            Some(&(Size::new(300.0, 100.0), 1.5))
        );
        owner.shutdown(&effects);
    }
}

fn pointer_stream_and_keyboard_survive_interleaved_resize() {
    use flui_foundation::geometry::Offset;
    use flui_interaction::{
        events::{Code, PointerKind},
        testing::input::{KeyEventBuilder, pointer_cancel, pointer_down, pointer_move, pointer_up},
    };
    use flui_platform_api::{PlatformInput, WindowExecutionState};
    use flui_runtime::owner::InputOutcome;
    use flui_widgets::{Listener, SizedBox, prelude::HitTestBehavior};

    for (pointer_type, resampling) in [
        (PointerKind::Mouse, false),
        (PointerKind::Touch, false),
        (PointerKind::Mouse, true),
        (PointerKind::Touch, true),
    ] {
        for (move_fails, key_fails) in [(false, false), (true, false), (false, true), (true, true)]
        {
            let owner = OwnerHost::new();
            let runtime = crate::owner_publication::runtime();
            runtime
                .gestures()
                .set_resampling_enabled(resampling)
                .expect("configure before contact");
            let events = Rc::new(RefCell::new(Vec::new()));
            let fail_motion_once = Cell::new(move_fails);
            let (down, movement, up, cancel, keyboard) = (
                Rc::clone(&events),
                Rc::clone(&events),
                Rc::clone(&events),
                Rc::clone(&events),
                Rc::clone(&events),
            );
            runtime
                .attach_root_widget_with_size(
                    &Listener::new()
                        .behavior(HitTestBehavior::Opaque)
                        .on_pointer_down(move |_, _| down.borrow_mut().push("down"))
                        .on_pointer_move(move |_, _| {
                            movement.borrow_mut().push("move");
                            assert!(!fail_motion_once.replace(false), "motion failure");
                        })
                        .on_pointer_up(move |_, _| up.borrow_mut().push("up"))
                        .on_pointer_cancel(move |_, _| cancel.borrow_mut().push("cancel"))
                        .child(SizedBox::new(800.0, 600.0)),
                    800.0,
                    600.0,
                )
                .expect("mount input consumer");
            runtime.synchronize_window_snapshot(
                runtime.presentation_id(),
                WindowExecutionState::Running,
                true,
                true,
            );
            runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
            runtime
                .focus_manager()
                .add_global_key_handler(Rc::new(move |_| {
                    keyboard.borrow_mut().push("key");
                    assert!(!key_fails, "keyboard failure");
                    flui_interaction::KeyEventResult::Handled
                }));
            let address = owner
                .publication(owner.prepare_runtime(runtime))
                .expect("publish")
                .commit();
            let size = Rc::new(Cell::new((800, 600)));
            let effects = Effects {
                address,
                sink: RefCell::new(Sink {
                    size: Rc::clone(&size),
                    submitted: 0,
                }),
                size,
                frame_time: Cell::new(web_time::Instant::now()),
                trace: RefCell::new(Vec::new()),
                expects_present: Cell::new(Some(true)),
                owner: owner.clone(),
                burst: Cell::new(false),
                native_sizes: RefCell::new(Vec::new()),
                fail_resize: Cell::new(false),
                fail_tail: Cell::new(false),
            };
            let frame = owner.frame_dispatcher(address).expect("frame");
            frame
                .deliver(&effects)
                .expect("layout and present hit-test tree");
            let target = owner.presentation_dispatcher(address).expect("input");
            let inputs = [
                PlatformInput::Pointer(
                    pointer_down(Offset::new(10.0, 10.0), pointer_type)
                        .expect("finite pointer position"),
                ),
                PlatformInput::Pointer(
                    pointer_move(Offset::new(20.0, 10.0), pointer_type)
                        .expect("finite pointer position"),
                ),
                PlatformInput::Pointer(
                    pointer_move(Offset::new(30.0, 10.0), pointer_type)
                        .expect("finite pointer position"),
                ),
                PlatformInput::Keyboard(KeyEventBuilder::new(Code::F4).build()),
                PlatformInput::Pointer(
                    pointer_move(Offset::new(40.0, 10.0), pointer_type)
                        .expect("finite pointer position"),
                ),
                PlatformInput::Pointer(
                    pointer_up(Offset::new(30.0, 10.0), pointer_type)
                        .expect("finite pointer position"),
                ),
                PlatformInput::Pointer(
                    pointer_down(Offset::new(10.0, 10.0), pointer_type)
                        .expect("finite pointer position"),
                ),
                PlatformInput::Pointer(pointer_cancel(pointer_type)),
            ];
            effects.expects_present.set(Some(false));
            {
                let _callback = owner.begin_callback(&effects).expect("physical callback");
                for _ in 0..32 {
                    frame.deliver(&effects).expect("use physical budget");
                }
                for (index, input) in inputs.into_iter().enumerate() {
                    for width in [800.0 + index as f64, 900.0 + index as f64] {
                        target
                            .observe(
                                WindowObservation::Metrics {
                                    size: Size::new(width, 600.0),
                                    scale_factor: 1.0,
                                },
                                &effects,
                            )
                            .expect("pending resize");
                    }
                    assert_eq!(target.input(input, &effects), Ok(InputOutcome::Queued));
                }
                assert!(
                    events.borrow().is_empty(),
                    "input must wait for the owner turn"
                );
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                owner.continue_work(&effects);
            }));
            if move_fails || key_fails {
                let failure = result.expect_err("callback failure escapes after input settlement");
                assert_eq!(
                    failure.downcast_ref::<&str>(),
                    Some(&if move_fails {
                        "motion failure"
                    } else {
                        "keyboard failure"
                    })
                );
                assert!(
                    events.borrow().contains(&"key"),
                    "accepted keyboard input survives a prior motion callback failure"
                );
                owner.continue_work(&effects);
            } else {
                result.expect("healthy input batch");
            }
            let expected: &[&str] = if resampling {
                &[
                    "down", "move", "move", "key", "move", "up", "down", "cancel",
                ]
            } else {
                &["down", "move", "key", "move", "up", "down", "cancel"]
            };
            assert_eq!(
                *events.borrow(),
                expected,
                "{pointer_type:?}, resampling={resampling}"
            );
            assert_eq!(
                *effects.native_sizes.borrow(),
                (0..8)
                    .map(|index| (Size::new(900.0 + f64::from(index), 600.0), 1.0))
                    .collect::<Vec<_>>()
            );
            owner.shutdown(&effects);
        }
    }
}
