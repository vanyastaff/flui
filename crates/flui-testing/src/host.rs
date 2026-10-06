//! A headless host for a [`UiRealm`]: the product frame transaction
//! (`UiRealm::pump`, ADR-0083 §1) on a manual clock, a window double and a
//! sink that keeps what the frame composited.
//!
//! [`HeadlessHost`] is to a realm what a runner is on screen: it owns the
//! window the realm presents into, the sink a frame is submitted through and
//! the clock the frame reads, and it drives frames and input through the
//! realm's own entry points. Nothing here re-implements a phase of the frame:
//! applying commands, begin frame, the pipeline, end frame and the text-store
//! commit anchor all run inside `UiRealm::pump`.
//!
//! One [`ManualClock`] drives the whole realm: the realm reads it as its
//! [`ClockSource`] (frame-time origin, gesture-arena deadlines, the
//! presentation's `FrameClock`), and [`HeadlessHost::pump`] hands a clone to
//! the pump as the frame's timestamp. A test advances time only through
//! [`HeadlessHost::pump`] or [`HeadlessHost::clock`].
//!
//! # Failures
//!
//! A realm contains a panic that escapes a frame segment, and a pipeline
//! error, as a dropped frame (ADR-0048) and reports it to its frame-failure
//! handler; on screen the process survives. Under test that containment would
//! hide the failure, so [`HeadlessHost::pump`] raises the first dropped-frame
//! report of the pump as a panic once the pump has returned, carrying the
//! report's text (the realm retains it verbatim here). A later panic that
//! unwinds out of the same pump does not replace it: the first failure stays
//! authoritative. A dropped-frame report the realm makes between pumps is
//! raised by the next pump, before it frames. A lifecycle panic the tree recovered from (an `ErrorView`
//! substitution) is not raised: the frame it happened in completed.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_foundation::ManualClock;
use flui_foundation::geometry::{Bounds, Size};
use flui_platform_api::TextStoreHost;
use flui_platform_api::{
    CursorError, CursorIcon, InMemoryClipboard, PlatformInput, PlatformTextInput, PlatformWindow,
    WindowId,
};
use flui_rendering::layer::{LayerTree, Scene};
use flui_runtime::dev_agent::{DevAgentAttachment, DevAgentHost};
use flui_runtime::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::pump::FrameOutcome;
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_realm::{RealmHostServices, UiRealm};
use flui_scheduler::{ClockSource, LocalPostFrameHandle};
use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};
use flui_view::dev_agent::DevAgentHook;
use parking_lot::Mutex;

use crate::text_store_host::RecordingTextStoreHost;

/// The window a [`HeadlessHost`] presents into: window 1 at scale factor 1,
/// focused and visible, with the logical size the realm was built at.
///
/// It records what the realm asks of a window that a test asserts on: the
/// cursor a hovered region sets, and, when built with a text input, every
/// IME enable and cursor-area call.
pub struct HeadlessWindow {
    size: Size<f64>,
    cursor: Mutex<CursorIcon>,
    text_input: Option<Arc<RecordingTextInput>>,
    text_store_host: bool,
}

impl std::fmt::Debug for HeadlessWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessWindow")
            .field("size", &self.size)
            .field("text_input", &self.text_input.is_some())
            .finish_non_exhaustive()
    }
}

impl HeadlessWindow {
    /// A window of `width` × `height` logical (and physical) pixels.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: Size::new(f64::from(width), f64::from(height)),
            cursor: Mutex::new(CursorIcon::Default),
            text_input: None,
            text_store_host: false,
        }
    }

    /// Offer a pull-model text input instead: the realm's presentation gets
    /// a [`RecordingTextStoreHost`] ([`HeadlessHost::text_store_host`]) and
    /// tells it which field's store takes input, as it tells the Win32 text
    /// services. It wins over [`Self::with_text_input`].
    #[must_use]
    pub fn with_text_store_host(mut self) -> Self {
        self.text_store_host = true;
        self
    }

    /// Offer a text-input capability that records every call the realm's
    /// text-input owner makes, so the presentation installs a working
    /// `TextInputHandle` over it.
    #[must_use]
    pub fn with_text_input(mut self) -> Self {
        self.text_input = Some(Arc::new(RecordingTextInput::default()));
        self
    }

    /// The last cursor the realm set on this window.
    #[must_use]
    pub fn cursor(&self) -> CursorIcon {
        *self.cursor.lock()
    }

    /// Every IME cursor area the realm reported, in delivery order; `None`
    /// for a window built without a text input.
    #[must_use]
    pub fn ime_cursor_areas(&self) -> Option<Vec<Bounds<f64>>> {
        self.text_input
            .as_ref()
            .map(|input| input.cursor_areas.lock().clone())
    }

    /// Every IME enable (`true`) and disable (`false`) the realm asked for,
    /// in delivery order; `None` for a window built without a text input.
    #[must_use]
    pub fn ime_allowed_calls(&self) -> Option<Vec<bool>> {
        self.text_input
            .as_ref()
            .map(|input| input.ime_allowed.lock().clone())
    }

    fn physical_size_i32(&self) -> Size<i32> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "built from u32 pixel counts, which a test keeps far below i32::MAX"
        )]
        Size::new(self.size.width as i32, self.size.height as i32)
    }
}

impl PlatformWindow for HeadlessWindow {
    fn id(&self) -> WindowId {
        WindowId(1)
    }

    fn physical_size(&self) -> Size<i32> {
        self.physical_size_i32()
    }

    fn logical_size(&self) -> Size<f64> {
        self.size
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn request_redraw(&self) {}

    fn is_focused(&self) -> bool {
        true
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input
            .as_ref()
            .map(|input| Arc::clone(input) as Arc<dyn PlatformTextInput>)
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        *self.cursor.lock() = cursor;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The IME capability of a [`HeadlessWindow`] built with a text input.
#[derive(Debug, Default)]
struct RecordingTextInput {
    cursor_areas: Mutex<Vec<Bounds<f64>>>,
    ime_allowed: Mutex<Vec<bool>>,
}

impl PlatformTextInput for RecordingTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.ime_allowed.lock().push(allowed);
    }

    fn set_ime_cursor_area(&self, area: Bounds<f64>) {
        self.cursor_areas.lock().push(area);
    }
}

/// The accessibility bridge of a [`HeadlessHost`]'s window: assistive
/// technology attaches when a test asks, published trees are dropped (the
/// harness reads the assembled tree from the pipeline instead), and the
/// action listener the realm registers is handed to a test that plays the
/// adapter.
#[derive(Default)]
struct HeadlessAccessibility {
    active: AtomicBool,
    activation: Mutex<Option<AccessibilityActivationListener>>,
    action: Mutex<Option<AccessibilityActionListener>>,
}

impl HeadlessAccessibility {
    /// Attach assistive technology: the listener the realm registered flips
    /// the presentation's semantics flag, which the next frame reconciles
    /// onto the pipeline.
    fn attach(&self) {
        self.active.store(true, Ordering::Relaxed);
        let listener = self.activation.lock().clone();
        if let Some(listener) = listener {
            listener(true);
        }
    }
}

impl PlatformAccessibility for HeadlessAccessibility {
    fn publish(&self, _update: accesskit::TreeUpdate) {}

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        *self.activation.lock() = Some(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        *self.action.lock() = Some(listener);
    }
}

/// The frame sink of a [`HeadlessHost`]: a surface of fixed size that
/// presents every scene and keeps the last one.
#[derive(Debug)]
pub struct HeadlessSink {
    size: (u32, u32),
    last_scene: Option<Scene>,
    submits: u64,
}

impl HeadlessSink {
    /// A `width` × `height` surface that has presented nothing yet.
    #[must_use]
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            size: (width, height),
            last_scene: None,
            submits: 0,
        }
    }

    /// The layer tree of the last scene submitted, retained across frames
    /// that painted nothing.
    #[must_use]
    pub fn layer_tree(&self) -> Option<&LayerTree> {
        self.last_scene.as_ref().map(Scene::tree)
    }

    /// How many scenes have been submitted.
    #[must_use]
    pub fn submits(&self) -> u64 {
        self.submits
    }
}

impl FrameSink for HeadlessSink {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn submit(&mut self, scene: Scene) -> SubmitVerdict {
        self.last_scene = Some(scene);
        self.submits = self.submits.saturating_add(1);
        SubmitVerdict::Presented
    }
}

/// A development-agent hook attached the way a runner's event loop attaches
/// it: once, when this value is built, and detached when it is dropped.
///
/// Every [`HeadlessHost`] built [`with`](HeadlessHost::with_dev_agent) it
/// hands the hook its window, as a runner hands it each window it opens, so
/// one hook can serve several realms, and a realm dropped before the hook is
/// a window that closed while the tool kept running. Containment is the
/// runtime's (`flui_runtime::dev_agent`): a hook that panics is dropped, and
/// the realms keep pumping.
pub struct HeadlessDevAgent {
    host: DevAgentHost,
    attachment: Option<DevAgentAttachment>,
}

impl HeadlessDevAgent {
    /// Take `hook` and attach it, as a runner does when its loop starts.
    #[must_use]
    pub fn attach(hook: impl DevAgentHook) -> Self {
        let host = DevAgentHost::new(hook);
        let attachment = host.attach();
        Self { host, attachment }
    }

    /// Whether the hook is attached: it did not panic in `attach` or since.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.attachment.is_some() && self.host.is_attached()
    }
}

impl std::fmt::Debug for HeadlessDevAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessDevAgent")
            .field("host", &self.host)
            .finish_non_exhaustive()
    }
}

/// A [`UiRealm`] hosted headlessly, driven frame by frame on a manual clock.
/// See the [module docs](self).
#[doc(alias = "HeadlessRealm")]
pub struct HeadlessHost {
    realm: UiRealm,
    clock: ManualClock,
    sink: HeadlessSink,
    window: Arc<HeadlessWindow>,
    accessibility: Arc<HeadlessAccessibility>,
    clipboard: Arc<InMemoryClipboard>,
    text_store_host: Option<Rc<RecordingTextStoreHost>>,
    /// Dropped-frame reports not yet raised, as the text a raised failure
    /// carries: those of the pump in progress, and any the realm made
    /// between pumps, which the next pump raises before it frames.
    failures: Arc<Mutex<Vec<String>>>,
}

impl std::fmt::Debug for HeadlessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessHost")
            .field("realm", &self.realm)
            .field("window", &self.window)
            .field("sink", &self.sink)
            .finish_non_exhaustive()
    }
}

impl HeadlessHost {
    /// A realm over `window`, whose surface has the window's size.
    ///
    /// # Panics
    ///
    /// If the realm's interaction lane cannot be created (its identity space
    /// is exhausted).
    #[must_use]
    pub fn new(window: HeadlessWindow) -> Self {
        Self::with_storage(window, None)
    }

    /// [`Self::new`], its widgets reaching `storage` through
    /// `LifecycleContext::storage`.
    pub(crate) fn with_storage(
        window: HeadlessWindow,
        storage: Option<Arc<dyn flui_platform_api::Storage>>,
    ) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the window size came from u32 pixel counts"
        )]
        let surface = (window.size.width as u32, window.size.height as u32);
        let text_store_host = window.text_store_host.then(RecordingTextStoreHost::new);
        let window = Arc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let clipboard = Arc::new(InMemoryClipboard::new());
        let clock = ManualClock::new();
        // A collection of its own: the realm owns a `TextContext` over it
        // (ADR-0092 §3), exactly as a hosted realm does. Deliberately
        // bundled-only, unlike the app's host-fed one
        // (`FontCollection::with_host_fonts`), so text measures the same on
        // every host a test runs on.
        let fonts = flui_painting::FontCollection::new();
        let mut host = RealmHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::clone(&clipboard) as Arc<dyn flui_platform_api::Clipboard>,
            &fonts,
            ClockSource::Manual(clock.clone()),
        );
        if let Some(storage) = storage {
            host = host.with_storage(storage);
        }
        let realm = UiRealm::new(
            PresentationWindow::new(
                Arc::clone(&window) as Arc<dyn PlatformWindow>,
                Some(Arc::clone(&accessibility) as Arc<dyn PlatformAccessibility>),
            )
            .with_text_store_host(
                text_store_host
                    .clone()
                    .map(|host| host as Rc<dyn TextStoreHost>),
            ),
            1.0,
            host,
        )
        .expect("BUG: interaction lane identity exhausted");
        let failures = Arc::new(Mutex::new(Vec::new()));
        realm.set_frame_failure_detail(FrameFailureDetail::Verbatim);
        let recorded = Arc::clone(&failures);
        realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            if let Some(text) = dropped_frame_text(report) {
                recorded.lock().push(text);
            }
        })));
        Self {
            realm,
            clock,
            sink: HeadlessSink::new(surface.0, surface.1),
            window,
            accessibility,
            clipboard,
            text_store_host,
            failures,
        }
    }

    /// Hand this realm's window to `agent`'s hook, as a runner hands it a
    /// window it has installed. The window's first semantics tree is built
    /// by the next [`pump`](Self::pump); dropping the realm closes the window,
    /// and the hook's handle to it answers `gone` from then on.
    ///
    /// Does nothing when the hook is not attached.
    #[must_use]
    pub fn with_dev_agent(self, agent: &HeadlessDevAgent) -> Self {
        agent
            .host
            .publish(&self.realm, self.realm.presentation_id());
        self
    }

    /// Attach `view` as the realm's root widget, the root view sized to the
    /// window. The realm wraps it in its root scopes (`GestureArenaScope`,
    /// `VsyncScope`, `FocusRoot`, `MediaQuery`); the first
    /// [`pump`](Self::pump) builds, lays out and paints it.
    ///
    /// # Errors
    ///
    /// [`flui_view::AttachError`] if a root is already attached.
    pub fn attach<V>(&self, view: &V) -> Result<(), flui_view::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.realm.attach_root_widget_with_size(
            view,
            self.window.size.width,
            self.window.size.height,
        )
    }

    /// Advance the clock by `dt`, then run one frame through
    /// `UiRealm::pump` at the new instant.
    ///
    /// The clock moves before the frame starts, once: a caller that catches
    /// a raised failure and pumps again with `Duration::ZERO` retries at the
    /// same instant.
    ///
    /// # Panics
    ///
    /// With a dropped-frame report the realm made since the last pump, before
    /// this one frames or moves the clock; with the first dropped-frame
    /// report of this pump (a segment panic or a pipeline error the realm
    /// contained), after the pump returned; or with a panic that unwound out
    /// of the pump when nothing was reported before it. See the
    /// [module docs](self#failures).
    pub fn pump(&mut self, dt: Duration) -> FrameOutcome {
        let pending = std::mem::take(&mut *self.failures.lock());
        if let Some(failure) = pending.into_iter().next() {
            panic!("{failure}");
        }
        self.clock.advance(dt);
        let mut frame_clock = self.clock.clone();
        let Self { realm, sink, .. } = self;
        let attempt = catch_unwind(AssertUnwindSafe(|| realm.pump(&mut frame_clock, sink)));
        let first_failure = {
            let mut failures = self.failures.lock();
            let first = failures.first().cloned();
            failures.clear();
            first
        };
        match (attempt, first_failure) {
            (Ok(outcome), None) => outcome,
            (Ok(_), Some(failure)) => panic!("{failure}"),
            (Err(payload), None) => resume_unwind(payload),
            (Err(payload), Some(failure)) => {
                tracing::error!(
                    first_failure = %failure,
                    "a panic unwound out of the pump after a contained frame failure; the \
                     contained failure is raised and the later panic is discarded"
                );
                // The payload is an arbitrary user value whose destructor may
                // itself panic; dropping it here could replace or abort the
                // unwind of the first failure, so it is leaked instead.
                std::mem::forget(payload);
                panic!("{failure}")
            }
        }
    }

    /// Deliver `input` to the realm's primary presentation, as a runner
    /// delivers a platform event, then flush the pointer moves it queued.
    ///
    /// The realm coalesces pointer moves and dispatches them at the next
    /// frame; the flush makes a synthetic move observable before that frame,
    /// so a test sees its effect immediately. It runs the same queue and
    /// dispatch code the frame would.
    ///
    /// # Panics
    ///
    /// With the first panic a dispatch raised; a later one is logged and
    /// discarded.
    pub fn dispatch(&self, input: PlatformInput) {
        self.realm.enter(|realm| {
            let delivered = catch_unwind(AssertUnwindSafe(|| {
                realm.handle_input_addressed(realm.presentation_id(), input);
            }));
            let flushed = catch_unwind(AssertUnwindSafe(|| {
                realm.gestures().flush_pending_moves();
                realm.gestures().drain_deferred_arena_resolutions();
            }));
            match (delivered, flushed) {
                (Err(first), Err(later)) => {
                    tracing::error!(
                        "flushing queued pointer moves panicked after the input dispatch \
                         panicked; only the first panic is resumed"
                    );
                    std::mem::forget(later);
                    resume_unwind(first);
                }
                (Err(payload), Ok(())) | (Ok(()), Err(payload)) => resume_unwind(payload),
                (Ok(()), Ok(())) => {}
            }
        });
    }

    /// The pointer left the window: the realm sweeps hover state, so every
    /// hovered region gets its exit and the cursor resets.
    pub fn dispatch_hover_left(&self) {
        self.realm.enter(|realm| {
            realm.handle_window_hover_addressed(realm.presentation_id(), false);
        });
    }

    /// Attach assistive technology to the window, as a platform adapter does:
    /// the next [`pump`](Self::pump) turns semantics on and assembles the
    /// tree.
    pub fn enable_semantics(&self) {
        self.accessibility.attach();
    }

    /// Whether the window's pipeline holds a semantics tree: assistive
    /// technology or a development agent asked for one, and a
    /// [`pump`](Self::pump) has run since. It stops once nothing asks and
    /// the next pump has run.
    #[must_use]
    pub fn collects_semantics(&self) -> bool {
        self.realm
            .presentation_collects_semantics_for_test(self.realm.presentation_id())
            .expect("BUG: a headless realm's window stays resident while the realm lives")
    }

    /// The listener the realm registered for actions assistive technology
    /// requests. It is `Send + Sync`: a test plays the platform adapter by
    /// calling it, from any thread, and the realm queues the request in its
    /// owner inbox for the next pump to apply.
    #[must_use]
    pub fn accessibility_action_listener(&self) -> Option<AccessibilityActionListener> {
        self.accessibility.action.lock().clone()
    }

    /// Run `f` inside the realm's owner scope (its interaction lane, global
    /// key registry and post-frame lane), as every realm entry point does.
    ///
    /// Crate-private, as is [`realm`](Self::realm): the realm's frame entry
    /// points are the host's (ADR-0083 §2), and `flui-runtime` is not an
    /// embedder API, so a test reaches the realm only through this host.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&UiRealm) -> R) -> R {
        self.realm.enter(f)
    }

    /// The hosted realm.
    pub(crate) fn realm(&self) -> &UiRealm {
        &self.realm
    }

    /// The owner-local post-frame handle the realm installed on its build
    /// owner: a callback scheduled through it runs in the next pump's
    /// post-frame phase.
    ///
    /// # Panics
    ///
    /// If the realm installed no owner-local post-frame handle, which a
    /// realm's presentation always does.
    #[must_use]
    pub fn local_post_frame_handle(&self) -> LocalPostFrameHandle {
        self.realm
            .widgets()
            .with_build_owner(|owner| owner.local_post_frame_handle().cloned())
            .expect("BUG: a realm's presentation installs its owner-local post-frame handle")
    }

    /// Ask the realm for a frame, as a widget's `setState` would: the next
    /// [`pump`](Self::pump) runs the pipeline even with nothing dirty.
    pub fn request_frame(&self) {
        self.realm.request_redraw();
    }

    /// The clock the realm reads. Advancing it moves the frame time, the
    /// gesture-arena deadlines and the produce gate together.
    #[must_use]
    pub fn clock(&self) -> &ManualClock {
        &self.clock
    }

    /// The sink frames are submitted through.
    #[must_use]
    pub fn sink(&self) -> &HeadlessSink {
        &self.sink
    }

    /// The window the realm presents into.
    #[must_use]
    pub fn window(&self) -> &HeadlessWindow {
        &self.window
    }

    /// The pull-model host the realm's presentation speaks to, for a window
    /// built [`HeadlessWindow::with_text_store_host`].
    #[must_use]
    pub fn text_store_host(&self) -> Option<&Rc<RecordingTextStoreHost>> {
        self.text_store_host.as_ref()
    }

    /// The clipboard the realm hands its widgets.
    #[must_use]
    pub fn clipboard(&self) -> Arc<InMemoryClipboard> {
        Arc::clone(&self.clipboard)
    }
}

/// The text a dropped-frame report raises with; `None` for a contained
/// recovery, which does not drop the frame.
fn dropped_frame_text(report: &FrameFailureReport) -> Option<String> {
    if report.disposition != FailureDisposition::FrameDropped {
        return None;
    }
    Some(match &report.kind {
        FrameFailureKind::SegmentPanic { message, phase, .. } => {
            format!("frame segment panicked in {phase:?}: {message}")
        }
        FrameFailureKind::Pipeline { error } => format!("frame pipeline failed: {error}"),
        other => format!("frame dropped: {other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::time::Duration;

    use super::{HeadlessHost, HeadlessWindow};

    /// A dropped-frame report the realm made between pumps (from input
    /// delivery, say) is raised by the next pump, before it frames or moves
    /// the clock, and only once.
    ///
    /// Fails against a pump that clears the buffer before it runs: the
    /// report is erased unseen and the pump returns normally.
    #[test]
    fn a_report_made_between_pumps_is_raised_by_the_next_pump() {
        let mut realm = HeadlessHost::new(HeadlessWindow::new(40, 24));
        realm
            .failures
            .lock()
            .push("frame dropped: reported between pumps".to_owned());
        let before = flui_foundation::MonotonicClock::now(realm.clock());

        let raised = catch_unwind(AssertUnwindSafe(|| realm.pump(Duration::from_millis(16))))
            .expect_err("the leftover report is raised");

        assert_eq!(
            raised.downcast_ref::<String>().map(String::as_str),
            Some("frame dropped: reported between pumps")
        );
        assert_eq!(
            flui_foundation::MonotonicClock::now(realm.clock()),
            before,
            "the raising pump did not move the clock"
        );
        let _outcome = realm.pump(Duration::ZERO);
    }
}
