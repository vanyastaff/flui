//! An agent acts on a window through its development-agent handle (ADR-0095 §3): a
//! `semantics.act` whose target is a locator is resolved in the owner turn that runs the
//! action, on the tree the action starts on.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_protocol::method::{SemanticsAct, SemanticsLocate};
use flui_protocol::{
    ActOutcome, ActParams, ActionName, ElementRef, ErrorCode, ErrorObject, HandleKind,
    LocateParams, Locator, LogicalPoint, Matches, Pick, Retry, Target, WindowId,
};

use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::Signal;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook};
use flui_view::prelude::*;
use flui_widgets::{Center, Column, RawButton, Text};
use parking_lot::Mutex;

/// A button named "Go" that moves under a new parent each time it is pressed, so every press
/// commits a frame in which the button is a new element; and two buttons named "Same".
#[derive(Clone, StatefulView)]
struct Moving {
    presses: Arc<AtomicU32>,
}

struct MovingState {
    moved: Signal<u32>,
}

impl StatefulView for Moving {
    type State = MovingState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        MovingState {
            moved: ctx.signal(0),
        }
    }
}

impl ViewState<Moving> for MovingState {
    fn build(&self, view: &Moving, ctx: &dyn BuildContext) -> impl IntoView {
        let moved = self.moved;
        let presses = Arc::clone(&view.presses);
        let go = RawButton::new(Text::new("Go")).on_press(move |cx| {
            presses.fetch_add(1, Ordering::SeqCst);
            moved.update(cx, |n| *n += 1)
        });
        let go = if moved.get(ctx).is_multiple_of(2) {
            go.into_view().boxed()
        } else {
            Center::new().child(go).into_view().boxed()
        };
        Column::new((
            go,
            RawButton::new(Text::new("Same")).on_press(|_| {}),
            RawButton::new(Text::new("Same")).on_press(|_| {}),
        ))
    }
}

/// Keeps every window the host hands it, as a tool serving agents does.
struct Windows(Arc<Mutex<Vec<AgentWindow>>>);

impl DevAgentHook for Windows {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.0.lock().push(window);
    }
}

/// The answer the next frame's drain gives.
#[expect(
    clippy::result_large_err,
    reason = "the error object is the answer under test"
)]
fn answer<T>(host: &mut HeadlessHost, mut answer: AgentAnswer<T>) -> Result<T, ErrorObject> {
    let _ = host.pump(Duration::ZERO);
    answer
        .try_take()
        .expect("the drain answers every call it serves")
}

#[expect(
    clippy::result_large_err,
    reason = "the error object is the answer under test"
)]
fn act(
    host: &mut HeadlessHost,
    window: &AgentWindow,
    target: Target,
) -> Result<ActOutcome, ErrorObject> {
    let call = window
        .call::<SemanticsAct>(ActParams::new(target, ActionName::Invoke))
        .expect("the inbox has room");
    answer(host, call)
}

fn locate(host: &mut HeadlessHost, window: &AgentWindow, locator: Locator) -> Matches {
    let call = window
        .call::<SemanticsLocate>(LocateParams::new(locator))
        .expect("the inbox has room");
    answer(host, call).expect("the committed tree resolves the locator")
}

/// The view shown in a host whose development agent holds its window, with the first
/// semantics tree committed.
fn shown() -> (HeadlessHost, HeadlessDevAgent, AgentWindow, Arc<AtomicU32>) {
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200)).with_dev_agent(&mut agent);
    let presses = Arc::new(AtomicU32::new(0));
    host.attach(&Moving {
        presses: Arc::clone(&presses),
    })
    .expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");
    (host, agent, window, presses)
}

/// A locate answered before a frame names an element that frame replaced: acting on that
/// handle answers `gone`, while acting through the locator acts on the element it matches on
/// the tree the action starts on.
#[test]
fn an_act_through_a_locator_acts_on_the_element_it_matches_when_it_runs() {
    let (mut host, _agent, window, presses) = shown();
    let go = Locator::named("Go");
    let before = locate(&mut host, &window, go.clone()).matches[0].element;

    let outcome = act(&mut host, &window, Target::Element(before)).expect("the press runs");
    assert_eq!(outcome.element, Some(before));
    let _ = host.pump(Duration::ZERO);
    let after = locate(&mut host, &window, go.clone()).matches[0].element;
    assert_ne!(after, before, "the press moved the button: a new element");

    let stale = act(&mut host, &window, Target::Element(before)).expect_err("replaced");
    assert_eq!(stale.code, ErrorCode::Gone, "{stale:?}");
    assert_eq!(stale.kind, Some(HandleKind::Element), "{stale:?}");
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "the stale handle ran nothing"
    );

    let outcome = act(&mut host, &window, Target::Locator(go)).expect("the locator resolves");
    assert_eq!(
        outcome.element,
        Some(after),
        "the action ran on the element the locator matches now"
    );
    assert_eq!(presses.load(Ordering::SeqCst), 2);
}

/// What a locator that resolves to no single element answers, with nothing run.
#[test]
fn an_act_through_a_locator_that_matches_no_single_element_runs_nothing() {
    let (mut host, _agent, window, presses) = shown();

    let none = act(&mut host, &window, Target::Locator(Locator::named("Stop")))
        .expect_err("nothing is named Stop");
    assert_eq!(none.code, ErrorCode::NotFound, "{none:?}");
    assert_eq!(none.retry, Retry::WhenAppears, "{none:?}");

    let several = act(&mut host, &window, Target::Locator(Locator::named("Same")))
        .expect_err("a strict pick of two");
    assert_eq!(several.code, ErrorCode::Ambiguous, "{several:?}");
    assert_eq!(several.candidates.len(), 2, "{several:?}");
    assert!(
        several
            .candidates
            .iter()
            .all(|outline| outline.contains("Same")),
        "each candidate is outlined: {several:?}"
    );

    let first = act(
        &mut host,
        &window,
        Target::Locator(Locator::named("Same").pick(Pick::First)),
    )
    .expect("a first pick resolves");
    assert_eq!(
        first.element,
        Some(locate(&mut host, &window, Locator::named("Same")).matches[0].element)
    );

    let point = act(
        &mut host,
        &window,
        Target::Point {
            window: window.id(),
            at: LogicalPoint::new(1.0, 1.0),
        },
    )
    .expect_err("a semantics action has no position");
    assert_eq!(point.code, ErrorCode::NotSupported, "{point:?}");
    assert_eq!(
        point.capability.as_deref(),
        Some("input.inject"),
        "{point:?}"
    );
    assert_eq!(presses.load(Ordering::SeqCst), 0, "no press ran");
}

/// Params that address another window are refused before they reach the owner.
#[test]
fn a_call_that_addresses_another_window_is_refused_where_it_is_made() {
    let (mut host, _agent, window, presses) = shown();
    let element = locate(&mut host, &window, Locator::named("Go")).matches[0].element;
    let other = WindowId::from_u64(window.id().get() + 1).expect("non-zero");

    let refused = window
        .call::<SemanticsAct>(ActParams::new(
            ElementRef::new(other, element.element),
            ActionName::Invoke,
        ))
        .expect_err("another window's element");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");
    let refused = window
        .call::<SemanticsLocate>(LocateParams::new(Locator::named("Go").in_window(other)))
        .expect_err("another window's locator");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        0,
        "nothing reached the owner"
    );
}
