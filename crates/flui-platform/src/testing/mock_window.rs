//! The one double of [`PlatformWindow`] (ADR-0243): a window with no window
//! system behind it, configured by its builders, that records what it was
//! asked and lets a test run its own code at each call.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::{DevicePixelRatio, Point, Size, WindowMetrics};

use crate::capability::{PortalParent, Resolver};
use crate::drag::PlatformDragSource;
use crate::haptics::PlatformHaptics;
use crate::input_regions::InputRegion;
use crate::{
    AdmittedCommand, ApplyError, CapabilityAbsence, Cursor, CursorError, DisplayId, FrameRateHint,
    GestureGeometry, PlatformSystemChrome, PlatformTextInput, PlatformWindow, PreferenceQueryError,
    RedrawRequester, SurfaceTarget, Unsupported, WindowAt, WindowCapabilities, WindowCommand,
    WindowExtent, WindowFocus, WindowGeometry, WindowId, WindowIdMint, WindowState,
};

/// What a gesture-geometry query answers.
type GeometryAnswer = Result<Option<GestureGeometry>, PreferenceQueryError>;

/// A call a [`MockWindow`] took, as its observer
/// ([`with_observer`](MockWindow::with_observer)) is told of it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum WindowCall<'a> {
    /// [`PlatformWindow::id`], before it answers.
    Id,
    /// [`PlatformWindow::state`], before it answers.
    State,
    /// [`PlatformWindow::gesture_geometry`], once it read its answer and
    /// before it returns it.
    GestureGeometry,
    /// [`PlatformWindow::apply`] took this command: told once the window took
    /// it (over another window, once that window did) and recorded it, never
    /// for a command refused.
    Apply(&'a WindowCommand),
    /// [`PlatformWindow::set_cursor`] took this cursor (over another window,
    /// once that window did); never told for a cursor refused.
    SetCursor(&'a Cursor),
    /// [`PlatformWindow::set_input_regions`] on a window built
    /// [taking them](MockWindow::taking_input_regions): told once the window
    /// took the set (over another window, once that window did) and recorded
    /// it, never for a set refused.
    SetInputRegions(&'a [InputRegion]),
}

/// The test's code a window runs at each call it takes.
type Observer = Rc<dyn Fn(WindowCall<'_>)>;

/// What refuses a command the window would otherwise take.
type Refusal = Box<dyn Fn(&WindowCommand) -> Option<ApplyError>>;

/// The display a window reports itself on.
#[derive(Clone, Copy)]
enum DisplayReport {
    /// Its own, or the inner window's.
    Unset,
    /// The one a test [set](MockWindow::set_display).
    On(Option<DisplayId>),
}

/// What a [`MockWindow`] stands on: a window of its own ([`Standalone`]) or
/// another window ([`Over`]), which answers what the double's builders do
/// not set.
mod base {
    use super::{
        AdmittedCommand, ApplyError, Cursor, CursorError, FrameRateHint, GeometryAnswer,
        InputRegion, PlatformDragSource, PlatformHaptics, PlatformSystemChrome, PlatformTextInput,
        Point, PortalParent, RedrawRequester, Resolver, SurfaceTarget, Unsupported, WindowAt,
        WindowCommand, WindowId, WindowState,
    };
    use std::sync::Arc;

    /// The answers of what a [`MockWindow`](super::MockWindow) stands on.
    /// Sealed: [`Standalone`](super::Standalone) and
    /// [`Over`](super::Over) are the two.
    pub trait Base {
        fn id(&self) -> WindowId;
        fn state(&self) -> WindowState;
        /// Carry out `command`; `took` runs with it once it is taken.
        fn take(
            &self,
            command: AdmittedCommand,
            took: &dyn Fn(WindowCommand),
        ) -> Result<(), ApplyError>;
        fn gesture_geometry(&self) -> GeometryAnswer;
        fn request_redraw(&self);
        fn request_frame_at(&self, at: Option<crate::EventTime>);
        fn redraw_requester(&self) -> Option<Arc<dyn RedrawRequester>>;
        fn surface_target(&self) -> Arc<dyn SurfaceTarget>;
        fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>>;
        fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>>;
        fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>>;
        fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>>;
        fn window_at(&self, position: Point<f64>) -> WindowAt;
        /// Take `regions`; `taking` is whether the double was built
        /// [`taking_input_regions`](super::MockWindow::taking_input_regions).
        fn set_input_regions(
            &self,
            regions: &[InputRegion],
            taking: bool,
        ) -> Result<(), Unsupported>;
        fn reveal_after_first_frame(&self);
        fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError>;
        fn set_frame_rate_hint(&self, hint: FrameRateHint) -> Result<(), Unsupported>;
        fn portal_parent(&self, answer: Resolver<PortalParent>);
    }
}

use base::Base;

/// A window of its own under a [`MockWindow`]: its id, extent, scale, focus,
/// visibility and capabilities, which the standalone builders set.
#[derive(Debug)]
pub struct Standalone {
    id: WindowId,
    extent: WindowExtent,
    scale_factor: DevicePixelRatio,
    focused: bool,
    visible: bool,
    capabilities: WindowCapabilities,
}

/// Another window under a [`MockWindow`] built [`over`](MockWindow::over) it:
/// its id, state, commands and every handle the double's builders leave
/// unset are that window's.
pub struct Over(Rc<dyn PlatformWindow>);

impl std::fmt::Debug for Over {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Over").field(&self.0.id()).finish()
    }
}

/// The refusal of a verb whose capability a window lacks.
fn not_on_this_platform() -> Unsupported {
    Unsupported::from(CapabilityAbsence::NOT_ON_THIS_PLATFORM)
}

impl Base for Standalone {
    fn id(&self) -> WindowId {
        self.id
    }

    fn state(&self) -> WindowState {
        let extent = self.extent;
        WindowState::new(WindowGeometry::unplaced(extent), extent)
            .with_visible(self.visible)
            .with_focus(if self.focused && self.visible {
                WindowFocus::Focused
            } else {
                WindowFocus::Inactive
            })
            .with_metrics(WindowMetrics::from_logical(extent.get(), self.scale_factor).ok())
            .with_capabilities(self.capabilities)
    }

    /// Takes what its capabilities admitted and leaves the window as it was.
    fn take(
        &self,
        command: AdmittedCommand,
        took: &dyn Fn(WindowCommand),
    ) -> Result<(), ApplyError> {
        took(command.against(&self.capabilities)?);
        Ok(())
    }

    fn gesture_geometry(&self) -> GeometryAnswer {
        Ok(None)
    }

    fn request_redraw(&self) {}

    /// Never asked: a standalone double keeps no surface while covered, so no
    /// frame is owed to it (ADR-0389).
    fn request_frame_at(&self, _: Option<crate::EventTime>) {}

    fn redraw_requester(&self) -> Option<Arc<dyn RedrawRequester>> {
        None
    }

    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        Arc::new(super::NoNativeSurface)
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        None
    }

    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        None
    }

    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        None
    }

    fn window_at(&self, _: Point<f64>) -> WindowAt {
        WindowAt::Unknown
    }

    /// Taken exactly when the double was built
    /// [taking them](MockWindow::taking_input_regions); else the window
    /// honours no kind of input region.
    fn set_input_regions(&self, _: &[InputRegion], taking: bool) -> Result<(), Unsupported> {
        if taking {
            Ok(())
        } else {
            Err(not_on_this_platform())
        }
    }

    fn reveal_after_first_frame(&self) {}

    /// Every cursor but an image, which it refuses exactly when its
    /// capabilities make no cursor image.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        if matches!(cursor, Cursor::Image(_)) && !self.capabilities.cursor_image().is_available() {
            return Err(not_on_this_platform().into());
        }
        Ok(())
    }

    /// Refused exactly when its capabilities take no refresh-rate request.
    fn set_frame_rate_hint(&self, _: FrameRateHint) -> Result<(), Unsupported> {
        if self.capabilities.frame_rate_hint().is_available() {
            Ok(())
        } else {
            Err(not_on_this_platform())
        }
    }

    fn portal_parent(&self, answer: Resolver<PortalParent>) {
        // The owner retiring first leaves nothing waiting for it.
        let _ = answer.resolve(PortalParent::none());
    }
}

impl Base for Over {
    fn id(&self) -> WindowId {
        self.0.id()
    }

    fn state(&self) -> WindowState {
        self.0.state()
    }

    /// Takes a command once the wrapped window took it.
    fn take(
        &self,
        command: AdmittedCommand,
        took: &dyn Fn(WindowCommand),
    ) -> Result<(), ApplyError> {
        let taken = command.command().clone();
        self.0.apply(command)?;
        took(taken);
        Ok(())
    }

    fn gesture_geometry(&self) -> GeometryAnswer {
        self.0.gesture_geometry()
    }

    fn request_redraw(&self) {
        self.0.request_redraw();
    }

    fn request_frame_at(&self, at: Option<crate::EventTime>) {
        self.0.request_frame_at(at);
    }

    fn redraw_requester(&self) -> Option<Arc<dyn RedrawRequester>> {
        Some(self.0.redraw_requester())
    }

    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        self.0.surface_target()
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.0.text_input()
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        self.0.haptics()
    }

    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        self.0.system_chrome()
    }

    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        self.0.drag_source()
    }

    fn window_at(&self, position: Point<f64>) -> WindowAt {
        self.0.window_at(position)
    }

    /// The wrapped window answers, whether the double records what it takes
    /// or not.
    fn set_input_regions(&self, regions: &[InputRegion], _: bool) -> Result<(), Unsupported> {
        self.0.set_input_regions(regions)
    }

    fn reveal_after_first_frame(&self) {
        self.0.reveal_after_first_frame();
    }

    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        self.0.set_cursor(cursor)
    }

    fn set_frame_rate_hint(&self, hint: FrameRateHint) -> Result<(), Unsupported> {
        self.0.set_frame_rate_hint(hint)
    }

    fn portal_parent(&self, answer: Resolver<PortalParent>) {
        self.0.portal_parent(answer);
    }
}

/// A [`PlatformWindow`] with no window system behind it: the one window
/// double, for a test that needs a window and not a host.
///
/// Built standalone ([`new`](MockWindow::new)) it is its own window: a fresh
/// id from its thread's mint, a zero extent at scale 1 (so no metrics),
/// visible and unfocused, honouring no command but the normal mode; its verbs
/// answer as its capabilities say. Built [`over`](MockWindow::over) another
/// window it is that window, with what its builders set in front: what a test
/// changes of a real host window, such as the headless host's. A window built
/// over another has no builders of its own state (size, scale, focus,
/// visibility, capabilities, id): those are the wrapped window's, and setting
/// them does not compile.
///
/// What a test sets is a builder; what it reads back is a recording
/// ([`redraw_requests`](Self::redraw_requests), [`applied`](Self::applied),
/// [`input_regions`](Self::input_regions)). Its own code runs at each call
/// through the [observer](Self::with_observer), which may count, re-enter the
/// caller or panic there, and at each redraw request through the
/// [redraw hook](Self::with_redraw_hook), from whichever thread asks. A value
/// the window [keeps](Self::keeping) drops with it, so a test watches the
/// window's drop through that value's.
///
/// The builders are named `with_*` where a bare name would shadow the
/// [`PlatformWindow`] method a test calls on the same value.
pub struct MockWindow<B = Standalone> {
    base: B,
    display: Cell<DisplayReport>,
    gesture_geometry: RefCell<Option<GeometryAnswer>>,
    text_input: Option<Arc<dyn PlatformTextInput>>,
    haptics: Option<Arc<dyn PlatformHaptics>>,
    system_chrome: Option<Arc<dyn PlatformSystemChrome>>,
    drag_source: Option<Arc<dyn PlatformDragSource>>,
    surface_target: Option<Arc<dyn SurfaceTarget>>,
    portal_parent: Option<Box<dyn Fn() -> PortalParent>>,
    refusal: Option<Refusal>,
    observer: Option<Observer>,
    redraw_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    redraw_requests: Arc<AtomicUsize>,
    applied: RefCell<Vec<WindowCommand>>,
    input_regions: Option<RefCell<Vec<Vec<InputRegion>>>>,
    /// Dropped with the window, after everything above.
    kept: Vec<Box<dyn Any>>,
}

impl<B: Base> std::fmt::Debug for MockWindow<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockWindow")
            .field("id", &self.base.id())
            .finish_non_exhaustive()
    }
}

impl Default for MockWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl MockWindow<Over> {
    /// `inner`, with what the builders set in front of it: its id, its state,
    /// its commands and every call this window does not answer itself are
    /// `inner`'s.
    #[must_use]
    pub fn over(inner: Rc<dyn PlatformWindow>) -> Self {
        MockWindow::on(Over(inner))
    }
}

impl MockWindow {
    /// A window of its own: a fresh id, a zero extent at scale 1, visible and
    /// unfocused, honouring no command but the normal mode.
    #[must_use]
    pub fn new() -> Self {
        thread_local! {
            // One mint per test thread: two windows of one test are two ids.
            static WINDOW_IDS: WindowIdMint = const { WindowIdMint::new() };
        }
        let id = WINDOW_IDS
            .with(WindowIdMint::next)
            .expect("BUG: a test opens far fewer windows than a mint issues");
        MockWindow::on(Standalone {
            id,
            extent: WindowExtent::ZERO,
            scale_factor: DevicePixelRatio::ONE,
            focused: false,
            visible: true,
            capabilities: publish(WindowCapabilities::NONE),
        })
    }

    /// Answer `id` from [`PlatformWindow::id`].
    #[must_use]
    pub fn with_id(mut self, id: WindowId) -> Self {
        self.base.id = id;
        self
    }

    /// A window `width` × `height` logical pixels large: its metrics are that
    /// extent at its [scale](Self::with_scale_factor).
    ///
    /// # Panics
    /// When a side is negative or not finite: a test names a window's size.
    #[must_use]
    pub fn with_size(mut self, width: f64, height: f64) -> Self {
        self.base.extent = WindowExtent::try_new(Size::new(width, height))
            .expect("a test names a finite, non-negative window size");
        self
    }

    /// Device pixels per logical pixel, which the metrics are at.
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: DevicePixelRatio) -> Self {
        self.base.scale_factor = scale_factor;
        self
    }

    /// Whether the window has focus; a hidden window never has.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.base.focused = focused;
        self
    }

    /// Whether the window is shown.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.base.visible = visible;
        self
    }

    /// Honour `capabilities`, as a new publication of them: what its commands
    /// are admitted against, and what its verbs answer by (a refresh-rate
    /// request, an image cursor).
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: WindowCapabilities) -> Self {
        self.base.capabilities = publish(capabilities);
        self
    }
}

/// `capabilities` as a new publication.
fn publish(capabilities: WindowCapabilities) -> WindowCapabilities {
    capabilities
        .publish()
        .expect("BUG: a test publishes far fewer than u64::MAX capabilities")
}

impl<B: Base> MockWindow<B> {
    fn on(base: B) -> Self {
        Self {
            base,
            display: Cell::new(DisplayReport::Unset),
            gesture_geometry: RefCell::new(None),
            text_input: None,
            haptics: None,
            system_chrome: None,
            drag_source: None,
            surface_target: None,
            portal_parent: None,
            refusal: None,
            observer: None,
            redraw_hook: None,
            redraw_requests: Arc::new(AtomicUsize::new(0)),
            applied: RefCell::new(Vec::new()),
            input_regions: None,
            kept: Vec::new(),
        }
    }

    /// Answer every gesture-geometry query with `answer`; a test changes it
    /// later with [`set_gesture_geometry`](Self::set_gesture_geometry).
    #[must_use]
    pub fn with_gesture_geometry(self, answer: GeometryAnswer) -> Self {
        self.set_gesture_geometry(answer);
        self
    }

    /// Offer `text_input` from [`PlatformWindow::text_input`].
    #[must_use]
    pub fn with_text_input(mut self, text_input: Arc<dyn PlatformTextInput>) -> Self {
        self.text_input = Some(text_input);
        self
    }

    /// Offer `haptics` from [`PlatformWindow::haptics`].
    #[must_use]
    pub fn with_haptics(mut self, haptics: Arc<dyn PlatformHaptics>) -> Self {
        self.haptics = Some(haptics);
        self
    }

    /// Offer `system_chrome` from [`PlatformWindow::system_chrome`].
    #[must_use]
    pub fn with_system_chrome(mut self, system_chrome: Arc<dyn PlatformSystemChrome>) -> Self {
        self.system_chrome = Some(system_chrome);
        self
    }

    /// Offer `drag_source` from [`PlatformWindow::drag_source`].
    #[must_use]
    pub fn with_drag_source(mut self, drag_source: Arc<dyn PlatformDragSource>) -> Self {
        self.drag_source = Some(drag_source);
        self
    }

    /// Present into `target` instead of no native surface, or the wrapped
    /// window's.
    #[must_use]
    pub fn with_surface_target(mut self, target: Arc<dyn SurfaceTarget>) -> Self {
        self.surface_target = Some(target);
        self
    }

    /// Name the window to a desktop portal with what `parent` builds, at each
    /// request.
    #[must_use]
    pub fn with_portal_parent(mut self, parent: impl Fn() -> PortalParent + 'static) -> Self {
        self.portal_parent = Some(Box::new(parent));
        self
    }

    /// Refuse a command `refusal` answers an error for, as the window system
    /// refusing it; the window records only the commands it takes.
    #[must_use]
    pub fn with_refusal(
        mut self,
        refusal: impl Fn(&WindowCommand) -> Option<ApplyError> + 'static,
    ) -> Self {
        self.refusal = Some(Box::new(refusal));
        self
    }

    /// Run `observer` at each call [`WindowCall`] names, with no borrow of the
    /// window held: it may count, call back into its caller, or panic, as a
    /// window system's callback can.
    #[must_use]
    pub fn with_observer(mut self, observer: impl Fn(WindowCall<'_>) + 'static) -> Self {
        self.observer = Some(Rc::new(observer));
        self
    }

    /// Run `hook` at each redraw request, once it is counted: from the owner
    /// thread through [`PlatformWindow::request_redraw`], from any thread
    /// through the [requester](PlatformWindow::redraw_requester).
    #[must_use]
    pub fn with_redraw_hook(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.redraw_hook = Some(Arc::new(hook));
        self
    }

    /// Record each input-region set the window takes
    /// ([`input_regions`](Self::input_regions)). A window of its own takes
    /// them exactly when built with this, and refuses them without it, as one
    /// whose system honours no kind of input region; a window over another
    /// hands every set to it either way, and records the sets it took.
    #[must_use]
    pub fn taking_input_regions(mut self) -> Self {
        self.input_regions = Some(RefCell::new(Vec::new()));
        self
    }

    /// Keep `value` until the window drops, and drop it then: a test watches
    /// the window's drop, or makes it fail, through the value's own.
    #[must_use]
    pub fn keeping(mut self, value: impl Any) -> Self {
        self.kept.push(Box::new(value));
        self
    }

    /// Answer gesture-geometry queries with `answer` from now on.
    pub fn set_gesture_geometry(&self, answer: GeometryAnswer) {
        *self.gesture_geometry.borrow_mut() = Some(answer);
    }

    /// Report the window on `display` from now on.
    pub fn set_display(&self, display: Option<DisplayId>) {
        self.display.set(DisplayReport::On(display));
    }

    /// How many redraws the window was asked for, through either path.
    #[must_use]
    pub fn redraw_requests(&self) -> usize {
        self.redraw_requests.load(Ordering::Relaxed)
    }

    /// Every command the window took, in order (over another window, every
    /// command that window took).
    #[must_use]
    pub fn applied(&self) -> Vec<WindowCommand> {
        self.applied.borrow().clone()
    }

    /// Every input-region set the window took, in order; empty for one that
    /// does not [take them](Self::taking_input_regions).
    #[must_use]
    pub fn input_regions(&self) -> Vec<Vec<InputRegion>> {
        self.input_regions
            .as_ref()
            .map_or_else(Vec::new, |taken| taken.borrow().clone())
    }

    fn observe(&self, call: WindowCall<'_>) {
        // A clone, so the observer runs with no borrow of the window held.
        if let Some(observer) = self.observer.clone() {
            observer(call);
        }
    }
}

/// Counts a redraw request and runs the hook, from whichever thread asks.
fn record_redraw(requests: &AtomicUsize, hook: Option<&(dyn Fn() + Send + Sync)>) {
    requests.fetch_add(1, Ordering::Relaxed);
    if let Some(hook) = hook {
        hook();
    }
}

impl<B: Base> PlatformWindow for MockWindow<B> {
    fn id(&self) -> WindowId {
        self.observe(WindowCall::Id);
        self.base.id()
    }

    /// What was set when the query began: an observer that
    /// [sets another answer](Self::set_gesture_geometry) sets the next
    /// query's, as a system whose geometry changes during a read.
    fn gesture_geometry(&self) -> GeometryAnswer {
        let set = self.gesture_geometry.borrow().clone();
        let answer = set.unwrap_or_else(|| self.base.gesture_geometry());
        self.observe(WindowCall::GestureGeometry);
        answer
    }

    fn request_redraw(&self) {
        record_redraw(&self.redraw_requests, self.redraw_hook.as_deref());
        self.base.request_redraw();
    }

    /// What its base does: a standalone double is never asked, one over
    /// another window hands the request to it.
    fn request_frame_at(&self, at: Option<crate::EventTime>) {
        self.base.request_frame_at(at);
    }

    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        let requests = Arc::clone(&self.redraw_requests);
        let hook = self.redraw_hook.clone();
        let inner = self.base.redraw_requester();
        Arc::new(move || {
            record_redraw(&requests, hook.as_deref());
            if let Some(inner) = &inner {
                inner.request_redraw();
            }
        })
    }

    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        self.surface_target
            .clone()
            .unwrap_or_else(|| self.base.surface_target())
    }

    /// The window's own state, or the wrapped window's; on the display a test
    /// [set](Self::set_display) either way.
    fn state(&self) -> WindowState {
        self.observe(WindowCall::State);
        let state = self.base.state();
        match self.display.get() {
            DisplayReport::On(display) => state.with_display(display),
            DisplayReport::Unset => state,
        }
    }

    /// Records the command once the window took it: a window of its own
    /// leaves itself as it was, a window over another hands the command to
    /// it first.
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        let refused = self
            .refusal
            .as_ref()
            .and_then(|refusal| refusal(command.command()));
        if let Some(error) = refused {
            return Err(error);
        }
        self.base.take(command, &|taken| {
            self.applied.borrow_mut().push(taken.clone());
            self.observe(WindowCall::Apply(&taken));
        })
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input.clone().or_else(|| self.base.text_input())
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        self.haptics.clone().or_else(|| self.base.haptics())
    }

    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        self.system_chrome
            .clone()
            .or_else(|| self.base.system_chrome())
    }

    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        self.drag_source.clone().or_else(|| self.base.drag_source())
    }

    fn window_at(&self, position: Point<f64>) -> WindowAt {
        self.base.window_at(position)
    }

    /// A set the window took is recorded and told to the observer: a window
    /// of its own takes them when built [taking them](Self::taking_input_regions),
    /// a window over another once that window took them.
    fn set_input_regions(&self, regions: &[InputRegion]) -> Result<(), Unsupported> {
        self.base
            .set_input_regions(regions, self.input_regions.is_some())?;
        if let Some(taken) = &self.input_regions {
            taken.borrow_mut().push(regions.to_vec());
            self.observe(WindowCall::SetInputRegions(regions));
        }
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        self.base.reveal_after_first_frame();
    }

    /// A cursor the window took is told to the observer once it is taken.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        self.base.set_cursor(cursor)?;
        self.observe(WindowCall::SetCursor(cursor));
        Ok(())
    }

    fn set_frame_rate_hint(&self, hint: FrameRateHint) -> Result<(), Unsupported> {
        self.base.set_frame_rate_hint(hint)
    }

    fn portal_parent(&self, answer: Resolver<PortalParent>) {
        match &self.portal_parent {
            Some(parent) => {
                // The owner retiring first leaves nothing waiting for it.
                let _ = answer.resolve(parent());
            }
            None => self.base.portal_parent(answer),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use super::{MockWindow, WindowCall};
    use crate::{
        ApplyError, Availability, Cursor, CursorImage, FrameRateHint, PlatformWindow,
        WindowCapabilities, WindowCommand,
    };

    /// What a standalone window is built with that a verb outside the command
    /// queue answers by.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Gate {
        FrameRateHint,
        CursorImage,
        TakingInputRegions,
    }

    /// A standalone window with `gate` alone available.
    fn window_with(gate: Option<Gate>) -> MockWindow {
        let mut parts = WindowCapabilities::NONE.into_parts();
        parts.frame_rate_hint = Availability::Unavailable;
        parts.cursor_image = Availability::Unavailable;
        match gate {
            Some(Gate::FrameRateHint) => parts.frame_rate_hint = Availability::Native,
            Some(Gate::CursorImage) => parts.cursor_image = Availability::Native,
            Some(Gate::TakingInputRegions) | None => {}
        }
        let window = MockWindow::new().with_capabilities(parts.into_capabilities());
        if gate == Some(Gate::TakingInputRegions) {
            window.taking_input_regions()
        } else {
            window
        }
    }

    fn image() -> Cursor {
        Cursor::Image(
            CursorImage::try_new(1, 1, (0, 0), vec![0; 4]).expect("a 1 × 1 RGBA image is a cursor"),
        )
    }

    /// Each verb a window takes outside the command queue answers by its own
    /// gate and no other, refused exactly when that gate is shut:
    /// `set_frame_rate_hint` by `frame_rate_hint`, `set_cursor` for an image
    /// by `cursor_image`, `set_input_regions` by the `taking_input_regions`
    /// builder. One row per gate, with only it open.
    #[test]
    fn a_standalone_windows_verbs_answer_as_its_capabilities_say() {
        for gate in [
            None,
            Some(Gate::FrameRateHint),
            Some(Gate::CursorImage),
            Some(Gate::TakingInputRegions),
        ] {
            let window = window_with(gate);
            let verbs = [
                (
                    Gate::FrameRateHint,
                    window.set_frame_rate_hint(FrameRateHint::High).is_ok(),
                ),
                (Gate::CursorImage, window.set_cursor(&image()).is_ok()),
                (
                    Gate::TakingInputRegions,
                    window.set_input_regions(&[]).is_ok(),
                ),
            ];
            for (verb, taken) in verbs {
                assert_eq!(
                    taken,
                    gate == Some(verb),
                    "the verb gated by {verb:?} with only {gate:?} open"
                );
            }
            assert!(
                window
                    .set_cursor(&Cursor::Icon(crate::CursorIcon::Default))
                    .is_ok(),
                "an icon cursor is taken with only {gate:?} open"
            );
        }
    }

    /// Over another window, an input-region set reaches that window first:
    /// one it refuses is refused, and neither recorded nor told to the
    /// observer, though the wrapper records the sets it takes.
    #[test]
    fn a_wrapped_window_records_only_the_input_regions_the_inner_window_took() {
        for honours in [true, false] {
            let inner = if honours {
                MockWindow::new().taking_input_regions()
            } else {
                MockWindow::new()
            };
            let told = Rc::new(Cell::new(0));
            let counted = Rc::clone(&told);
            let wrapper = MockWindow::over(Rc::new(inner))
                .taking_input_regions()
                .with_observer(move |call| {
                    if matches!(call, WindowCall::SetInputRegions(_)) {
                        counted.set(counted.get() + 1);
                    }
                });

            let outcome = wrapper.set_input_regions(&[]);

            assert_eq!(outcome.is_ok(), honours);
            let recorded: Vec<Vec<crate::input_regions::InputRegion>> =
                if honours { vec![vec![]] } else { vec![] };
            assert_eq!(wrapper.input_regions(), recorded, "honoured: {honours}");
            assert_eq!(told.get(), usize::from(honours), "honoured: {honours}");
        }
    }

    /// The observer hears a cursor only once the window took it: an image a
    /// window without cursor images refuses is not reported.
    #[test]
    fn the_observer_hears_only_cursors_the_window_took() {
        let told = Rc::new(Cell::new(0));
        let counted = Rc::clone(&told);
        let window = window_with(None).with_observer(move |call| {
            if matches!(call, WindowCall::SetCursor(_)) {
                counted.set(counted.get() + 1);
            }
        });

        assert!(window.set_cursor(&image()).is_err());
        assert_eq!(told.get(), 0, "a refused image is not reported");
        assert!(
            window
                .set_cursor(&Cursor::Icon(crate::CursorIcon::Default))
                .is_ok()
        );
        assert_eq!(told.get(), 1, "a taken icon is");
    }

    /// Over another window, a command is recorded and told to the observer
    /// only once that window took it: one it refuses leaves no record.
    #[test]
    fn a_wrapped_window_records_only_what_the_inner_window_took() {
        for refuses in [true, false] {
            let inner = MockWindow::new()
                .with_capabilities(WindowCapabilities::ALL)
                .with_refusal(move |_| {
                    refuses.then(|| ApplyError::failed("the system said no".to_owned()))
                });
            let inner = Rc::new(inner);
            let told = Rc::new(Cell::new(0));
            let counted = Rc::clone(&told);
            let wrapper = MockWindow::over(Rc::clone(&inner) as Rc<dyn PlatformWindow>)
                .with_observer(move |call| {
                    if matches!(call, WindowCall::Apply(_)) {
                        counted.set(counted.get() + 1);
                    }
                });
            let command = WindowCommand::SetTitle("Notes".to_owned());
            let admitted = command
                .clone()
                .admit(&wrapper.state().capabilities)
                .expect("the inner window honours every command");

            let outcome = wrapper.apply(admitted);

            assert_eq!(outcome.is_err(), refuses);
            let recorded = if refuses { vec![] } else { vec![command] };
            assert_eq!(wrapper.applied(), recorded, "refused: {refuses}");
            assert_eq!(inner.applied(), recorded, "refused: {refuses}");
            assert_eq!(told.get(), usize::from(!refuses), "refused: {refuses}");
        }
    }

    /// A haptics handle set on a wrapper answers in place of the wrapped
    /// window's, which answers the rest.
    #[test]
    fn a_wrapped_window_answers_what_its_builders_leave_unset() {
        let inner: Rc<dyn PlatformWindow> = Rc::new(window_with(Some(Gate::FrameRateHint)));
        let haptics = Arc::new(crate::testing::FakeHaptics::new());
        let wrapper = MockWindow::over(Rc::clone(&inner)).with_haptics(haptics);

        assert!(wrapper.haptics().is_some());
        assert_eq!(wrapper.id(), inner.id());
        assert!(wrapper.set_frame_rate_hint(FrameRateHint::High).is_ok());
        assert_eq!(
            wrapper.state().capabilities.frame_rate_hint(),
            Availability::Native
        );
    }
}
