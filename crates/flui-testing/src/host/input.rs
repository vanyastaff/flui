//! Input, semantics and recording entry points of a [`HeadlessHost`].

#[cfg(doc)]
use flui_platform::WindowId;

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

use std::time::Duration;

use flui_foundation::PresentationId;
use flui_platform::PlatformInput;

use flui_semantics::platform::AccessibilityActionListener;

use super::{HeadlessHost, HeadlessWindowId};

impl HeadlessHost {
    /// Deliver `input` to the UI runtime's primary presentation, as a runner
    /// delivers a platform event, then flush the pointer moves it queued.
    ///
    /// The UI runtime coalesces pointer moves and dispatches them at the next
    /// frame; the flush makes a synthetic move observable before that frame,
    /// so a test sees its effect immediately. It runs the same queue and
    /// dispatch code the frame would. An opted-in resampling presentation
    /// retains measured moves until its owner frame is pumped.
    ///
    /// # Panics
    ///
    /// With the first panic a dispatch raised; a later one is logged and
    /// discarded.
    pub fn dispatch(&self, input: PlatformInput) {
        self.dispatch_to(self.primary_window(), input);
    }

    /// [`dispatch`](Self::dispatch) `input` as `window` reported it. Keyboard
    /// input goes to the window that holds the OS focus, whichever window
    /// reported it, as on every real host.
    ///
    /// # Panics
    ///
    /// As [`dispatch`](Self::dispatch).
    pub fn dispatch_to(&self, window: HeadlessWindowId, input: PlatformInput) {
        self.dispatch_on_to(window.0, flui_interaction::ids::InputSurface::Window, input);
    }

    /// [`dispatch`](Self::dispatch) as `surface` of `presentation`'s window.
    fn dispatch_on_to(
        &self,
        presentation: PresentationId,
        surface: flui_interaction::ids::InputSurface,
        input: PlatformInput,
    ) {
        // A real host routes nothing to a window that closed.
        if !self.is_open(HeadlessWindowId(presentation)) {
            return;
        }
        self.ui_runtime().enter(|ui_runtime| {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: the kit keeps the input panic so the flush still runs"
            )]
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                ui_runtime.handle_input_on(presentation, surface, input);
            }));
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: the kit keeps the flush panic so later input steps still run"
            )]
            let flushed = catch_unwind(AssertUnwindSafe(|| {
                if !ui_runtime.gestures().is_resampling_enabled() {
                    ui_runtime.gestures().flush_pending_moves();
                }
                ui_runtime.gestures().drain_deferred_arena_resolutions();
            }));
            match (delivered, flushed) {
                (Err(first), Err(later)) => {
                    tracing::error!(
                        "flushing queued pointer moves panicked after the input dispatch \
                         panicked; only the first panic is resumed"
                    );
                    flui_foundation::panic::discard_payload(later);
                    resume_unwind(first);
                }
                (Err(payload), Ok(())) | (Ok(()), Err(payload)) => resume_unwind(payload),
                (Ok(()), Ok(())) => {}
            }
        });
        // The dispatch is over: the OS drag sessions it decided on start here,
        // as a host's outermost callback starts them (ADR-0211 §7).
        self.ui_runtime().enter_pending_os_drags();
    }

    /// Set exactly this window's presentation policy through the runtime owner.
    ///
    /// # Errors
    /// The runtime refuses a mode change while the presentation has a contact.
    pub fn set_pointer_resampling(
        &self,
        window: HeadlessWindowId,
        policy: flui_runtime::presentation::PointerResampling,
    ) -> Result<(), flui_runtime::presentation::PointerResamplingError> {
        self.ui_runtime().set_pointer_resampling(window.0, policy)
    }

    /// The pointer left the window: the UI runtime sweeps hover state, so every
    /// hovered region gets its exit and the cursor resets.
    pub fn dispatch_hover_left(&self) {
        self.observe(
            self.primary.id,
            flui_runtime::owner::WindowObservation::Hover(false),
        );
    }

    /// Attach assistive technology to the window, as a platform adapter does:
    /// the next [`pump`](Self::pump) turns semantics on and assembles the
    /// tree.
    pub fn enable_semantics(&self) {
        self.primary.accessibility.attach();
    }

    /// Detach assistive technology from the window, as a platform adapter
    /// does when the last screen reader goes: the next [`pump`](Self::pump)
    /// stops collecting semantics unless a development agent still asks.
    pub fn disable_semantics(&self) {
        self.primary.accessibility.detach();
    }

    /// Whether the window's pipeline holds a semantics tree: assistive
    /// technology or a development agent asked for one, and a
    /// [`pump`](Self::pump) has run since. It stops once nothing asks and
    /// the next pump has run.
    #[must_use]
    pub fn collects_semantics(&self) -> bool {
        self.ui_runtime()
            .presentation_collects_semantics_for_test(self.primary.id)
            .expect("BUG: a headless ui_runtime's window stays resident while the ui_runtime lives")
    }

    /// The listener the UI runtime registered for actions assistive technology
    /// requests. It is `Send + Sync`: a test plays the platform adapter by
    /// calling it, from any thread, and the UI runtime queues the request in its
    /// owner inbox for the next pump to apply.
    #[must_use]
    pub fn accessibility_action_listener(&self) -> Option<AccessibilityActionListener> {
        self.primary.accessibility.action.lock().clone()
    }

    /// Move the screen reader's cursor to `node` of the published tree, or
    /// off it for `None`, as VoiceOver or TalkBack reports it: the UI runtime
    /// queues the move in its owner inbox for the next [`pump`](Self::pump),
    /// which tells the subtrees the cursor left and entered.
    ///
    /// # Panics
    ///
    /// Before the UI runtime registered its listener, which it does as the
    /// window opens.
    pub fn move_assistive_focus(&self, node: Option<accesskit::NodeId>) {
        let listener = self
            .primary
            .accessibility
            .assistive_focus
            .lock()
            .clone()
            .expect("BUG: the UI runtime registers its cursor listener as the window opens");
        listener(node);
    }

    /// The primary window's gesture arena, the one its detectors compete in:
    /// observe it to watch the arena decide what this host dispatches.
    #[must_use]
    pub fn arena(&self) -> &flui_interaction::gesture::GestureArena {
        self.ui_runtime().gestures().arena()
    }

    /// Install a tap from `factory` on this host's runtime for the rest of
    /// its life, as `flui-app` installs an application's on every runtime it
    /// builds: the factory is handed what the recording starts from.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`](flui_foundation::IdExhausted) when the runtime issued
    /// every tap registration generation.
    pub fn install_input_tap(
        &self,
        factory: &flui_runtime::InputTapFactory,
    ) -> Result<(), flui_foundation::IdExhausted> {
        factory.install_on(self.ui_runtime())
    }

    /// The UI runtime's tap registry: [`record`](crate::recording::record)
    /// into it to record everything this host applies (ADR-0215 §1).
    #[must_use]
    pub fn input_taps(&self) -> &flui_runtime::InputTaps {
        self.ui_runtime().input_taps()
    }

    /// Where a native journal sends its lines for this host's recordings of the `native` layer
    /// (ADR-0288 §11). A headless host has no backend to journal, so a test plays the backend
    /// and sends the lines itself; until it calls this, `native` is refused.
    #[must_use]
    pub fn attach_native_journal(&self) -> flui_runtime::ui_runtime::NativeLines {
        let lines = flui_runtime::ui_runtime::NativeLines::default();
        self.ui_runtime().attach_native_journal(&lines);
        lines
    }

    /// The header of a recording of this host: its virtual clock, every
    /// window with its scale factor and the per-kind gesture values its
    /// contacts are admitted with, the system-preference revision in effect,
    /// and the text kept under `privacy`. A headless host has no hardware, so
    /// it names no device.
    #[must_use]
    pub fn recording_header(
        &self,
        privacy: crate::recording::Privacy,
    ) -> crate::recording::RecordingHeader {
        let mut header = crate::recording::RecordingHeader::starting(
            crate::recording::RecordingClock::Virtual,
            &self.ui_runtime().recording_start(),
        );
        header.privacy = privacy;
        header
    }

    /// Embed a surface in the primary window, as an embedder does through
    /// its presentation's gesture binding: input [`dispatch_on`](Self::dispatch_on)
    /// that surface keeps its own contacts, hover and latches (ADR-0207 §5).
    ///
    /// # Errors
    ///
    /// [`SurfaceIdsExhausted`](flui_interaction::ids::SurfaceIdsExhausted) once the
    /// binding minted every identity.
    pub fn embed_surface(
        &mut self,
    ) -> Result<flui_interaction::ids::InputSurface, flui_interaction::ids::SurfaceIdsExhausted>
    {
        let surface = self.ui_runtime().gestures().embed_surface()?;
        self.embedded.push(surface);
        Ok(surface)
    }

    /// [`dispatch`](Self::dispatch) `input` as `surface` of the primary window
    /// reported it.
    ///
    /// # Panics
    ///
    /// As [`dispatch`](Self::dispatch).
    pub fn dispatch_on(&self, surface: flui_interaction::ids::InputSurface, input: PlatformInput) {
        self.dispatch_on_to(self.primary.id, surface, input);
    }

    /// Replay `recording` on this host, row by row on its virtual clock, and
    /// return the arena decisions the replay made, to compare with
    /// [`ReplayableRecording::decisions`](crate::recording::ReplayableRecording::decisions).
    ///
    /// Before the first row the header's starting state is restored: each
    /// recorded window moves to its recorded scale (as a runner delivers a
    /// scale change) and its per-kind gesture values replace this host's, and
    /// the recorded system-preference values apply. Each input is then
    /// applied at its recorded arrival: the clock advances to it first, and
    /// never back, since pointer input the recorded host held for its first
    /// commit is recorded where it was dispatched. Input the recorded host
    /// accepted and never delivered (`InputOutcome::Queued`) is not applied. The
    /// windows the recording names must be this host's (the same
    /// [`WindowId`]s), and an embedded surface it names one this host
    /// [embedded](Self::embed_surface) in the same order; preference
    /// revisions apply at their positions.
    ///
    /// # Panics
    ///
    /// If the recording names a window this host has not opened or a surface
    /// it has not embedded, or with the first panic an applied input or frame
    /// raised, a frame's semantics author error included ([`pump`](Self::pump)):
    /// a recording of an application with one stops at that frame.
    pub fn replay(
        &mut self,
        recording: &crate::recording::ReplayableRecording,
    ) -> Vec<flui_runtime::DecisionRow> {
        use flui_runtime::{HostInput, InputRecord};
        let decisions = Rc::new(std::cell::RefCell::new(Vec::new()));
        let handle = {
            let decisions = Rc::clone(&decisions);
            self.input_taps()
                .install(move |record: &InputRecord| {
                    if let InputRecord::Decision(decision) = record {
                        decisions.borrow_mut().push(decision.clone());
                    }
                })
                .expect("BUG: a headless host issues a replay its tap registration")
        };
        let header = recording.header();
        for window in &header.windows {
            let presentation = self.presentation_of(window.window);
            let id = HeadlessWindowId(presentation);
            if self.window_of(id).scale_factor.get() != window.scale_factor {
                self.set_scale_factor(id, window.scale_factor);
            }
            if let Some(profiles) = &window.gesture_profiles {
                self.ui_runtime()
                    .replace_gesture_profiles(presentation, profiles.clone());
            }
        }
        if let Some(preferences) = &header.preferences {
            self.set_system_preferences((*preferences.values).clone());
        }
        let mut last_frame = None;
        for row in recording.rows() {
            match row {
                // Held input its host discarded: nothing dispatched it.
                InputRecord::Input(row)
                    if row.outcome == flui_runtime::owner::InputOutcome::Queued =>
                {
                    continue;
                }
                InputRecord::Input(row) => {
                    let presentation = self.presentation_of(row.window);
                    self.advance_to(row.arrival);
                    match &row.input {
                        HostInput::Frame { .. } => {
                            // One pump delivers a frame to every window.
                            if last_frame != Some(row.arrival) {
                                let _ = self.pump(Duration::ZERO);
                            }
                            last_frame = Some(row.arrival);
                            continue;
                        }
                        HostInput::Platform(input) => {
                            let surface = self.surface_of(presentation, row.surface);
                            self.dispatch_on_to(presentation, surface, input.clone());
                        }
                        HostInput::Window(observation) => {
                            self.observe(presentation, *observation);
                            self.size_sink_to_primary();
                        }
                        HostInput::Accessibility(request) => {
                            self.ui_runtime().enter(|ui_runtime| {
                                ui_runtime.dispatch_accessibility(presentation, request);
                            });
                        }
                        HostInput::AssistiveFocus(node) => {
                            self.ui_runtime().enter(|ui_runtime| {
                                ui_runtime.dispatch_assistive_focus(presentation, *node);
                            });
                        }
                        other => panic!("this host cannot replay {other:?}"),
                    }
                }
                InputRecord::Preferences(row) => {
                    self.set_system_preferences((*row.values).clone());
                }
                _ => {}
            }
            last_frame = None;
        }
        handle
            .finish()
            .expect("BUG: the replay's own tap records every row");
        decisions.take()
    }

    /// Deliver `script` to the primary window on the virtual clock: each step
    /// at its offset from now, a frame step as a frame. Several events
    /// between two frame steps reach the runtime within one frame, and an
    /// event between two vsyncs keeps its own time.
    ///
    /// # Panics
    ///
    /// With the first panic a delivered input or frame raised, a frame's
    /// semantics author error included ([`pump`](Self::pump)).
    pub fn run_script(&mut self, script: &crate::replay::PointerScript) {
        let window = self.primary_window();
        let base = self.event_now();
        for (step, input) in script.steps().iter().zip(script.inputs_from(base)) {
            self.advance_to(base + step.at());
            match input {
                flui_runtime::HostInput::Platform(input) => self.dispatch_to(window, input),
                _ => {
                    let _ = self.pump(Duration::ZERO);
                }
            }
        }
    }

    /// Where a script finds the control it drives: the center, in logical
    /// pixels, of `window`'s single semantics node with `role` and `label`,
    /// as assistive technology sees it.
    ///
    /// # Errors
    ///
    /// [`A11yQueryError`](crate::a11y::A11yQueryError) when no node, or more than
    /// one, matches.
    ///
    /// # Panics
    ///
    /// If `window` has published no semantics tree (attach assistive
    /// technology and pump first), or the node has no bounds.
    pub fn find_center(
        &self,
        window: HeadlessWindowId,
        role: accesskit::Role,
        label: &str,
    ) -> Result<flui_foundation::geometry::Offset<f64>, crate::a11y::A11yQueryError> {
        let tree = self
            .published_a11y_tree(window)
            .expect("the window published no semantics tree; enable semantics and pump first");
        let node = tree.find_by_role_and_label(role, label)?;
        let bounds = tree
            .window_bounds(&node)
            .expect("a laid-out semantics node carries bounds");
        let scale = self.window_of(window).scale_factor.get();
        Ok(flui_foundation::geometry::Offset::new(
            scale.to_logical(bounds.x0.midpoint(bounds.x1)),
            scale.to_logical(bounds.y0.midpoint(bounds.y1)),
        ))
    }

    /// Advance the clock to `time` on the event timeline, if it is ahead.
    fn advance_to(&self, time: flui_platform::EventTime) {
        let now = self.event_now();
        self.advance(time.saturating_duration_since(now));
    }

    /// The clock's reading on the event timeline.
    pub(super) fn event_now(&self) -> flui_platform::EventTime {
        flui_platform::EventTime::from_nanos(
            u64::try_from(self.clock.elapsed().as_nanos())
                .expect("BUG: a headless test's virtual time fits the event timeline"),
        )
    }
}
