//! What only a [`Tester`] of the in-process [`Headless`] connection has (ADR-0400 §4): a mount
//! with no agent, a measured window over the tester's own steps, and a [`Gesture`] sent as it was
//! built. The reads of its frame are [`crate::frame`]'s.
//!
//! [`Gesture`]: super::gesture::Gesture

use std::panic::Location;

use flui_protocol::client::Connection as _;
use flui_protocol::input::ActionSequence;
use flui_view::IntoView;

use super::error::{OrRaise as _, TestError};
use super::headless::{Headless, Observed, Unobserved};
use super::target::Target;
use super::tester::Tester;
use super::window::TestWindow;
use crate::perf::{Cost, measure};

impl Tester<Headless<Unobserved>> {
    /// Mount `view` in `window` with no agent attached (ADR-0400 §1): the state a cost scenario
    /// measures, where the agent's journal and its work after each frame are not part of the
    /// counts. Its first frame is presented.
    ///
    /// The tester pumps, reads its frame through [`Tester::frame`] and measures through
    /// [`Tester::measure`]; it finds, acts and expects nothing, since nothing resolves a locator.
    /// Its end settles, unmounts, releases and reads the [ledger](Self::ledger) and what is left
    /// behind; it audits nothing, as no agent reads the window's semantics.
    ///
    /// # Panics
    ///
    /// When a registration of `window` conflicts with another, or the first frame fails.
    #[track_caller]
    #[must_use]
    pub fn mount_unobserved(view: impl IntoView, window: TestWindow) -> Self {
        let connection = Headless::mount_unobserved(view, &window);
        let id = connection.window_id();
        Self::over(connection, id)
    }
}

impl<S> Tester<Headless<S>>
where
    Headless<S>: Target,
{
    /// Run `steps` in a perf window and return what they cost ([`crate::perf::measure`],
    /// ADR-0396 §1): what they asked of the heap on this thread, and the window a hold that
    /// counts instructions runs again under callgrind. What their frames did, phase by phase, is
    /// the [frame's report](crate::frame::Frame::report) after the last pump in them.
    ///
    /// The counterpart in Flutter 3.50.1 is `traceAction`, whose timeline a benchmark reads in
    /// time; FLUI counts, so a baseline is held exactly.
    pub fn measure(&mut self, steps: impl FnOnce(&mut Self)) -> Cost {
        let ((), cost) = measure(|| steps(self));
        cost
    }
}

impl Tester<Headless<Observed>> {
    /// Send `actions`, a [`Gesture`](super::gesture::Gesture) as it was built, to the window's
    /// agent, which plays it at its frame boundaries on the window's own input path, as
    /// `input.inject` does. Nothing is aimed or checked: the points are the gesture's.
    ///
    /// # Panics
    ///
    /// As [`Self::try_perform`] fails.
    #[track_caller]
    pub fn perform(&mut self, actions: &ActionSequence) {
        self.try_perform(actions).or_raise();
    }

    /// Send `actions` to the window's agent.
    ///
    /// # Errors
    ///
    /// [`TestError::Agent`] when the window refuses the sequence (a plan it cannot play, input
    /// held by another lease); [`TestError::Unanswered`] when it never answered.
    #[track_caller]
    pub fn try_perform(&mut self, actions: &ActionSequence) -> Result<(), TestError> {
        let (step, _running) = self.begin("perform", Location::caller());
        match self.connection_mut().perform(actions) {
            Ok(_performed) => Ok(()),
            Err(error) => Err(self.failed(&step, error)),
        }
    }
}
