use super::*;

use flui_interaction::{FocusManager, FocusNode, TextInputError, TextInputHandle};
use flui_platform_api::{PlatformTextInput, TextStore};

fn close_frame_reports(realm: &UiRealm) -> Arc<parking_lot::Mutex<Vec<String>>> {
    let reports = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = Arc::clone(&reports);
    realm.set_frame_failure_detail(crate::frame_failure::FrameFailureDetail::Verbatim);
    realm.set_frame_failure_handler(Some(crate::frame_failure::FrameFailureHandler::new(
        move |report| captured.lock().push(format!("{report:?}")),
    )));
    reports
}

fn assert_close_frames_healthy(reports: &parking_lot::Mutex<Vec<String>>, stage: &str) {
    let reports = reports.lock().clone();
    assert!(reports.is_empty(), "{stage} frame failures: {reports:?}");
}

fn close_painted_scene(realm: &UiRealm, id: PresentationId, sink: &mut ScriptedSink) -> String {
    let (width, height) = crate::sink::FrameSink::surface_size(sink);
    realm.enter(|realm| {
        let presentation = realm.presentations.get(id).expect("resident producer");
        let dpr = presentation
            .pipeline()
            .with(flui_rendering::PipelineOwner::device_pixel_ratio);
        let constraints = BoxConstraints::tight(flui_foundation::geometry::Size::new(
            f64::from(width) / dpr,
            f64::from(height) / dpr,
        ));
        // The first pump already settled each owner. Request fresh paint so
        // these are actual producer outcomes, rather than an idle second draw.
        presentation.pipeline().with_mut(|owner| {
            if let Some(root) = owner.root_id() {
                owner.mark_needs_paint(root);
            }
        });
        match UiRealm::draw_frame_for_presentation(presentation, constraints, &realm.text) {
            Ok(FramePaintOutcome::Painted(scene)) => format!("{:?}", scene.tree()),
            Ok(FramePaintOutcome::Idle) => panic!("{id:?} must produce a Painted scene"),
            Ok(FramePaintOutcome::Errored) => panic!("{id:?} producer returned Errored"),
            Err(error) => panic!("{id:?} producer failed: {error}"),
        }
    })
}

#[derive(Debug, PartialEq)]
struct CloseObservation {
    focus_closed: bool,
    primary_focus_cleared: bool,
    text_input: Result<(), TextInputError>,
    ime_allowed: Option<bool>,
}

/// Observe inside the real disposal hook: owner destruction after close must
/// not make a skipped text-input close look successful.
#[derive(Clone)]
struct CloseObserver {
    key: flui_view::GlobalKey<CloseObserver>,
    focus: Rc<FocusManager>,
    input: TextInputHandle,
    platform: Arc<CloseTextInput>,
    seen: Rc<std::cell::RefCell<Vec<CloseObservation>>>,
    capabilities: Rc<std::cell::RefCell<Option<CloseCapabilities>>>,
}

struct CloseCapabilities {
    graph: flui_view::reactive::Reactive,
    signal: flui_view::Signal<u32>,
    writer: flui_view::WriterSource,
    rebuild: flui_view::RebuildHandle,
    lifecycle: flui_view::LifecycleHandle,
}

impl StatefulView for CloseObserver {
    type State = Self;

    fn create_state(&self) -> Self::State {
        self.clone()
    }
}

impl ViewState<CloseObserver> for CloseObserver {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.capabilities.borrow_mut().replace(CloseCapabilities {
            graph: ctx.reactive(),
            signal: ctx.signal(7),
            writer: ctx.writer_source(),
            rebuild: ctx.rebuild_handle(),
            lifecycle: ctx.lifecycle_handle().expect("bound lifecycle"),
        });
    }
    fn build(&self, _view: &CloseObserver, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }

    fn dispose(&mut self) {
        self.seen.borrow_mut().push(CloseObservation {
            focus_closed: self.focus.is_closed(),
            primary_focus_cleared: self.focus.primary_focus().is_none(),
            text_input: self.input.ensure_open(),
            ime_allowed: self.platform.recorder.last_ime_allowed(),
        });
    }
}

impl View for CloseObserver {
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// Exercise the platform capability the presentation actually calls; the
/// headless recorder remains the oracle even when disable itself panics.
struct CloseTextInput {
    recorder: flui_platform::FakeTextInput,
    fail_disable: bool,
    reenter_disable: bool,
}

thread_local! {
    static CLOSE_REENTRY: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

impl PlatformTextInput for CloseTextInput {
    fn set_ime_allowed(&self, allowed: bool) {
        self.recorder.set_ime_allowed(allowed);
        if !allowed && self.reenter_disable {
            CLOSE_REENTRY.with(|hook| {
                if let Some(hook) = &*hook.borrow() {
                    hook();
                }
            });
        }
        assert!(allowed || !self.fail_disable, "IME disable failure");
    }

    fn set_ime_cursor_area(&self, area: flui_foundation::geometry::Bounds<f64>) {
        self.recorder.set_ime_cursor_area(area);
    }
}

struct CursorCapture {
    fail: bool,
    drops: Arc<AtomicUsize>,
}

struct DropCompetition {
    first: CursorCapture,
    second: CursorCapture,
}

struct CloseArenaMember {
    calls: Rc<Cell<usize>>,
    captures: DropCompetition,
}

struct ArmedKeyCapture {
    armed: Arc<AtomicBool>,
    fail: bool,
}

impl Drop for ArmedKeyCapture {
    fn drop(&mut self) {
        assert!(
            !self.armed.load(Ordering::Relaxed) || !self.fail,
            "competing key capture failure"
        );
    }
}

struct RetirementKey {
    id: u8,
    armed: Arc<AtomicBool>,
    fail: bool,
    cloned: bool,
    drops: Arc<AtomicUsize>,
    captures: Option<(ArmedKeyCapture, ArmedKeyCapture)>,
}

impl Clone for RetirementKey {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            armed: Arc::clone(&self.armed),
            fail: self.fail,
            cloned: false,
            drops: Arc::clone(&self.drops),
            captures: None,
        }
    }
}

impl flui_foundation::ViewKey for RetirementKey {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn key_eq(&self, other: &dyn flui_foundation::ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id)
    }
    fn key_hash(&self) -> u64 {
        0xBEDE_AAAA
    }
    fn clone_key(&self) -> Box<dyn flui_foundation::ViewKey> {
        let mut clone = self.clone();
        clone.cloned = true;
        clone.captures = Some((
            ArmedKeyCapture {
                armed: Arc::clone(&self.armed),
                fail: self.fail && self.id == 2,
            },
            ArmedKeyCapture {
                armed: Arc::clone(&self.armed),
                fail: self.fail && self.id == 2,
            },
        ));
        Box::new(clone)
    }
    fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RetirementKey({})", self.id)
    }
    fn is_global_key(&self) -> bool {
        true
    }
}

impl Drop for RetirementKey {
    fn drop(&mut self) {
        if !self.cloned || !self.armed.load(Ordering::Relaxed) {
            return;
        }
        let _ = &self.captures;
        self.drops.fetch_add(1, Ordering::Relaxed);
        CLOSE_REENTRY.with(|hook| {
            if let Some(hook) = &*hook.borrow() {
                hook();
            }
        });
        assert!(!self.fail || self.id != 1, "custom key retirement failure");
    }
}

#[derive(Clone)]
struct KeyedCloseView(RetirementKey);
impl StatelessView for KeyedCloseView {
    fn build(&self, _: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
}
impl View for KeyedCloseView {
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.0)
    }
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

fn run_custom_key_child(fail: bool) {
    let mut realm = UiRealm::for_test();
    let frame_reports = close_frame_reports(&realm);
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    let armed = Arc::new(AtomicBool::new(false));
    let first_drops = Arc::new(AtomicUsize::new(0));
    let second_drops = Arc::new(AtomicUsize::new(0));
    let key = |id, drops| RetirementKey {
        id,
        armed: Arc::clone(&armed),
        fail,
        cloned: false,
        drops,
        captures: None,
    };
    let first = KeyedCloseView(key(1, Arc::clone(&first_drops)));
    let second = KeyedCloseView(key(2, Arc::clone(&second_drops)));
    let root = flui_widgets::Column::new(flui_widgets::column![first, second]);
    realm
        .attach_root_widget(&root)
        .expect("custom global keys mount");
    realm
        .attach_root_widget_to_for_test(b, &SizedBox::square(30.0))
        .expect("B mounts");
    let mut clock = flui_foundation::ManualClock::new();
    let mut sink = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Presented);
    assert!(realm.pump(&mut clock, &mut sink).presented());
    assert_close_frames_healthy(&frame_reports, "initial");
    let _a_scene = close_painted_scene(&realm, a, &mut sink);
    let _b_scene = close_painted_scene(&realm, b, &mut sink);
    assert_eq!(
        sink.submit_calls, 1,
        "one pump submits its last produced scene; both owners separately produced Painted"
    );
    assert_eq!(realm.global_key_scope.claim_count(), 2);
    let scope = realm.global_key_scope.clone();
    let closing = realm.presentations.get(a).expect("A");
    let (focus, input) = (closing.focus_manager(), closing.text_input_handle());
    let reentered = Rc::new(Cell::new(0));
    let calls = Rc::clone(&reentered);
    CLOSE_REENTRY.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            assert_eq!(
                scope.claim_count(),
                0,
                "all claims leave the RefCell before arbitrary key Drop"
            );
            assert!(
                focus.is_closed() && input.ensure_open() == Err(TextInputError::Closed),
                "a key's destructor finds focus and text input already closed"
            );
            calls.set(calls.get() + 1);
        }));
    });
    armed.store(true, Ordering::Relaxed);
    let close = catch_unwind(AssertUnwindSafe(|| realm.close_presentation_entered(a)));
    CLOSE_REENTRY.with(|hook| hook.borrow_mut().take());
    if fail {
        let first = close.expect_err("first key retirement failure");
        let message = first
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| first.downcast_ref::<&str>().copied());
        assert!(message.is_some_and(|message| message.contains("custom key retirement failure")));
        assert_eq!(first_drops.load(Ordering::Relaxed), 1);
        assert_eq!(
            second_drops.load(Ordering::Relaxed),
            0,
            "opaque key tail is retained"
        );
        assert_eq!(reentered.get(), 1);
    } else {
        assert!(close.expect("healthy custom key close"));
        assert!(first_drops.load(Ordering::Relaxed) > 0);
        assert!(second_drops.load(Ordering::Relaxed) > 0);
        assert!(reentered.get() >= 2);
    }
    assert_eq!(realm.global_key_scope.claim_count(), 0);
    assert!(!realm.close_presentation_entered(a));
    // Existing state can remain retained, but the same keys must be usable by B.
    armed.store(false, Ordering::Relaxed);
    realm.presentation_widgets_for_test(b).detach_root_widget();
    realm
        .presentation_widgets_for_test(b)
        .attach_root_widget(&root)
        .expect("B reuses both custom keys");
    realm.request_redraw();
    clock.advance(std::time::Duration::from_millis(16));
    assert!(realm.pump(&mut clock, &mut sink).presented());
    assert_eq!(
        sink.submit_calls, 2,
        "initial pump and later B pump each submit one scene"
    );
    assert_close_frames_healthy(&frame_reports, "sibling key reuse");
    assert_eq!(realm.global_key_scope.claim_count(), 2);
}

impl flui_interaction::GestureArenaMember for CloseArenaMember {
    fn accept_gesture(&self, _: flui_interaction::PointerId) {
        self.calls.set(self.calls.get() + 1);
    }
    fn reject_gesture(&self, _: flui_interaction::PointerId) {
        let _ = (&self.captures.first, &self.captures.second);
        self.calls.set(self.calls.get() + 1);
        CLOSE_REENTRY.with(|hook| {
            if let Some(hook) = &*hook.borrow() {
                hook();
            }
        });
    }
}

type SiblingHandles = (
    Rc<FocusManager>,
    TextInputHandle,
    Rc<std::cell::RefCell<Option<flui_view::reactive::Reactive>>>,
);

/// Disposed while its realm drops: the sibling presentation it saved handles
/// to must already refuse them.
#[derive(Clone)]
struct SiblingProbe {
    graph: Rc<std::cell::RefCell<Option<flui_view::reactive::Reactive>>>,
    sibling: Rc<std::cell::RefCell<Option<SiblingHandles>>>,
    disposed: Rc<Cell<usize>>,
}

impl StatefulView for SiblingProbe {
    type State = Self;

    fn create_state(&self) -> Self::State {
        self.clone()
    }
}

impl ViewState<SiblingProbe> for SiblingProbe {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.graph.borrow_mut().replace(ctx.reactive());
    }
    fn build(&self, _view: &SiblingProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::square(10.0)
    }
    fn dispose(&mut self) {
        let sibling = self.sibling.borrow();
        let (focus, input, graph) = sibling.as_ref().expect("sibling handles");
        assert!(focus.is_closed(), "sibling focus is withdrawn");
        assert_eq!(input.ensure_open(), Err(TextInputError::Closed));
        assert_eq!(
            graph
                .borrow()
                .as_ref()
                .expect("sibling graph")
                .try_signal(1_u32)
                .expect_err("sibling graph is withdrawn"),
            flui_view::SignalError::OwnerClosed
        );
        self.disposed.set(self.disposed.get() + 1);
    }
}

impl View for SiblingProbe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

/// Dropping a realm withdraws every presentation before any disposes: each
/// presentation's dispose finds its sibling's saved handles refused.
fn run_realm_sibling_child() {
    let mut realm = UiRealm::for_test();
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    let disposed = Rc::new(Cell::new(0));
    let graphs = [
        Rc::new(std::cell::RefCell::new(None)),
        Rc::new(std::cell::RefCell::new(None)),
    ];
    let siblings = [
        Rc::new(std::cell::RefCell::new(None)),
        Rc::new(std::cell::RefCell::new(None)),
    ];
    let probe = |index: usize| SiblingProbe {
        graph: Rc::clone(&graphs[index]),
        sibling: Rc::clone(&siblings[index]),
        disposed: Rc::clone(&disposed),
    };
    realm.attach_root_widget(&probe(0)).expect("A mounts");
    realm
        .attach_root_widget_to_for_test(b, &probe(1))
        .expect("B mounts");
    let mut clock = flui_foundation::ManualClock::new();
    let mut sink = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Presented);
    assert!(realm.pump(&mut clock, &mut sink).presented());
    for (index, (own, other)) in [(a, b), (b, a)].into_iter().enumerate() {
        let _ = own;
        let presentation = realm.presentations.get(other).expect("sibling");
        siblings[index].borrow_mut().replace((
            presentation.focus_manager(),
            presentation.text_input_handle(),
            Rc::clone(&graphs[1 - index]),
        ));
    }
    assert!(graphs.iter().all(|graph| graph.borrow().is_some()));
    drop(realm);
    assert_eq!(disposed.get(), 2, "both presentations disposed");
}

struct OwnershipWindow {
    delegate: crate::testing::TestWindow,
    external:
        std::sync::Weak<parking_lot::Mutex<Option<Arc<dyn flui_platform_api::PlatformWindow>>>>,
    armed: AtomicBool,
    fail_callback: bool,
    _captures: DropCompetition,
}

impl flui_platform_api::PlatformWindow for OwnershipWindow {
    fn id(&self) -> flui_platform_api::WindowId {
        self.delegate.id()
    }
    fn physical_size(&self) -> flui_foundation::geometry::Size<i32> {
        self.delegate.physical_size()
    }
    fn logical_size(&self) -> flui_foundation::geometry::Size<f64> {
        self.delegate.logical_size()
    }
    fn scale_factor(&self) -> f64 {
        self.delegate.scale_factor()
    }
    fn request_redraw(&self) {
        self.delegate.request_redraw();
    }
    fn is_focused(&self) -> bool {
        true
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.delegate.text_input()
    }
    fn set_cursor(
        &self,
        cursor: flui_platform_api::CursorIcon,
    ) -> Result<(), flui_platform_api::CursorError> {
        if self.armed.load(Ordering::Relaxed) {
            if let Some(external) = self.external.upgrade() {
                external.lock().take();
            }
            assert!(!self.fail_callback, "window cursor failure");
        }
        self.delegate.set_cursor(cursor)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct OwnershipBridge {
    delegate: flui_platform::FakeAccessibility,
    external: std::sync::Weak<
        parking_lot::Mutex<Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>>>,
    >,
    armed: AtomicBool,
    fail_callback: bool,
    _captures: DropCompetition,
}

impl flui_semantics::platform::PlatformAccessibility for OwnershipBridge {
    fn publish(&self, update: flui_semantics::TreeUpdate) {
        self.delegate.publish(update);
    }
    fn is_active(&self) -> bool {
        self.delegate.is_active()
    }
    fn set_activation_listener(
        &self,
        listener: flui_semantics::platform::AccessibilityActivationListener,
    ) {
        self.delegate.set_activation_listener(listener);
        if self.armed.load(Ordering::Relaxed) {
            if let Some(external) = self.external.upgrade() {
                external.lock().take();
            }
            assert!(!self.fail_callback, "bridge withdrawal failure");
        }
    }
    fn set_action_listener(&self, listener: flui_semantics::platform::AccessibilityActionListener) {
        self.delegate.set_action_listener(listener);
    }
}

fn run_platform_ownership_child(kind: &str) {
    let healthy = kind == "healthy-platform";
    let fail_window = kind == "window-owner";
    let fail_bridge = kind == "bridge-owner";
    let cursor_first = kind == "cursor-platform";
    let window_drops = Arc::new(AtomicUsize::new(0));
    let bridge_drops = Arc::new(AtomicUsize::new(0));
    let external_window = Arc::new(parking_lot::Mutex::new(None));
    let external_bridge = Arc::new(parking_lot::Mutex::new(None));
    // The window and the bridge are framework-owned (ADR-0127): close
    // releases them even after a failure, so their captures must drop.
    let bundle = |counts: &Arc<AtomicUsize>| DropCompetition {
        first: CursorCapture {
            fail: false,
            drops: Arc::clone(counts),
        },
        second: CursorCapture {
            fail: false,
            drops: Arc::clone(counts),
        },
    };
    let window = Arc::new(OwnershipWindow {
        delegate: crate::testing::TestWindow::new().focused(true),
        external: Arc::downgrade(&external_window),
        armed: AtomicBool::new(false),
        fail_callback: fail_window || cursor_first,
        _captures: bundle(&window_drops),
    });
    let bridge = Arc::new(OwnershipBridge {
        delegate: flui_platform::FakeAccessibility::new(),
        external: Arc::downgrade(&external_bridge),
        armed: AtomicBool::new(false),
        fail_callback: fail_bridge || cursor_first,
        _captures: bundle(&bridge_drops),
    });
    *external_window.lock() = Some(window.clone() as Arc<dyn flui_platform_api::PlatformWindow>);
    *external_bridge.lock() =
        Some(bridge.clone() as Arc<dyn flui_semantics::platform::PlatformAccessibility>);
    let mut realm = UiRealm::new(
        crate::presentation::PresentationWindow::new(window.clone(), Some(bridge.clone())),
        1.0,
        crate::realm_services::RealmHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("owned platform realm");
    let a = realm.presentation_id();
    let _b = realm.install_second_presentation_for_test();
    let capture = CursorCapture {
        fail: cursor_first,
        drops: Arc::new(AtomicUsize::new(0)),
    };
    realm
        .gestures()
        .mouse_tracker()
        .set_cursor_change_callback(Rc::new(move |_, _| {
            let _ = &capture;
        }));
    window.armed.store(true, Ordering::Relaxed);
    bridge.armed.store(true, Ordering::Relaxed);
    drop(window);
    drop(bridge);
    let result = catch_unwind(AssertUnwindSafe(|| realm.close_presentation_entered(a)));
    if healthy {
        assert!(result.expect("ordinary platform close"));
    } else {
        let payload = result.expect_err("first platform failure survives");
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .expect("string failure");
        let expected = if cursor_first {
            "cursor capture retirement failure"
        } else if fail_bridge {
            "bridge withdrawal failure"
        } else {
            "window cursor failure"
        };
        assert!(message.contains(expected), "first failure: {message}");
    }
    assert_eq!(
        window_drops.load(Ordering::Relaxed),
        2,
        "close releases the platform window"
    );
    assert_eq!(
        bridge_drops.load(Ordering::Relaxed),
        2,
        "close releases the accessibility bridge"
    );
    assert!(external_window.lock().is_none());
    assert!(external_bridge.lock().is_none());
    assert!(!realm.close_presentation_entered(a));
}

impl Drop for CursorCapture {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
        assert!(!self.fail, "cursor capture retirement failure");
    }
}

/// A dispatch capture that records, when destroyed, what its saved handles
/// observed: a healthy close withdraws every capability first.
struct WithdrawalProbe {
    focus: Rc<FocusManager>,
    input: TextInputHandle,
    seen: Rc<Cell<Option<(bool, bool)>>>,
}

impl Drop for WithdrawalProbe {
    fn drop(&mut self) {
        self.seen.set(Some((
            self.focus.is_closed(),
            self.input.ensure_open() == Err(TextInputError::Closed),
        )));
    }
}

fn run_scoped_routes_child(fail_cursor: bool) {
    use flui_interaction::{HitTestEntry, HitTestResult, InteractionDispatchError};
    let mut realm = UiRealm::for_test();
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    realm
        .attach_root_widget(&SizedBox::square(10.0))
        .expect("A root");
    realm
        .attach_root_widget_to_for_test(b, &SizedBox::square(30.0))
        .expect("B root");
    let a_handle = realm
        .presentations
        .get(a)
        .expect("A")
        .interaction_dispatch()
        .expect("owner scope")
        .clone();
    let b_handle = realm
        .presentations
        .get(b)
        .expect("B")
        .interaction_dispatch()
        .expect("owner scope")
        .clone();
    let calls_a = Rc::new(Cell::new(0));
    let calls_b = Rc::new(Cell::new(0));
    let capture_drops = Arc::new(AtomicUsize::new(0));
    let rejected_drops = Arc::new(AtomicUsize::new(0));
    let bundle = DropCompetition {
        first: CursorCapture {
            fail: fail_cursor,
            drops: Arc::clone(&capture_drops),
        },
        second: CursorCapture {
            fail: fail_cursor,
            drops: Arc::clone(&capture_drops),
        },
    };
    let event = flui_interaction::events::make_down_event(
        flui_foundation::geometry::Offset::new(2.0, 2.0),
        flui_interaction::events::PointerKind::Touch,
    )
    .expect("finite test position");
    let withdrawal_seen = Rc::new(Cell::new(None));
    let probe = WithdrawalProbe {
        focus: realm.focus_manager(),
        input: realm.text_input_handle(),
        seen: Rc::clone(&withdrawal_seen),
    };
    let (a_target, b_target, saved_route, dispatch) = realm.enter(|realm| {
        // No route saves this target, so close is its last owner.
        a_handle
            .register_pointer(move |_| {
                let _ = &probe;
            })
            .expect("A probe target");
        let counts = Rc::clone(&calls_a);
        let target_a = a_handle
            .register_pointer(move |_| {
                let _ = (&bundle.first, &bundle.second);
                counts.set(counts.get() + 1);
            })
            .expect("A target");
        let counts = Rc::clone(&calls_b);
        let target_b = b_handle
            .register_pointer(move |_| counts.set(counts.get() + 1))
            .expect("B target");
        let mut result = HitTestResult::new();
        result.add(HitTestEntry::new(flui_foundation::RenderId::new(1)).pointer_target(target_a));
        result.add(HitTestEntry::new(flui_foundation::RenderId::new(2)).pointer_target(target_b));
        let dispatch = realm.interaction_lane.dispatch_handle();
        let saved = dispatch
            .resolve_pointer_route(result.path())
            .expect("mixed saved route")
            .token();
        // This second route is owned by A's real binding cache. The externally
        // saved mixed route also holds A's cell, so close must guard both owners.
        realm
            .gestures()
            .handle_pointer_event_with_result(&event, &result);
        (target_a, target_b, saved, dispatch)
    });
    assert_eq!((calls_a.get(), calls_b.get()), (1, 1));
    let cursor_drops = Arc::new(AtomicUsize::new(0));
    let cursor = CursorCapture {
        fail: fail_cursor,
        drops: Arc::clone(&cursor_drops),
    };
    realm
        .gestures()
        .mouse_tracker()
        .set_cursor_change_callback(Rc::new(move |_, _| {
            let _ = &cursor;
        }));
    let close = catch_unwind(AssertUnwindSafe(|| realm.close_presentation_entered(a)));
    if fail_cursor {
        let first = close.expect_err("cursor failure");
        let message = first
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| first.downcast_ref::<&str>().copied());
        assert!(
            message.is_some_and(|message| message.contains("cursor capture retirement failure"))
        );
        assert_eq!(
            withdrawal_seen.get(),
            None,
            "a capture withdrawn after the failure is retained"
        );
    } else {
        assert!(close.expect("healthy close"));
        assert_eq!(
            withdrawal_seen.get(),
            Some((true, true)),
            "a dispatch capture is destroyed only after focus and text input are closed"
        );
    }
    realm.enter(|_| {
        assert!(
            dispatch
                .invoke_pointer_route(saved_route, &event)
                .expect("mixed route still resolves")
                .is_none()
        );
        assert_eq!(
            (calls_a.get(), calls_b.get()),
            (1, 2),
            "retained A cannot replay while B remains live"
        );
        let stale = [HitTestEntry::new(flui_foundation::RenderId::new(1)).pointer_target(a_target)];
        let resolution = dispatch
            .resolve_pointer_route(&stale)
            .expect("closed target is a miss");
        assert_eq!(resolution.misses().len(), 1);
        dispatch
            .release_route(resolution.token())
            .expect("empty route release");
        if !fail_cursor {
            struct RejectDuringUnwind {
                handle: flui_interaction::InteractionDispatchHandle,
                drops: Arc<AtomicUsize>,
            }
            impl Drop for RejectDuringUnwind {
                fn drop(&mut self) {
                    let bundle = DropCompetition {
                        first: CursorCapture {
                            fail: true,
                            drops: Arc::clone(&self.drops),
                        },
                        second: CursorCapture {
                            fail: true,
                            drops: Arc::clone(&self.drops),
                        },
                    };
                    assert_eq!(
                        self.handle.register_pointer(move |_| {
                            let _ = (&bundle.first, &bundle.second);
                        }),
                        Err(InteractionDispatchError::OwnerGone)
                    );
                }
            }
            let unwind_drops = Arc::new(AtomicUsize::new(0));
            let rejected = catch_unwind(AssertUnwindSafe(|| {
                let _guard = RejectDuringUnwind {
                    handle: a_handle.clone(),
                    drops: Arc::clone(&unwind_drops),
                };
                panic!("unrelated outer rejection unwind");
            }));
            assert!(rejected.is_err());
            assert_eq!(unwind_drops.load(Ordering::Relaxed), 0);
        }
        // The close and its failure are over: a healthy rejection through the
        // stale handle destroys its captures instead of leaking them.
        let bundle = DropCompetition {
            first: CursorCapture {
                fail: false,
                drops: Arc::clone(&rejected_drops),
            },
            second: CursorCapture {
                fail: false,
                drops: Arc::clone(&rejected_drops),
            },
        };
        assert_eq!(
            a_handle.register_pointer(move |_| {
                let _ = (&bundle.first, &bundle.second);
            }),
            Err(InteractionDispatchError::OwnerGone)
        );
        let counts = Rc::clone(&calls_b);
        let fresh = b_handle
            .register_pointer(move |_| counts.set(counts.get() + 1))
            .expect("B admits fresh target");
        assert_ne!(fresh, a_target, "withdrawn identity is never reused");
        let fresh_path =
            [HitTestEntry::new(flui_foundation::RenderId::new(3)).pointer_target(fresh)];
        let token = dispatch
            .resolve_pointer_route(&fresh_path)
            .expect("B fresh route")
            .token();
        assert!(
            dispatch
                .invoke_pointer_route(token, &event)
                .expect("B dispatch")
                .is_none()
        );
        assert_eq!(calls_b.get(), 3);
        dispatch.release_route(token).expect("B route release");
        b_handle
            .unregister_pointer(fresh)
            .expect("B fresh target release");
        dispatch
            .release_route(saved_route)
            .expect("saved route releases safely after containment");
        assert!(matches!(
            dispatch.invoke_pointer_route(saved_route, &event),
            Err(InteractionDispatchError::StaleRoute)
        ));
        b_handle
            .unregister_pointer(b_target)
            .expect("B original target release");
    });
    assert_eq!(
        capture_drops.load(Ordering::Relaxed),
        if fail_cursor { 0 } else { 2 }
    );
    assert_eq!(
        rejected_drops.load(Ordering::Relaxed),
        2,
        "a rejection after the close retires normally"
    );
    assert!(!realm.close_presentation_entered(a));
    let mut clock = flui_foundation::ManualClock::new();
    let mut sink = ScriptedSink::new(|_, _| crate::sink::SubmitVerdict::Presented);
    realm.request_redraw();
    assert!(realm.pump(&mut clock, &mut sink).presented());
    assert_eq!(
        sink.submit_calls, 1,
        "B paints after route withdrawal and fresh admission"
    );
}

pub(crate) fn run_presentation_close_child(kind: &str) {
    if kind == "realm-sibling" {
        run_realm_sibling_child();
        return;
    }
    if matches!(kind, "healthy-keys" | "competing-keys") {
        run_custom_key_child(kind == "competing-keys");
        return;
    }
    if matches!(kind, "healthy-routes" | "cursor-routes") {
        run_scoped_routes_child(kind == "cursor-routes");
        return;
    }
    if matches!(
        kind,
        "healthy-platform" | "window-owner" | "bridge-owner" | "cursor-platform"
    ) {
        run_platform_ownership_child(kind);
        return;
    }
    let cursor_failure = matches!(
        kind,
        "cursor" | "cursor-focus" | "cursor-ime" | "cursor-opaque" | "cursor-reentry"
    );
    let between_rounds = kind == "lifecycle-between-rounds";
    let outer_close = kind == "outer-close";
    let prior_failure = matches!(
        kind,
        "prior-opaque" | "realm-prior" | "lifecycle-between-rounds"
    );
    let opaque_failure = matches!(
        kind,
        "cursor-opaque"
            | "prior-opaque"
            | "realm-prior"
            | "outer-unwind"
            | "lifecycle-between-rounds"
            | "outer-close"
    );
    let realm_drop = kind == "realm-prior";
    let outer_unwind = kind == "outer-unwind";
    let reentry = matches!(kind, "cursor-reentry" | "healthy-reentry");
    let gesture_reentry = kind == "gesture-reentry";
    let focus_failure = matches!(kind, "focus" | "cursor-focus");
    let ime_failure = matches!(kind, "ime" | "cursor-ime");
    assert!(matches!(
        kind,
        "healthy"
            | "gesture-reentry"
            | "cursor"
            | "focus"
            | "ime"
            | "cursor-focus"
            | "cursor-ime"
            | "cursor-opaque"
            | "prior-opaque"
            | "realm-prior"
            | "cursor-reentry"
            | "healthy-reentry"
            | "outer-unwind"
            | "lifecycle-between-rounds"
            | "outer-close"
    ));

    let platform = Arc::new(CloseTextInput {
        recorder: flui_platform::FakeTextInput::new(),
        fail_disable: ime_failure,
        reenter_disable: reentry,
    });
    let capability: Arc<dyn PlatformTextInput> = platform.clone();
    let mut realm = UiRealm::for_test_with_text_input(Some(capability));
    let frame_reports = close_frame_reports(&realm);
    let a = realm.presentation_id();
    let b = realm.install_second_presentation_for_test();
    let focus = realm.focus_manager();
    let focused = FocusNode::new();
    let _attachment = focus
        .root_scope()
        .attach_node(&focused)
        .expect("focus node attaches");
    let _ = focused.request_focus();
    assert!(focused.has_primary_focus());

    let input = realm.text_input_handle();
    let store: Rc<dyn TextStore> = flui_platform_api::text_store::InMemoryTextStore::new("editing");
    let opaque_drops = Arc::new(AtomicUsize::new(0));
    let bundle = DropCompetition {
        first: CursorCapture {
            fail: opaque_failure,
            drops: Arc::clone(&opaque_drops),
        },
        second: CursorCapture {
            fail: opaque_failure,
            drops: Arc::clone(&opaque_drops),
        },
    };
    let _token = input
        .attach(
            flui_interaction::TextInputClient::new(store).on_session_start(move || {
                let _ = (&bundle.first, &bundle.second);
            }),
        )
        .expect("IME client attaches");
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let capabilities = Rc::new(std::cell::RefCell::new(None));
    let observer = CloseObserver {
        key: flui_view::GlobalKey::new(),
        focus: Rc::clone(&focus),
        input: input.clone(),
        platform: Arc::clone(&platform),
        seen: Rc::clone(&seen),
        capabilities: Rc::clone(&capabilities),
    };
    realm
        .attach_root_widget(&observer)
        .expect("A mounts disposal observer");
    realm
        .attach_root_widget_to_for_test(b, &SizedBox::square(30.0))
        .expect("B mounts");
    let agent = realm
        .dev_agent_window(a)
        .expect("agent window before close");
    let arena_calls = Rc::new(Cell::new(0));
    let arena_drops = Arc::new(AtomicUsize::new(0));
    let route_drops = Arc::new(AtomicUsize::new(0));
    let saved_arena = if outer_unwind {
        let arena = realm.gestures().arena().clone();
        let captures = DropCompetition {
            first: CursorCapture {
                fail: true,
                drops: Arc::clone(&arena_drops),
            },
            second: CursorCapture {
                fail: true,
                drops: Arc::clone(&arena_drops),
            },
        };
        let member = Rc::new(CloseArenaMember {
            calls: Rc::clone(&arena_calls),
            captures,
        });
        let entry = arena.add(
            flui_interaction::PointerId::new(std::num::NonZeroU64::MIN),
            &member,
        );
        let captures = DropCompetition {
            first: CursorCapture {
                fail: true,
                drops: Arc::clone(&route_drops),
            },
            second: CursorCapture {
                fail: true,
                drops: Arc::clone(&route_drops),
            },
        };
        realm
            .gestures()
            .pointer_router()
            .add_global_handler(Rc::new(move |_| {
                // The presentation's registered handler owns its recognizer;
                // arena admission itself intentionally retains only Weak.
                let _ = &member;
                let _ = (&captures.first, &captures.second);
            }));
        Some((arena, entry))
    } else {
        None
    };
    let scenes = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let submitted = Arc::clone(&scenes);
    let mut sink = ScriptedSink::new(move |_, scene| {
        submitted.lock().push(format!("{:?}", scene.tree()));
        crate::sink::SubmitVerdict::Presented
    });
    let mut clock = flui_foundation::ManualClock::new();
    assert!(realm.pump(&mut clock, &mut sink).presented());
    assert_close_frames_healthy(&frame_reports, "initial");
    let b_scene = scenes
        .lock()
        .last()
        .cloned()
        .expect("B presents before close");
    let _a_scene = close_painted_scene(&realm, a, &mut sink);
    let actual_b_scene = close_painted_scene(&realm, b, &mut sink);
    assert_eq!(
        actual_b_scene, b_scene,
        "B's actual producer scene matches the submitted scene"
    );
    assert_eq!(
        sink.submit_calls, 1,
        "one pump submits its last produced scene; both owners separately produced Painted"
    );
    assert!(
        seen.borrow().is_empty(),
        "root is mounted and not yet disposed"
    );

    let focus_calls = Rc::new(Cell::new(0));
    let calls = Rc::clone(&focus_calls);
    let weak_focus = Rc::downgrade(&focus);
    focus.add_listener(Rc::new(move |_, new| {
        if new.is_none() {
            calls.set(calls.get() + 1);
            // Real callback reentry must see close already committed.
            weak_focus.upgrade().expect("held manager").close();
            assert!(!focus_failure, "focus loss failure");
        }
    }));
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = CursorCapture {
        fail: cursor_failure,
        drops: Arc::clone(&drops),
    };
    realm
        .gestures()
        .mouse_tracker()
        .set_cursor_change_callback(Rc::new(move |_, _| {
            let _keep_capture = &capture;
        }));

    let rejected_drops = Arc::new(AtomicUsize::new(0));
    if reentry {
        let target_focus = Rc::clone(&focus);
        let target_input = input.clone();
        let counts = Arc::clone(&rejected_drops);
        let closed_key = observer.key.clone();
        CLOSE_REENTRY.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                assert!(target_focus.is_closed());
                assert_eq!(target_input.ensure_open(), Err(TextInputError::Closed));
                assert!(
                    closed_key.with_current_state(|_| ()).is_none(),
                    "mandatory reentry cannot access retained A state"
                );
                let bundle = DropCompetition {
                    first: CursorCapture {
                        fail: cursor_failure,
                        drops: Arc::clone(&counts),
                    },
                    second: CursorCapture {
                        fail: cursor_failure,
                        drops: Arc::clone(&counts),
                    },
                };
                let store: Rc<dyn TextStore> =
                    flui_platform_api::text_store::InMemoryTextStore::new("rejected");
                let client =
                    flui_interaction::TextInputClient::new(store).on_session_start(move || {
                        let _ = (&bundle.first, &bundle.second);
                    });
                assert_eq!(target_input.attach(client), Err(TextInputError::Closed));
                let bundle = DropCompetition {
                    first: CursorCapture {
                        fail: cursor_failure,
                        drops: Arc::clone(&counts),
                    },
                    second: CursorCapture {
                        fail: cursor_failure,
                        drops: Arc::clone(&counts),
                    },
                };
                target_focus.add_global_key_handler(Rc::new(move |_| {
                    let _ = (&bundle.first, &bundle.second);
                    flui_interaction::KeyEventResult::Ignored
                }));
            }));
        });
    }

    let rejected_member = if gesture_reentry {
        // A recognizer rejected by the close reenters through saved handles:
        // every presentation-wide capability is already withdrawn.
        let target_focus = Rc::clone(&focus);
        let target_input = input.clone();
        let closed_key = observer.key.clone();
        let captured = Rc::clone(&capabilities);
        CLOSE_REENTRY.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                assert!(target_focus.is_closed(), "focus closes before rejection");
                assert_eq!(target_input.ensure_open(), Err(TextInputError::Closed));
                assert!(closed_key.with_current_state(|_| ()).is_none());
                let captured = captured.borrow();
                let captured = captured.as_ref().expect("mounted capabilities");
                assert_eq!(
                    captured.graph.try_signal(1_u32).expect_err("closed graph"),
                    flui_view::SignalError::OwnerClosed
                );
                assert!(!captured.rebuild.is_active());
            }));
        });
        let captures = DropCompetition {
            first: CursorCapture {
                fail: false,
                drops: Arc::clone(&arena_drops),
            },
            second: CursorCapture {
                fail: false,
                drops: Arc::clone(&arena_drops),
            },
        };
        let member = Rc::new(CloseArenaMember {
            calls: Rc::clone(&arena_calls),
            captures,
        });
        let entry = realm.gestures().arena().add(
            flui_interaction::PointerId::new(std::num::NonZeroU64::MIN),
            &member,
        );
        realm
            .gestures()
            .pointer_router()
            .add_global_handler(Rc::new(move |_| {
                let _ = &member;
            }));
        Some(entry)
    } else {
        None
    };
    if between_rounds {
        // A pump renders content but does not adopt the initial window
        // lifecycle snapshot. Initialize it through the same host API as a
        // runner, so closing really walks Resumed -> Inactive -> ... Detached.
        realm.enter(UiRealm::synchronize_window_lifecycle);
        assert_eq!(
            capabilities
                .borrow()
                .as_ref()
                .expect("mounted capabilities")
                .lifecycle
                .snapshot(),
            Ok(Some(flui_scheduler::AppLifecycleState::Resumed)),
            "fixture must have a real ladder before injecting its first failure"
        );
    }
    let terminal_calls = Rc::new(Cell::new(0));
    let _prior_listener = if prior_failure {
        let captured = capabilities.borrow();
        let source = &captured.as_ref().expect("mounted capabilities").lifecycle;
        let calls = Rc::clone(&terminal_calls);
        Some(
            source
                .subscribe(move |state| {
                    calls.set(calls.get() + 1);
                    assert_ne!(
                        state,
                        if between_rounds {
                            flui_scheduler::AppLifecycleState::Inactive
                        } else {
                            flui_scheduler::AppLifecycleState::Detached
                        },
                        "prior lifecycle failure"
                    );
                })
                .expect("open lifecycle")
                .1,
        )
    } else {
        None
    };
    if realm_drop || outer_unwind {
        struct CloseDuringUnwind(Option<UiRealm>);
        impl Drop for CloseDuringUnwind {
            fn drop(&mut self) {
                drop(self.0.take());
            }
        }
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if outer_unwind {
                let _close = CloseDuringUnwind(Some(realm));
                panic!("outer unwind remains authoritative");
            }
            drop(realm);
        }));
        let payload = outcome.expect_err("whole realm keeps the lifecycle failure");
        let actual = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        assert!(
            actual.is_some_and(|message| message.contains(if outer_unwind {
                "outer unwind remains authoritative"
            } else {
                "prior lifecycle failure"
            }))
        );
        assert!(focus.is_closed());
        assert_eq!(input.ensure_open(), Err(TextInputError::Closed));
        assert!(
            seen.borrow().is_empty(),
            "recovering forest skips optional disposal"
        );
        assert_eq!(opaque_drops.load(Ordering::Relaxed), 0);
        if let Some((arena, entry)) = saved_arena {
            assert!(arena.is_empty());
            assert!(entry.member().is_none());
            entry.resolve(flui_interaction::GestureDisposition::Accepted);
            assert_eq!(
                arena_calls.get(),
                0,
                "closed arena cannot notify saved entries"
            );
            // The close and the outer unwind are over: a healthy rejection
            // destroys its captures instead of leaking them.
            let captures = DropCompetition {
                first: CursorCapture {
                    fail: false,
                    drops: Arc::clone(&arena_drops),
                },
                second: CursorCapture {
                    fail: false,
                    drops: Arc::clone(&arena_drops),
                },
            };
            let rejected = arena.add(
                flui_interaction::PointerId::new(std::num::NonZeroU64::MIN),
                &Rc::new(CloseArenaMember {
                    calls: Rc::clone(&arena_calls),
                    captures,
                }),
            );
            assert!(rejected.member().is_none());
            assert!(arena.is_empty());
            assert_eq!(
                arena_drops.load(Ordering::Relaxed),
                2,
                "only the rejected member is destroyed; the retained one is not"
            );
            assert_eq!(route_drops.load(Ordering::Relaxed), 0);
        }
        assert!(!agent.is_open());
        return;
    }
    let outcome = if outer_close {
        struct CloseEnteredDuringUnwind<'a>(&'a mut UiRealm, PresentationId);
        impl Drop for CloseEnteredDuringUnwind<'_> {
            fn drop(&mut self) {
                self.0.close_presentation_entered(self.1);
            }
        }
        catch_unwind(AssertUnwindSafe(|| {
            let _close = CloseEnteredDuringUnwind(&mut realm, a);
            panic!("outer unwind remains authoritative");
        }))
    } else {
        catch_unwind(AssertUnwindSafe(|| realm.close_presentation_entered(a)))
    };
    if let Some(entry) = rejected_member {
        CLOSE_REENTRY.with(|hook| hook.borrow_mut().take());
        assert!(entry.member().is_none());
        assert_eq!(arena_calls.get(), 1, "the close rejected the recognizer");
    }
    if reentry {
        CLOSE_REENTRY.with(|hook| hook.borrow_mut().take());
        assert_eq!(
            rejected_drops.load(Ordering::Relaxed),
            if cursor_failure { 0 } else { 4 }
        );
    }
    let expected = if outer_close {
        Some("outer unwind remains authoritative")
    } else if prior_failure {
        Some("prior lifecycle failure")
    } else if cursor_failure {
        Some("cursor capture retirement failure")
    } else if focus_failure {
        Some("focus loss failure")
    } else if ime_failure {
        Some("IME disable failure")
    } else {
        None
    };
    match expected {
        Some(message) => {
            let payload = outcome.expect_err("the original close failure propagates");
            // assert! formats its failure as a String on the host toolchain.
            let actual = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied());
            assert!(
                actual.is_some_and(|actual| actual.contains(message)),
                "first failure must remain authoritative: {actual:?}"
            );
        }
        None => assert!(outcome.expect("healthy close succeeds")),
    }
    let healthy = expected.is_none();
    if between_rounds {
        assert_eq!(
            terminal_calls.get(),
            1,
            "host caught first before later required ladder commits; later optional notifications stop"
        );
    }
    if healthy {
        assert_eq!(
            *seen.borrow(),
            [CloseObservation {
                focus_closed: true,
                primary_focus_cleared: true,
                text_input: Err(TextInputError::Closed),
                ime_allowed: Some(false),
            }],
            "input closes before healthy widget disposal"
        );
    } else {
        assert!(seen.borrow().is_empty(), "recovery skips optional disposal");
    }
    assert_eq!(
        input.ensure_open(),
        Err(if healthy {
            TextInputError::OwnerGone
        } else {
            TextInputError::Closed
        }),
        "healthy removal destroys the physical owner; exceptional removal retains the closed owner"
    );
    assert!(focus.primary_focus().is_none());
    assert!(
        !agent.is_open(),
        "retained envelope cannot admit agent work"
    );
    assert_eq!(
        opaque_drops.load(Ordering::Relaxed),
        if healthy { 2 } else { 0 }
    );
    {
        let captured = capabilities.borrow();
        let captured = captured.as_ref().expect("mounted capabilities");
        assert_eq!(
            captured.graph.try_signal(12_u32).expect_err("closed graph"),
            flui_view::SignalError::OwnerClosed
        );
        captured.writer.write(|cx| {
            assert_eq!(
                captured.signal.peek(&captured.graph, |value| *value),
                Err(flui_view::SignalError::OwnerClosed)
            );
            assert_eq!(
                captured.writer.check_context(cx),
                Err(flui_view::EventContextError::Detached)
            );
        });
        // A call on the closed graph is a new operation: the value or
        // closure it rejects is dropped, even after an exceptional close
        // (ADR-0127).
        let probe = Rc::new(());
        assert_eq!(
            captured
                .graph
                .try_signal(Rc::clone(&probe))
                .expect_err("closed graph"),
            flui_view::SignalError::OwnerClosed
        );
        let held = Rc::clone(&probe);
        assert_eq!(
            flui_view::SignalWriteExt::update(captured.signal, &captured.graph, move |value| {
                let _ = &held;
                *value
            }),
            Err(flui_view::SignalError::OwnerClosed)
        );
        assert_eq!(
            Rc::strong_count(&probe),
            1,
            "rejected signal value and updater are dropped"
        );
        assert!(!captured.rebuild.is_active());
        captured
            .rebuild
            .schedule(flui_foundation::RebuildReason::AsyncCompletion);
        assert!(captured.lifecycle.snapshot().is_err());
    }
    assert!(
        focus.is_closed(),
        "external focus clone is explicitly closed"
    );
    assert_eq!(
        focus_calls.get(),
        usize::from(!cursor_failure && !prior_failure && !outer_close),
        "optional focus-loss delivery stops after an earlier failure"
    );
    assert_eq!(
        drops.load(Ordering::Relaxed),
        usize::from(!prior_failure && !outer_close),
        "cursor retirement stops after an earlier lifecycle failure"
    );
    assert_eq!(platform.recorder.ime_allowed_calls(), [true, false]);
    assert!(
        !realm.close_presentation_entered(a),
        "repeated close is inert"
    );
    assert_eq!(seen.borrow().len(), usize::from(healthy));
    assert_eq!(
        focus_calls.get(),
        usize::from(!cursor_failure && !prior_failure && !outer_close)
    );
    assert_eq!(platform.recorder.ime_allowed_calls(), [true, false]);

    // A real later pump reproduces B's unchanged content after A failed.
    realm.mark_primary_needs_full_repaint();
    realm.request_redraw();
    clock.advance(std::time::Duration::from_millis(16));
    assert!(realm.pump(&mut clock, &mut sink).presented());
    assert_eq!(
        sink.submit_calls, 2,
        "surviving B presents on the next frame"
    );
    assert_eq!(scenes.lock().last(), Some(&b_scene));
    assert_close_frames_healthy(&frame_reports, "sibling after close");
    // Mount through the same public widget producer with A's identical key.
    // Its failed BuildOwner remains retained; explicit withdrawal must make
    // the key available to the surviving owner.
    realm.presentation_widgets_for_test(b).detach_root_widget();
    realm
        .presentation_widgets_for_test(b)
        .attach_root_widget(&observer)
        .expect("surviving presentation mounts the reclaimed GlobalKey");
    realm.request_redraw();
    clock.advance(std::time::Duration::from_millis(16));
    assert!(realm.pump(&mut clock, &mut sink).presented());
    assert_eq!(sink.submit_calls, 3, "sibling frame after reusing the key");
    assert_close_frames_healthy(&frame_reports, "sibling key reuse");
}

/// Child isolation makes accidental double-panic or reentry deadlock a bounded
/// failure rather than terminating or hanging the full isolation matrix.
pub(crate) fn presentation_close_retirement_failures_preserve_focus_ime_and_siblings() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut failures = Vec::new();
    for kind in [
        "healthy",
        "cursor",
        "focus",
        "ime",
        "cursor-focus",
        "cursor-ime",
        "cursor-opaque",
        "prior-opaque",
        "realm-prior",
        "cursor-reentry",
        "healthy-reentry",
        "gesture-reentry",
        "realm-sibling",
        "healthy-platform",
        "window-owner",
        "bridge-owner",
        "cursor-platform",
        "outer-unwind",
        "healthy-routes",
        "cursor-routes",
        "healthy-keys",
        "competing-keys",
        "lifecycle-between-rounds",
        "outer-close",
    ] {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "ui_realm::tests::realm_and_presentation_isolation_matrix",
                "--nocapture",
            ])
            .env("FLUI_PRESENTATION_CLOSE_CHILD", kind)
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LIB_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn presentation close child");
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut stderr = child.stderr.take().expect("child stderr");
        let stdout_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).expect("read child stdout");
            bytes
        });
        let stderr_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).expect("read child stderr");
            bytes
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut timed_out = false;
        while child.try_wait().expect("child status").is_none() {
            if Instant::now() >= deadline {
                child.kill().expect("kill blocked child");
                timed_out = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("reap child");
        let stdout = stdout_reader.join().expect("stdout reader");
        let stderr = stderr_reader.join().expect("stderr reader");
        if timed_out || !status.success() || !String::from_utf8_lossy(&stdout).contains("1 passed")
        {
            failures.push(format!("presentation close {kind}: timeout={timed_out}, status={status}, stdout={}, stderr={}",
                String::from_utf8_lossy(&stdout), String::from_utf8_lossy(&stderr)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Oracle: a LAYER-TREE COMPARE (`format!("{:?}", ...)` on the
/// produced `LayerTree`), never a rebuild/flush count — B's own
/// render output, not merely whether B ran, must be byte-for-byte
/// unaffected by A closing.
pub(crate) fn closing_presentation_a_leaves_sibling_layer_tree_identical() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();

    // Mount independent content on both presentations.
    realm
        .enter(|realm| realm.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0)))
        .expect("A mounts");
    realm
        .enter(|realm| {
            realm
                .presentations
                .get(b_id)
                .expect("B installed")
                .widgets()
                .attach_root_widget(&flui_widgets::SizedBox::new(30.0, 30.0))
        })
        .expect("B mounts");

    let constraints = BoxConstraints::tight(flui_foundation::geometry::Size::new(50.0, 50.0));
    let b_layer_tree_before = realm.enter(|realm| {
        let b = realm.presentations.get(b_id).expect("B installed");
        match UiRealm::draw_frame_for_presentation(b, constraints, &realm.text) {
            Ok(FramePaintOutcome::Painted(scene)) => format!("{:?}", scene.tree()),
            Ok(FramePaintOutcome::Idle) => panic!("B's first frame must paint, got Idle"),
            Ok(FramePaintOutcome::Errored) => {
                panic!("draw_frame_for_presentation never returns Ok(Errored)")
            }
            Err(error) => panic!("B's first frame failed: {error}"),
        }
    });

    assert!(
        realm.close_presentation_entered(a_id),
        "A must have been installed and removable"
    );

    let b_layer_tree_after = realm.enter(|realm| {
        let b = realm.presentations.get(b_id).expect("B still installed");
        // The first draw already settled B (nothing left dirty), so
        // an unconditional second draw would correctly report Idle
        // regardless of A -- that would prove nothing about B's
        // CONTENT. Force B's root render node dirty directly so this
        // second draw genuinely re-produces its layer tree from
        // scratch, the same content as the first draw, to compare
        // against it (a rebuild alone is not enough: reconciling an
        // unchanged `SizedBox` config is legitimately a no-op that
        // marks nothing dirty).
        b.pipeline().with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });
        match UiRealm::draw_frame_for_presentation(b, constraints, &realm.text) {
            Ok(FramePaintOutcome::Painted(scene)) => format!("{:?}", scene.tree()),
            Ok(FramePaintOutcome::Idle) => {
                panic!("B's post-A-close frame must still paint, got Idle")
            }
            Ok(FramePaintOutcome::Errored) => {
                panic!("draw_frame_for_presentation never returns Ok(Errored)")
            }
            Err(error) => panic!("B's post-A-close frame failed: {error}"),
        }
    });

    assert_eq!(
        b_layer_tree_before, b_layer_tree_after,
        "B's own layer tree must be byte-for-byte identical before and \
         after A closes -- nothing about B's content changed, so \
         nothing about its render output may either"
    );
}
