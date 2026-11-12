//! `Tester` drives an application through its window's agent: it finds by
//! locator, acts through the input lease, and fails with what the window shows.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

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
                .on_press(move |cx| count.update(cx, |n| *n += 1)),
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
        RawButton::new(Text::new("Same")).on_press(|_| {}),
        RawButton::new(Text::new("Same")).on_press(|_| {}),
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
            .child(RawButton::new(Text::new("Inside")).on_press(|_| {})),
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
