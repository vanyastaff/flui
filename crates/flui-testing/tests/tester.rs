//! `Tester` drives an application through its window's agent: it finds by
//! locator, acts through the input lease, and fails with what the window shows.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::{Duration, Instant};

use flui_foundation::MonotonicClock as _;
use flui_testing::{Locator, TestError, TestWindow, Tester, find};
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
    use flui_interaction::focus::KeyEventResult;
    use flui_platform::keyboard::{Key, KeyPress, KeyState, Modifiers};

    let log = Rc::new(std::cell::RefCell::new(Vec::new()));
    let heard = Rc::clone(&log);
    let t = Tester::mount(
        flui_widgets::Focus::new(SizedBox::new(40.0, 40.0))
            .autofocus(true)
            .on_key_event(move |_ctx, event| {
                let state = match event.state {
                    KeyState::Down(KeyPress::AutoRepeat) => "repeat",
                    KeyState::Down(_) => "down",
                    KeyState::Up => "up",
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
fn a_chord_naming_a_held_key_leaves_it_held() {
    let (mut t, log) = key_log();
    t.key_down("Shift");
    t.press_keys(&["Shift+Tab", "Tab"]);
    t.key_up("Shift");
    assert_eq!(
        *log.borrow(),
        [
            "down Shift^",
            "down Tab^",
            "up Tab^",
            "down Tab^",
            "up Tab^",
            "up Shift",
        ],
        "the chord neither repeats nor releases the held Shift, so it modifies both Tabs"
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
        flui_interaction::focus::FocusNode::with_debug_label("title"),
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
    /// How many session-ended notices reached the window.
    session_ended: usize,
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
    /// A write hold each state of the tree takes on this storage and drops
    /// with the state.
    state_hold: Option<flui_testing::storage::MemoryStorage>,
}

/// The notebook, and the write hold the state keeps for its lifetime.
struct NotebookState(
    Notebook,
    #[expect(
        dead_code,
        reason = "held for its drop: the state releases it as it is disposed"
    )]
    Option<flui_testing::storage::WriteHold>,
);

impl StatefulView for Notebook {
    type State = NotebookState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        NotebookState(
            self.clone(),
            self.state_hold
                .as_ref()
                .map(flui_testing::storage::MemoryStorage::hold_writes),
        )
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
        let noticed = Rc::clone(&self.0.opened);
        let notices = ctx
            .lifecycle_handle()
            .expect("a hosted window has a lifecycle")
            .subscribe_notices(move |notice| {
                if matches!(notice, flui_platform::HostNotice::SessionEnded) {
                    noticed.borrow_mut().session_ended += 1;
                }
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
            session_ended: 0,
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
        state_hold: None,
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

#[test]
fn hiding_the_window_hides_the_application_and_showing_it_resumes() {
    use flui_view::AppLifecycleState::{Hidden, Inactive, Resumed};

    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    opened.borrow_mut().lifecycle.clear();
    t.set_window_visible(false);
    assert_eq!(
        opened.borrow().lifecycle,
        [Inactive, Hidden],
        "hidden, the window steps down through Inactive"
    );
    t.set_window_visible(true);
    assert!(
        opened
            .borrow()
            .lifecycle
            .ends_with(&[Hidden, Inactive, Resumed]),
        "shown again, the window climbs back through Inactive: {:?}",
        opened.borrow().lifecycle
    );
}

/// A window that is not seen asks for no wake even while an animation runs in
/// it: the host's loop waits for input alone until the window is seen again,
/// when its frames come back. The hidden lifecycle disables frames, and no
/// host fact of an unseen window may arm a wake of its own.
#[test]
fn an_unseen_window_asks_for_no_wake_until_it_is_seen() {
    use flui_testing::{HeadlessHost, HeadlessWindow};

    let mut host = HeadlessHost::new(HeadlessWindow::new(40, 40));
    host.attach(&flui_widgets::ActivityIndicator::new())
        .expect("the indicator mounts");
    let _ = host.pump(Duration::from_millis(16));
    assert!(
        host.time_to_next_frame().is_some(),
        "a seen window keeps its animation's frames"
    );
    host.set_window_visible(false);
    let _ = host.pump(Duration::from_millis(16));
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "unseen, the loop waits for input alone"
    );
    assert!(host.pump_next_frame().is_none(), "and pumps nothing");
    host.set_window_visible(true);
    assert!(
        host.time_to_next_frame().is_some(),
        "seen again, the animation asks for its frames"
    );
}

/// A window opened beside the focused one takes the focus, as a native host
/// reports it once the window is installed: the first window steps down to
/// `Inactive`.
#[test]
fn opening_a_second_window_makes_the_first_inactive() {
    use flui_testing::{HeadlessHost, HeadlessWindow};
    use flui_view::AppLifecycleState::Inactive;

    let (view, opened) = notebook(false);
    let mut host = HeadlessHost::new(HeadlessWindow::new(100, 100));
    host.attach(&view).expect("the notebook mounts");
    let _ = host.pump(Duration::ZERO);
    opened.borrow_mut().lifecycle.clear();
    host.open_window(HeadlessWindow::new(100, 100));
    assert_eq!(opened.borrow().lifecycle, [Inactive]);
}

#[test]
fn showing_an_unfocused_window_leaves_it_unfocused() {
    use flui_view::AppLifecycleState::Inactive;

    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    t.set_window_focused(false);
    t.set_window_visible(false);
    t.set_window_visible(true);
    assert_eq!(
        opened.borrow().lifecycle.last(),
        Some(&Inactive),
        "the OS focus never came back"
    );
}

/// A trip to the background walks the lifecycle down to `Paused` in a
/// phone's order and tells the application its session may end, with the
/// window still open; the return walks it back up to `Resumed`.
///
/// Fails against a tester that only hides the window: the application stops
/// at `Hidden` and hears nothing of its session.
#[test]
fn a_trip_to_the_background_pauses_the_application_and_its_return_resumes_it() {
    use flui_view::AppLifecycleState::{Hidden, Inactive, Paused, Resumed};

    use flui_platform::{SessionEndAnswer, TargetPlatform};

    let (view, opened) = notebook(false);
    let mut t = Tester::mount_in(
        view,
        TestWindow::default().platform(TargetPlatform::Android),
    );
    opened.borrow_mut().lifecycle.clear();
    assert_eq!(t.try_foreground(), Err(TestError::InForeground));

    assert_eq!(
        t.background(),
        SessionEndAnswer::Proceed,
        "nothing in flight"
    );
    assert_eq!(opened.borrow().lifecycle, [Inactive, Hidden, Paused]);
    assert_eq!(
        opened.borrow().session_ended,
        1,
        "a phone reports its session may end on each trip to the background"
    );
    assert!(
        !t.is_closed(),
        "the window stays open for the user's return"
    );
    assert_eq!(
        t.try_background(),
        Err(TestError::InBackground),
        "a phone reports no second trip before a return"
    );
    assert_eq!(opened.borrow().session_ended, 1, "announced once a trip");

    opened.borrow_mut().lifecycle.clear();
    t.foreground();
    assert_eq!(opened.borrow().lifecycle, [Hidden, Inactive, Resumed]);
}

/// Only a phone leaves the screen this way: a window that names no phone is
/// refused, and nothing is reported to it.
#[test]
fn a_window_that_names_no_phone_has_no_trip_to_the_background() {
    use flui_platform::TargetPlatform;

    for platform in [
        None,
        Some(TargetPlatform::Windows),
        Some(TargetPlatform::Linux),
    ] {
        let (view, opened) = notebook(false);
        let mut window = TestWindow::default();
        if let Some(platform) = platform {
            window = window.platform(platform);
        }
        let mut t = Tester::mount_in(view, window);
        opened.borrow_mut().lifecycle.clear();
        assert_eq!(
            t.try_background(),
            Err(TestError::NotAPhone {
                platform: platform.unwrap_or(TargetPlatform::Unknown)
            })
        );
        assert!(opened.borrow().lifecycle.is_empty(), "{platform:?}");
    }
}

/// A phone's trip to the background with a write still in flight answers
/// `Block`, so the platform keeps the process running for it (ADR-0350 §2).
///
/// Fails against a host that answers `Proceed` to every session end.
#[test]
fn a_trip_to_the_background_with_a_write_in_flight_holds_the_process() {
    use flui_platform::{SessionEndAnswer, TargetPlatform};

    let (view, opened) = notebook(false);
    let mut t = Tester::mount_in(
        view,
        TestWindow::default().platform(TargetPlatform::Android),
    );
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    let storage = t
        .storage()
        .cloned()
        .expect("a mounted window has a storage");
    let hold = storage.hold_writes();
    note(&opened)
        .set(Note("in flight".to_owned()))
        .expect("a loaded note takes a value");
    assert_eq!(t.background(), SessionEndAnswer::Block);
    drop(hold);
}

/// Closing the application's only window retires it, as on a real host: the
/// window detaches, and the tester keeps pumping a host with nothing left to
/// frame.
///
/// Fails against a host whose loop asks the retired runtime when to wake:
/// the close's own frame panics.
#[test]
fn closing_the_only_window_retires_the_application() {
    use flui_view::AppLifecycleState::Detached;

    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    assert!(t.request_close(), "nothing holds the window open");
    assert!(t.is_closed());
    assert_eq!(opened.borrow().lifecycle.last(), Some(&Detached));
    t.pump();
    t.pump_and_settle();
}

/// The capability a widget acquires.
struct GreetingCapability;

impl flui_platform::capability::Capability for GreetingCapability {
    type Handle = &'static str;
    const NAME: &'static str = "test-greeting";
}

/// A plugin that provides [`GreetingCapability`] with its greeting.
struct Greets(&'static str);

impl flui_platform::capability::Plugin for Greets {
    fn register(&self, registrar: &mut flui_platform::capability::CapabilityRegistrar) {
        let greeting = self.0;
        registrar.provide::<GreetingCapability>(
            move |_: &flui_platform::capability::ProviderContext<'_>| Ok(greeting),
        );
    }
}

/// Shows the greeting [`GreetingCapability`] answers, or the refusal's
/// reason.
#[derive(Clone, StatefulView)]
struct Greeter;

struct GreeterState {
    shown: String,
}

impl StatefulView for Greeter {
    type State = GreeterState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        GreeterState {
            shown: match ctx.capability::<GreetingCapability>() {
                Ok(greeting) => greeting.to_owned(),
                Err(refusal) => format!("{:?}", refusal.reason),
            },
        }
    }
}

impl ViewState<Greeter> for GreeterState {
    fn build(&self, _view: &Greeter, _ctx: &dyn BuildContext) -> impl IntoView {
        Center::new().child(Text::new(self.shown.clone()))
    }
}

/// A plugin the test registers answers the application's widget, an
/// override answers in its place, and a relaunch registers both again;
/// without either the widget is refused, as in a run without them.
#[test]
fn a_plugin_the_test_registers_answers_and_an_override_replaces_it() {
    let mut t = Tester::mount(Greeter);
    t.expect(find::text("NotRegistered")).to_exist();

    let mut t = Tester::mount_in(Greeter, TestWindow::default().plugin(Greets("hello")));
    t.expect(find::text("hello")).to_exist();

    let window = TestWindow::default()
        .plugin(Greets("hello"))
        .capability::<GreetingCapability>(|_: &flui_platform::capability::ProviderContext<'_>| {
            Ok("overridden")
        });
    let mut t = Tester::mount_in(Greeter, window);
    t.expect(find::text("overridden")).to_exist();
    let mut t = t.restart();
    t.expect(find::text("overridden")).to_exist();
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

/// A kill is the process ending with nothing more run: a write the run
/// accepted and had not carried out never reaches the storage.
#[test]
fn a_relaunch_after_a_kill_loses_what_the_run_had_not_written() {
    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    note(&opened)
        .set(Note("unsaved".to_owned()))
        .expect("a loaded note takes a value");

    let mut t = t.relaunch_after_kill();
    let loaded = settle(&mut t, note(&opened).load()).expect("the empty storage loads");
    assert_ne!(*loaded, Note("unsaved".to_owned()));
}

/// A write the storage accepted but held, unstarted at a write hold or
/// uncommitted at a commit barrier, dies with the killed run: releasing the
/// hold after the relaunch stores nothing of it. Without the kill the same
/// release stores it, so each case did hold a write.
#[test]
fn a_relaunch_after_a_kill_loses_a_write_the_storage_held() {
    type Hold = fn(&flui_testing::storage::MemoryStorage) -> Box<dyn FnOnce()>;
    let holds: [(&str, Hold); 2] = [
        ("unstarted", |storage| {
            let hold = storage.hold_writes();
            Box::new(move || drop(hold))
        }),
        ("uncommitted", |storage| {
            let barrier = storage.hold_commits();
            Box::new(move || barrier.commit())
        }),
    ];
    for (case, hold) in holds {
        for kill in [false, true] {
            let (view, opened) = notebook(false);
            let mut t = Tester::mount(view);
            let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
            let storage = t
                .storage()
                .cloned()
                .expect("a mounted window has a storage");
            let release = hold(&storage);
            note(&opened)
                .set(Note("held".to_owned()))
                .expect("a loaded note takes a value");
            t.pump_for(Duration::from_secs(1));
            if kill {
                t = t.relaunch_after_kill();
            }
            release();
            t.pump_for(Duration::from_secs(1));
            let stored = storage
                .contents(&<Note as flui_view::persist::Document>::NAME)
                .is_some_and(|bytes| String::from_utf8_lossy(&bytes).contains("held"));
            assert_eq!(stored, !kill, "{case}, killed: {kill}");
        }
    }
}

/// A kill loses only the killed run's held writes: a write the test itself
/// published, and a write another run over the same storage published under
/// the same name before the killed run's, still start when the hold ends.
#[test]
fn a_relaunch_after_a_kill_keeps_a_held_write_the_run_did_not_publish() {
    use flui_platform::{Storage as _, StorageName, WriteMode};

    const OTHER: StorageName = StorageName::from_static("other");
    let (view, opened) = notebook(false);
    let mut killed = Tester::mount(view);
    let _ = settle(&mut killed, note(&opened).load()).expect("an empty storage loads");
    let storage = killed
        .storage()
        .cloned()
        .expect("a mounted window has a storage");
    let (other_view, other_opened) = notebook(false);
    let mut other = Tester::mount_in(other_view, TestWindow::default().storage(storage.clone()));
    let _ = settle(&mut other, note(&other_opened).load()).expect("an empty storage loads");

    let hold = storage.hold_writes();
    let own = storage.publish(&OTHER, b"kept".to_vec(), WriteMode::Replace);
    note(&other_opened)
        .set(Note("other run".to_owned()))
        .expect("a loaded note takes a value");
    other.pump_for(Duration::from_secs(1));
    note(&opened)
        .set(Note("killed run".to_owned()))
        .expect("a loaded note takes a value");
    killed.pump_for(Duration::from_secs(1));
    let mut killed = killed.relaunch_after_kill();
    drop(hold);
    killed.pump();
    other.pump();

    assert_eq!(storage.contents(&OTHER).as_deref(), Some(&b"kept"[..]));
    let stored = storage
        .contents(&<Note as flui_view::persist::Document>::NAME)
        .expect("the other run's note was written");
    assert!(
        String::from_utf8_lossy(&stored).contains("other run"),
        "{}",
        String::from_utf8_lossy(&stored)
    );
    drop(own);
}

/// A kill whose teardown panics still loses the run's held write: the
/// observer that fails as the window detaches does not let it through.
#[test]
fn a_relaunch_after_a_kill_whose_teardown_panics_loses_the_held_write() {
    let (view, opened) = notebook_saving(false, None, true);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    let storage = t
        .storage()
        .cloned()
        .expect("a mounted window has a storage");
    let hold = storage.hold_writes();
    note(&opened)
        .set(Note("held".to_owned()))
        .expect("a loaded note takes a value");
    t.pump_for(Duration::from_secs(1));
    let killed = catch_unwind(AssertUnwindSafe(|| t.relaunch_after_kill()));
    assert!(killed.is_err(), "the detach observer fails the teardown");
    drop(hold);
    assert_eq!(
        storage.contents(&<Note as flui_view::persist::Document>::NAME),
        None
    );
}

/// A run's newer value does not replace a value the test published that is
/// still waiting at a hold: the run never read it, so the test's value lands
/// first and the run's, based on the storage without it, conflicts.
#[test]
fn a_runs_value_does_not_replace_the_tests_held_write() {
    use flui_platform::{Storage as _, WriteMode};

    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    let storage = t
        .storage()
        .cloned()
        .expect("a mounted window has a storage");
    let name = <Note as flui_view::persist::Document>::NAME;
    let hold = storage.hold_writes();
    let own = storage.publish(&name, b"the test's".to_vec(), WriteMode::Replace);
    note(&opened)
        .set(Note("the run's".to_owned()))
        .expect("a loaded note takes a value");
    t.pump_for(Duration::from_secs(1));
    drop(hold);
    t.pump_for(Duration::from_secs(1));
    assert_eq!(storage.contents(&name).as_deref(), Some(&b"the test's"[..]));
    drop(own);
}

/// A kill loses the run's held write even when the hold belongs to the run's
/// own state and is released by its teardown: the run is dead before its
/// states are disposed. Without the kill, the state's release lands it.
#[test]
fn a_relaunch_after_a_kill_loses_a_write_its_own_state_held() {
    for kill in [false, true] {
        let storage = flui_testing::storage::MemoryStorage::new();
        let (mut view, opened) = notebook(false);
        view.state_hold = Some(storage.clone());
        let mut t = Tester::mount_in(view, TestWindow::default().storage(storage.clone()));
        let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
        note(&opened)
            .set(Note("held".to_owned()))
            .expect("a loaded note takes a value");
        t.pump_for(Duration::from_secs(1));
        assert_eq!(
            storage.contents(&<Note as flui_view::persist::Document>::NAME),
            None
        );
        if kill {
            drop(t.relaunch_after_kill());
        } else {
            drop(t);
        }
        let stored = storage.contents(&<Note as flui_view::persist::Document>::NAME);
        assert_eq!(stored.is_some(), !kill, "killed: {kill}");
    }
}

/// A value the test publishes while a run's write of the same name waits at
/// a hold does not replace it: the test never read the run's value, so the
/// run's write lands first and the test's, based on the empty storage,
/// conflicts.
#[test]
fn the_tests_value_does_not_replace_a_runs_held_write() {
    use flui_platform::{Storage as _, StoredVersion, WriteMode};

    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    let storage = t
        .storage()
        .cloned()
        .expect("a mounted window has a storage");
    let name = <Note as flui_view::persist::Document>::NAME;
    let hold = storage.hold_writes();
    note(&opened)
        .set(Note("the run's".to_owned()))
        .expect("a loaded note takes a value");
    t.pump_for(Duration::from_secs(1));
    let own = storage.publish(
        &name,
        b"the test's".to_vec(),
        WriteMode::IfUnchanged(StoredVersion::ABSENT),
    );
    drop(hold);
    t.pump_for(Duration::from_secs(1));
    let stored = storage.contents(&name).expect("the run's write landed");
    assert!(
        String::from_utf8_lossy(&stored).contains("the run's"),
        "{}",
        String::from_utf8_lossy(&stored)
    );
    drop(own);
}

/// A quit write the storage holds lands when the hold ends, ahead of the next
/// run's newer value, which was based on the storage without it and so
/// conflicts: what a disk that had not finished the quit write gives, and
/// the same whether the next run publishes before or after the release.
#[test]
fn a_restart_under_a_write_hold_lands_the_quit_write_first() {
    for set_before_release in [true, false] {
        let (view, opened) = notebook(false);
        let mut t = Tester::mount(view);
        let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
        let storage = t
            .storage()
            .cloned()
            .expect("a mounted window has a storage");
        let mut hold = Some(storage.hold_writes());
        note(&opened)
            .set(Note("quit".to_owned()))
            .expect("a loaded note takes a value");
        let mut t = t.restart();
        let _ = settle(&mut t, note(&opened).load()).expect("the storage loads");
        if !set_before_release {
            hold = None;
            t.pump_for(Duration::from_secs(1));
        }
        note(&opened)
            .set(Note("next run".to_owned()))
            .expect("a loaded note takes a value");
        t.pump_for(Duration::from_secs(1));
        drop(hold);
        t.pump_for(Duration::from_secs(1));
        let stored = storage
            .contents(&<Note as flui_view::persist::Document>::NAME)
            .expect("the quit write landed");
        assert!(
            String::from_utf8_lossy(&stored).contains("quit"),
            "set before the release: {set_before_release}: {}",
            String::from_utf8_lossy(&stored)
        );
    }
}

/// A value set twice before the restart is written as its latest: the first
/// write finishes as the run stops and the second, queued behind it, runs too.
#[test]
fn a_restart_writes_the_latest_of_two_values_set_before_it() {
    let (view, opened) = notebook(false);
    let mut t = Tester::mount(view);
    let _ = settle(&mut t, note(&opened).load()).expect("an empty storage loads");
    note(&opened)
        .set(Note("first".to_owned()))
        .expect("a loaded note takes a value");
    note(&opened)
        .set(Note("second".to_owned()))
        .expect("a loaded note takes a value");

    let mut t = t.restart();
    let loaded = settle(&mut t, note(&opened).load()).expect("the stored note loads");
    assert_eq!(*loaded, Note("second".to_owned()));
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
        flui_interaction::focus::FocusNode::with_debug_label("title"),
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

/// Locators search the whole built tree, as `flutter_test`'s `find` does: a
/// row a list keeps in its cache past its viewport is found, though the
/// window shows the spot it lies on, and a tap on it fails its `visible`
/// check, as `flutter_test` warns that it is not hit-testable.
#[test]
fn a_row_past_a_lists_viewport_is_found_and_a_tap_on_it_is_not_visible() {
    use flui_protocol::act::ActionabilityCheck;
    use flui_widgets::ListView;

    let items: Vec<_> = (0..40)
        .map(|index| Text::new(format!("Item {index}")))
        .collect();
    let list = Center::new().child(SizedBox::new(100.0, 100.0).child(ListView::new(50.0, items)));
    let mut t = Tester::mount_in(list, TestWindow::new(400, 400));
    assert!(t.is_visible(&find::text("Item 1")), "in view");
    for row in ["Item 2", "Item 4"] {
        let past = find::text(row);
        assert_eq!(t.find_all(&past).len(), 1, "{row} is built and found");
        assert!(!t.is_visible(&past), "{row} lies past the viewport's edge");
        assert!(
            matches!(
                t.try_tap(&past),
                Err(TestError::NotActionable {
                    check: ActionabilityCheck::Visible,
                    ..
                })
            ),
            "{row}: a tap on it is not visible"
        );
    }
    assert!(t.find_all(&find::text("Item 39")).is_empty(), "never built");
}

/// Paints its whole size in one colour.
#[derive(Debug)]
struct SwatchPainter(flui_painting::styling::Color);

impl flui_widgets::CustomPainter for SwatchPainter {
    fn paint(&self, canvas: &mut flui_painting::Canvas, size: flui_foundation::geometry::Size) {
        canvas.draw_rect(
            flui_foundation::geometry::Rect::from_origin_size(
                flui_foundation::geometry::Point::ZERO,
                size,
            ),
            &flui_painting::paint::Paint::fill(self.0),
        );
    }
    fn should_repaint(&self, _old: &dyn flui_widgets::CustomPainter) -> bool {
        true
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `child` inside a group `find::id(identifier)` locates.
fn named(identifier: &str, child: impl IntoView) -> impl IntoView {
    flui_widgets::Semantics::new()
        .container(true)
        .role(flui_widgets::SemanticsRole::Group)
        .identifier(identifier)
        .child(child)
}

/// `draw_commands` reads the scene the window submitted last, after the
/// frames due now: a change from outside the tree repaints the swatch, and
/// the next read sees the new colour with no pump between.
///
/// Fails if the read samples the scene before the frames due now.
#[test]
fn draw_commands_read_the_last_submitted_scene() {
    use flui_painting::styling::Color;

    let changed = Rc::new(flui_foundation::ChangeNotifier::new());
    let shade = Rc::new(Cell::new(0_u8));
    let read = Rc::clone(&shade);
    let mut t = Tester::mount(flui_widgets::AnimatedBuilder::new(
        Rc::clone(&changed) as Rc<dyn flui_foundation::Listenable>,
        move || {
            Center::new().child(named(
                "swatch",
                flui_widgets::CustomPaint::new()
                    .size(flui_foundation::geometry::Size::new(24.0, 16.0))
                    .painter(SwatchPainter(Color::rgb(read.get(), 0, 0))),
            ))
        },
    ));
    let rectangles = |t: &mut Tester| -> Vec<(flui_foundation::geometry::Rect, Color)> {
        t.draw_commands(&find::id("swatch"))
            .into_iter()
            .filter_map(|command| match command.op {
                flui_painting::DrawOp::Rect { rect, paint } => Some((rect, paint.color)),
                _ => None,
            })
            .collect()
    };
    let swatch = flui_foundation::geometry::Rect::from_origin_size(
        flui_foundation::geometry::Point::ZERO,
        flui_foundation::geometry::Size::new(24.0, 16.0),
    );
    assert_eq!(rectangles(&mut t), [(swatch, Color::rgb(0, 0, 0))]);
    shade.set(100);
    changed.notify_listeners();
    assert_eq!(rectangles(&mut t), [(swatch, Color::rgb(100, 0, 0))]);
}

/// `draw_commands` lists the commands in paint order and places each in the
/// window, across repaint boundaries: a row of a boundary, a plain painter
/// and a boundary reads left to right, each swatch at its slot in the row.
///
/// Fails if the read walks the scene's layers in insertion order, where a
/// boundary's content comes after its parent's and sibling boundaries come
/// reversed, or leaves out the offsets of the layers above a command.
#[test]
fn draw_commands_follow_paint_order_across_repaint_boundaries() {
    use flui_foundation::geometry::{Matrix4, Point, Rect, Size};
    use flui_painting::styling::Color;

    let swatch = |shade: u8| {
        flui_widgets::CustomPaint::new()
            .size(Size::new(10.0, 10.0))
            .painter(SwatchPainter(Color::rgb(shade, 0, 0)))
    };
    let mut t = Tester::mount(Center::new().child(named(
        "row",
        flui_widgets::Row::new(vec![
        flui_widgets::RepaintBoundary::new().child(swatch(1)).into_view().boxed(),
        swatch(2).into_view().boxed(),
        flui_widgets::RepaintBoundary::new().child(swatch(3)).into_view().boxed(),
        ]),
    )));
    let painted: Vec<(Rect, Color, Matrix4)> = t
        .draw_commands(&find::id("row"))
        .into_iter()
        .filter_map(|command| match command.op {
            flui_painting::DrawOp::Rect { rect, paint } => {
                Some((rect, paint.color, command.transform))
            }
            _ => None,
        })
        .collect();
    let local = Rect::from_origin_size(Point::ZERO, Size::new(10.0, 10.0));
    // The row fills the window's width and centres its 10 px height.
    let top = (600.0 - 10.0) / 2.0;
    assert_eq!(
        painted,
        [
            (
                local,
                Color::rgb(1, 0, 0),
                Matrix4::translation(0.0, top, 0.0)
            ),
            (
                local,
                Color::rgb(2, 0, 0),
                Matrix4::translation(10.0, top, 0.0)
            ),
            (
                local,
                Color::rgb(3, 0, 0),
                Matrix4::translation(20.0, top, 0.0)
            ),
        ]
    );
}

/// `draw_commands` places a command under a transform layer with the whole
/// matrix, composed in order with the canvas transform it was recorded
/// under: a swatch padded by 5 inside a repaint boundary, under a twofold
/// scale, lands scaled at the scaled padding.
///
/// Fails if the read applies only a transform layer's translation, or
/// composes a layer's transform or a command's on the wrong side.
#[test]
fn draw_commands_place_a_command_through_a_transform_layer() {
    use flui_foundation::geometry::{Matrix4, Size};

    let mut t = Tester::mount(
        Center::new().child(
            flui_widgets::Transform::new(Matrix4::scaling(2.0, 2.0, 1.0)).child(
                flui_widgets::RepaintBoundary::new().child(named(
                    "swatch",
                    flui_widgets::Padding::all(5.0).child(
                        flui_widgets::CustomPaint::new()
                            .size(Size::new(10.0, 10.0))
                            .painter(SwatchPainter(flui_painting::styling::Color::rgb(1, 0, 0))),
                    ),
                )),
            ),
        ),
    );
    let transforms: Vec<Matrix4> = t
        .draw_commands(&find::id("swatch"))
        .into_iter()
        .filter(|command| matches!(command.op, flui_painting::DrawOp::Rect { .. }))
        .map(|command| command.transform)
        .collect();
    // The 20 px box sits centred at (390, 290); the scale about its origin
    // doubles the 5 px padding.
    let placed = Matrix4::translation(390.0, 290.0, 0.0)
        * Matrix4::scaling(2.0, 2.0, 1.0)
        * Matrix4::translation(5.0, 5.0, 0.0);
    assert_eq!(transforms, [placed]);
}

/// An element with no semantics of its own is found by the identifier on a
/// group around it, the form `find`'s documentation and the failure below
/// give.
#[test]
fn an_element_is_found_by_the_identifier_on_a_group_around_it() {
    let mut t = Tester::mount(
        Center::new().child(
            flui_widgets::Semantics::new()
                .container(true)
                .role(flui_widgets::SemanticsRole::Group)
                .identifier("swatch")
                .child(SizedBox::new(24.0, 16.0)),
        ),
    );
    let rect = t.rect(&find::id("swatch"));
    assert_eq!((rect.width, rect.height), (24.0, 16.0));
}

/// An identifier on a node that publishes nothing else selects nothing, and
/// the failure says so and gives the form that does: beside other content,
/// and inside a button, which keeps the name its label gives it.
///
/// Fails if the failure is the plain `NotFound`, which tells the author to
/// do what they already did.
#[test]
fn an_identifier_on_a_node_that_publishes_nothing_else_fails_saying_so() {
    let unpublished = |identifier: &str| {
        flui_widgets::Semantics::new()
            .container(true)
            .identifier(identifier)
    };
    let mut t = Tester::mount(
        Center::new().child(Column::new((
            unpublished("swatch").child(SizedBox::new(24.0, 16.0)),
            RawButton::new(
                unpublished("save").child(
                    flui_widgets::Semantics::new()
                        .container(true)
                        .label("Save")
                        .child(SizedBox::new(40.0, 20.0)),
                ),
            )
            .on_pressed(|_| {}),
            // A published group and a lifted node share "legend".
            flui_widgets::Semantics::new()
                .container(true)
                .role(flui_widgets::SemanticsRole::Group)
                .identifier("legend")
                .child(SizedBox::new(24.0, 16.0)),
            unpublished("legend").child(SizedBox::new(24.0, 16.0)),
            // "save" lifted outside the button too, which the read reports
            // first: the fix still names the button, the control it is in.
            unpublished("save").child(SizedBox::new(24.0, 16.0)),
        ))),
    );
    let _ = t.rect(&find::button("Save"));
    // The identifier blamed, and the message.
    let failure = |t: &mut Tester, locator: Locator| {
        let error = t
            .try_rect(&locator)
            .expect_err("an identifier on a lifted node selects nothing");
        let message = error.to_string();
        let TestError::UnpublishedIdentifier { identifier, .. } = error else {
            panic!("{locator:?}: {message}");
        };
        (identifier, message)
    };
    let (_, swatch) = failure(&mut t, find::id("swatch"));
    assert!(
        swatch.contains("publishes nothing of its own")
            && swatch.contains(".role(SemanticsRole::Group)"),
        "{swatch}"
    );
    // Inside a control a group would cut the name its label gives it, so
    // the fix names the control instead.
    let (_, save) = failure(&mut t, find::id("save"));
    assert!(
        save.contains("Give the identifier to the control")
            && !save.contains(".role(SemanticsRole::Group)"),
        "{save}"
    );
    // A scope names the identifier as much as the target does.
    let (scoped, _) = failure(&mut t, find::button("Save").within(find::id("swatch")));
    assert_eq!(scoped, "swatch");
    // An expectation that gives up says the same.
    let expected = panic_text(|| t.expect(find::id("swatch")).to_exist());
    assert!(
        expected.contains("publishes nothing of its own"),
        "{expected}"
    );
    // The reason only explains a failure: an expectation that holds by
    // matching nothing passes.
    t.expect(find::id("swatch")).to_be_gone();
    // The target's own identifier is the likely reason even where a
    // published node carries it too.
    let (own, _) = failure(&mut t, find::id("legend").within(find::button("Save")));
    assert_eq!(own, "legend");
    // An identifier a published node carries selects it, and a failure
    // there is the expectation's own, not the lifted node's.
    let _ = t.rect(&find::id("legend"));
    let present = panic_text(|| t.expect(find::id("legend")).to_be_gone());
    assert!(
        present.contains("0 matches") && !present.contains("publishes nothing of its own"),
        "{present}"
    );
    // A scope a published node satisfies is not the reason a lookup misses.
    assert!(
        matches!(
            t.try_rect(&find::button("Nope").within(find::id("legend"))),
            Err(TestError::NotFound { .. })
        ),
        "the published legend scopes the lookup"
    );
    assert!(
        matches!(
            t.try_rect(&find::id("absent")),
            Err(TestError::NotFound { .. })
        ),
        "an identifier nothing carries is not found"
    );
}

/// Paints a swatch over its own bounds and a mark past their right edge.
#[derive(Debug)]
struct OverhangPainter(flui_painting::styling::Color);

impl flui_widgets::CustomPainter for OverhangPainter {
    fn paint(&self, canvas: &mut flui_painting::Canvas, size: flui_foundation::geometry::Size) {
        use flui_foundation::geometry::{Point, Rect, Size};
        let paint = flui_painting::paint::Paint::fill(self.0);
        canvas.draw_rect(Rect::from_origin_size(Point::ZERO, size), &paint);
        canvas.draw_rect(
            Rect::from_origin_size(Point::new(size.width, 0.0), Size::new(4.0, 4.0)),
            &paint,
        );
    }
    fn should_repaint(&self, _old: &dyn flui_widgets::CustomPainter) -> bool {
        true
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `draw_commands` reads one element's drawing: of two painted siblings,
/// each read finds its own painter's commands, the mark one draws past its
/// bounds over the other included, and none of the other's.
///
/// Fails if the read selects by the element's bounds, or returns the whole
/// scene.
#[test]
fn draw_commands_read_one_element_and_none_of_its_sibling() {
    use flui_foundation::geometry::Size;
    use flui_painting::styling::Color;

    let painter = |shade: u8| {
        flui_widgets::CustomPaint::new()
            .size(Size::new(10.0, 10.0))
            .painter(OverhangPainter(Color::rgb(shade, 0, 0)))
    };
    let mut t = Tester::mount(Center::new().child(flui_widgets::Row::new(vec![
        named("left", painter(1)).into_view().boxed(),
        named("right", painter(2)).into_view().boxed(),
    ])));
    let colours = |t: &mut Tester, identifier: &str| -> Vec<Color> {
        t.draw_commands(&find::id(identifier))
            .into_iter()
            .filter_map(|command| match command.op {
                flui_painting::DrawOp::Rect { paint, .. } => Some(paint.color),
                _ => None,
            })
            .collect()
    };
    let left = Color::rgb(1, 0, 0);
    let right = Color::rgb(2, 0, 0);
    assert_eq!(
        colours(&mut t, "left"),
        [left, left],
        "the overhang is the left painter's"
    );
    assert_eq!(colours(&mut t, "right"), [right, right]);
}

/// `draw_commands` reads a subtree larger than one inspection lists whole,
/// in several inspections: six columns of a thousand render objects, each
/// ending in a swatch, give all six swatches in paint order.
///
/// Fails if the read stops at the first inspection's 4096 nodes, or if it
/// pages only from the node an inspection cut, which never reaches the
/// columns after the one the budget ran out in.
#[test]
fn draw_commands_read_a_subtree_past_one_inspection_whole() {
    use flui_painting::styling::Color;

    let column = |shade: u8| {
        let mut children: Vec<_> = (0..1000)
            .map(|_| SizedBox::new(1.0, 0.0).into_view().boxed())
            .collect();
        children.push(
            flui_widgets::CustomPaint::new()
                .size(flui_foundation::geometry::Size::new(1.0, 1.0))
                .painter(SwatchPainter(Color::rgb(shade, 0, 0)))
                .into_view()
                .boxed(),
        );
        flui_widgets::Column::new(children).into_view().boxed()
    };
    let columns: Vec<_> = (1..=6).map(column).collect();
    let mut t = Tester::mount(Center::new().child(named("many", flui_widgets::Row::new(columns))));
    let colours: Vec<Color> = t
        .draw_commands(&find::id("many"))
        .into_iter()
        .filter_map(|command| match command.op {
            flui_painting::DrawOp::Rect { paint, .. } => Some(paint.color),
            _ => None,
        })
        .collect();
    let expected: Vec<Color> = (1..=6).map(|shade| Color::rgb(shade, 0, 0)).collect();
    assert_eq!(colours, expected);
}

/// `draw_commands` refuses an element under which one render object has
/// more children than one inspection lists, rather than returning part of
/// its drawing.
///
/// Fails if the read returns the commands of the listed children.
#[test]
fn draw_commands_refuse_a_render_object_with_more_children_than_one_inspection() {
    let boxes: Vec<_> = (0..4200)
        .map(|_| SizedBox::new(1.0, 0.0).into_view().boxed())
        .collect();
    let mut t = Tester::mount(Center::new().child(named("many", flui_widgets::Column::new(boxes))));
    let refused = t.try_draw_commands(&find::id("many"));
    assert!(
        matches!(refused, Err(TestError::TooManyChildren { .. })),
        "{refused:?}"
    );
}

/// A painter's region of its own semantics, with a stable identifier.
#[derive(Debug)]
struct RegionPainter;

impl flui_widgets::CustomPainter for RegionPainter {
    fn paint(&self, canvas: &mut flui_painting::Canvas, size: flui_foundation::geometry::Size) {
        canvas.draw_rect(
            flui_foundation::geometry::Rect::from_origin_size(
                flui_foundation::geometry::Point::ZERO,
                size,
            ),
            &flui_painting::paint::Paint::fill(flui_painting::styling::Color::rgb(1, 0, 0)),
        );
    }
    fn should_repaint(&self, _old: &dyn flui_widgets::CustomPainter) -> bool {
        false
    }
    fn semantics_regions(
        &self,
        size: flui_foundation::geometry::Size,
    ) -> Vec<flui_semantics::SemanticsRegion> {
        let mut config = flui_semantics::SemanticsConfiguration::new();
        config.set_label("dot");
        config.set_identifier("dot");
        vec![flui_semantics::SemanticsRegion::new(
            flui_semantics::SemanticsRegionKey(1),
            flui_foundation::geometry::Rect::from_origin_size(
                flui_foundation::geometry::Point::ZERO,
                size,
            ),
            config,
        )]
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `draw_commands` refuses a painter's authored region, which no render
/// object forms: its drawing is the painter's, located by the painter.
///
/// Fails if the read decodes the region's identity as a render object's.
#[test]
fn draw_commands_refuse_an_authored_region() {
    let mut t = Tester::mount(
        Center::new().child(
            flui_widgets::CustomPaint::new()
                .size(flui_foundation::geometry::Size::new(10.0, 10.0))
                .painter(RegionPainter),
        ),
    );
    let refused = t.try_draw_commands(&find::id("dot"));
    assert!(
        matches!(refused, Err(TestError::NoRenderObject { .. })),
        "{refused:?}"
    );
}
