//! The framework's half of the profiler's wiring contract.
//!
//! `frame_timing_layer`'s unit tests emit the frame and phase spans
//! themselves, so they can only prove that the layer maps the right names.
//! This test drives a real tree through `HeadlessBinding::pump_frame`, which
//! runs the same `UpdateScheduler::drive_frame` the desktop, Android and web
//! runners run, and reads the profile back. If the scheduler stops opening
//! the `frame` span, or the pipeline renames a phase span, this is the test
//! that fails — the unit tests cannot.
#![cfg(feature = "profiling")]

use std::sync::Arc;
use std::time::Duration;

use flui_devtools::profiler::{FramePhase, FrameStats};
use flui_devtools::{FrameTimingLayer, Profiler};
use flui_objects::RenderSizedBox;
use flui_sdk::view::{RenderView, View};
use flui_testing::{HeadlessBinding, MountOptions, MountOwners};
use tracing::Dispatch;
use tracing_subscriber::layer::SubscriberExt;

/// The smallest tree that builds, lays out and paints: one render leaf.
#[derive(Clone)]
struct Leaf;

impl RenderView for Leaf {
    type Protocol = flui_sdk::rendering::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_sdk::view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_sdk::view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_sdk::rendering::RenderUpdateImpact {
        flui_sdk::rendering::RenderUpdateImpact::NONE
    }
}

impl View for Leaf {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::render_variable(self)
    }
}

fn phases(stats: &FrameStats) -> Vec<FramePhase> {
    stats.phases.iter().map(|info| info.phase).collect()
}

#[test]
fn a_real_frame_reaches_the_profiler_with_its_phases() {
    // `tracing` caches callsite interest process-wide on first use; a sibling
    // test binary cannot poison this one, but the sentinel is cheap and it
    // is the documented way to capture under a thread-local subscriber.
    flui_testing::log_capture::disarm_interest_cache();

    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));

    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        let mounted =
            binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        assert!(mounted.painted, "the bootstrap frame must commit a paint");

        // The bootstrap runs the pipeline directly, outside `drive_frame`, so
        // it is not a frame: nothing may be attributed to a frame that never
        // opened. An empty history here is the honest reading.
        assert!(
            profiler.frame_history().is_empty(),
            "the bootstrap is not a frame; got {:?}",
            profiler.frame_history()
        );

        // An idle frame: build and layout run (their spans are unconditional),
        // there is nothing to paint.
        binding.pump_frame(Duration::from_millis(16));

        // Dirty the root and pump again: this frame paints.
        binding
            .pipeline_owner()
            .expect("mount_root leaves the binding tree-bound")
            .with_mut(|owner| owner.mark_needs_paint(mounted.render_root));
        binding.pump_frame(Duration::from_millis(16));
        assert!(
            binding.did_paint_last_frame(),
            "marking the root dirty must make the second pump paint"
        );
    });

    let history = profiler.frame_history();
    assert_eq!(
        history.len(),
        2,
        "one profiled frame per `drive_frame`, and only those; got {history:?}"
    );
    for (index, frame) in history.iter().enumerate() {
        let seen = phases(frame);
        assert!(
            seen.contains(&FramePhase::Build),
            "frame {index}: the pipeline's `build` span must land as Build; saw {seen:?}"
        );
        assert!(
            seen.contains(&FramePhase::Layout),
            "frame {index}: the pipeline's `layout` span must land as Layout; saw {seen:?}"
        );
        assert!(
            frame.total_time_ms() > 0.0,
            "frame {index}: a driven frame takes time; got {frame:?}"
        );
    }
    let painted = phases(&history[1]);
    assert!(
        painted.contains(&FramePhase::Paint),
        "the dirtied frame's `paint` span must land as Paint; saw {painted:?}"
    );
}

#[test]
fn a_real_animation_reaches_the_profiler_with_its_work() {
    use flui_sdk::animation::{Animation, AnimationController, MotionSpec, SpringDescription};
    use flui_sdk::foundation::Listenable;
    use std::cell::Cell;
    use std::rc::Rc;

    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
    let notifications = Rc::new(Cell::new(0));
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        for source in ["time", "spring", "repeat"] {
            let mut binding = HeadlessBinding::new();
            let mounted =
                binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
            let owner = AnimationController::builder(Duration::from_millis(100))
                .build_on(Some(binding.vsync()));
            let controller = owner.controller();
            let pipeline = binding.pipeline_owner().expect("mounted pipeline").clone();
            let delivered = Rc::clone(&notifications);
            controller.add_listener(Rc::new(move || {
                delivered.set(delivered.get() + 1);
                pipeline.with_mut(|pipeline| pipeline.mark_needs_paint(mounted.render_root));
            }));
            let run = match source {
                "time" => controller.forward().expect("registered run"),
                "spring" => controller
                    .retarget(
                        1.0,
                        &MotionSpec::Spring(
                            SpringDescription::with_damping_ratio(1.0, 100.0, 1.0).expect("spring"),
                        ),
                    )
                    .expect("spring run"),
                "repeat" => controller
                    .repeat(
                        flui_sdk::animation::RepeatSpec::new(
                            flui_sdk::animation::RepeatMode::Restart,
                        )
                        .with_period(Duration::from_millis(100))
                        .with_iterations(flui_sdk::animation::Iterations::Finite(1)),
                    )
                    .expect("finite repeat"),
                _ => unreachable!(),
            };
            binding.pump_frame(Duration::ZERO);
            binding.pump_frame(Duration::from_millis(50));
            if source == "spring" {
                assert!(controller.value() > 0.0 && controller.value() < 1.0);
            } else {
                assert!((controller.value() - 0.5).abs() < 1e-9);
            }
            assert!(
                binding.did_paint_last_frame(),
                "the animation listener must produce paint"
            );
            assert!(notifications.get() >= 2);
            assert!(controller.is_animating());
            let frame = profiler.frame_stats().expect("real pumped frame");
            for phase in [
                "Animation tick",
                "Animation sampling",
                "Animation listeners",
            ] {
                assert!(
                    frame.phase(FramePhase::Custom(phase)).is_some(),
                    "a real progressing animation must profile {phase}: {frame:?}"
                );
            }
            binding.pump_frame(Duration::from_secs(5));
            assert!(run.is_complete());
            binding.pump_frame(Duration::from_millis(50));
            let idle = profiler.frame_stats().expect("idle frame");
            assert!(
                idle.phase(FramePhase::Custom("Animation sampling"))
                    .is_none()
            );
            assert!(
                idle.phase(FramePhase::Custom("Animation listeners"))
                    .is_none()
            );
        }
    });
}

fn a_nested_runtime_cannot_blend_its_animation_into_the_owning_frame() {
    use flui_sdk::animation::{Animation, AnimationController};
    use flui_sdk::view::PostFrameHandle;
    use std::{cell::RefCell, rc::Rc};
    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut outer = HeadlessBinding::new();
        outer.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        let mut inner = HeadlessBinding::new();
        inner.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        let owner =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(inner.vsync()));
        let controller = owner.controller().clone();
        let run = controller.forward().expect("inner run");
        inner.pump_frame(Duration::ZERO);
        let inner = Rc::new(RefCell::new(inner));
        let nested = Rc::clone(&inner);
        let previous_frames = profiler.frame_history().len();
        PostFrameHandle::new(outer.scheduler())
            .schedule(move |_| {
                nested.borrow_mut().pump_frame(Duration::from_millis(50));
                assert!((controller.value() - 0.5).abs() < 1e-9);
                assert!(!run.is_complete());
            })
            .expect("nested runtime callback");
        outer.pump_frame(Duration::from_millis(16));
        // Keep foreign owners alive through the outer frame: cancellation in
        // its post-frame callback would be real outer delivery work.
        drop(owner);
        drop(inner);
        let history = profiler.frame_history();
        assert_eq!(
            history.len(),
            previous_frames + 1,
            "the nested frame is dropped whole"
        );
        let owning = history.last().expect("outer frame");
        assert!(owning.phase(FramePhase::Custom("Animation tick")).is_some());
        assert!(
            owning
                .phase(FramePhase::Custom("Animation sampling"))
                .is_none(),
            "inner sampling must not blend into an idle outer frame: {owning:?}"
        );
        assert!(
            owning
                .phase(FramePhase::Custom("Animation listeners"))
                .is_none(),
            "inner delivery must not blend into an idle outer frame: {owning:?}"
        );
    });
}

fn a_runtime_driven_fade_reaches_the_profiler() {
    use flui_sdk::animation::{Animation, AnimationController, Vsync};
    use flui_sdk::rendering::BoxConstraints;
    use flui_sdk::widgets::{FadeTransition, SizedBox};
    use std::rc::Rc;

    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let registry = Vsync::new();
        let owner =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let controller = owner.controller();
        let mut laid = flui_testing::widgets::lay_out_animated(
            FadeTransition::new(Rc::new(controller.clone()), SizedBox::new(32.0, 32.0)),
            BoxConstraints::tight(flui_sdk::geometry::Size::new(64.0, 64.0)),
            registry,
        );
        let run = controller.forward().expect("runtime fade");
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(50));
        assert!((controller.value() - 0.5).abs() < 1e-9);
        assert!((laid.opacity(laid.root()) - 0.5).abs() < 1e-9);
        assert!(laid.did_paint_last_frame());
        let frame = profiler.frame_stats().expect("runtime frame");
        for phase in [
            "Animation tick",
            "Animation sampling",
            "Animation listeners",
            "Compositing",
        ] {
            assert!(
                frame.phase(FramePhase::Custom(phase)).is_some(),
                "runtime {phase}: {frame:?}"
            );
        }
        assert!(frame.phase(FramePhase::Paint).is_some());
        laid.pump_for(Duration::from_millis(50));
        assert!(run.is_complete());
    });
}

fn zero_history_capacity_keeps_counters_without_frames() {
    let profiler = Profiler::with_config(flui_devtools::ProfilerConfig {
        max_frame_history: 0,
        jank_threshold_ms: -1.0,
    });
    for _ in 0..2 {
        profiler.begin_frame();
        let phase = profiler.profile_phase(FramePhase::Build);
        drop(phase);
        profiler.end_frame();
        assert!(profiler.frame_history().is_empty());
        assert!(profiler.frame_stats().is_none());
    }
    assert_eq!(profiler.jank_percentage(), 100.0);
}

fn retained_rendering_spans_cannot_charge_a_later_frame() {
    use flui_sdk::view::PostFrameHandle;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use tracing::Subscriber;
    use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

    struct Retain {
        armed: Arc<AtomicBool>,
        spans: Arc<Mutex<Vec<tracing::Span>>>,
    }
    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Retain {
        fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
            if self.armed.load(Ordering::Relaxed)
                && ctx.span(id).is_some_and(|span| {
                    matches!(span.name(), "build" | "layout" | "paint" | "compositing")
                })
            {
                self.spans
                    .lock()
                    .expect("retained producer spans")
                    .push(tracing::Span::current());
            }
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let armed = Arc::new(AtomicBool::new(false));
    let retained = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry()
        .with(FrameTimingLayer::new(Arc::clone(&profiler)))
        .with(Retain {
            armed: Arc::clone(&armed),
            spans: Arc::clone(&retained),
        });
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        let mounted =
            binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        binding
            .pipeline_owner()
            .expect("mounted owner")
            .with_mut(|owner| {
                owner.mark_needs_layout(mounted.render_root);
                owner.mark_needs_compositing_bits_update(mounted.render_root);
                owner.mark_needs_paint(mounted.render_root);
            });
        armed.store(true, Ordering::Relaxed);
        binding.pump_frame(Duration::from_millis(16));
        armed.store(false, Ordering::Relaxed);
        let original = profiler.frame_stats().expect("rendered frame");
        let current_is_complete = [
            FramePhase::Build,
            FramePhase::Layout,
            FramePhase::Paint,
            FramePhase::Custom("Compositing"),
        ]
        .into_iter()
        .all(|phase| {
            original
                .phase(phase)
                .is_some_and(|info| info.duration <= original.total_time)
        });
        for name in ["build", "layout", "paint", "compositing"] {
            assert!(
                retained
                    .lock()
                    .expect("retained phases")
                    .iter()
                    .any(|span| span
                        .metadata()
                        .is_some_and(|metadata| metadata.name() == name)),
                "retained real {name} producer"
            );
        }
        std::thread::sleep(original.total_time + Duration::from_millis(20));
        PostFrameHandle::new(binding.scheduler())
            .schedule(move |_| {
                let spans = std::mem::take(&mut *retained.lock().expect("retained phases"));
                drop(spans);
            })
            .expect("retire old rendering spans");
        binding.pump_frame(Duration::from_millis(16));
        let later = profiler.frame_stats().expect("later idle frame");
        assert_eq!(
            (
                current_is_complete,
                later.phase(FramePhase::Paint).is_none()
            ),
            (true, true),
            "original: {original:?}; later: {later:?}"
        );
    });
}

fn application_frame_names_do_not_hide_real_animation_work() {
    use flui_sdk::animation::{Animation, AnimationController};
    use flui_sdk::view::PostFrameHandle;

    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::layer::Identity::new())
        .with(FrameTimingLayer::new(Arc::clone(&profiler)));
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        let controller = AnimationController::builder(Duration::from_millis(100)).build();
        let run = controller.forward().expect("manual run");
        PostFrameHandle::new(binding.scheduler())
            .schedule(move |_| {
                let frame = tracing::Span::current();
                {
                    let _reentered = frame.enter();
                }
                let _application = tracing::debug_span!(target: "application", "frame").entered();
                controller.tick_at(Duration::from_millis(50));
                assert!((controller.value() - 0.5).abs() < 1e-9);
                assert!(!run.is_complete());
            })
            .expect("animation callback");
        binding.pump_frame(Duration::from_millis(16));
        let frame = profiler.frame_stats().expect("scheduler frame");
        for phase in ["Animation sampling", "Animation listeners"] {
            assert!(
                frame.phase(FramePhase::Custom(phase)).is_some(),
                "{phase}: {frame:?}"
            );
        }
        let previous_frames = profiler.frame_history().len();
        {
            let _application = tracing::debug_span!(target: "application", "frame").entered();
        }
        assert_eq!(profiler.frame_history().len(), previous_frames);
    });
}

fn retained_sampling_spans_end_at_exit_and_do_not_charge_a_later_frame() {
    use flui_sdk::animation::{Animation, AnimationController, Curve};
    use flui_sdk::view::PostFrameHandle;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct RetainingCurve(Rc<RefCell<Option<tracing::Span>>>, Rc<Cell<Duration>>);
    impl Curve for RetainingCurve {
        fn transform(&self, t: f64) -> f64 {
            let started = std::time::Instant::now();
            let span = tracing::Span::current();
            {
                let _reentered = span.enter();
            }
            std::thread::sleep(Duration::from_millis(20));
            self.1.set(started.elapsed());
            *self.0.borrow_mut() = Some(span);
            t.clamp(0.0, 1.0)
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        let retained = Rc::new(RefCell::new(None));
        let source_time = Rc::new(Cell::new(Duration::ZERO));
        let owner = AnimationController::builder(Duration::from_millis(100))
            .build_on(Some(binding.vsync()));
        let controller = owner.controller();
        let run = controller
            .animate_to_curved(
                1.0,
                None,
                RetainingCurve(Rc::clone(&retained), Rc::clone(&source_time)),
            )
            .expect("curved run");
        binding.pump_frame(Duration::ZERO);
        binding.pump_frame(Duration::from_millis(50));
        assert!(!run.is_complete());
        assert!((controller.value() - 0.5).abs() < 1e-9);
        let frame = profiler.frame_stats().expect("sampled frame");
        let sampling = frame.phase(FramePhase::Custom("Animation sampling"));
        let current_is_correct = sampling.is_some_and(|phase| {
            phase.duration >= source_time.get() && phase.duration <= frame.total_time
        });
        assert!(retained.borrow().is_some());
        controller.stop().expect("stop before the next frame");
        assert!(run.is_canceled());
        PostFrameHandle::new(binding.scheduler())
            .schedule(move |_| {
                let span = retained
                    .borrow_mut()
                    .take()
                    .expect("retained sampling span");
                let _late_entry = span.enter();
            })
            .expect("release retained span in next frame");
        binding.pump_frame(Duration::from_millis(16));
        let next = profiler.frame_stats().expect("next frame");
        assert_eq!(
            (
                current_is_correct,
                next.phase(FramePhase::Custom("Animation sampling"))
                    .is_none()
            ),
            (true, true),
            "current: {frame:?}; later: {next:?}"
        );
    });
}

fn a_closed_sampling_span_cannot_charge_listener_work_in_the_same_frame() {
    use flui_sdk::animation::{Animation, AnimationController, Curve};
    use flui_sdk::foundation::Listenable;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Instant,
    };
    use tracing::Subscriber;
    use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

    #[derive(Clone)]
    struct Boundary {
        before: bool,
        armed: Arc<AtomicBool>,
        started: Arc<Mutex<Option<(tracing::span::Id, Instant)>>>,
        elapsed: Arc<Mutex<Duration>>,
    }
    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Boundary {
        fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
            if self.before
                && ctx.span(id).is_some_and(|span| {
                    span.name() == "animation_sampling"
                        && span.metadata().target() == "flui_animation"
                })
                && self.armed.swap(false, Ordering::Relaxed)
            {
                *self.started.lock().expect("sampling boundary") =
                    Some((id.clone(), Instant::now()));
            }
        }
        fn on_exit(&self, id: &tracing::span::Id, _: Context<'_, S>) {
            if !self.before {
                let start = {
                    let mut started = self.started.lock().expect("sampling boundary");
                    if started
                        .as_ref()
                        .is_some_and(|(candidate, _)| candidate == id)
                    {
                        started.take()
                    } else {
                        None
                    }
                };
                if let Some((_, started)) = start {
                    *self.elapsed.lock().expect("sampling extent") = started.elapsed();
                }
            }
        }
    }
    struct RetainingCurve(Rc<RefCell<Option<tracing::Span>>>);
    impl Curve for RetainingCurve {
        fn transform(&self, t: f64) -> f64 {
            *self.0.borrow_mut() = Some(tracing::Span::current());
            t.clamp(0.0, 1.0)
        }
    }

    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let boundary = Boundary {
        before: true,
        armed: Arc::new(AtomicBool::new(false)),
        started: Arc::new(Mutex::new(None)),
        elapsed: Arc::new(Mutex::new(Duration::ZERO)),
    };
    // Layer callbacks run from inner to outer. These independent clocks
    // enclose the producer's first enter/exit, including profiler callbacks.
    let subscriber = tracing_subscriber::registry()
        .with(boundary.clone())
        .with(FrameTimingLayer::new(Arc::clone(&profiler)))
        .with(Boundary {
            before: false,
            ..boundary.clone()
        });
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let mut binding = HeadlessBinding::new();
        binding.mount_root(&Leaf, MountOwners::fresh(), MountOptions::tight(64.0, 64.0));
        let retained = Rc::new(RefCell::new(None));
        let owner = AnimationController::builder(Duration::from_millis(100))
            .build_on(Some(binding.vsync()));
        let controller = owner.controller();
        let run = controller
            .animate_to_curved(1.0, None, RetainingCurve(Rc::clone(&retained)))
            .expect("curved run");
        let visit = Rc::clone(&retained);
        let elapsed = Arc::clone(&boundary.elapsed);
        let calls = Rc::new(Cell::new(0));
        let delivered = Rc::clone(&calls);
        controller.add_listener(Rc::new(move || {
            let span = visit.borrow().clone();
            if let Some(span) = span {
                let envelope = *elapsed.lock().expect("completed producer extent");
                assert!(!envelope.is_zero());
                let _entered = span.enter();
                // The later visit exceeds the measured first visit on any host;
                // counting it as sampling must exceed that original envelope.
                std::thread::sleep(envelope + Duration::from_millis(20));
                delivered.set(delivered.get() + 1);
            }
        }));
        binding.pump_frame(Duration::ZERO);
        boundary.armed.store(true, Ordering::Relaxed);
        binding.pump_frame(Duration::from_millis(50));
        assert!((controller.value() - 0.5).abs() < 1e-9);
        assert!(!run.is_complete());
        assert_eq!(calls.get(), 1);
        let frame = profiler.frame_stats().expect("same scheduler frame");
        let sampling = frame
            .phase(FramePhase::Custom("Animation sampling"))
            .expect("real sampling");
        let envelope = *boundary.elapsed.lock().expect("producer extent");
        assert!(
            sampling.duration <= envelope,
            "sampling includes a closed span's listener visit: {sampling:?}; producer: {envelope:?}"
        );
        assert!(
            frame
                .phase(FramePhase::Custom("Animation listeners"))
                .expect("real listener work")
                .duration
                > envelope
        );
    });
}

fn animation_phase_names_and_inclusive_extents_are_respected() {
    fn phase_span(name: &str) -> tracing::Span {
        match name {
            "Animation tick" => tracing::debug_span!(target: "flui_animation", "animation_tick"),
            "Animation sampling" => {
                tracing::debug_span!(target: "flui_animation", "animation_sampling")
            }
            "Animation listeners" => {
                tracing::debug_span!(target: "flui_animation", "animation_listeners")
            }
            _ => unreachable!(),
        }
    }
    // Consumer-only spans pin filtering and inclusive timing. The real
    // controller and mounted FadeTransition rows pin the producer separately.
    for name in [
        "Animation tick",
        "Animation sampling",
        "Animation listeners",
    ] {
        flui_testing::log_capture::disarm_interest_cache();
        let profiler = Arc::new(Profiler::new());
        let subscriber =
            tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
            let _frame =
                tracing::debug_span!(target: "flui_scheduler::scheduler::execution", "frame")
                    .entered();
            let _outer = phase_span(name).entered();
            let _nested = phase_span(name).entered();
            std::thread::sleep(Duration::from_millis(20));
        });
        let frame = profiler.frame_stats().expect("consumer frame");
        let measured = frame.phase(FramePhase::Custom(name)).expect("phase extent");
        assert!(
            measured.duration <= frame.total_time,
            "overlapping {name} spans cannot exceed their enclosing frame: {frame:?}"
        );
    }
    flui_testing::log_capture::disarm_interest_cache();
    let profiler = Arc::new(Profiler::new());
    let subscriber =
        tracing_subscriber::registry().with(FrameTimingLayer::new(Arc::clone(&profiler)));
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        let _frame =
            tracing::debug_span!(target: "flui_scheduler::scheduler::execution", "frame").entered();
        let _foreign = tracing::debug_span!(target: "application", "animation_sampling").entered();
    });
    assert!(
        profiler
            .frame_stats()
            .expect("filtered frame")
            .phase(FramePhase::Custom("Animation sampling"))
            .is_none()
    );
}

#[cfg(feature = "timeline")]
fn a_guard_before_clear_cannot_update_a_new_event() {
    use flui_devtools::timeline::{EventCategory, Timeline};
    let timeline = Timeline::new();
    let old = timeline.record_event("old", EventCategory::Custom);
    timeline.clear();
    let duration = Duration::from_secs(123_456_789);
    timeline.record_completed_event(
        "new",
        EventCategory::Custom,
        web_time::Instant::now(),
        duration,
        serde_json::Value::Null,
    );
    drop(old);
    let events = timeline.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "new");
    assert_eq!(events[0].duration_micros, duration.as_micros());
}

#[cfg(feature = "timeline")]
fn a_guard_after_clear_updates_its_own_event() {
    use flui_devtools::timeline::{EventCategory, Timeline};
    let timeline = Timeline::new();
    timeline.record_instant("old", EventCategory::Custom);
    timeline.clear();
    let current = timeline.record_event("current", EventCategory::Custom);
    std::thread::sleep(Duration::from_millis(1));
    drop(current);
    let events = timeline.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "current");
    assert!(events[0].duration_micros > 0);
}

#[cfg(feature = "timeline")]
fn completed_event_duration_preserves_large_values() {
    use flui_devtools::timeline::{EventCategory, Timeline};
    let timeline = Timeline::new();
    let duration = Duration::new(18_446_744_073_710, 123_456_000);
    timeline.record_completed_event(
        "large",
        EventCategory::Custom,
        web_time::Instant::now(),
        duration,
        serde_json::Value::Null,
    );
    assert_eq!(timeline.get_events()[0].duration(), duration);
    let mut event = timeline.get_events().remove(0);
    event.duration_micros = u128::MAX;
    assert_eq!(event.duration(), Duration::MAX);
}

#[cfg(feature = "timeline")]
fn run_reentrant_name_case(kind: &str) {
    use flui_devtools::timeline::{EventCategory, Timeline};
    struct Name(Timeline);
    impl From<Name> for String {
        fn from(name: Name) -> Self {
            name.0.clear();
            "reentrant".to_owned()
        }
    }
    let timeline = Timeline::new();
    timeline.record_instant("removed", EventCategory::Custom);
    let name = Name(timeline.clone());
    match kind {
        "guard" => drop(timeline.record_event(name, EventCategory::Custom)),
        "instant" => timeline.record_instant(name, EventCategory::Custom),
        "completed" => timeline.record_completed_event(
            name,
            EventCategory::Custom,
            web_time::Instant::now(),
            Duration::from_secs(7),
            serde_json::Value::Null,
        ),
        _ => panic!("unknown name case"),
    }
    let events = timeline.get_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].name, "reentrant");
    timeline.record_instant("next", EventCategory::Custom);
    assert_eq!(timeline.event_count(), 2);
}

#[cfg(feature = "timeline")]
fn isolated_reentrant_name(kind: &str) {
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "developer_history_respects_capacity_and_clear",
            "--nocapture",
        ])
        .env("FLUI_TIMELINE_NAME_CASE", kind)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn timeline consumer child");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().expect("child status").is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill blocked child");
            let output = child.wait_with_output().expect("reap blocked child");
            panic!("name conversion {kind} blocked: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("consumer output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "name conversion {kind} failed: {output:?}"
    );
}

#[cfg(feature = "timeline")]
fn guard_name_conversion_can_reenter_the_timeline() {
    isolated_reentrant_name("guard");
}
#[cfg(feature = "timeline")]
fn instant_name_conversion_can_reenter_the_timeline() {
    isolated_reentrant_name("instant");
}
#[cfg(feature = "timeline")]
fn completed_name_conversion_can_reenter_the_timeline() {
    isolated_reentrant_name("completed");
}

#[test]
fn developer_history_respects_capacity_and_clear() {
    #[cfg(feature = "timeline")]
    if let Ok(kind) = std::env::var("FLUI_TIMELINE_NAME_CASE") {
        run_reentrant_name_case(&kind);
        return;
    }
    let cases: &[(&str, fn())] = &[
        (
            "retained_rendering_spans",
            retained_rendering_spans_cannot_charge_a_later_frame,
        ),
        (
            "closed_sampling_span_in_same_frame",
            a_closed_sampling_span_cannot_charge_listener_work_in_the_same_frame,
        ),
        (
            "zero_capacity",
            zero_history_capacity_keeps_counters_without_frames,
        ),
        (
            "nested_runtime_animation",
            a_nested_runtime_cannot_blend_its_animation_into_the_owning_frame,
        ),
        (
            "runtime_fade_animation",
            a_runtime_driven_fade_reaches_the_profiler,
        ),
        (
            "animation_phase_contract",
            animation_phase_names_and_inclusive_extents_are_respected,
        ),
        (
            "application_frame_name",
            application_frame_names_do_not_hide_real_animation_work,
        ),
        (
            "retained_sampling_span",
            retained_sampling_spans_end_at_exit_and_do_not_charge_a_later_frame,
        ),
        #[cfg(feature = "timeline")]
        (
            "stale_guard",
            a_guard_before_clear_cannot_update_a_new_event,
        ),
        #[cfg(feature = "timeline")]
        ("fresh_guard", a_guard_after_clear_updates_its_own_event),
        #[cfg(feature = "timeline")]
        (
            "large_duration",
            completed_event_duration_preserves_large_values,
        ),
        #[cfg(feature = "timeline")]
        (
            "reentrant_guard_name",
            guard_name_conversion_can_reenter_the_timeline,
        ),
        #[cfg(feature = "timeline")]
        (
            "reentrant_instant_name",
            instant_name_conversion_can_reenter_the_timeline,
        ),
        #[cfg(feature = "timeline")]
        (
            "reentrant_completed_name",
            completed_name_conversion_can_reenter_the_timeline,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed cases: {failures:?}");
}
