//! Test doubles for driving a realm with no platform backend and no GPU.
//!
//! Compiled for this crate's own tests and, through the `test-support`
//! feature, for `flui-app`'s, which enables it on its dev edge only.
//!
//! - [`TestWindow`]: a [`PlatformWindow`] value with every knob the realm
//!   tests vary (id, scale factor, sizes, focus, visibility, an injected
//!   text-input capability) and recorders for what the realm asked of it
//!   (redraws, pre-present notifications, the cursor).
//! - [`ScriptedSink`]: a [`FrameSink`] whose submit verdicts a test scripts.
//!
//! Neither is `flui_platform`'s `MockWindow` or a raster backend: those are
//! minted by a live headless platform or a GPU device, and a state-level
//! realm test wants a value with no platform or device ceremony. A test that
//! needs the real headless window's capabilities opens one from
//! `flui_platform::headless_platform()` (a dev dependency here).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_layer::Scene;
use flui_platform_api::{
    CursorError, CursorIcon, PlatformTextInput, PlatformWindow, WindowId, WindowShowError,
};
use flui_semantics::platform::PlatformAccessibility;
use flui_types::geometry::{DevicePixels, Pixels, Size};

use crate::sink::{FrameSink, SubmitVerdict};

/// Configurable [`PlatformWindow`] double. Construct with [`TestWindow::new`],
/// adjust via the builder-style setters, then `Arc::new` it where an
/// `Arc<dyn PlatformWindow>` is needed.
pub struct TestWindow {
    id: WindowId,
    scale_factor: f64,
    physical_size: Size<DevicePixels>,
    logical_size: Size<Pixels>,
    focused: bool,
    on_show: Option<Arc<dyn Fn() + Send + Sync>>,
    visible: bool,
    /// Incremented by every [`PlatformWindow::request_redraw`]; hand the
    /// [`Self::redraw_calls_handle`] to the asserting side.
    redraw_calls: Arc<AtomicU32>,
    /// The thread each [`PlatformWindow::request_redraw`] ran on, in call
    /// order. `request_redraw` is owner-thread-only (see that method's
    /// contract), and a counter alone cannot tell a conforming call from a
    /// violating one — only the thread can. See
    /// [`Self::redraw_threads_handle`].
    ///
    /// This duplicates [`Self::redraw_calls`]'s count — `len()` would give it.
    /// Both are kept because the counter is the handle existing callers
    /// already hold (an `Arc<AtomicU32>` readable without a lock). They cannot
    /// drift: both are written in the single `request_redraw` body below.
    redraw_threads: Arc<parking_lot::Mutex<Vec<std::thread::ThreadId>>>,
    /// How many times `pre_present_notify` ran — see
    /// [`TestWindow::pre_present_notifies_handle`].
    pre_present_notifies: Arc<AtomicU32>,
    text_input: Option<Arc<dyn PlatformTextInput>>,
    /// The bridge a host-side wrapper reports for this window. A
    /// `PlatformWindow` names no accessibility type (ADR-0082 §1), so the
    /// double only carries it for a test that builds a presentation from it
    /// (see [`Self::accessibility`]).
    accessibility: Option<Arc<dyn PlatformAccessibility>>,
    /// Last cursor set through [`PlatformWindow::set_cursor`]; read back via
    /// [`Self::cursor`].
    cursor: parking_lot::Mutex<CursorIcon>,
}

impl std::fmt::Debug for TestWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestWindow")
            .field("id", &self.id)
            .field("scale_factor", &self.scale_factor)
            .finish_non_exhaustive()
    }
}

impl Default for TestWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl TestWindow {
    /// Window 1, scale factor 1, zero sizes, unfocused and visible, with no
    /// capabilities.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: WindowId(1),
            scale_factor: 1.0,
            physical_size: Size::default(),
            logical_size: Size::default(),
            focused: false,
            on_show: None,
            visible: true,
            redraw_calls: Arc::new(AtomicU32::new(0)),
            redraw_threads: Arc::new(parking_lot::Mutex::new(Vec::new())),
            pre_present_notifies: Arc::new(AtomicU32::new(0)),
            text_input: None,
            accessibility: None,
            cursor: parking_lot::Mutex::new(CursorIcon::Default),
        }
    }

    /// Report `id` from [`PlatformWindow::id`].
    #[must_use]
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = WindowId(id);
        self
    }

    /// Report `scale_factor` from [`PlatformWindow::scale_factor`].
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: f64) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// Report these physical and logical sizes.
    #[must_use]
    pub fn with_sizes(mut self, physical: Size<DevicePixels>, logical: Size<Pixels>) -> Self {
        self.physical_size = physical;
        self.logical_size = logical;
        self
    }

    /// Run `callback` from [`PlatformWindow::show`].
    #[must_use]
    pub fn with_show_callback(mut self, callback: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.on_show = Some(callback);
        self
    }

    /// Report `visible` from [`PlatformWindow::is_visible`].
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Report `focused` from [`PlatformWindow::is_focused`].
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Offer `text_input` from [`PlatformWindow::text_input`].
    #[must_use]
    pub fn with_text_input(mut self, text_input: Option<Arc<dyn PlatformTextInput>>) -> Self {
        self.text_input = text_input;
        self
    }

    /// Carry `accessibility` as this window's bridge; see
    /// [`Self::accessibility`].
    #[must_use]
    pub fn with_accessibility(mut self, accessibility: Arc<dyn PlatformAccessibility>) -> Self {
        self.accessibility = Some(accessibility);
        self
    }

    /// The bridge set by [`Self::with_accessibility`], for a test that
    /// builds a presentation from this window with its bridge, as a host
    /// window would offer it.
    #[must_use]
    pub fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        self.accessibility.clone()
    }

    /// A handle on the pre-present-notify counter, for a test that must
    /// observe the count from inside a sink script (at the moment of the
    /// submit) rather than after the fact.
    #[must_use]
    pub fn pre_present_notifies_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.pre_present_notifies)
    }

    /// The counter [`PlatformWindow::request_redraw`] bumps — clone it out
    /// before `Arc`-ing the window.
    #[must_use]
    pub fn redraw_calls_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.redraw_calls)
    }

    /// The threads [`PlatformWindow::request_redraw`] was called on, in call
    /// order — the oracle for that method's owner-thread rule.
    #[must_use]
    pub fn redraw_threads_handle(&self) -> Arc<parking_lot::Mutex<Vec<std::thread::ThreadId>>> {
        Arc::clone(&self.redraw_threads)
    }

    /// The last cursor recorded by [`PlatformWindow::set_cursor`].
    #[must_use]
    pub fn cursor(&self) -> CursorIcon {
        *self.cursor.lock()
    }
}

impl PlatformWindow for TestWindow {
    fn show(&self) -> Result<(), WindowShowError> {
        if let Some(callback) = &self.on_show {
            callback();
        }
        Ok(())
    }

    fn id(&self) -> WindowId {
        self.id
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        self.physical_size
    }

    fn logical_size(&self) -> Size<Pixels> {
        self.logical_size
    }

    fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    fn pre_present_notify(&self) {
        self.pre_present_notifies.fetch_add(1, Ordering::Relaxed);
    }

    fn request_redraw(&self) {
        self.redraw_calls.fetch_add(1, Ordering::Relaxed);
        self.redraw_threads.lock().push(std::thread::current().id());
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn is_visible(&self) -> bool {
        self.visible
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input.clone()
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        *self.cursor.lock() = cursor;
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The scripted submit behavior a [`ScriptedSink`] carries: zero-based call
/// index and submitted scene in, the verdict out.
type SubmitScript = Box<dyn FnMut(u32, &Scene) -> SubmitVerdict + Send>;

/// Closure-configurable [`FrameSink`] double: the headless sink a realm's
/// frame transaction is tested against.
///
/// It scripts [`SubmitVerdict`]s, the realm-side classification every host
/// sink produces, so a test pins the realm's handling of each verdict. How a
/// host maps its own backend outcomes onto verdicts is that host's test.
///
/// Construct through [`always_presents`], [`single_shot`],
/// [`fails_once_then_presents`], or — for a behavior none of those name —
/// the general [`new`] with an explicit script. The surface is `(800, 600)`
/// unless overridden with [`with_size`].
///
/// [`always_presents`]: ScriptedSink::always_presents
/// [`single_shot`]: ScriptedSink::single_shot
/// [`fails_once_then_presents`]: ScriptedSink::fails_once_then_presents
/// [`new`]: ScriptedSink::new
/// [`with_size`]: ScriptedSink::with_size
pub struct ScriptedSink {
    /// Invoked with the zero-based index of the current call (the counter's
    /// value before this call), so a script can vary by call without keeping
    /// its own state.
    script: SubmitScript,
    /// How many times `submit` ran — the "did the scene actually leave the
    /// realm" oracle most tests assert on.
    pub submit_calls: u32,
    /// What `surface_size` reports.
    size: (u32, u32),
}

impl std::fmt::Debug for ScriptedSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedSink")
            .field("submit_calls", &self.submit_calls)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl ScriptedSink {
    /// A sink whose `submit` runs the given script.
    pub fn new(script: impl FnMut(u32, &Scene) -> SubmitVerdict + Send + 'static) -> Self {
        Self {
            script: Box::new(script),
            submit_calls: 0,
            size: (800, 600),
        }
    }

    /// Every submit presents.
    #[must_use]
    pub fn always_presents() -> Self {
        Self::new(|_, _| SubmitVerdict::Presented)
    }

    /// Exactly one submit is allowed; it returns `verdict`.
    ///
    /// A second submit panics: single-frame tests script one outcome and
    /// rely on the panic to catch a frame that unexpectedly leaves the realm
    /// twice.
    #[must_use]
    pub fn single_shot(verdict: SubmitVerdict) -> Self {
        let mut verdict = Some(verdict);
        Self::new(move |_, _| {
            verdict
                .take()
                .expect("submit called more than once in a single-frame test")
        })
    }

    /// The first submit returns `verdict`; every submit after presents — the
    /// shape a transient submit failure followed by a genuine retry produces.
    #[must_use]
    pub fn fails_once_then_presents(verdict: SubmitVerdict) -> Self {
        let mut verdict = Some(verdict);
        Self::new(move |_, _| verdict.take().unwrap_or(SubmitVerdict::Presented))
    }

    /// Override the reported surface size (default `(800, 600)`).
    #[must_use]
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }
}

impl FrameSink for ScriptedSink {
    fn surface_size(&mut self) -> (u32, u32) {
        self.size
    }

    fn submit(&mut self, scene: Scene) -> SubmitVerdict {
        let call_index = self.submit_calls;
        self.submit_calls += 1;
        (self.script)(call_index, &scene)
    }
}

#[cfg(test)]
mod tests {
    use flui_layer::{CanvasLayer, Layer, LayerTree};

    use super::*;

    fn empty_scene() -> Scene {
        Scene::new(LayerTree::new(Layer::from(CanvasLayer::new())))
    }

    #[test]
    fn scripted_sink_counts_submits_and_replays_its_script() {
        let mut sink =
            ScriptedSink::fails_once_then_presents(SubmitVerdict::SurfaceStale).with_size(640, 480);
        assert_eq!(sink.surface_size(), (640, 480));
        assert_eq!(sink.submit(empty_scene()), SubmitVerdict::SurfaceStale);
        assert_eq!(sink.submit(empty_scene()), SubmitVerdict::Presented);
        assert_eq!(sink.submit_calls, 2);
    }

    #[test]
    #[should_panic(expected = "submit called more than once")]
    fn single_shot_sink_refuses_a_second_submit() {
        let mut sink = ScriptedSink::single_shot(SubmitVerdict::Presented);
        let _ = sink.submit(empty_scene());
        let _ = sink.submit(empty_scene());
    }

    #[test]
    fn test_window_records_what_the_realm_asked_of_it() {
        let window = TestWindow::new().with_id(7).focused(true);
        let redraws = window.redraw_calls_handle();
        let window: Arc<dyn PlatformWindow> = Arc::new(window);
        window.request_redraw();
        window
            .set_cursor(CursorIcon::Pointer)
            .expect("the double accepts every cursor");
        assert_eq!(window.id(), WindowId(7));
        assert!(window.is_focused());
        assert_eq!(redraws.load(Ordering::Relaxed), 1);
        let concrete = window
            .as_any()
            .downcast_ref::<TestWindow>()
            .expect("as_any exposes the double");
        assert_eq!(concrete.cursor(), CursorIcon::Pointer);
    }
}
