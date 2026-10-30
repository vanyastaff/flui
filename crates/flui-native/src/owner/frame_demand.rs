//! Whether a window owes its application a frame: the one record, on every
//! host whose frames come from a producer it can stop (Android's looper turn,
//! iOS's `CADisplayLink`, the page's `requestAnimationFrame`), of the requests
//! not yet served.
//!
//! A frame is delivered only when it was requested. A request from any thread
//! marks the window; the first since the last frame tells the host to wake its
//! producer, and a repeat costs one atomic swap. The producer takes the mark
//! before it runs the frame, so a request made during the frame or after it
//! marks the window again and is served by the next one, and an older wake can
//! never clear a newer request. A producer that finds nothing marked after a
//! frame stops until the next request wakes it.

use std::sync::atomic::{AtomicBool, Ordering};

/// A window's unserved frame requests.
#[derive(Debug)]
pub(crate) struct FrameDemand {
    wanted: AtomicBool,
}

impl FrameDemand {
    /// A window that owes its first frame.
    pub(crate) fn new() -> Self {
        Self {
            wanted: AtomicBool::new(true),
        }
    }

    /// Ask for a frame. True for the first request since the producer last
    /// took one: the host wakes its producer then, and only then.
    pub(crate) fn request(&self) -> bool {
        !self.wanted.swap(true, Ordering::SeqCst)
    }

    /// Take the requests a frame about to run serves. True when one was
    /// waiting.
    pub(crate) fn take(&self) -> bool {
        self.wanted.swap(false, Ordering::SeqCst)
    }

    /// Whether a request waits, without taking it: after a frame, a producer
    /// that finds none stops.
    pub(crate) fn pending(&self) -> bool {
        self.wanted.load(Ordering::SeqCst)
    }
}

impl Default for FrameDemand {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::FrameDemand;

    #[derive(Debug, Clone, Copy)]
    enum Step {
        Request,
        Take,
    }

    /// A case: its name, each step with what it answers, and whether a
    /// request is left after them.
    type Row = (&'static str, &'static [(Step, bool)], bool);

    /// Each row runs its steps on a fresh window and checks what each step
    /// answers, then whether a request is left for the next frame.
    #[test]
    fn a_frame_is_owed_only_until_a_frame_takes_it() {
        use Step::{Request, Take};
        let rows: &[Row] = &[
            ("a new window owes its first frame", &[], true),
            ("the first frame takes it", &[(Take, true)], false),
            (
                "a request after the producer stopped wakes it once",
                &[(Take, true), (Request, true), (Request, false)],
                true,
            ),
            (
                "a request during a frame keeps the producer running",
                &[(Take, true), (Request, true), (Take, true)],
                false,
            ),
            (
                "a frame with nothing requested takes nothing",
                &[(Take, true), (Take, false)],
                false,
            ),
            (
                "a request before the first frame does not wake twice",
                &[(Request, false), (Take, true)],
                false,
            ),
        ];
        for (name, steps, left) in rows {
            let demand = FrameDemand::new();
            for (index, (step, answer)) in steps.iter().enumerate() {
                let got = match step {
                    Request => demand.request(),
                    Take => demand.take(),
                };
                assert_eq!(got, *answer, "{name}: step {index} ({step:?})");
            }
            assert_eq!(demand.pending(), *left, "{name}: a request left");
        }
    }
}
