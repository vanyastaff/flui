//! `Tester` drives an application through its window's agent: it finds by
//! locator, acts through the input lease, and fails with what the window shows.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::{Duration, Instant};

use flui_foundation::MonotonicClock as _;
use flui_testing::{TestError, TestWindow, Tester, find};
use flui_view::prelude::*;
use flui_view::{Signal, StatefulView};
use flui_widgets::{Center, Column, RawButton, SizedBox, Text};

#[derive(Clone, StatefulView)]
struct Counter;

struct CounterState {
    count: Signal<u32>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        CounterState {
            count: ctx.signal(0),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        let count = self.count;
        Center::new().child(Column::new((
            Text::new(count.get(ctx).to_string()),
            SizedBox::height(16.0),
            RawButton::new(Text::new("Increment"))
                .on_pressed(move |ctx| count.update(ctx, |n| *n += 1)),
        )))
    }
}

fn panic_text(run: impl FnOnce()) -> String {
    let payload = catch_unwind(AssertUnwindSafe(run)).expect_err("the expectation fails");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_default()
}

#[test]
fn a_tap_on_a_located_button_runs_its_press_and_the_next_frame() {
    let mut t = Tester::mount(Counter);
    t.expect(find::text("0")).to_exist();
    t.tap(&find::button("Increment"));
    t.expect(find::text("1")).to_exist();
    t.expect(find::text("0")).to_be_gone();
    t.tap(&find::button("Increment"));
    t.expect(find::text("2")).to_exist();
}

#[test]
fn each_tester_owns_its_application() {
    let mut first = Tester::mount(Counter);
    first.tap(&find::button("Increment"));
    let mut second = Tester::mount(Counter);
    second.expect(find::text("0")).to_exist();
    first.expect(find::text("1")).to_exist();
}

#[test]
fn an_expectation_on_an_idle_application_fails_at_once_with_what_the_window_shows() {
    let mut t = Tester::mount(Counter);
    let before = t.clock().elapsed();
    let message = panic_text(|| t.expect(find::text("7")).to_exist());
    assert!(message.contains("idle"), "{message}");
    assert!(
        message.contains("Increment"),
        "the outline lists the button: {message}"
    );
    assert_eq!(
        t.clock().elapsed(),
        before,
        "an idle application's clock does not move inside an expectation"
    );
}

#[test]
fn a_locator_that_matches_nothing_names_the_window_and_the_identifier_way_out() {
    let mut t = Tester::mount(Counter);
    let error = t
        .try_tap(&find::button("Decrement"))
        .expect_err("there is no Decrement button");
    let TestError::NotFound { shown, .. } = &error else {
        panic!("expected NotFound, got {error:?}");
    };
    assert!(shown.contains("Increment"), "{shown}");
    assert!(error.to_string().contains("find::id"), "{error}");
}

#[test]
fn a_locator_that_matches_several_elements_lists_them() {
    let mut t = Tester::mount(Column::new((
        RawButton::new(Text::new("Same")).on_pressed(|_| {}),
        RawButton::new(Text::new("Same")).on_pressed(|_| {}),
    )));
    let error = t
        .try_tap(&find::button("Same"))
        .expect_err("two buttons are named Same");
    assert!(
        matches!(error, TestError::Ambiguous { count: 2, .. }),
        "{error:?}"
    );
}

#[test]
fn a_rect_is_in_the_windows_logical_pixels() {
    let mut t = Tester::mount_in(Counter, TestWindow::new(480, 320));
    let button = t.rect(&find::button("Increment"));
    let centre = button.x + button.width / 2.0;
    assert!((centre - 240.0).abs() <= 1.0, "{button:?}");
}

#[test]
fn pump_for_moves_the_clock_by_what_it_is_given() {
    let mut t = Tester::mount(Counter);
    let before = t.clock().elapsed();
    t.pump_for(Duration::from_millis(40));
    assert_eq!(
        t.clock().elapsed().checked_sub(before),
        Some(Duration::from_millis(40))
    );
}

#[test]
fn an_element_disabled_through_an_ancestor_is_not_enabled() {
    let mut t = Tester::mount(
        flui_widgets::Semantics::new()
            .container(true)
            .enabled(false)
            .label("Group")
            .child(RawButton::new(Text::new("Inside")).on_pressed(|_| {})),
    );
    t.expect(find::button("Inside")).to_be_disabled();
    let message = panic_text(|| t.expect(find::button("Inside")).to_be_enabled());
    assert!(message.contains("enabled"), "{message}");
}

#[test]
fn a_long_press_longer_than_the_answer_budget_is_delivered() {
    let mut t = Tester::mount(Counter);
    let before = t.clock().elapsed();
    t.long_press(&find::button("Increment"), Duration::from_secs(2));
    let took = t.clock().elapsed().checked_sub(before).unwrap_or_default();
    assert!(took >= Duration::from_secs(2), "the press lasted {took:?}");
}

/// A `FutureBuilder` that shows "waiting" until the returned call wakes it
/// with 7.
fn pending_future() -> (Tester, impl FnOnce()) {
    use std::cell::RefCell;
    use std::task::{Context, Poll, Waker};

    use flui_widgets::{FutureBuilder, FutureFactory, SnapshotBuilder};

    let waker: Rc<RefCell<Option<Waker>>> = Rc::default();
    let ready: Rc<RefCell<bool>> = Rc::default();
    let factory: FutureFactory<u32, ()> = {
        let waker = Rc::clone(&waker);
        let ready = Rc::clone(&ready);
        Rc::new(move || {
            let waker = Rc::clone(&waker);
            let ready = Rc::clone(&ready);
            Box::pin(std::future::poll_fn(move |ctx: &mut Context<'_>| {
                if *ready.borrow() {
                    return Poll::Ready(Ok(7));
                }
                *waker.borrow_mut() = Some(ctx.waker().clone());
                Poll::Pending
            }))
        })
    };
    let builder: SnapshotBuilder<u32, ()> = Rc::new(|_ctx, snapshot| {
        let text = snapshot
            .data()
            .map_or_else(|| "waiting".to_owned(), |value| format!("got {value}"));
        Text::new(text).into_view().boxed()
    });
    let mut t = Tester::mount(FutureBuilder::keyed(Some(1_u32), factory, builder));
    t.expect(find::text("waiting")).to_exist();
    let wake = move || {
        *ready.borrow_mut() = true;
        waker
            .borrow_mut()
            .take()
            .expect("the first poll stored its waker")
            .wake();
    };
    (t, wake)
}

/// A completion the test wakes between frames is polled by a frame the
/// expectation runs before it reads; the expectation reads the tree that
/// frame committed before it calls the application idle.
#[test]
fn an_expectation_reads_the_tree_a_ready_completion_committed() {
    let (mut t, wake) = pending_future();
    wake();
    t.expect(find::text("got 7")).to_exist();
}

/// The same completion: an expectation of what it replaces does not pass on
/// the tree from before it.
#[test]
fn an_expectation_does_not_pass_on_the_tree_a_ready_completion_replaced() {
    let (mut t, wake) = pending_future();
    wake();
    let message = panic_text(|| t.expect(find::text("waiting")).to_exist());
    assert!(message.contains("got 7"), "{message}");
}

const DEBOUNCE: Duration = Duration::from_millis(100);

/// Sleeps [`DEBOUNCE`] on the runtime's clock from `init_state`, recording
/// when it started, then sets `done`.
#[derive(Clone, StatefulView)]
struct Debounce {
    started: Rc<Cell<Option<Instant>>>,
    done: Rc<Cell<bool>>,
}

struct DebounceState {
    view: Debounce,
    task: Option<flui_view::TaskToken>,
}

impl StatefulView for Debounce {
    type State = DebounceState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        DebounceState {
            view: self.clone(),
            task: None,
        }
    }
}

impl ViewState<Debounce> for DebounceState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let timer: flui_view::TimerHandle = ctx.timer().expect("a hosted tree has a timer");
        self.view.started.set(Some(timer.now()));
        let sleep = timer.sleep(DEBOUNCE);
        let done = Rc::clone(&self.view.done);
        self.task = ctx.async_driver().map(|driver| {
            driver.spawn_local(Box::pin(async move {
                sleep.await;
                done.set(true);
            }))
        });
    }

    fn build(&self, _view: &Debounce, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new("debouncing")
    }
}

/// A timer due within the next frame is still the test's to reach: a failing
/// expectation neither moves the clock nor fires it.
#[test]
fn an_expectation_does_not_fire_a_timer_due_within_a_frame() {
    let view = Debounce {
        started: Rc::default(),
        done: Rc::default(),
    };
    let (started, done) = (Rc::clone(&view.started), Rc::clone(&view.done));
    let mut t = Tester::mount(view);
    let deadline = started.get().expect("init_state ran") + DEBOUNCE;
    let short = Duration::from_millis(5);
    let left = deadline
        .checked_duration_since(t.clock().now())
        .and_then(|left| left.checked_sub(short))
        .expect("mounting took less than the debounce");
    t.pump_for(left);
    assert!(!done.get(), "the deadline is still {short:?} away");

    let before = t.clock().elapsed();
    let message = panic_text(|| t.expect(find::text("settled")).to_exist());
    assert!(message.contains("idle"), "{message}");
    assert_eq!(t.clock().elapsed(), before, "the clock did not move");
    assert!(!done.get(), "the timer did not fire");

    t.pump_for(short);
    assert!(done.get(), "the test reaches it with pump_for");
}

#[test]
fn a_drag_scrolls_a_default_scroll_view() {
    use flui_widgets::ListView;

    let items: Vec<_> = (0..40)
        .map(|index| Text::new(format!("Item {index}")))
        .collect();
    let mut t = Tester::mount_in(ListView::new(50.0, items), TestWindow::new(200, 300));
    let before = t.rect(&find::text("Item 1"));
    t.drag(
        &find::text("Item 1"),
        0.0,
        -100.0,
        Duration::from_millis(200),
    );
    t.pump_and_settle();
    let after = t.rect(&find::text("Item 1"));
    assert!(after.y < before.y - 50.0, "{before:?} -> {after:?}");
}

#[test]
fn a_drag_from_a_point_presses_there_and_moves_by_the_offset() {
    use std::cell::RefCell;

    use flui_interaction::events::PointerEvent;
    use flui_testing::LogicalPoint;
    use flui_widgets::{ColoredBox, Listener};

    // An edge swipe starts where no element's centre is: the press lands at
    // the point named, not at the centre of what is under it.
    let pressed = Rc::new(RefCell::new(Vec::new()));
    let released = Rc::new(RefCell::new(Vec::new()));
    let (on_down, on_up) = (Rc::clone(&pressed), Rc::clone(&released));
    let surface = Listener::new()
        .on_pointer_down(move |_, dispatch| {
            if let PointerEvent::Down(down) = dispatch.global {
                let at = down.sample.position.get();
                on_down.borrow_mut().push((at.x, at.y));
            }
        })
        .on_pointer_up(move |_, dispatch| {
            if let PointerEvent::Up(up) = dispatch.global {
                let at = up.sample.position.get();
                on_up.borrow_mut().push((at.x, at.y));
            }
        })
        .child(
            SizedBox::new(200.0, 300.0)
                .child(ColoredBox::new(flui_painting::styling::Color::WHITE)),
        );
    let mut t = Tester::mount_in(surface, TestWindow::new(200, 300));

    t.drag_from(
        LogicalPoint::new(4.0, 150.0),
        120.0,
        -10.0,
        Duration::from_millis(100),
    );

    assert_eq!(*pressed.borrow(), [(4.0, 150.0)], "one press, at the start");
    assert_eq!(
        *released.borrow(),
        [(124.0, 140.0)],
        "one release, at start + offset"
    );
}

/// What a focused element heard: each key event as `down`/`up`/`repeat`, its
/// key, and `^` for each of Control and Shift held.
fn key_log() -> (Tester, Rc<std::cell::RefCell<Vec<String>>>) {
    use flui_interaction::KeyEventResult;
    use flui_platform::keyboard::{Key, KeyRepeat, KeyState, Modifiers};

    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let heard = Rc::clone(&log);
    let t = Tester::mount(
        flui_widgets::Focus::new(SizedBox::new(40.0, 40.0))
            .autofocus(true)
            .on_key_event(move |_ctx, event| {
                let state = match (event.state(), event.repeat()) {
                    (KeyState::Down, KeyRepeat::AutoRepeat) => "repeat",
                    (KeyState::Down, _) => "down",
                    _ => "up",
                };
                let key = match &event.key {
                    Key::Named(named) => named.as_str().to_owned(),
                    Key::Character(text) => text.as_str().to_owned(),
                    other => format!("{other:?}"),
                };
                let held = [Modifiers::CONTROL, Modifiers::SHIFT]
                    .into_iter()
                    .filter(|modifier| event.modifiers.contains(*modifier))
                    .count();
                heard
                    .borrow_mut()
                    .push(format!("{state} {key}{}", "^".repeat(held)));
                KeyEventResult::Handled
            }),
    );
    (t, log)
}

#[test]
fn press_keys_presses_each_chord_in_turn_and_releases_it_in_reverse() {
    let (mut t, log) = key_log();
    t.press_keys(&["Control+a", "Enter", "Shift++"]);
    assert_eq!(
        *log.borrow(),
        [
            "down Control^",
            "down a^",
            "up a^",
            "up Control",
            "down Enter",
            "up Enter",
            "down Shift^",
            "down +^",
            "up +^",
            "up Shift",
        ]
    );
}

#[test]
fn a_held_key_repeats_when_pressed_again_and_modifies_chords_meanwhile() {
    let (mut t, log) = key_log();
    t.key_down("Shift");
    t.press_keys(&["Tab"]);
    t.key_down("Shift");
    t.key_up("Shift");
    t.key_up("Shift");
    assert_eq!(
        *log.borrow(),
        [
            "down Shift^",
            "down Tab^",
            "up Tab^",
            "repeat Shift^",
            "up Shift",
        ],
        "a second press of a held key is its auto-repeat, and lifting a key that is up sends \
         nothing"
    );
}

#[test]
fn a_chord_with_an_unnamed_key_presses_nothing() {
    let (mut t, log) = key_log();
    let error = t
        .try_press_keys(&["Enter", "Control+"])
        .expect_err("an empty key is refused");
    assert!(matches!(error, TestError::Agent(_)), "{error:?}");
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

/// A field holding `Trip `, focused by a tap.
fn title_field() -> Tester {
    let field = flui_widgets::EditableText::new(
        flui_widgets::TextEditingController::with_text("Trip "),
        flui_interaction::FocusNode::with_debug_label("title"),
    );
    let mut t = Tester::mount(Center::new().child(SizedBox::new(300.0, 40.0).child(field)));
    t.tap(&find::role(flui_testing::Role::TextInput));
    t
}

/// The field's editable render object's `name` field, as an agent inspects it.
fn editable_field(t: &mut Tester, name: &str) -> Option<flui_testing::InspectValue> {
    fn find_editable(nodes: &[flui_testing::InspectNode]) -> Option<&flui_testing::InspectNode> {
        nodes.iter().find_map(|node| {
            if node.kind == "RenderEditable" {
                Some(node)
            } else {
                find_editable(&node.children)
            }
        })
    }
    let reading = t.inspect_render();
    let node = find_editable(&reading.roots).expect("the field has an editable render object");
    node.fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.value.clone())
}

#[test]
fn a_composition_shows_in_the_focused_field_until_it_is_committed() {
    use flui_testing::InspectValue::{Bool, Uint};

    let mut t = title_field();
    t.compose("とう", Some(1));
    t.expect(find::role(flui_testing::Role::TextInput))
        .to_have_value("Trip とう");
    assert_eq!(editable_field(&mut t, "composing_start"), Some(Uint(5)));
    assert_eq!(editable_field(&mut t, "composing_end"), Some(Uint(11)));
    assert_eq!(
        editable_field(&mut t, "caret_byte_offset"),
        Some(Uint(8)),
        "the caret is one character into the composition"
    );
    assert_eq!(editable_field(&mut t, "show_caret"), Some(Bool(true)));

    t.compose("東京", None);
    assert_eq!(
        editable_field(&mut t, "show_caret"),
        Some(Bool(false)),
        "a composition without a caret hides it"
    );

    t.commit_composition("東京");
    t.expect(find::role(flui_testing::Role::TextInput))
        .to_have_value("Trip 東京");
    assert_ne!(editable_field(&mut t, "composing_start"), Some(Uint(5)));
}

#[test]
fn a_cancelled_composition_leaves_the_text_as_it_was() {
    let mut t = title_field();
    t.compose("abc", Some(3));
    t.cancel_composition();
    t.expect(find::role(flui_testing::Role::TextInput))
        .to_have_value("Trip ");

    let error = t
        .try_compose("ab", Some(3))
        .expect_err("a caret past the composition is refused");
    assert!(matches!(error, TestError::Agent(_)), "{error:?}");
}

/// One line of text, kept in the window's storage.
#[derive(Debug, PartialEq)]
struct Note(String);

impl flui_view::persist::Document for Note {
    const NAME: flui_platform::StorageName = flui_platform::StorageName::from_static("note");
    const VERSION: u32 = 1;
    const MAX_BYTES: u64 = 64;

    fn initial() -> Self {
        Self(String::new())
    }

    fn encode(&self) -> Vec<u8> {
        self.0.clone().into_bytes()
    }

    fn decode(_version: u32, body: &[u8]) -> Result<Self, flui_view::persist::DecodeError> {
        String::from_utf8(body.to_vec())
            .map(Self)
            .map_err(|error| flui_view::persist::DecodeError::new(error.to_string()))
    }
}

/// What a [`Notebook`] took in `init_state`: its document, its window's close
/// guard and a hold over the close when it was asked to take one.
#[derive(Default)]
struct Opened {
    note: Option<flui_view::persist::Persisted<Note>>,
    hold: Option<flui_view::CloseHold>,
    lifecycle: Vec<flui_view::AppLifecycleState>,
    _observation: Option<flui_view::LifecycleSubscription>,
    _notices: Option<flui_view::HostNoticeSubscription>,
}

/// Opens [`Note`] and the window's close guard, and records the window's
/// lifecycle; each mount, a restart's included, refills `opened`.
#[derive(Clone, StatefulView)]
struct Notebook {
    hold_close: bool,
    /// Written into the note when the window detaches.
    save_on_detach: Option<&'static str>,
    /// Panics when the session-ended notice arrives and when the window
    /// detaches.
    panic_at_session_end: bool,
    opened: Rc<std::cell::RefCell<Opened>>,
}

struct NotebookState(Notebook);

impl StatefulView for Notebook {
    type State = NotebookState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        NotebookState(self.clone())
    }
}

impl ViewState<Notebook> for NotebookState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let guard = ctx
            .close_guard()
            .expect("a hosted window has a close guard");
        let lifecycle = Rc::clone(&self.0.opened);
        let save_on_detach = self.0.save_on_detach;
        let panic_at_session_end = self.0.panic_at_session_end;
        let notices = ctx
            .lifecycle_handle()
            .expect("a hosted window has a lifecycle")
            .subscribe_notices(move |notice| {
                assert!(
                    !(panic_at_session_end
                        && matches!(notice, flui_platform::HostNotice::SessionEnded)),
                    "a subscriber fails at the session's end"
                );
            })
            .expect("the window is open");
        let (_, observation) = ctx
            .lifecycle_handle()
            .expect("a hosted window has a lifecycle")
            .subscribe(move |state| {
                if state == flui_view::AppLifecycleState::Detached
                    && let Some(text) = save_on_detach
                {
                    let note = lifecycle.borrow().note.clone();
                    if let Some(note) = note {
                        note.set(Note(text.to_owned()))
                            .expect("a loaded note takes a value");
                    }
                }
                lifecycle.borrow_mut().lifecycle.push(state);
                assert!(
                    !(panic_at_session_end && state == flui_view::AppLifecycleState::Detached),
                    "an observer fails as the window detaches"
                );
            })
            .expect("the window is open");
        *self.0.opened.borrow_mut() = Opened {
            note: Some(flui_view::persist::Persisted::open(ctx)),
            hold: self.0.hold_close.then(|| guard.hold()),
            lifecycle: Vec::new(),
            _observation: Some(observation),
            _notices: Some(notices),
        };
    }

    fn build(&self, _view: &Notebook, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(40.0, 40.0)
    }
}

fn notebook(hold_close: bool) -> (Notebook, Rc<std::cell::RefCell<Opened>>) {
    notebook_saving(hold_close, None, false)
}

fn notebook_saving(
    hold_close: bool,
    save_on_detach: Option<&'static str>,
    panic_at_session_end: bool,
) -> (Notebook, Rc<std::cell::RefCell<Opened>>) {
    let opened = Rc::new(std::cell::RefCell::new(Opened::default()));
    let view = Notebook {
        hold_close,
        save_on_detach,
        panic_at_session_end,
        opened: Rc::clone(&opened),
    };
    (view, opened)
}

/// The note the current run opened.
fn note(opened: &Rc<std::cell::RefCell<Opened>>) -> flui_view::persist::Persisted<Note> {
    opened
        .borrow()
        .note
        .clone()
        .expect("init_state opened the note")
}

/// Poll `future` between frames until it completes.
fn settle<F: std::future::Future>(t: &mut Tester, future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut ctx = std::task::Context::from_waker(std::task::Waker::noop());
    for _ in 0..20 {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut ctx) {
            return output;
        }
        t.pump();
    }
    panic!("the future did not complete within 20 frames");
}

#[test]
fn a_restart_launches_the_same_view_over_the_storage_the_run_left() {
    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    let loaded = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    assert_eq!(*loaded, Note(String::new()));
    note(&opened)
        .set(Note("groceries".to_owned()))
        .expect("a loaded note takes a value");
    t.pump_and_settle();
    let storage = t.storage().expect("a window keeps a storage").clone();

    let mut t = t.restart();
    let loaded = settle(&mut t, note(&opened).load()).expect("the stored note loads");
    assert_eq!(
        *loaded,
        Note("groceries".to_owned()),
        "the restart read what the run wrote"
    );
    assert!(
        storage
            .contents(&flui_platform::StorageName::from_static("note"))
            .is_some(),
        "the run wrote into the storage the test kept"
    );

    let mut t = Tester::mount_in(notebook(false).0, TestWindow::default().storage(storage));
    assert!(t.storage().is_some());
    t.pump();
}

#[test]
fn a_window_without_storage_refuses_a_write() {
    let (view, opened) = notebook(false);
    let mut t = Tester::mount_in(view, TestWindow::default().without_storage());
    assert!(t.storage().is_none());
    let _ = settle(&mut t, note(&opened).load());
    assert!(
        note(&opened).set(Note("lost".to_owned())).is_err(),
        "a document with nowhere to go is read-only"
    );
}

#[test]
fn a_held_close_waits_for_its_hold_and_a_session_end_does_not() {
    let (view, opened) = notebook(true);
    let mut t = Tester::mount(view);
    assert!(!t.request_close(), "the hold refuses the close");
    assert!(!t.is_closed());
    drop(opened.borrow_mut().hold.take());
    t.pump();
    assert!(
        t.is_closed(),
        "the frame after the release carries the close out"
    );

    let (view, _opened) = notebook(true);
    let mut t = Tester::mount(view);
    t.end_session();
    assert!(
        t.is_closed(),
        "a session that is ending closes past the hold"
    );
}

#[test]
fn the_window_losing_focus_makes_the_application_inactive() {
    use flui_view::AppLifecycleState::{Inactive, Resumed};

    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    opened.borrow_mut().lifecycle.clear();
    t.set_window_focused(false);
    t.set_window_focused(true);
    assert_eq!(opened.borrow().lifecycle, [Inactive, Resumed]);
}

/// A restart is the user quitting and opening the application again: a write
/// the run accepted and had not yet carried out still reaches the storage.
#[test]
fn a_restart_writes_what_the_run_accepted_before_it_stops() {
    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    note(&opened)
        .set(Note("unsaved".to_owned()))
        .expect("a loaded note takes a value");

    let mut t = t.restart();
    let loaded = settle(&mut t, note(&opened).load()).expect("the stored note loads");
    assert_eq!(*loaded, Note("unsaved".to_owned()));
}

/// At a session's end the window detaches before the documents are written,
/// so what a Detached observer publishes is in the storage the run leaves.
#[test]
fn a_session_end_writes_what_the_window_publishes_as_it_detaches() {
    let (view, opened) = notebook_saving(false, Some("draft"), false);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    t.end_session();
    assert!(
        opened
            .borrow()
            .lifecycle
            .contains(&flui_view::AppLifecycleState::Detached),
        "the window detached"
    );
    let storage = t.storage().expect("a window keeps a storage").clone();
    drop(t);

    let (view, opened) = notebook(false);
    let mut t = Tester::mount_in(view, TestWindow::default().storage(storage));
    let loaded = settle(&mut t, note(&opened).load()).expect("the stored note loads");
    assert_eq!(*loaded, Note("draft".to_owned()));
}

/// A subscriber and an observer that panic at the session's end do not cost the session
/// its close or its write: the window still detaches and publishes, the
/// documents are written, and the panic reaches the test afterwards.
#[test]
fn a_session_end_writes_even_when_a_subscriber_panics() {
    let (view, opened) = notebook_saving(false, Some("draft"), true);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    let storage = t.storage().expect("a window keeps a storage").clone();
    let ended = catch_unwind(AssertUnwindSafe(|| t.end_session()));
    assert!(ended.is_err(), "the subscriber's panic reaches the test");
    drop(t);

    let (view, opened) = notebook(false);
    let mut t = Tester::mount_in(view, TestWindow::default().storage(storage));
    let loaded = settle(&mut t, note(&opened).load()).expect("the stored note loads");
    assert_eq!(*loaded, Note("draft".to_owned()));
}

/// A listener that panics as the window's composition is committed at the
/// session's end does not cost the session its close or its write.
#[test]
fn a_session_end_writes_even_when_committing_the_composition_panics() {
    use flui_foundation::Listenable as _;

    let (book, opened) = notebook_saving(false, Some("draft"), false);
    let controller = flui_widgets::TextEditingController::with_text("Trip ");
    let field = flui_widgets::EditableText::new(
        controller.clone(),
        flui_interaction::FocusNode::with_debug_label("title"),
    );
    let mut t = Tester::mount(Column::new((book, SizedBox::new(300.0, 40.0).child(field))));
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    t.tap(&find::role(flui_testing::Role::TextInput));
    t.compose("とう", Some(1));
    let _listening = controller.listen(Rc::new({
        let controller = controller.clone();
        move || {
            assert!(
                controller.is_composing(),
                "a listener fails as the composition is committed"
            );
        }
    }));
    let storage = t.storage().expect("a window keeps a storage").clone();
    let ended = catch_unwind(AssertUnwindSafe(|| t.end_session()));
    assert!(ended.is_err(), "the listener's panic reaches the test");
    assert!(
        opened
            .borrow()
            .lifecycle
            .contains(&flui_view::AppLifecycleState::Detached),
        "the window detached past the panic"
    );
    drop(t);

    let (view, opened) = notebook(false);
    let mut t = Tester::mount_in(view, TestWindow::default().storage(storage));
    let loaded = settle(&mut t, note(&opened).load()).expect("the stored note loads");
    assert_eq!(*loaded, Note("draft".to_owned()));
}
