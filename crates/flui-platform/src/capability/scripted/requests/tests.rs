//! Every transition of a script's held requests as a row: the requests in
//! one state, one event, the state they are left in and the effect handed
//! back. The rules the script carries out around them (what a release
//! counts, how the test's panics are kept) are pinned by the scenario tests
//! `crates/flui-platform/ARCHITECTURE.md` names.

use super::{Effect, Event, Pending, Waits};

/// What a row's request waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitsFor {
    Hold,
    Test,
    Clock,
    Answering,
}

/// The requests in a row, each a key and what it waits for, oldest first,
/// with the live holds and whether a drain runs.
#[derive(Debug, PartialEq, Eq)]
struct RequestsState {
    requests: Vec<(u32, WaitsFor)>,
    holds: usize,
    draining: bool,
}

fn state(requests: &[(u32, WaitsFor)], holds: usize, draining: bool) -> RequestsState {
    RequestsState {
        requests: requests.to_vec(),
        holds,
        draining,
    }
}

/// The requests a row runs on: a request's payload is its key, and a clock's
/// answer is `()`.
type RowRequests = Pending<u32, u32, ()>;

fn waits(waits_for: WaitsFor) -> Waits<()> {
    match waits_for {
        WaitsFor::Hold => Waits::Hold,
        WaitsFor::Test => Waits::Test,
        WaitsFor::Clock => Waits::Clock(()),
        WaitsFor::Answering => Waits::Answering,
    }
}

fn build(state: &RequestsState) -> RowRequests {
    let mut requests = RowRequests::default();
    for &(key, waits_for) in &state.requests {
        requests.push(key, waits(waits_for), key);
    }
    requests.holds = state.holds;
    requests.draining = state.draining;
    requests
}

fn snapshot(requests: &RowRequests) -> RequestsState {
    let entries: Vec<(u32, WaitsFor)> = requests
        .entries
        .iter()
        .map(|entry| {
            assert_eq!(entry.key, entry.payload, "a row's payload is its key");
            let waits_for = match entry.waits {
                Waits::Hold => WaitsFor::Hold,
                Waits::Test => WaitsFor::Test,
                Waits::Clock(()) => WaitsFor::Clock,
                Waits::Answering => WaitsFor::Answering,
            };
            (entry.key, waits_for)
        })
        .collect::<Vec<(u32, WaitsFor)>>();
    RequestsState {
        requests: entries,
        holds: requests.holds,
        draining: requests.draining,
    }
}

/// One rule: `event` on `before` leaves `after` and hands back `effect`.
struct Transition {
    rule: &'static str,
    before: RequestsState,
    event: Event<u32, u32, ()>,
    after: RequestsState,
    effect: Effect<u32, u32, ()>,
}

fn transition(
    rule: &'static str,
    before: RequestsState,
    event: Event<u32, u32, ()>,
    after: RequestsState,
    effect: Effect<u32, u32, ()>,
) -> Transition {
    Transition {
        rule,
        before,
        event,
        after,
        effect,
    }
}

#[test]
fn every_event_moves_the_unanswered_requests_as_the_rules_say() {
    use WaitsFor::{Answering, Clock, Hold, Test};
    let transitions = vec![
        transition(
            "a request with no hold and no drain is answered, pending meanwhile",
            state(&[(1, Test)], 0, false),
            Event::Arrived(5, 5),
            state(&[(1, Test), (5, Answering)], 0, false),
            Effect::SendOn(5),
        ),
        transition(
            "a request that arrives while a hold lives is kept, last",
            state(&[(1, Hold)], 1, false),
            Event::Arrived(5, 5),
            state(&[(1, Hold), (5, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a request that arrives during a drain queues behind the kept ones",
            state(&[(1, Answering), (2, Hold)], 0, true),
            Event::Arrived(5, 5),
            state(&[(1, Answering), (2, Hold), (5, Hold)], 0, true),
            Effect::None,
        ),
        transition(
            "a request a step holds, or whose answer panicked, waits for the test where it is",
            state(&[(4, Answering), (5, Hold)], 1, false),
            Event::KeepForTest(4),
            state(&[(4, Test), (5, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a delayed answer waits for the clock where the request is",
            state(&[(4, Answering), (5, Hold)], 1, false),
            Event::Schedule(4, ()),
            state(&[(4, Clock), (5, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a request a ticket withdrew while its answer was made is not kept for the test",
            state(&[(5, Hold)], 1, false),
            Event::KeepForTest(4),
            state(&[(5, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "an answer scheduled for a request a ticket withdrew meanwhile is handed back",
            state(&[(5, Hold)], 1, false),
            Event::Schedule(4, ()),
            state(&[(5, Hold)], 1, false),
            Effect::Orphaned(4, ()),
        ),
        transition(
            "a request whose answer is made leaves the pending requests to be answered",
            state(&[(4, Answering), (5, Hold)], 1, false),
            Event::Answered(4),
            state(&[(5, Hold)], 1, false),
            Effect::Taken(4, 4, Waits::Answering),
        ),
        transition(
            "an answer made for a request no longer being answered takes nothing",
            state(&[(4, Test)], 0, false),
            Event::Answered(4),
            state(&[(4, Test)], 0, false),
            Effect::None,
        ),
        transition(
            "a hold taken is counted",
            state(&[], 1, false),
            Event::HoldTaken,
            state(&[], 2, false),
            Effect::None,
        ),
        transition(
            "the last hold dropped starts the drain",
            state(&[(1, Test), (2, Hold)], 1, false),
            Event::HoldDropped,
            state(&[(1, Test), (2, Hold)], 0, true),
            Effect::Drain,
        ),
        transition(
            "a hold dropped while another lives sends nothing on",
            state(&[(2, Hold)], 2, false),
            Event::HoldDropped,
            state(&[(2, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a hold dropped by the code a drain runs leaves the rest to that drain",
            state(&[(1, Answering), (2, Hold)], 1, true),
            Event::HoldDropped,
            state(&[(1, Answering), (2, Hold)], 0, true),
            Effect::None,
        ),
        transition(
            "the last hold left leaves the kept requests to the test",
            state(&[(1, Test), (2, Hold), (3, Clock)], 1, false),
            Event::HoldLeft,
            state(&[(1, Test), (2, Test), (3, Clock)], 0, false),
            Effect::None,
        ),
        transition(
            "a hold left while another lives leaves the kept requests to it",
            state(&[(2, Hold)], 2, false),
            Event::HoldLeft,
            state(&[(2, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a hold left by the code a drain runs leaves the rest to that drain",
            state(&[(1, Answering), (2, Hold), (3, Hold)], 1, true),
            Event::HoldLeft,
            state(&[(1, Answering), (2, Hold), (3, Hold)], 0, true),
            Effect::None,
        ),
        transition(
            "a drain sends on the oldest kept request, past one the test holds",
            state(&[(1, Test), (2, Hold), (3, Hold)], 0, true),
            Event::DrainNext,
            state(&[(1, Test), (2, Answering), (3, Hold)], 0, true),
            Effect::SendOn(2),
        ),
        transition(
            "a hold taken during a drain stops it, the rest kept in order",
            state(&[(2, Hold), (3, Hold)], 1, true),
            Event::DrainNext,
            state(&[(2, Hold), (3, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a drain with nothing kept left ends",
            state(&[(1, Test), (3, Clock)], 0, true),
            Event::DrainNext,
            state(&[(1, Test), (3, Clock)], 0, false),
            Effect::None,
        ),
        transition(
            "no drain runs, so there is nothing next",
            state(&[(2, Hold)], 1, false),
            Event::DrainNext,
            state(&[(2, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "what verify reported waits for the test, so no later drain answers it",
            state(
                &[(1, Test), (2, Hold), (3, Clock), (4, Answering)],
                1,
                false,
            ),
            Event::Reported,
            state(
                &[(1, Test), (2, Test), (3, Clock), (4, Answering)],
                1,
                false,
            ),
            Effect::None,
        ),
        transition(
            "a ticket withdraws a kept request",
            state(&[(1, Test), (2, Hold)], 1, false),
            Event::Withdraw(2),
            state(&[(1, Test)], 1, false),
            Effect::Taken(2, 2, Waits::Hold),
        ),
        transition(
            "a ticket withdraws a request while its answer is being made",
            state(&[(1, Answering), (2, Hold)], 0, true),
            Event::Withdraw(1),
            state(&[(2, Hold)], 0, true),
            Effect::Taken(1, 1, Waits::Answering),
        ),
        transition(
            "a ticket withdraws a request waiting for the clock",
            state(&[(3, Clock)], 0, false),
            Event::Withdraw(3),
            state(&[], 0, false),
            Effect::Taken(3, 3, Waits::Clock(())),
        ),
        transition(
            "a ticket of a request no longer waiting withdraws nothing",
            state(&[(1, Test)], 0, false),
            Event::Withdraw(9),
            state(&[(1, Test)], 0, false),
            Effect::None,
        ),
        transition(
            "release_oldest takes the oldest held request, past the clock's and one being answered",
            state(
                &[(3, Clock), (4, Answering), (2, Hold), (1, Test)],
                1,
                false,
            ),
            Event::ReleaseOldest,
            state(&[(3, Clock), (4, Answering), (1, Test)], 1, false),
            Effect::Taken(2, 2, Waits::Hold),
        ),
        transition(
            "release_held takes a request the test holds",
            state(&[(1, Test), (2, Hold)], 1, false),
            Event::Release(1),
            state(&[(2, Hold)], 1, false),
            Effect::Taken(1, 1, Waits::Test),
        ),
        transition(
            "release_held does not take a request waiting for the clock",
            state(&[(3, Clock)], 0, false),
            Event::Release(3),
            state(&[(3, Clock)], 0, false),
            Effect::None,
        ),
        transition(
            "a request whose release answer panicked waits for the test, where it is",
            state(&[(1, Test), (2, Hold), (3, Hold)], 1, false),
            Event::AnswerPanicked(2),
            state(&[(1, Test), (2, Test), (3, Hold)], 1, false),
            Effect::None,
        ),
        transition(
            "a release answer that panicked leaves a request waiting for the clock as it is",
            state(&[(3, Clock), (4, Answering)], 0, false),
            Event::AnswerPanicked(3),
            state(&[(3, Clock), (4, Answering)], 0, false),
            Effect::None,
        ),
        transition(
            "a release answer that panicked leaves a request being answered as it is",
            state(&[(3, Clock), (4, Answering)], 0, false),
            Event::AnswerPanicked(4),
            state(&[(3, Clock), (4, Answering)], 0, false),
            Effect::None,
        ),
        transition(
            "the clock hands out the answer scheduled for its request, which waits meanwhile",
            state(&[(1, Hold), (3, Clock), (4, Hold)], 1, false),
            Event::ClockReached(3),
            state(&[(1, Hold), (3, Answering), (4, Hold)], 1, false),
            Effect::Due(3, ()),
        ),
        transition(
            "the clock hands out nothing for a held request",
            state(&[(1, Hold)], 1, false),
            Event::ClockReached(1),
            state(&[(1, Hold)], 1, false),
            Effect::None,
        ),
    ];
    let mut failed = Vec::new();
    for Transition {
        rule,
        before,
        event,
        after,
        effect,
    } in transitions
    {
        let mut requests = build(&before);
        let seen = requests.apply(event);
        let left = snapshot(&requests);
        if seen != effect || left != after {
            failed.push(format!(
                "{rule}: effect {seen:?} (expected {effect:?}), state {left:?} (expected {after:?})"
            ));
        }
    }
    assert!(failed.is_empty(), "rows that fail:\n{}", failed.join("\n"));
}
