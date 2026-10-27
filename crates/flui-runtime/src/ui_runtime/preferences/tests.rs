//! Synchronous same-presentation replacement is below OwnerHost's queued lease
//! boundary. These private fixtures invoke the real query/diagnostic path.
use super::*;
use flui_foundation::{ManualClock, geometry::Offset};
use flui_painting::TextSizing;
use flui_platform::{Distance, PlatformWindow};
use flui_view::prelude::*;
use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

type Callback = RefCell<Option<Box<dyn FnOnce()>>>;

thread_local! {
    static QUERY: Callback = RefCell::new(None);
    static DIAGNOSTIC: Callback = RefCell::new(None);
}

fn invoke(slot: &'static std::thread::LocalKey<Callback>) {
    let callback = slot.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

struct Window {
    inner: Arc<dyn PlatformWindow>,
    answer: Mutex<Result<Option<GestureGeometry>, PreferenceQueryError>>,
    panic_query: AtomicBool,
}
impl PlatformWindow for Window {
    fn id(&self) -> flui_platform::WindowId {
        self.inner.id()
    }
    fn request_redraw(&self) {
        self.inner.request_redraw();
    }
    fn set_cursor(
        &self,
        cursor: flui_platform::CursorIcon,
    ) -> Result<(), flui_platform::CursorError> {
        self.inner.set_cursor(cursor)
    }
    fn set_frame_rate_hint(
        &self,
        hint: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        self.inner.set_frame_rate_hint(hint)
    }
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        self.inner.set_input_regions(regions)
    }
    fn set_material(
        &self,
        material: flui_platform::WindowMaterial,
    ) -> Result<(), flui_platform::Unsupported> {
        self.inner.set_material(material)
    }
    fn state(&self) -> flui_platform::WindowState {
        self.inner.state()
    }

    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        command.against(&flui_platform::WindowCapabilities::NONE)?;
        Ok(())
    }

    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        None
    }
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }
    fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
        let answer = self.answer.lock().expect("query script").clone();
        let panic = self.panic_query.swap(false, Ordering::Relaxed);
        invoke(&QUERY);
        assert!(!panic, "native query failure");
        answer
    }
}

struct Diagnostics;
impl tracing::Subscriber for Diagnostics {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            invoke(&DIAGNOSTIC);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[derive(Clone, StatelessView)]
struct Reader(Rc<RefCell<Vec<(TextSizing, bool)>>>);
impl StatelessView for Reader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let media = flui_widgets::MediaQuery::of(ctx);
        self.0
            .borrow_mut()
            .push((media.text_sizing.clone(), media.high_contrast));
        flui_widgets::ColoredBox::new(flui_painting::styling::Color::RED)
    }
}

pub(crate) fn newer_geometry_publication_survives_query_and_diagnostic_reentry() {
    use crate::{presentation::PresentationWindow, runtime_services::RuntimeHostServices};
    use flui_interaction::{
        events::PointerKind,
        testing::input::{pointer_down, pointer_move, pointer_up},
    };
    let geometry = |slop| {
        Some(
            GestureGeometry::new(DevicePixelRatio::ONE)
                .with_touch_slop(Distance::new(slop).expect("slop")),
        )
    };
    for (in_query, panics) in [(true, false), (true, true), (false, false), (false, true)] {
        let window = Arc::new(Window {
            inner: flui_native::headless_platform()
                .open_window(flui_platform::WindowOptions::default())
                .expect("window"),
            answer: Mutex::new(Ok(geometry(10.0))),
            panic_query: AtomicBool::new(false),
        });
        let clock = ManualClock::new();
        let mut runtime = Rc::new(
            UiRuntime::new(
                PresentationWindow::new(window.clone(), None),
                1.0,
                RuntimeHostServices::new(
                    Arc::new(|| {}),
                    Arc::new(AtomicBool::new(false)),
                    Arc::new(flui_platform::InMemoryClipboard::new()),
                    &flui_painting::FontCollection::new(),
                    flui_scheduler::ClockSource::Manual(clock.clone()),
                ),
            )
            .expect("runtime"),
        );
        let observed = Rc::new(RefCell::new(Vec::new()));
        let taps = Rc::new(Cell::new(0));
        let output = Rc::clone(&taps);
        let holds = Rc::new(Cell::new(0));
        let held = Rc::clone(&holds);
        runtime
            .attach_root_widget_with_size(
                &flui_widgets::GestureDetector::new()
                    .on_tap(move |_| output.set(output.get() + 1))
                    .on_long_press(move |_| held.set(held.get() + 1))
                    .child(Reader(Rc::clone(&observed))),
                800.0,
                600.0,
            )
            .expect("root");
        let replacement = Rc::clone(&runtime);
        let next_window = Arc::clone(&window);
        let next_clock = clock.clone();
        let callback = Box::new(move || {
            *next_window.answer.lock().expect("replacement") = Ok(geometry(2.0));
            let values = SystemPreferences::default()
                .with_text_scale(3.0)
                .expect("scale")
                .with_high_contrast(true)
                .with_gestures(
                    GesturePreferences::default()
                        .with_long_press_timeout(Duration::from_millis(200)),
                );
            publish(
                replacement.presentations.primary(),
                &values,
                MonotonicClock::now(&next_clock),
            );
            assert!(in_query || !panics, "diagnostic failure");
        }) as Box<dyn FnOnce()>;
        if in_query {
            QUERY.with(|slot| *slot.borrow_mut() = Some(callback));
            window.panic_query.store(panics, Ordering::Relaxed);
        } else {
            *window.answer.lock().expect("old query") = Err(PreferenceQueryError::Unsupported(
                flui_platform::Unsupported::new(flui_platform::UnsupportedReason::Unavailable),
            ));
            DIAGNOSTIC.with(|slot| *slot.borrow_mut() = Some(callback));
        }
        let old = SystemPreferences::default()
            .with_text_scale(1.5)
            .expect("scale");
        let result = tracing::subscriber::with_default(Diagnostics, || {
            catch_unwind(AssertUnwindSafe(|| {
                publish(
                    runtime.presentations.primary(),
                    &old,
                    MonotonicClock::now(&clock),
                );
            }))
        });
        if panics {
            let failure = result.expect_err("preserve boundary failure");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&if in_query {
                    "native query failure"
                } else {
                    "diagnostic failure"
                })
            );
        } else {
            result.expect("healthy replacement");
        }
        QUERY.with(|slot| assert!(slot.borrow().is_none()));
        DIAGNOSTIC.with(|slot| assert!(slot.borrow().is_none()));
        let runtime = Rc::get_mut(&mut runtime).expect("callback released replacement owner");
        let mut sink = crate::testing::ScriptedSink::always_presents();
        let _ = runtime.pump(
            &mut crate::pump::SampledClock(MonotonicClock::now(&clock)),
            &mut sink,
        );
        assert_eq!(
            observed.borrow().last(),
            Some(&(TextSizing::linear(3.0).expect("valid scale"), true)),
            "stale query/diagnostic continuation must not overwrite the new inherited publication"
        );
        for event in [
            pointer_down(Offset::new(40.0, 40.0), PointerKind::Touch).expect("down"),
            pointer_move(Offset::new(45.0, 40.0), PointerKind::Touch).expect("move"),
            pointer_up(Offset::new(45.0, 40.0), PointerKind::Touch).expect("up"),
        ] {
            runtime.enter(|runtime| {
                runtime.handle_input_addressed(
                    runtime.presentations.primary().id(),
                    flui_platform::PlatformInput::Pointer(event),
                )
            });
        }
        assert_eq!(
            taps.get(),
            0,
            "new admission must retain the newer native2 geometry"
        );
        runtime.enter(|runtime| {
            runtime.handle_input_addressed(
                runtime.presentations.primary().id(),
                flui_platform::PlatformInput::Pointer(
                    pointer_down(Offset::new(40.0, 40.0), PointerKind::Touch)
                        .expect("stationary down"),
                ),
            )
        });
        clock.advance(Duration::from_millis(250));
        let _ = runtime.pump(
            &mut crate::pump::SampledClock(MonotonicClock::now(&clock)),
            &mut sink,
        );
        assert_eq!(
            holds.get(),
            1,
            "fresh hold uses newer200ms timing through reentrant replacement"
        );
        runtime.enter(|runtime| {
            runtime.handle_input_addressed(
                runtime.presentations.primary().id(),
                flui_platform::PlatformInput::Pointer(
                    pointer_up(Offset::new(40.0, 40.0), PointerKind::Touch).expect("stationary up"),
                ),
            )
        });
        assert!(
            runtime.next_wake().is_none(),
            "old failure cannot restore acknowledged retry debt"
        );
        publish(
            runtime.presentations.primary(),
            &SystemPreferences::default(),
            MonotonicClock::now(&clock),
        );
        let _ = runtime.pump(
            &mut crate::pump::SampledClock(MonotonicClock::now(&clock)),
            &mut sink,
        );
        assert_eq!(
            observed.borrow().last(),
            Some(&(TextSizing::linear(1.0).expect("valid scale"), false)),
            "the next healthy publication still reaches its mounted consumer"
        );
    }
}

struct HapticWindow {
    inner: Arc<dyn PlatformWindow>,
    haptics: Arc<flui_platform::haptics::FakeHaptics>,
}
impl PlatformWindow for HapticWindow {
    fn id(&self) -> flui_platform::WindowId {
        self.inner.id()
    }
    fn request_redraw(&self) {
        self.inner.request_redraw();
    }
    fn set_cursor(
        &self,
        cursor: flui_platform::CursorIcon,
    ) -> Result<(), flui_platform::CursorError> {
        self.inner.set_cursor(cursor)
    }
    fn set_frame_rate_hint(
        &self,
        hint: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        self.inner.set_frame_rate_hint(hint)
    }
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        self.inner.set_input_regions(regions)
    }
    fn set_material(
        &self,
        material: flui_platform::WindowMaterial,
    ) -> Result<(), flui_platform::Unsupported> {
        self.inner.set_material(material)
    }
    fn state(&self) -> flui_platform::WindowState {
        self.inner.state()
    }
    fn apply(
        &self,
        command: flui_platform::AdmittedCommand,
    ) -> Result<(), flui_platform::ApplyError> {
        self.inner.apply(command)
    }
    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(self.haptics.clone())
    }
    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        self.inner.text_input()
    }
    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        self.inner.system_chrome()
    }
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        self.inner.drag_source()
    }
}

/// An observed "off" suppresses both tiers, immediate and frame-aligned,
/// before they reach the backend; unobserved and "on" play them, and each
/// accepted revision decides the next request (ADR-0212 §1).
pub(crate) fn haptics_preference_suppresses_both_tiers() {
    use flui_platform::HapticFeedback;
    use flui_platform::haptics::{
        FakeHaptics, HapticOutcome, HapticPattern, HapticRequest, HapticTiming,
    };
    let haptics = Arc::new(FakeHaptics::new());
    let window = Arc::new(HapticWindow {
        inner: flui_native::headless_platform()
            .open_window(flui_platform::WindowOptions::default())
            .expect("window"),
        haptics: haptics.clone(),
    });
    let runtime = UiRuntime::new(
        crate::presentation::PresentationWindow::new(window.clone(), None),
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime");
    let origin = Rc::new(());
    let mut revision = 0;
    let mut accept = |values: SystemPreferences| {
        revision += 1;
        runtime.apply_preferences(crate::owner::SystemPreferencesSnapshot {
            origin: Rc::clone(&origin),
            revision,
            values: Arc::new(values),
        });
    };
    let pattern = HapticPattern::builder()
        .transient(std::time::Duration::ZERO, 1.0, 1.0)
        .build()
        .expect("valid");
    let handle = runtime.haptics();
    let now = HapticRequest::new(HapticTiming::Immediate);
    let frame = HapticRequest::new(HapticTiming::FramePresented);
    // Each round requests both tiers at both timings.
    let round = || {
        [
            handle.perform(HapticFeedback::LightImpact, now),
            handle.play(&pattern, HapticFeedback::LightImpact, now),
            handle.perform(HapticFeedback::LightImpact, frame),
            handle.play(&pattern, HapticFeedback::LightImpact, frame),
        ]
    };
    let played = |outcomes: [HapticOutcome; 4]| {
        outcomes
            .iter()
            .all(|outcome| matches!(outcome, HapticOutcome::Submitted | HapticOutcome::Queued(_)))
    };
    let suppressed =
        |outcomes: [HapticOutcome; 4]| outcomes.iter().all(|o| *o == HapticOutcome::Suppressed);
    assert!(played(round()), "no snapshot yet: unobserved plays");
    for (observed, plays) in [(None, true), (Some(true), true), (Some(false), false)] {
        let values = observed.map_or_else(SystemPreferences::default, |enabled| {
            SystemPreferences::default().with_haptics(enabled)
        });
        accept(values);
        let before = haptics.calls().len();
        let outcomes = round();
        if plays {
            assert!(played(outcomes), "observed {observed:?}: {outcomes:?}");
        } else {
            assert!(suppressed(outcomes), "observed {observed:?}: {outcomes:?}");
            assert_eq!(
                haptics.calls().len(),
                before,
                "a suppressed request never reaches the backend"
            );
        }
    }
    accept(SystemPreferences::default());
    assert!(played(round()), "a later unobserved revision plays again");
    // The runtime reaches the window weakly; the test owns it until here.
    drop(window);
}

/// A frame-aligned request plays only with a frame that reached `present()`
/// (ADR-0210 §4). `NotShown` never reached the screen; `NoPresent` found no
/// damage, so the screen kept the previous frame and the change the request
/// confirms never showed. Both leave the request to its deadline, where it
/// resolves `Suppressed` without reaching the backend.
pub(crate) fn frame_aligned_haptics_play_only_with_a_presented_frame() {
    use crate::{presentation::PresentationWindow, runtime_services::RuntimeHostServices};
    use crate::{sink::SubmitVerdict, testing::ScriptedSink};
    use flui_platform::HapticFeedback;
    use flui_platform::haptics::{FakeHaptics, HapticOutcome, HapticRequest, HapticTiming};
    for verdict in [
        SubmitVerdict::Presented,
        SubmitVerdict::NotShown,
        SubmitVerdict::NoPresent,
    ] {
        let haptics = Arc::new(FakeHaptics::new());
        let window = Arc::new(HapticWindow {
            inner: flui_native::headless_platform()
                .open_window(flui_platform::WindowOptions::default())
                .expect("window"),
            haptics: haptics.clone(),
        });
        let clock = ManualClock::new();
        let mut runtime = UiRuntime::new(
            PresentationWindow::new(window.clone(), None),
            1.0,
            RuntimeHostServices::new(
                Arc::new(|| {}),
                Arc::new(AtomicBool::new(false)),
                Arc::new(flui_platform::InMemoryClipboard::new()),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Manual(clock.clone()),
            ),
        )
        .expect("runtime");
        runtime
            .attach_root_widget_with_size(&flui_widgets::SizedBox::square(20.0), 800.0, 600.0)
            .expect("root");
        let handle = runtime.haptics();
        let delivered = Rc::new(RefCell::new(Vec::new()));
        let sink_of = Rc::clone(&delivered);
        let _observer = handle.observe(move |ticket, outcome| {
            sink_of.borrow_mut().push((ticket, outcome));
        });
        // Queued before the first frame, which paints the root.
        let HapticOutcome::Queued(ticket) = handle.perform(
            HapticFeedback::DragLift,
            HapticRequest::new(HapticTiming::FramePresented),
        ) else {
            panic!("{verdict:?}: a frame-aligned request queues");
        };
        let mut sink = ScriptedSink::new(move |_, _| verdict);
        let pump = |runtime: &mut UiRuntime, sink: &mut ScriptedSink| {
            let _ = runtime.pump(
                &mut crate::pump::SampledClock(MonotonicClock::now(&clock)),
                sink,
            );
        };
        pump(&mut runtime, &mut sink);
        assert!(
            sink.submit_calls > 0,
            "{verdict:?}: the first frame painted"
        );
        let presented = verdict == SubmitVerdict::Presented;
        assert_eq!(
            haptics.feedback(),
            if presented {
                vec![HapticFeedback::DragLift]
            } else {
                Vec::new()
            },
            "{verdict:?}: only a presented frame plays the request"
        );
        // Past any deadline: the request has resolved and been delivered.
        clock.advance(Duration::from_secs(1));
        pump(&mut runtime, &mut sink);
        pump(&mut runtime, &mut sink);
        let expected = if presented {
            HapticOutcome::Submitted
        } else {
            HapticOutcome::Suppressed
        };
        assert_eq!(*delivered.borrow(), [(ticket, expected)], "{verdict:?}");
        if !presented {
            assert_eq!(haptics.calls(), [], "{verdict:?}: never played late");
        }
        drop(runtime);
        drop(window);
    }
}

/// A revision no newer than the accepted one is dropped whole: its values reach
/// neither the runtime's snapshot nor the inherited data a mounted consumer
/// reads, and a newer revision after it still applies.
pub(crate) fn an_older_or_repeated_preference_revision_is_ignored() {
    let origin = Rc::new(());
    let scaled = |scale: f64, contrast: bool| {
        SystemPreferences::default()
            .with_text_scale(scale)
            .expect("valid scale")
            .with_high_contrast(contrast)
    };
    let mut runtime = UiRuntime::for_test();
    let observed = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size(&Reader(Rc::clone(&observed)), 800.0, 600.0)
        .expect("root");
    let mut sink = crate::testing::ScriptedSink::always_presents();
    for (revision, offered, label, shown, accepted) in [
        (2, scaled(2.0, true), "the first revision", (2.0, true), 2),
        (1, scaled(3.0, false), "an older revision", (2.0, true), 2),
        (
            2,
            scaled(3.0, false),
            "the accepted revision again",
            (2.0, true),
            2,
        ),
        (3, scaled(3.0, false), "a newer revision", (3.0, false), 3),
    ] {
        runtime.apply_preferences(SystemPreferencesSnapshot {
            origin: Rc::clone(&origin),
            revision,
            values: Arc::new(offered),
        });
        let _ = runtime.pump(
            &mut crate::pump::SampledClock(web_time::Instant::now()),
            &mut sink,
        );
        assert_eq!(
            observed.borrow().last(),
            Some(&(TextSizing::linear(shown.0).expect("valid scale"), shown.1)),
            "{label} (revision {revision}) changed what a mounted consumer reads"
        );
        let current = runtime.preferences.borrow();
        let current = current.as_ref().expect("an accepted snapshot");
        assert_eq!(
            (current.revision, current.values.text_scale()),
            (accepted, Some(shown.0)),
            "{label} (revision {revision}) changed the accepted snapshot"
        );
    }
}
