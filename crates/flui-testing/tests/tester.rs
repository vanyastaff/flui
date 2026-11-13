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
