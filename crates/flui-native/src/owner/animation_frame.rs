//! One turn of the page's `requestAnimationFrame` loop, which runs on every
//! host so its order is tested off the browser.

use crate::owner::frame_demand::FrameDemand;

/// Run one animation frame. The frame takes the requests `demand` holds, and
/// `work` runs knowing whether there was one. After it, also when it unwinds
/// (a `panic = "unwind"` build), the loop goes on: `ask_next` asks the
/// browser for the next frame when a request waits or the refresh estimate is
/// still `measuring`, and `go_idle` stops the loop otherwise, until a request
/// resumes it.
pub(crate) fn turn(
    demand: &FrameDemand,
    measuring: impl FnOnce() -> bool,
    ask_next: impl FnOnce(),
    go_idle: impl FnOnce(),
    work: impl FnOnce(bool),
) {
    struct After<'a, M: FnOnce() -> bool, A: FnOnce(), I: FnOnce()> {
        demand: &'a FrameDemand,
        measuring: Option<M>,
        ask_next: Option<A>,
        go_idle: Option<I>,
    }
    impl<M: FnOnce() -> bool, A: FnOnce(), I: FnOnce()> Drop for After<'_, M, A, I> {
        fn drop(&mut self) {
            let measuring = self.measuring.take().is_some_and(|measuring| measuring());
            if self.demand.pending() || measuring {
                if let Some(ask_next) = self.ask_next.take() {
                    ask_next();
                }
            } else if let Some(go_idle) = self.go_idle.take() {
                go_idle();
            }
        }
    }
    let after = After {
        demand,
        measuring: Some(measuring),
        ask_next: Some(ask_next),
        go_idle: Some(go_idle),
    };
    work(demand.take());
    drop(after);
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::turn;
    use crate::owner::frame_demand::FrameDemand;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Next {
        Asked,
        Idle,
    }

    /// What the frame's work does.
    #[derive(Debug, Clone, Copy)]
    enum Work {
        Nothing,
        Requests,
        Unwinds,
        RequestsThenUnwinds,
    }

    #[test]
    fn a_turn_goes_on_only_while_a_frame_is_asked_for_or_measured() {
        // (asked before the turn, measuring, what the work does,
        //  what the work was told, what follows the turn)
        let rows = [
            (true, false, Work::Nothing, true, Next::Idle),
            (true, false, Work::Requests, true, Next::Asked),
            (false, false, Work::Nothing, false, Next::Idle),
            (false, false, Work::Requests, false, Next::Asked),
            (false, true, Work::Nothing, false, Next::Asked),
            (true, false, Work::Unwinds, true, Next::Idle),
            (true, false, Work::RequestsThenUnwinds, true, Next::Asked),
            (false, true, Work::Unwinds, false, Next::Asked),
        ];
        for (asked, measuring, work, told, follows) in rows {
            let demand = FrameDemand::new();
            if !asked {
                demand.take();
            }
            let next = Cell::new(None);
            let seen = Cell::new(None);
            let _ = catch_unwind(AssertUnwindSafe(|| {
                turn(
                    &demand,
                    || measuring,
                    || next.set(Some(Next::Asked)),
                    || next.set(Some(Next::Idle)),
                    |requested| {
                        seen.set(Some(requested));
                        if matches!(work, Work::Requests | Work::RequestsThenUnwinds) {
                            demand.request();
                        }
                        if matches!(work, Work::Unwinds | Work::RequestsThenUnwinds) {
                            panic!("the frame's work unwinds");
                        }
                    },
                );
            }));
            let case = format!("asked {asked}, measuring {measuring}, {work:?}");
            assert_eq!(seen.get(), Some(told), "{case}: what the work was told");
            assert_eq!(next.get(), Some(follows), "{case}: what follows the turn");
        }
    }
}
