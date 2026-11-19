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
    AdmittedCommand, ApplyError, Cursor, CursorError, DisplayId, FrameRateHint, GestureGeometry,
    PlatformSystemChrome, PlatformTextInput, PlatformWindow, PreferenceQueryError, RedrawRequester,
    SurfaceTarget, Unsupported, UnsupportedReason, WindowAt, WindowCapabilities, WindowCommand,
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
    /// [`PlatformWindow::apply`] took this command: told after the window
    /// recorded it, never for a command it refused.
    Apply(&'a WindowCommand),
    /// [`PlatformWindow::set_cursor`], before the window takes the cursor.
    SetCursor(&'a Cursor),
    /// [`PlatformWindow::set_input_regions`] on a window that
    /// [takes them](MockWindow::taking_input_regions): told after it recorded
    /// them.
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

/// A [`PlatformWindow`] with no window system behind it: the one window
/// double, for a test that needs a window and not a host.
///
/// Built standalone ([`new`](Self::new)) it is its own window: a fresh id from
/// its thread's mint, a zero extent at scale 1 (so no metrics), visible and
/// unfocused, honouring no command but the normal mode. Built
/// [`over`](Self::over) another window it is that window, with what its
/// builders set in front: what a test changes of a real host window, such as
/// the headless host's.
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
pub struct MockWindow {
    inner: Option<Rc<dyn PlatformWindow>>,
    id: WindowId,
    extent: WindowExtent,
    scale_factor: DevicePixelRatio,
    focused: bool,
    visible: bool,
    display: Cell<DisplayReport>,
    capabilities: WindowCapabilities,
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

impl std::fmt::Debug for MockWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockWindow")
            .field("id", &self.id)
            .field("over", &self.inner.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for MockWindow {
    fn default() -> Self {
        Self::new()
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
        Self::built(None, id)
    }

    /// `inner`, with what the builders set in front of it: its id, and every
    /// call this window does not answer itself, are `inner`'s.
    #[must_use]
    pub fn over(inner: Rc<dyn PlatformWindow>) -> Self {
        let id = inner.id();
        Self::built(Some(inner), id)
    }

    fn built(inner: Option<Rc<dyn PlatformWindow>>, id: WindowId) -> Self {
        Self {
            inner,
            id,
            extent: WindowExtent::ZERO,
            scale_factor: DevicePixelRatio::ONE,
            focused: false,
            visible: true,
            display: Cell::new(DisplayReport::Unset),
            capabilities: Self::publish(WindowCapabilities::NONE),
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

    fn publish(capabilities: WindowCapabilities) -> WindowCapabilities {
        capabilities
            .publish()
            .expect("BUG: a test publishes far fewer than u64::MAX capabilities")
    }

    /// Answer `id` from [`PlatformWindow::id`].
    #[must_use]
    pub fn with_id(mut self, id: WindowId) -> Self {
        self.id = id;
        self
    }

    /// A window `width` × `height` logical pixels large: its metrics are that
    /// extent at its [scale](Self::with_scale_factor).
    ///
    /// # Panics
    /// When a side is negative or not finite: a test names a window's size.
    #[must_use]
    pub fn with_size(mut self, width: f64, height: f64) -> Self {
        self.extent = WindowExtent::try_new(Size::new(width, height))
            .expect("a test names a finite, non-negative window size");
        self
    }

    /// Device pixels per logical pixel, which the metrics are at.
    #[must_use]
    pub fn with_scale_factor(mut self, scale_factor: DevicePixelRatio) -> Self {
        self.scale_factor = scale_factor;
        self
    }

    /// Whether the window has focus; a hidden window never has.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Whether the window is shown.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Honour `capabilities`, as a new publication of them: what its commands
    /// are admitted against.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: WindowCapabilities) -> Self {
        self.capabilities = Self::publish(capabilities);
        self
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

    /// Present into `target` instead of no native surface.
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

    /// Take input regions, recording each set
    /// ([`input_regions`](Self::input_regions)); without it the window refuses
    /// them, as one whose system has none.
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

    /// Every command the window took, in order.
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

    /// The state of a window of its own.
    fn own_state(&self) -> WindowState {
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
}

/// Counts a redraw request and runs the hook, from whichever thread asks.
fn record_redraw(requests: &AtomicUsize, hook: Option<&(dyn Fn() + Send + Sync)>) {
    requests.fetch_add(1, Ordering::Relaxed);
    if let Some(hook) = hook {
        hook();
    }
}

impl PlatformWindow for MockWindow {
    fn id(&self) -> WindowId {
        self.observe(WindowCall::Id);
        self.id
    }

    /// What was set when the query began: an observer that
    /// [sets another answer](Self::set_gesture_geometry) sets the next
    /// query's, as a system whose geometry changes during a read.
    fn gesture_geometry(&self) -> GeometryAnswer {
        let set = self.gesture_geometry.borrow().clone();
        let answer = match (set, &self.inner) {
            (Some(answer), _) => answer,
            (None, Some(inner)) => inner.gesture_geometry(),
            (None, None) => Ok(None),
        };
        self.observe(WindowCall::GestureGeometry);
        answer
    }

    fn request_redraw(&self) {
        record_redraw(&self.redraw_requests, self.redraw_hook.as_deref());
        if let Some(inner) = &self.inner {
            inner.request_redraw();
        }
    }

    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        let requests = Arc::clone(&self.redraw_requests);
        let hook = self.redraw_hook.clone();
        let inner = self.inner.as_ref().map(|inner| inner.redraw_requester());
        Arc::new(move || {
            record_redraw(&requests, hook.as_deref());
            if let Some(inner) = &inner {
                inner.request_redraw();
            }
        })
    }

    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        match (&self.surface_target, &self.inner) {
            (Some(target), _) => Arc::clone(target),
            (None, Some(inner)) => inner.surface_target(),
            (None, None) => Arc::new(super::NoNativeSurface),
        }
    }

    /// The window's own state, or `inner`'s; on the display a test
    /// [set](Self::set_display) either way.
    fn state(&self) -> WindowState {
        self.observe(WindowCall::State);
        let state = match &self.inner {
            Some(inner) => inner.state(),
            None => self.own_state(),
        };
        match self.display.get() {
            DisplayReport::On(display) => state.with_display(display),
            DisplayReport::Unset => state,
        }
    }

    /// Records the command and leaves the window as it was; over another
    /// window, that window carries it out.
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        let refused = self
            .refusal
            .as_ref()
            .and_then(|refusal| refusal(command.command()));
        if let Some(error) = refused {
            return Err(error);
        }
        let Some(inner) = &self.inner else {
            let command = command.against(&self.capabilities)?;
            self.applied.borrow_mut().push(command.clone());
            self.observe(WindowCall::Apply(&command));
            return Ok(());
        };
        let taken = command.command().clone();
        self.applied.borrow_mut().push(taken.clone());
        self.observe(WindowCall::Apply(&taken));
        inner.apply(command)
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input
            .clone()
            .or_else(|| self.inner.as_ref().and_then(|inner| inner.text_input()))
    }

    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        self.haptics
            .clone()
            .or_else(|| self.inner.as_ref().and_then(|inner| inner.haptics()))
    }

    fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
        self.system_chrome
            .clone()
            .or_else(|| self.inner.as_ref().and_then(|inner| inner.system_chrome()))
    }

    fn drag_source(&self) -> Option<Arc<dyn PlatformDragSource>> {
        self.drag_source
            .clone()
            .or_else(|| self.inner.as_ref().and_then(|inner| inner.drag_source()))
    }

    fn window_at(&self, position: Point<f64>) -> WindowAt {
        self.inner
            .as_ref()
            .map_or(WindowAt::Unknown, |inner| inner.window_at(position))
    }

    fn set_input_regions(&self, regions: &[InputRegion]) -> Result<(), Unsupported> {
        if let Some(taken) = &self.input_regions {
            taken.borrow_mut().push(regions.to_vec());
            self.observe(WindowCall::SetInputRegions(regions));
            return Ok(());
        }
        match &self.inner {
            Some(inner) => inner.set_input_regions(regions),
            None => Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform)),
        }
    }

    fn reveal_after_first_frame(&self) {
        if let Some(inner) = &self.inner {
            inner.reveal_after_first_frame();
        }
    }

    /// A window of its own takes every cursor but an image, which it refuses
    /// exactly when its capabilities make no cursor image, as the contract
    /// says; over another window, that window answers.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        self.observe(WindowCall::SetCursor(cursor));
        match &self.inner {
            Some(inner) => inner.set_cursor(cursor),
            None if matches!(cursor, Cursor::Image(_))
                && !self.capabilities.cursor_image().is_available() =>
            {
                Err(Unsupported::from(crate::CapabilityAbsence::NOT_ON_THIS_PLATFORM).into())
            }
            None => Ok(()),
        }
    }

    fn set_frame_rate_hint(&self, hint: FrameRateHint) -> Result<(), Unsupported> {
        match &self.inner {
            Some(inner) => inner.set_frame_rate_hint(hint),
            None => Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform)),
        }
    }

    fn portal_parent(&self, answer: Resolver<PortalParent>) {
        match (&self.portal_parent, &self.inner) {
            (Some(parent), _) => {
                // The owner retiring first leaves nothing waiting for it.
                let _ = answer.resolve(parent());
            }
            (None, Some(inner)) => inner.portal_parent(answer),
            (None, None) => {
                let _ = answer.resolve(PortalParent::none());
            }
        }
    }
}
