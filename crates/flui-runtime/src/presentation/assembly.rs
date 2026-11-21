//! Construction of a presentation: the gesture and haptics assembly shared by
//! every constructor, and the production and test constructors.

use std::cell::{Cell, RefCell};
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_animation::__runtime::VsyncDriver;
use flui_animation::MotionClock;
use flui_foundation::PresentationId;
use flui_interaction::{binding::GestureBinding, focus::FocusManager, text_input::TextInputOwner};
use flui_platform::PlatformWindow;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::pipeline::PipelineOwner;
use flui_scheduler::{ClockSource, FrameClock, FramePhaseDurations};
use flui_view::__runtime::FramePhaseMarker;
use flui_view::__runtime::{CloseGuardSource, WidgetsBinding};

#[cfg(test)]
use super::test_platform_window;
use super::{
    FailureRetry, PointerResampling, PresentationLifecycle, PresentationState, PresentationWindow,
    RuntimeCapabilities, show_cursor,
};
use crate::epoch::TreeRevision;
use crate::frame_failure::SegmentPhase;
use crate::held_input::HeldPointerQueue;
use crate::occluded_frame::{OccludedFrame, Session};
use crate::renderer_binding::RenderingBinding;
use crate::semantics_host::SemanticsHost;
use crate::ui_runtime::text_preparation::TextPreparation;
#[cfg(test)]
use flui_platform::PlatformTextInput;
#[cfg(test)]
use flui_semantics::platform::PlatformAccessibility;

impl PresentationState {
    /// Assemble the gesture arena, wiring its mouse-tracker cursor callback
    /// to `window` (shared by every constructor below — production and
    /// test alike — since the callback shape never varies with capability
    /// wiring).
    fn build_gestures(
        id: PresentationId,
        window: &Rc<dyn PlatformWindow>,
        clock: &ClockSource,
        preset: flui_interaction::gesture::GestureProfiles,
    ) -> GestureBinding {
        // The arena's deadlines read the ui_runtime's clock: a recognizer's
        // timeout and the frame that polls it share one timeline.
        let gestures = GestureBinding::with_settings_and_clock(preset, Arc::new(clock.clone()));
        flui_interaction::__runtime::set_pointer_capture_wake(&gestures, Rc::downgrade(window));
        let cursor_window = Rc::downgrade(window);
        gestures
            .mouse_tracker()
            .set_cursor_change_callback(Rc::new(move |device_id, cursor| {
                let Some(window) = cursor_window.upgrade() else {
                    tracing::trace!(
                        ?id,
                        ?device_id,
                        ?cursor,
                        "dropping cursor update after the platform window closed"
                    );
                    return;
                };
                show_cursor(&*window, cursor, id, device_id);
            }));
        gestures
    }

    /// Assemble the presentation's haptics on its gesture clock: it plays on
    /// the window's actuator, reached weakly, and its frame deadline follows
    /// the refresh period of the display the host last delivered for the
    /// window (ADR-0210 §4, ADR-0236 §6).
    fn build_haptics(
        window: &Rc<dyn PlatformWindow>,
        display: &Rc<Cell<Option<flui_platform::WindowDisplay>>>,
        gestures: &GestureBinding,
        request_frame: impl Fn() + 'static,
    ) -> flui_interaction::haptics::HapticsHost {
        let actuator = Rc::downgrade(window);
        let display = Rc::downgrade(display);
        let haptics = flui_interaction::haptics::HapticsHost::builder(gestures, move || {
            actuator.upgrade().and_then(|window| window.haptics())
        })
        .frame_requester(request_frame)
        .refresh_period(move || {
            display
                .upgrade()
                .and_then(|display| display.get())
                .and_then(|display| display.refresh_period)
                .map(flui_platform::RefreshPeriod::get)
        })
        .deadline_factor(gestures.default_profiles().haptic_frame_deadline_factor())
        .build();
        let snapshot = window.state();
        haptics.set_visible(
            snapshot.is_seen()
                && snapshot.execution == flui_platform::WindowExecutionState::Running,
        );
        haptics
    }

    /// Assemble a presentation wired into a UI runtime (ADR-0043 §1): installs
    /// `capabilities.global_key_scope` FIRST, then the UI runtime's shared
    /// dispatch handles, before this presentation's own focus/IME are
    /// wired to its fresh [`WidgetsBinding`] and [`RenderingBinding`]
    /// — all before the caller ever attaches/mounts a root widget.
    ///
    /// Builds the presentation's pipeline here, from the UI runtime's text
    /// context, so no presentation pipeline measures on any other; a
    /// `device_pixel_ratio` of `None` keeps the pipeline's default of `1.0`.
    pub(crate) fn new(
        id: PresentationId,
        device_pixel_ratio: Option<flui_foundation::geometry::DevicePixelRatio>,
        window: impl Into<PresentationWindow>,
        capabilities: RuntimeCapabilities<'_>,
    ) -> Self {
        let PresentationWindow {
            window,
            accessibility,
            text_store_host,
            pointer_resampling,
            gesture_profiles,
            platform,
            occluded_surface,
        } = window.into();
        let pipeline = PipelineCell::new(PipelineOwner::new(capabilities.text));
        pipeline.withdraw_layout();
        if let Some(device_pixel_ratio) = device_pixel_ratio {
            pipeline.with_mut(|owner| owner.set_device_pixel_ratio(device_pixel_ratio));
        }
        let gestures = Self::build_gestures(id, &window, capabilities.clock, gesture_profiles);
        gestures
            .set_resampling_enabled(pointer_resampling == PointerResampling::FrameAligned)
            .expect("BUG: newly assembled presentation has no active contacts");
        let interaction_dispatch = flui_interaction::__runtime::presentation_dispatch(
            &capabilities.interaction_dispatch_handle,
        );
        let frame_clock = FrameClock::with_source(capabilities.clock.clone());
        let alive = Rc::new(());
        let focus = FocusManager::new();
        let text_input = TextInputOwner::new(PresentationWindow::text_input_backend(
            window.as_ref(),
            text_store_host,
        ));

        // Frames and retirements wake the runtime so the raster executor applies them.
        let textures = flui_layer::TextureRegistry::new(Arc::clone(&capabilities.wake));
        let (window_closer, window_lifetime) = flui_platform::capability::WindowCloser::new();
        let close_guard = CloseGuardSource::new(
            &capabilities.close.vetoes,
            capabilities.close.notify.clone(),
        );
        let url_owner = flui_view::__runtime::UrlOwnerSlot::new();
        let widgets = WidgetsBinding::with_focus_manager(Rc::clone(&focus));
        // A window nobody reads through collects no semantics: the pipeline
        // starts with them off, and `reconcile_semantics_enablement` turns
        // both on together.
        widgets.set_collects_semantics(false);
        widgets.set_pipeline_owner(pipeline.clone());
        widgets.with_build_owner_mut(|owner| {
            owner.set_global_key_scope(capabilities.global_key_scope);
            owner.set_build_in_progress(capabilities.build_in_progress);
            owner.set_async_driver(capabilities.async_driver);
            owner.set_post_frame_handle(capabilities.completion_scope.post_frame_handle());
            owner.set_interaction_dispatch_handle(interaction_dispatch.clone());
            if let Some(registry) = capabilities.close.flush_registry.clone() {
                owner.set_flush_registry(registry);
            }
            let presentation = owner.capabilities_mut();
            presentation.text_input = Some(text_input.handle());
            presentation.textures = Some(textures.clone());
            presentation.capabilities =
                Some(Rc::new(crate::capability::PresentationCapabilities {
                    registry: capabilities.capabilities,
                    completions: capabilities.completions,
                    locals: capabilities.locals,
                    window: Rc::downgrade(&window),
                    lifetime: window_lifetime,
                }));
            presentation.close_guard = Some(close_guard.guard());
            presentation.url_owner = Some(url_owner.clone());
            // Paired here, the one place holding both halves: the ui_runtime's
            // dispatch ticket (identity) and THIS presentation's pipeline
            // (the tree). A ui_runtime may host several presentations, each with
            // its own `PipelineOwner`, so a probe installed once per ui_runtime
            // would answer every presentation with the first one's tree.
            owner.set_hit_test_handle(flui_interaction::pointer::HitTestHandle::new(
                interaction_dispatch.clone(),
                Rc::new(
                    flui_rendering::pipeline::hit_test_probe::PipelineHitTestProbe::new(
                        &pipeline,
                        Rc::downgrade(&alive),
                    ),
                ),
            ));
        });

        let renderer = RenderingBinding::new_with_pipeline(pipeline.clone());

        // Idle-wake wiring: a dirty mark (mark_needs_layout / mark_needs_paint)
        // fires this callback so a quiescent event loop produces the frame.
        // Reentrancy-safe: the callback fires while the CALLER holds the
        // pipeline cell checked out, and the scheduler's `ensure_visual_update`
        // only touches `Send + Sync` runtime-level state (its own atomic phase
        // and owner thread, and, briefly, the `on_frame_scheduled` slot — none of
        // which is held across a call back into the pipeline) — never this
        // presentation's own `widgets` / `renderer` / gesture state.
        //
        // Routes through the scheduler's phase gate so this presentation's
        // demand is subject to the SAME gate (and the SAME `frame_scheduled`
        // edge) as ticker/`end_of_frame` demand: a dirty mark issued mid-frame
        // on the driving thread no longer reaches `request_redraw` — the
        // in-flight frame's own surplus-frame guard is what picks it up. The
        // ui_runtime-wide wake still happens through the `on_frame_scheduled` hook
        // (`UiRuntime::construct` wires it to this same `wake`), so the closure
        // no longer calls `wake` directly.
        //
        // Pokes this presentation's own window through its redraw requester:
        // a mark can come from any thread (a signal written on a worker), and
        // the window is the owner thread's (ADR-0254 §5). The runtime wake
        // targets its primary window; a runtime with several presentations
        // still needs this addressed secondary wake. It is gated on
        // `ensure_visual_update`'s return so it fires only when the scheduler
        // actually accepted the demand.
        //
        // One poke per frame, not one per mark: a hundred state changes before
        // the next frame make one platform request. The latch reopens when the
        // next pump samples this presentation (`release_redraw_poke`), and a
        // demand the scheduler declined does not close it.
        let scheduler = capabilities.scheduler.frame_waker();
        let redraw_window = window.redraw_requester();
        let redraw_poked = Arc::new(AtomicBool::new(false));
        let poked = Arc::clone(&redraw_poked);
        let request_frame = Arc::new(move || {
            if scheduler.ensure_visual_update() && !poked.swap(true, Ordering::AcqRel) {
                redraw_window.request_redraw();
            }
        });
        let vsync_driver = VsyncDriver::new();
        // Controllers ticked here refuse mutation while this runtime builds.
        vsync_driver.set_build_phase(Some(
            widgets.with_build_owner(flui_view::__runtime::BuildOwner::build_phase),
        ));
        let request_animation_frame = Arc::clone(&request_frame);
        let animation_owner = Rc::downgrade(&alive);
        vsync_driver.set_frame_requester(Some(Rc::new(move || {
            if animation_owner.upgrade().is_some() {
                request_animation_frame();
            }
        })));
        // Install before mounting any element: rebuild handles capture this
        // hook at creation. Every presentation needs the same scheduling edge,
        // including those assembled without a desktop runner.
        widgets.with_build_owner_mut(|owner| {
            let request_frame = Arc::clone(&request_frame);
            owner.set_on_build_scheduled(move || request_frame());
        });
        {
            let request_frame = Arc::clone(&request_frame);
            widgets.set_on_need_frame(move || request_frame());
        }
        let window_display = Rc::new(Cell::new(None));
        let haptics = Self::build_haptics(&window, &window_display, &gestures, {
            let request_frame = Arc::clone(&request_frame);
            move || request_frame()
        });
        widgets.with_build_owner_mut(|owner| {
            owner.capabilities_mut().haptics = Some(haptics.handle());
        });
        // Announcements queue on the pipeline, which keeps them across
        // accessibility being switched off and on, and request this
        // presentation's frame so an idle window still speaks.
        let announcer = pipeline
            .with_mut(flui_rendering::pipeline::PipelineOwner::announcements)
            .map(|announcements| {
                let request_frame = Arc::clone(&request_frame);
                announcements.announcer(Arc::new(move || request_frame()))
            });
        widgets.with_build_owner_mut(|owner| {
            owner.capabilities_mut().announcer = announcer;
        });
        let drag_and_drop = flui_interaction::drag::DragAndDrop::new(
            capabilities.drag.clone(),
            id,
            Rc::downgrade(&window),
            gestures.mouse_tracker(),
        );
        widgets.with_build_owner_mut(|owner| {
            owner.capabilities_mut().drag_and_drop = Some(drag_and_drop);
        });
        pipeline.with_mut(|owner| {
            owner.set_on_need_visual_update(move || request_frame());
        });

        let semantics = SemanticsHost::new();

        // Whether this window's adapter says where the assistive technology's
        // cursor is: a widget that acts on it asks before relying on it.
        let assistive_focus = accessibility.as_ref().map_or(
            flui_semantics::platform::AssistiveFocusSupport::Unreported,
            |bridge| bridge.assistive_focus_support(),
        );
        widgets.with_build_owner_mut(|owner| {
            owner.capabilities_mut().assistive_focus = assistive_focus;
        });
        Self::wire_platform_accessibility(
            &window,
            accessibility.as_ref(),
            &pipeline,
            &semantics,
            &capabilities.wake,
            capabilities.command_sender,
        );

        let snapshot = window.state();
        let state = Self {
            id,
            platform,
            gesture_geometry: RefCell::new(
                crate::ui_runtime::preferences::GeometryProjection::default(),
            ),
            gesture_settings: flui_interaction::gesture::GestureSettingsSource::new(
                gestures.default_profiles(),
            ),
            media_query: Rc::new(crate::media_query_root::MediaQuerySource::from_window(
                window.as_ref(),
                pipeline.with(PipelineOwner::device_pixel_ratio).get(),
            )),
            window_visible: Cell::new(snapshot.is_seen()),
            window_focused: Cell::new(snapshot.focus.is_focused()),
            window_display,
            window_execution: Cell::new(snapshot.execution),
            closing_requested: Cell::new(false),
            window_closer,
            overlay_style: crate::overlay_style::OverlayStyleCoalescer::default(),
            scene: crate::presentation_scene::SceneSlot::default(),
            textures,
            lifecycle: Cell::new(PresentationLifecycle::Created),
            close_mode: Cell::new(flui_interaction::__runtime::CloseMode::Ordinary),
            withdrawn_dispatch: RefCell::new(None),
            frame_request: Cell::new(None),
            keyboard: Cell::new(snapshot.insets.keyboard()),
            pipeline,
            completion_scope: Some(capabilities.completion_scope),
            text_preparation: RefCell::new(TextPreparation::default()),
            sampled_motion_tick: Cell::new(None),
            frame_rate: Cell::default(),
            alive: RefCell::new(Some(alive)),
            window_id: window.id(),
            window: Rc::downgrade(&window),
            accessibility: accessibility.as_ref().map(Arc::downgrade),
            gestures,
            haptics,
            wheel_preferences: flui_interaction::pointer::WheelPreferencesSource::new(
                flui_platform::WheelPreferences::default(),
            ),
            interaction_dispatch: Some(interaction_dispatch),
            held_pointer_input: RefCell::new(HeldPointerQueue::new(id)),
            staged_input_regions: RefCell::default(),
            published_input_regions: RefCell::default(),
            focus,
            text_input,
            semantics,
            dev_agent: RefCell::new(None),
            agent_stability: RefCell::default(),
            agent_reads: RefCell::default(),
            agent_input: RefCell::default(),
            agent_waits: RefCell::default(),
            url_owner,
            agent_layer_reads: RefCell::default(),
            pending_frames: RefCell::default(),
            in_flight_limit: Cell::new(NonZeroUsize::MIN),
            agent_events: RefCell::default(),
            window_control: RefCell::new(None),
            window_placement: Cell::new(None),
            widgets,
            close_guard,
            renderer,
            frames_rendered: Cell::new(0),
            frames_dropped: Cell::new(0),
            not_shown_streak: Cell::new(0),
            withheld_reported: Cell::new(false),
            occluded_frame: Cell::new(OccludedFrame::NotOwed),
            owed_frame_asked: Rc::default(),
            owed_frame_served: Cell::new(false),
            occluded_surface,
            performance_overlay: Cell::new(false),
            redraw_pending: Cell::new(false),
            redraw_requests: Cell::new(0),
            redraw_requests_watched: Cell::new(0),
            vsync_driver,
            animation_retirement: RefCell::new(None),
            motion_clock: RefCell::new(MotionClock::new()),
            clock: frame_clock,
            redraw_poked,
            last_segment_span: Cell::new(None),
            segment_phases: Cell::new(FramePhaseDurations::default()),
            tree_revision: Cell::new(TreeRevision::ZERO),
            presented_revision: Cell::new(TreeRevision::ZERO),
            frame_failure_streak: Cell::new(0),
            failure_retry: Cell::new(FailureRetry::default()),
            device_recovery: Cell::new(None),
            segment_phase: FramePhaseMarker::new(SegmentPhase::Build),
            #[cfg(test)]
            segment_probe: RefCell::new(None),
            #[cfg(test)]
            flush_count: Cell::new(0),
        };
        state.attach_surface();
        state.clock.set_hidden(!state.window_visible.get());
        // A window opened covered owes its surface from the start; a runtime that installs the
        // presentation reads it again in its own session.
        state.follow_sight(Session::default(), state.clock.now());
        state
    }

    /// Standalone assembly with no UI runtime above it: this presentation's
    /// `WidgetsBinding` lazily self-owns a private `GlobalKeyScope` on first
    /// `GlobalKey` registration (never shared, so it never conflicts with
    /// anything).
    /// Used only by this module's own unit tests, which exercise
    /// presentation-local behavior (gestures/focus/haptics/overlay) in
    /// isolation; UI runtime-backed tests use [`Self::new`] through
    /// `UiRuntime::for_test`, exactly like production.
    #[cfg(test)]
    pub(crate) fn new_for_test_with_window(
        id: PresentationId,
        pipeline: PipelineCell,
        window: Rc<dyn PlatformWindow>,
    ) -> Self {
        let gestures = Self::build_gestures(
            id,
            &window,
            &ClockSource::Platform,
            flui_interaction::gesture::GestureSettings::default().into(),
        );
        let window_display = Rc::new(Cell::new(None));
        let haptics = Self::build_haptics(&window, &window_display, &gestures, || {});
        let alive = Rc::new(());
        let focus = FocusManager::new();
        let text_input = TextInputOwner::new(PresentationWindow::text_input_backend(
            window.as_ref(),
            None,
        ));

        let textures = flui_layer::TextureRegistry::new(Arc::new(|| {}));
        let url_owner = flui_view::__runtime::UrlOwnerSlot::new();
        let widgets = WidgetsBinding::with_focus_manager(Rc::clone(&focus));
        // A window nobody reads through collects no semantics: the pipeline
        // starts with them off, and `reconcile_semantics_enablement` turns
        // both on together.
        widgets.set_collects_semantics(false);
        widgets.set_pipeline_owner(pipeline.clone());
        widgets.with_build_owner_mut(|owner| {
            owner.capabilities_mut().url_owner = Some(url_owner.clone());
        });

        let renderer = RenderingBinding::new_with_pipeline(pipeline.clone());
        // This path wires no platform accessibility (see the doc above).
        let accessibility: Option<Arc<dyn PlatformAccessibility>> = None;

        let semantics = SemanticsHost::new();

        let snapshot = window.state();
        let state = Self {
            id,
            platform: flui_platform::TargetPlatform::Unknown,
            gesture_geometry: RefCell::new(
                crate::ui_runtime::preferences::GeometryProjection::default(),
            ),
            gesture_settings: flui_interaction::gesture::GestureSettingsSource::new(
                gestures.default_profiles(),
            ),
            media_query: Rc::new(crate::media_query_root::MediaQuerySource::from_window(
                window.as_ref(),
                pipeline.with(PipelineOwner::device_pixel_ratio).get(),
            )),
            window_visible: Cell::new(snapshot.is_seen()),
            window_focused: Cell::new(snapshot.focus.is_focused()),
            window_display,
            window_execution: Cell::new(snapshot.execution),
            closing_requested: Cell::new(false),
            window_closer: flui_platform::capability::WindowCloser::new().0,
            overlay_style: crate::overlay_style::OverlayStyleCoalescer::default(),
            scene: crate::presentation_scene::SceneSlot::default(),
            textures,
            lifecycle: Cell::new(PresentationLifecycle::Created),
            close_mode: Cell::new(flui_interaction::__runtime::CloseMode::Ordinary),
            withdrawn_dispatch: RefCell::new(None),
            frame_request: Cell::new(None),
            keyboard: Cell::new(snapshot.insets.keyboard()),
            pipeline,
            completion_scope: None,
            text_preparation: RefCell::new(TextPreparation::default()),
            sampled_motion_tick: Cell::new(None),
            frame_rate: Cell::default(),
            alive: RefCell::new(Some(alive)),
            window_id: window.id(),
            window: Rc::downgrade(&window),
            accessibility: accessibility.as_ref().map(Arc::downgrade),
            gestures,
            haptics,
            wheel_preferences: flui_interaction::pointer::WheelPreferencesSource::new(
                flui_platform::WheelPreferences::default(),
            ),
            interaction_dispatch: None,
            held_pointer_input: RefCell::new(HeldPointerQueue::new(id)),
            staged_input_regions: RefCell::default(),
            published_input_regions: RefCell::default(),
            focus,
            text_input,
            semantics,
            dev_agent: RefCell::new(None),
            agent_stability: RefCell::default(),
            agent_reads: RefCell::default(),
            agent_input: RefCell::default(),
            agent_waits: RefCell::default(),
            url_owner,
            agent_layer_reads: RefCell::default(),
            pending_frames: RefCell::default(),
            in_flight_limit: Cell::new(NonZeroUsize::MIN),
            agent_events: RefCell::default(),
            window_control: RefCell::new(None),
            window_placement: Cell::new(None),
            widgets,
            close_guard: CloseGuardSource::new(
                &[],
                flui_view::__runtime::OwnerNotify::new(|_| Ok(())),
            ),
            renderer,
            frames_rendered: Cell::new(0),
            frames_dropped: Cell::new(0),
            not_shown_streak: Cell::new(0),
            withheld_reported: Cell::new(false),
            occluded_frame: Cell::new(OccludedFrame::NotOwed),
            owed_frame_asked: Rc::default(),
            owed_frame_served: Cell::new(false),
            occluded_surface: flui_platform::OccludedSurface::Discarded,
            performance_overlay: Cell::new(false),
            redraw_pending: Cell::new(false),
            redraw_requests: Cell::new(0),
            redraw_requests_watched: Cell::new(0),
            vsync_driver: VsyncDriver::new(),
            animation_retirement: RefCell::new(None),
            motion_clock: RefCell::new(MotionClock::new()),
            clock: FrameClock::new(),
            redraw_poked: Arc::new(AtomicBool::new(false)),
            last_segment_span: Cell::new(None),
            segment_phases: Cell::new(FramePhaseDurations::default()),
            tree_revision: Cell::new(TreeRevision::ZERO),
            presented_revision: Cell::new(TreeRevision::ZERO),
            frame_failure_streak: Cell::new(0),
            failure_retry: Cell::new(FailureRetry::default()),
            device_recovery: Cell::new(None),
            segment_phase: FramePhaseMarker::new(SegmentPhase::Build),
            #[cfg(test)]
            segment_probe: RefCell::new(None),
            #[cfg(test)]
            flush_count: Cell::new(0),
        };
        state.attach_surface();
        state.clock.set_hidden(!state.window_visible.get());
        state
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        id: PresentationId,
        pipeline: PipelineCell,
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let window: Rc<dyn PlatformWindow> = test_platform_window(platform_text_input);
        Self::new_for_test_with_window(id, pipeline, window)
    }
}
