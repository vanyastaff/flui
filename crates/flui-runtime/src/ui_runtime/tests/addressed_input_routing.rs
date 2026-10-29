use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_foundation::geometry::Offset;
use flui_interaction::events::pointer::PointerButton;
use flui_interaction::events::{
    Code, Key, KeyState, NamedKey, PointerKind, make_down_event, make_down_event_with_button,
    make_up_event, make_up_event_with_button,
};
use flui_interaction::focus::{FocusNode, KeyEventResult};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::{Signal, SignalWriteExt};
use flui_widgets::{Focus, Listener, SizedBox};

use super::*;

#[derive(Clone)]
struct PanickingKeyReader {
    signal: Signal<u32>,
    node: Rc<FocusNode>,
}

impl flui_view::View for PanickingKeyReader {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for PanickingKeyReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let _ = self.signal.get(ctx);
        let signal = self.signal;
        Focus::new(SizedBox::square(10.0))
            .focus_node(Rc::clone(&self.node))
            .on_key_event(move |cx, _event| {
                let _ = signal.update(cx, |value| {
                    *value = 7;
                    panic!("keyboard updater probe");
                });
                KeyEventResult::Handled
            })
    }
}

/// A pointer event addressed to B (the NON-primary presentation)
/// must reach ONLY B's own gesture arena — never A's (the primary),
/// even though both presentations share one UI runtime's `enter()` scope
/// and dispatch machinery.
///
/// Deliberately stamps for B, not A: A is this UI runtime's primary, so a
/// mutant that reroutes `handle_input_addressed` to
/// `self.presentations.primary()` regardless of the addressed id
/// would still (accidentally) satisfy an A-stamped version of this
/// test — addressed-vs-primary is unobservable when the stamped
/// target IS the primary. Stamping for B is the only fixture that
/// actually exercises the addressed lookup: reverting the `Pointer`
/// arm to `self.presentations.primary()` makes this fail (B's count
/// stays `0`, A's becomes `1`).
pub(crate) fn input_stamped_for_b_never_reaches_as_arena() {
    let mut ui_runtime = UiRuntime::for_test();
    let a_id = ui_runtime.presentation_id();
    let b_id = ui_runtime.install_second_presentation_for_test();

    let down =
        make_down_event(Offset::new(4.0, 6.0), PointerKind::Mouse).expect("finite test position");
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(b_id, PlatformInput::Pointer(down));
    });

    assert_eq!(
        ui_runtime
            .presentations
            .get(b_id)
            .expect("B installed")
            .gestures()
            .active_pointer_count(),
        1,
        "the addressed (non-primary) presentation must receive the pointer event"
    );
    assert_eq!(
        ui_runtime
            .presentations
            .get(a_id)
            .expect("A installed")
            .gestures()
            .active_pointer_count(),
        0,
        "a pointer event stamped for B must never reach A's own gesture arena, even \
         though A is this ui_runtime's primary"
    );
}

pub(crate) fn panicking_keyboard_dispatch_keeps_priority_over_a_panicking_wake() {
    let panic_on_wake = Arc::new(AtomicBool::new(false));
    let wake_attempts = Arc::new(AtomicUsize::new(0));
    let wake_gate = Arc::clone(&panic_on_wake);
    let attempts = Arc::clone(&wake_attempts);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        attempts.fetch_add(1, Ordering::AcqRel);
        assert!(!wake_gate.load(Ordering::Acquire), "wake probe");
    });
    let mut ui_runtime = new_runtime(wake).expect("runtime");
    let a_id = ui_runtime.presentation_id();
    let b_id = ui_runtime.install_second_presentation_for_test();
    let graph_b = ui_runtime
        .presentation_widgets_for_test(b_id)
        .with_build_owner(|owner| owner.reactive().clone());
    let signal = graph_b.signal(1u32);
    let node = FocusNode::with_debug_label("panicking-key-reader-with-panicking-wake");
    ui_runtime
        .attach_root_widget_to_for_test(
            b_id,
            &PanickingKeyReader {
                signal,
                node: Rc::clone(&node),
            },
        )
        .expect("B root attaches");
    let mut backend = ScriptedSink::always_presents();
    ui_runtime.render_frame(&mut backend);
    ui_runtime.notify_presentation_focus_gained(b_id);
    let _ = node.request_focus();
    let _ = ui_runtime.take_redraw_request();
    let _ = ui_runtime
        .presentations
        .get(b_id)
        .expect("B installed")
        .take_redraw_pending();
    panic_on_wake.store(true, Ordering::Release);

    let key_event = KeyEventBuilder::new(flui_interaction::events::Code::KeyA).build();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        ui_runtime.enter(|ui_runtime| {
            ui_runtime.handle_input_addressed(a_id, PlatformInput::Keyboard(key_event));
        });
    }));

    let payload = outcome.expect_err("the original keyboard panic must resume");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"keyboard updater probe"),
        "the secondary wake panic must not replace the dispatch failure"
    );
    assert_eq!(signal.peek(&graph_b, |value| *value), Ok(7));
    assert!(
        ui_runtime
            .presentations
            .get(b_id)
            .expect("B installed")
            .take_redraw_pending(),
        "the active presentation retains redraw demand"
    );

    let attempts_after_failure = wake_attempts.load(Ordering::Acquire);
    panic_on_wake.store(false, Ordering::Release);
    assert_eq!(ui_runtime.drain_commands(), DrainReport::default());
    assert_eq!(
        wake_attempts.load(Ordering::Acquire),
        attempts_after_failure + 1,
        "the next owner boundary must retry the failed input-redraw wake"
    );
    assert!(
        ui_runtime.needs_redraw(),
        "the committed redraw demand survives"
    );
    ui_runtime.render_frame(&mut backend);
    assert_eq!(signal.peek(&graph_b, |value| *value), Ok(7));
}

// This module can only exercise `UiRuntime`'s own write side
// (`notify_presentation_focus_gained`) directly -- the production
// `WindowFocus(false)`-never-moves-active guard lives in the runner's
// `PlatformToUi::run` (the `if focused` check around the call to
// `notify_presentation_focus_gained`), which a direct `UiRuntime`-level
// test cannot reach or mutate.

/// Pointer contacts the primary's `Listener` observed, by phase.
#[derive(Default)]
struct ContactLog {
    downs: Cell<usize>,
    ups: Cell<usize>,
    cancels: Cell<usize>,
}

/// A primary presentation with a contact-logging `Listener`, its window
/// shown, running and focused. With `hold_input`, the first reveal is
/// deferred: the tree is laid out and painted but not presented, so pointer
/// input waits in the held queue until [`commit_and_count_open_routes`].
fn listener_ui_runtime(hold_input: bool) -> (UiRuntime, PresentationId, Rc<ContactLog>) {
    let ui_runtime = UiRuntime::for_test();
    let id = ui_runtime.presentation_id();
    let log = Rc::new(ContactLog::default());
    let (down, up, cancel) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));
    ui_runtime
        .attach_root_widget(
            &Listener::new()
                .behavior(HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| down.downs.set(down.downs.get() + 1))
                .on_pointer_up(move |_, _| up.ups.set(up.ups.get() + 1))
                .on_pointer_cancel(move |_, _| cancel.cancels.set(cancel.cancels.get() + 1))
                .child(SizedBox::new(40.0, 40.0)),
        )
        .expect("root attaches");
    ui_runtime.synchronize_window_snapshot(
        id,
        flui_platform::WindowExecutionState::Running,
        true,
        true,
    );
    if hold_input {
        ui_runtime.defer_first_frame();
    }
    ui_runtime.enter(|ui_runtime| ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    (ui_runtime, id, log)
}

fn press(ui_runtime: &UiRuntime, id: PresentationId) {
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(
            id,
            PlatformInput::Pointer(
                make_down_event(Offset::new(4.0, 6.0), PointerKind::Touch)
                    .expect("finite test position"),
            ),
        );
    });
}

fn press_held(ui_runtime: &UiRuntime, id: PresentationId) {
    press(ui_runtime, id);
    assert!(
        !ui_runtime
            .presentations
            .get(id)
            .expect("installed")
            .held_pointer_input()
            .borrow()
            .is_empty(),
        "the Down waits for the first commit"
    );
}

fn release(ui_runtime: &UiRuntime, id: PresentationId) {
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(
            id,
            PlatformInput::Pointer(
                make_up_event(Offset::new(4.0, 6.0), PointerKind::Touch)
                    .expect("finite test position"),
            ),
        );
    });
}

/// Present the deferred first frame, which replays held input, and report
/// how many contact routes stay open afterwards.
fn commit_and_count_open_routes(ui_runtime: &UiRuntime, id: PresentationId) -> usize {
    let presentation = ui_runtime.presentations.get(id).expect("installed");
    ui_runtime.allow_first_frame();
    assert!(
        ui_runtime
            .enter(|ui_runtime| ui_runtime.render_frame(&mut ScriptedSink::always_presents()))
    );
    assert!(
        presentation.held_pointer_input().borrow().is_empty(),
        "the commit replays or drops everything held"
    );
    presentation.gestures().active_pointer_count()
}

/// A Down held before the first commit, then a focus loss before its Up:
/// the Up goes to whichever window took focus, so replaying the Down at
/// commit would open a route nothing ever closes.
pub(crate) fn focus_loss_drops_a_held_open_contact_before_it_replays() {
    let (ui_runtime, id, log) = listener_ui_runtime(true);
    press_held(&ui_runtime, id);
    ui_runtime.update_window_focus(id, false);

    assert_eq!(commit_and_count_open_routes(&ui_runtime, id), 0);
    assert_eq!(
        log.downs.get(),
        0,
        "the abandoned Down never reaches a widget"
    );
    assert_eq!(log.cancels.get(), 0);
}

/// A hover an embedded surface reported before the first commit is held,
/// then replays to that surface: the window's own hover stays untouched.
pub(crate) fn a_held_embedded_hover_replays_to_its_surface() {
    let (ui_runtime, id, _log) = listener_ui_runtime(true);
    let presentation = ui_runtime.presentations.get(id).expect("installed");
    let embedded = presentation
        .gestures()
        .embed_surface()
        .expect("a surface identity");
    let mut hover = flui_interaction::events::make_move_event_for_id(
        flui_interaction::PointerId::try_from(3_u64).expect("nonzero"),
        Offset::new(4.0, 6.0),
        PointerKind::Mouse,
    )
    .expect("finite test position");
    let flui_interaction::PointerEvent::Move(update) = &mut hover else {
        unreachable!("the move helper builds a move")
    };
    update.buttons = flui_interaction::events::pointer::PointerButtons::NONE;
    let pointer = update.pointer;
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_on(id, embedded, PlatformInput::Pointer(hover));
    });
    assert!(
        !presentation.held_pointer_input().borrow().is_empty(),
        "the hover waits for the first commit"
    );
    commit_and_count_open_routes(&ui_runtime, id);
    presentation.gestures().flush_pending_moves();
    assert_eq!(
        presentation
            .gestures()
            .mouse_tracker()
            .source_position(&pointer),
        None,
        "the replayed hover reached the embedded surface, not the window"
    );
}

/// A host pause (the app is backgrounded mid-touch) cancels the same way:
/// the held Down is gone once the app resumes and commits.
pub(crate) fn host_pause_drops_a_held_open_contact_before_it_replays() {
    let (ui_runtime, id, log) = listener_ui_runtime(true);
    press_held(&ui_runtime, id);
    ui_runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Paused);
    ui_runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);

    assert_eq!(commit_and_count_open_routes(&ui_runtime, id), 0);
    assert_eq!(
        log.downs.get(),
        0,
        "the abandoned Down never reaches a widget"
    );
}

/// A tap that completed before the window lost focus is the user's
/// finished input: it still replays, Down and Up, at commit.
pub(crate) fn focus_loss_keeps_a_held_complete_tap_for_replay() {
    let (ui_runtime, id, log) = listener_ui_runtime(true);
    press_held(&ui_runtime, id);
    release(&ui_runtime, id);
    ui_runtime.update_window_focus(id, false);

    assert_eq!(commit_and_count_open_routes(&ui_runtime, id), 0);
    assert_eq!((log.downs.get(), log.ups.get()), (1, 1));
}

/// A touch already routed when the host pauses the app (backgrounded
/// mid-touch) is cancelled through its route: the widget sees a Cancel, and
/// no route survives to the resume.
pub(crate) fn host_pause_cancels_a_routed_contact_with_a_delivered_cancel() {
    let (ui_runtime, id, log) = listener_ui_runtime(false);
    press(&ui_runtime, id);
    let gestures = || {
        ui_runtime
            .presentations
            .get(id)
            .expect("installed")
            .gestures()
            .active_pointer_count()
    };
    assert_eq!((gestures(), log.downs.get()), (1, 1));

    ui_runtime.enter(|ui_runtime| {
        ui_runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Paused);
    });

    assert_eq!(gestures(), 0);
    assert_eq!((log.cancels.get(), log.ups.get()), (1, 0));
}

/// What a focused node's key handler received: key, code and state.
type KeyLog = Rc<std::cell::RefCell<Vec<(Key, Code, KeyState)>>>;

/// A primary presentation whose only focusable node is focused, logs every
/// key that reaches it and handles it; its window shown, running and focused.
fn keyboard_ui_runtime() -> (UiRuntime, PresentationId, Rc<FocusNode>, KeyLog) {
    let ui_runtime = UiRuntime::for_test();
    let id = ui_runtime.presentation_id();
    let node = FocusNode::with_debug_label("key-log");
    let log = KeyLog::default();
    let seen = Rc::clone(&log);
    ui_runtime
        .attach_root_widget(
            &Focus::new(SizedBox::new(40.0, 40.0))
                .focus_node(Rc::clone(&node))
                .on_key_event(move |_, event| {
                    seen.borrow_mut()
                        .push((event.key.clone(), event.code, event.state()));
                    KeyEventResult::Handled
                }),
        )
        .expect("root attaches");
    ui_runtime.synchronize_window_snapshot(
        id,
        flui_platform::WindowExecutionState::Running,
        true,
        true,
    );
    ui_runtime.enter(|ui_runtime| ui_runtime.render_frame(&mut ScriptedSink::always_presents()));
    let _ = node.request_focus();
    (ui_runtime, id, node, log)
}

fn deliver(ui_runtime: &UiRuntime, id: PresentationId, input: PlatformInput) {
    ui_runtime.enter(|ui_runtime| {
        ui_runtime.handle_input_addressed(id, input);
    });
}

fn mouse_button(
    ui_runtime: &UiRuntime,
    id: PresentationId,
    state: KeyState,
    button: PointerButton,
) {
    let at = Offset::new(4.0, 6.0);
    let event = match state {
        KeyState::Down => make_down_event_with_button(at, PointerKind::Mouse, button),
        KeyState::Up => make_up_event_with_button(at, PointerKind::Mouse, button),
    };
    deliver(
        ui_runtime,
        id,
        PlatformInput::Pointer(event.expect("finite test position")),
    );
}

fn key(code: Code, key: Key, state: KeyState, repeat: bool) -> PlatformInput {
    PlatformInput::Keyboard(
        KeyEventBuilder::new(code)
            .with_key(key)
            .with_state(state)
            .with_repeat(repeat)
            .build(),
    )
}

/// The mouse's back button (X1) reaches the focused node as a
/// `BrowserBack` press and release, without a physical key code, and it is a
/// pointer press: it does not make focus visible.
pub(crate) fn the_back_button_is_a_back_key() {
    let (ui_runtime, id, node, log) = keyboard_ui_runtime();
    mouse_button(&ui_runtime, id, KeyState::Down, PointerButton::BACK);
    mouse_button(&ui_runtime, id, KeyState::Up, PointerButton::BACK);
    let back = Key::Named(NamedKey::BrowserBack);
    assert_eq!(
        *log.borrow(),
        [
            (back.clone(), Code::Unidentified, KeyState::Down),
            (back, Code::Unidentified, KeyState::Up),
        ]
    );
    assert!(!node.has_visible_focus(), "a pointer button shows no focus");

    log.borrow_mut().clear();
    mouse_button(&ui_runtime, id, KeyState::Down, PointerButton::PRIMARY);
    mouse_button(&ui_runtime, id, KeyState::Up, PointerButton::PRIMARY);
    assert!(log.borrow().is_empty(), "only the back button is a key");
}

/// The back button is its own held key: releasing it does not end a held
/// keyboard `BrowserBack` key, whose repeats keep reaching the node — also
/// when the platform could not place that key physically.
pub(crate) fn the_back_button_is_held_apart_from_the_back_key() {
    for code in [Code::BrowserBack, Code::Unidentified] {
        let (ui_runtime, id, _node, log) = keyboard_ui_runtime();
        let back = Key::Named(NamedKey::BrowserBack);
        deliver(
            &ui_runtime,
            id,
            key(code, back.clone(), KeyState::Down, false),
        );
        mouse_button(&ui_runtime, id, KeyState::Down, PointerButton::BACK);
        mouse_button(&ui_runtime, id, KeyState::Up, PointerButton::BACK);
        deliver(
            &ui_runtime,
            id,
            key(code, back.clone(), KeyState::Down, true),
        );
        assert_eq!(
            log.borrow().last(),
            Some(&(back, code, KeyState::Down)),
            "the {code:?} key's repeat still reaches the node"
        );
        assert_eq!(log.borrow().len(), 4);
    }
}

/// A pointer press hides the focus a key showed.
pub(crate) fn a_pointer_press_hides_keyboard_focus() {
    let (ui_runtime, id, node, _log) = keyboard_ui_runtime();
    deliver(
        &ui_runtime,
        id,
        key(Code::KeyA, Key::character("a"), KeyState::Down, false),
    );
    assert!(node.has_visible_focus(), "a key shows focus");
    mouse_button(&ui_runtime, id, KeyState::Down, PointerButton::PRIMARY);
    assert!(!node.has_visible_focus(), "a pointer press hides it");
}

/// A window that loses activation forgets its held keys: their repeats and
/// release after it returns went to another window first, and reach nothing.
pub(crate) fn deactivation_forgets_held_keys() {
    held_enter_after(|ui_runtime, id| {
        ui_runtime.update_window_focus(id, false);
        ui_runtime.update_window_focus(id, true);
    });
}

/// The same, when the host reports the loss as a whole-window snapshot.
pub(crate) fn an_unfocused_snapshot_forgets_held_keys() {
    held_enter_after(|ui_runtime, id| {
        let running = flui_platform::WindowExecutionState::Running;
        ui_runtime.synchronize_window_snapshot(id, running, false, true);
        ui_runtime.synchronize_window_snapshot(id, running, true, true);
    });
}

/// The same, when a sibling window gaining focus takes it (`update_window_focus`).
pub(crate) fn a_sibling_gaining_focus_forgets_held_keys() {
    held_enter_after(|ui_runtime, id| {
        let sibling = ui_runtime.install_second_presentation_for_test();
        ui_runtime.update_window_focus(sibling, true);
        ui_runtime.update_window_focus(id, true);
    });
}

/// The same, when the sibling's gain arrives as a window snapshot.
pub(crate) fn a_sibling_snapshot_forgets_held_keys() {
    held_enter_after(|ui_runtime, id| {
        let sibling = ui_runtime.install_second_presentation_for_test();
        let running = flui_platform::WindowExecutionState::Running;
        ui_runtime.synchronize_window_snapshot(sibling, running, true, true);
        ui_runtime.synchronize_window_snapshot(id, running, true, true);
    });
}

/// A key-cancel handler that panics while a window loses activation, alone or
/// to a sibling, still leaves every window's focus and lifecycle updated: the
/// panic reaches the caller after the pass, not halfway through it.
pub(crate) fn a_panicking_key_cancel_still_moves_window_focus() {
    for to_sibling in [false, true] {
        let (mut ui_runtime, id, node, _log) = keyboard_ui_runtime();
        let _cancel = node.register_on_key_cancel(Rc::new(|_| panic!("key cancel handler")));
        let sibling = to_sibling.then(|| ui_runtime.install_second_presentation_for_test());
        deliver(
            &ui_runtime,
            id,
            key(
                Code::Enter,
                Key::Named(NamedKey::Enter),
                KeyState::Down,
                false,
            ),
        );
        let payload = catch_unwind(AssertUnwindSafe(|| match sibling {
            Some(sibling) => ui_runtime.update_window_focus(sibling, true),
            None => ui_runtime.update_window_focus(id, false),
        }))
        .expect_err("the handler's panic reaches the caller");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"key cancel handler"));
        let presentation = ui_runtime.presentations.get(id).expect("installed");
        assert!(!presentation.window_focused.get(), "sibling: {to_sibling}");
        assert_eq!(
            flui_view::__runtime::BindingRuntime::lifecycle_source(presentation.widgets())
                .current(),
            Some(flui_scheduler::AppLifecycleState::Inactive),
            "the window that lost activation is told so (sibling: {to_sibling})"
        );
        if let Some(sibling) = sibling {
            assert!(
                ui_runtime
                    .presentations
                    .get(sibling)
                    .expect("installed")
                    .window_focused
                    .get(),
                "the sibling took the focus"
            );
        }
    }
}

/// Hold Enter, run `deactivate`, then send Enter's repeat and release: the
/// node saw only the key-down.
fn held_enter_after(deactivate: impl FnOnce(&mut UiRuntime, PresentationId)) {
    let (mut ui_runtime, id, _node, log) = keyboard_ui_runtime();
    let enter = Key::Named(NamedKey::Enter);
    deliver(
        &ui_runtime,
        id,
        key(Code::Enter, enter.clone(), KeyState::Down, false),
    );
    deactivate(&mut ui_runtime, id);
    deliver(
        &ui_runtime,
        id,
        key(Code::Enter, enter.clone(), KeyState::Down, true),
    );
    deliver(
        &ui_runtime,
        id,
        key(Code::Enter, enter.clone(), KeyState::Up, false),
    );
    assert_eq!(*log.borrow(), [(enter, Code::Enter, KeyState::Down)]);
}
