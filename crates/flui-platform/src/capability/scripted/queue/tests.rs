//! Every documented rule of a script's held requests as a row: a queue in
//! one state, one event, the state it leaves and the effect it hands back.

use super::{Effect, Event, Queue, Waits};

/// What a row's request waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum W {
    Hold,
    Test,
    Clock,
}

/// A queue's state: its requests (key, what each waits for), its holds and
/// whether a drain runs.
type State = (Vec<(u32, W)>, usize, bool);

type Q = Queue<u32, u32, ()>;

fn waits(w: W) -> Waits<()> {
    match w {
        W::Hold => Waits::Hold,
        W::Test => Waits::Test,
        W::Clock => Waits::Clock(()),
    }
}

fn build((entries, holds, draining): &State) -> Q {
    let mut queue = Q::default();
    for &(key, w) in entries {
        queue.push(key, waits(w), key);
    }
    queue.holds = *holds;
    queue.draining = *draining;
    queue
}

fn snapshot(queue: &Q) -> State {
    let entries = queue
        .entries
        .iter()
        .map(|entry| {
            assert_eq!(entry.key, entry.payload, "a row's payload is its key");
            let w = match entry.waits {
                Waits::Hold => W::Hold,
                Waits::Test => W::Test,
                Waits::Clock(()) => W::Clock,
            };
            (entry.key, w)
        })
        .collect();
    (entries, queue.holds, queue.draining)
}

struct Row {
    rule: &'static str,
    before: State,
    event: Event<u32, u32, ()>,
    after: State,
    effect: Effect<u32, u32, ()>,
}

fn row(
    rule: &'static str,
    before: State,
    event: Event<u32, u32, ()>,
    after: State,
    effect: Effect<u32, u32, ()>,
) -> Row {
    Row {
        rule,
        before,
        event,
        after,
        effect,
    }
}

#[test]
fn every_event_moves_the_queue_as_the_rules_say() {
    use W::{Clock, Hold, Test};
    let rows = vec![
        row(
            "a request with no hold and no drain goes on at once",
            (vec![(1, Test)], 0, false),
            Event::Arrived(5, 5),
            (vec![(1, Test)], 0, false),
            Effect::SendOn(5, 5),
        ),
        row(
            "a request that arrives while a hold lives is kept, last",
            (vec![(1, Hold)], 1, false),
            Event::Arrived(5, 5),
            (vec![(1, Hold), (5, Hold)], 1, false),
            Effect::None,
        ),
        row(
            "a request that arrives during a drain queues behind the kept ones",
            (vec![(1, Hold)], 0, true),
            Event::Arrived(5, 5),
            (vec![(1, Hold), (5, Hold)], 0, true),
            Effect::None,
        ),
        row(
            "a request a step holds, or whose answer panicked, waits for the test",
            (vec![(1, Hold)], 1, false),
            Event::KeepForTest(5, 5),
            (vec![(1, Hold), (5, Test)], 1, false),
            Effect::None,
        ),
        row(
            "a delayed answer waits for the clock",
            (vec![], 0, false),
            Event::Schedule(5, 5, ()),
            (vec![(5, Clock)], 0, false),
            Effect::None,
        ),
        row(
            "a hold taken is counted",
            (vec![], 1, false),
            Event::HoldTaken,
            (vec![], 2, false),
            Effect::None,
        ),
        row(
            "the last hold dropped starts the drain",
            (vec![(1, Test), (2, Hold)], 1, false),
            Event::HoldDropped,
            (vec![(1, Test), (2, Hold)], 0, true),
            Effect::Drain,
        ),
        row(
            "a hold dropped while another lives sends nothing on",
            (vec![(2, Hold)], 2, false),
            Event::HoldDropped,
            (vec![(2, Hold)], 1, false),
            Effect::None,
        ),
        row(
            "a hold dropped by the code a drain runs leaves the rest to that drain",
            (vec![(2, Hold)], 1, true),
            Event::HoldDropped,
            (vec![(2, Hold)], 0, true),
            Effect::None,
        ),
        row(
            "the last hold left leaves the kept requests to the test",
            (vec![(1, Test), (2, Hold), (3, Clock)], 1, false),
            Event::HoldLeft,
            (vec![(1, Test), (2, Test), (3, Clock)], 0, false),
            Effect::None,
        ),
        row(
            "a hold left while another lives leaves the kept requests to it",
            (vec![(2, Hold)], 2, false),
            Event::HoldLeft,
            (vec![(2, Hold)], 1, false),
            Effect::None,
        ),
        row(
            "a hold left by the code a drain runs leaves the rest to that drain",
            (vec![(2, Hold), (3, Hold)], 1, true),
            Event::HoldLeft,
            (vec![(2, Hold), (3, Hold)], 0, true),
            Effect::None,
        ),
        row(
            "a drain sends on the oldest kept request, past one the test holds",
            (vec![(1, Test), (2, Hold), (3, Hold)], 0, true),
            Event::DrainNext,
            (vec![(1, Test), (3, Hold)], 0, true),
            Effect::SendOn(2, 2),
        ),
        row(
            "a hold taken during a drain stops it, the rest kept in order",
            (vec![(2, Hold), (3, Hold)], 1, true),
            Event::DrainNext,
            (vec![(2, Hold), (3, Hold)], 1, false),
            Effect::None,
        ),
        row(
            "a drain with nothing kept left ends",
            (vec![(1, Test), (3, Clock)], 0, true),
            Event::DrainNext,
            (vec![(1, Test), (3, Clock)], 0, false),
            Effect::None,
        ),
        row(
            "no drain runs, so there is nothing next",
            (vec![(2, Hold)], 1, false),
            Event::DrainNext,
            (vec![(2, Hold)], 1, false),
            Effect::None,
        ),
        row(
            "what verify reported waits for the test, so no later drain answers it",
            (vec![(1, Test), (2, Hold), (3, Clock)], 1, false),
            Event::Reported,
            (vec![(1, Test), (2, Test), (3, Clock)], 1, false),
            Effect::None,
        ),
        row(
            "a ticket withdraws a kept request",
            (vec![(1, Test), (2, Hold)], 1, false),
            Event::Withdraw(2),
            (vec![(1, Test)], 1, false),
            Effect::Taken(2, 2, Waits::Hold),
        ),
        row(
            "a ticket withdraws a request waiting for the clock",
            (vec![(3, Clock)], 0, false),
            Event::Withdraw(3),
            (vec![], 0, false),
            Effect::Taken(3, 3, Waits::Clock(())),
        ),
        row(
            "a ticket of a request no longer waiting withdraws nothing",
            (vec![(1, Test)], 0, false),
            Event::Withdraw(9),
            (vec![(1, Test)], 0, false),
            Effect::None,
        ),
        row(
            "release_oldest takes the oldest held request, past the clock's",
            (vec![(3, Clock), (2, Hold), (1, Test)], 1, false),
            Event::ReleaseOldest,
            (vec![(3, Clock), (1, Test)], 1, false),
            Effect::Taken(2, 2, Waits::Hold),
        ),
        row(
            "release_held takes a request the test holds",
            (vec![(1, Test), (2, Hold)], 1, false),
            Event::Release(1),
            (vec![(2, Hold)], 1, false),
            Effect::Taken(1, 1, Waits::Test),
        ),
        row(
            "release_held does not take a request waiting for the clock",
            (vec![(3, Clock)], 0, false),
            Event::Release(3),
            (vec![(3, Clock)], 0, false),
            Effect::None,
        ),
        row(
            "a request whose release answer panicked waits for the test, where it is",
            (vec![(1, Hold), (2, Hold)], 1, false),
            Event::AnswerPanicked(1),
            (vec![(1, Test), (2, Hold)], 1, false),
            Effect::None,
        ),
        row(
            "the clock takes the request scheduled for it",
            (vec![(1, Hold), (3, Clock)], 1, false),
            Event::ClockReached(3),
            (vec![(1, Hold)], 1, false),
            Effect::Taken(3, 3, Waits::Clock(())),
        ),
        row(
            "the clock takes nothing held",
            (vec![(1, Hold)], 1, false),
            Event::ClockReached(1),
            (vec![(1, Hold)], 1, false),
            Effect::None,
        ),
    ];
    let mut failed = Vec::new();
    for Row {
        rule,
        before,
        event,
        after,
        effect,
    } in rows
    {
        let mut queue = build(&before);
        let seen = queue.step(event);
        let left = snapshot(&queue);
        if seen != effect || left != after {
            failed.push(format!(
                "{rule}: effect {seen:?} (expected {effect:?}), state {left:?} (expected {after:?})"
            ));
        }
    }
    assert!(failed.is_empty(), "rows that fail:\n{}", failed.join("\n"));
}
