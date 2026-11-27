//! [`Target`]: what a [`Tester`](super::Tester) runs over (ADR-0400 §1).

use flui_protocol::client::{ClockReading, Connection, EndChecks};
use flui_protocol::method::SemanticsRead;
use flui_protocol::report::UnpassedCheck;
use flui_protocol::tree::{ReadParams, WindowId};

use super::headless::{Headless, Unobserved};

pub(super) mod sealed {
    use flui_protocol::client::{ClockReading, EndChecks};
    use flui_protocol::report::UnpassedCheck;
    use flui_protocol::tree::WindowId;

    /// What the tester's own bookkeeping asks of its target, whatever its depth: the clock a
    /// diagnosis names, the end of the test, and the outline a diagnosis shows.
    pub trait Ends {
        /// The target's clock, as [`Connection::now`](flui_protocol::client::Connection::now)
        /// reads it.
        fn reading(&self) -> ClockReading;
        /// The end of the test: what the target can check and release (ADR-0400 §7).
        fn end(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck>;
        /// The window's outline a diagnosis shows, cut to its first lines.
        fn outline(&mut self, window: WindowId) -> String;
    }
}

/// What a [`Tester`](super::Tester) runs over: every [`Connection`], and
/// [`Headless<Unobserved>`](Headless), the in-process host with no agent attached, which
/// resolves no locator and is therefore no `Connection` (ADR-0400 §1).
///
/// The verbs that find, act and expect need a `Connection`; the virtual clock's need a
/// [`VirtualClock`](super::VirtualClock); a tester of any target keeps its journal and its
/// report and runs the end of its test. The trait is sealed: a depth is a `Connection`, and the
/// one target that is not is the kit's.
pub trait Target: sealed::Ends {}

impl<C: Connection> Target for C {}

impl Target for Headless<Unobserved> {}

impl<C: Connection> sealed::Ends for C {
    fn reading(&self) -> ClockReading {
        self.now()
    }

    fn end(&mut self, checks: &EndChecks) -> Vec<UnpassedCheck> {
        self.finish(checks)
    }

    fn outline(&mut self, window: WindowId) -> String {
        match self.call::<SemanticsRead>(ReadParams::new(window)) {
            Ok(tree) => super::tester::excerpt(&tree),
            Err(error) => format!("(the semantics tree could not be read: {error})"),
        }
    }
}
