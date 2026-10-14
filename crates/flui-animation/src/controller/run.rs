//! The controller's run: one value, so a run's mode, its completer and how it
//! settles are installed and retired together and cannot disagree.

use std::rc::Rc;
use std::time::Duration;

use super::{Opaque, RepeatRun, RetiredSources, SimulationRun};
use crate::curve::Curve;
use crate::run_future::RunCompleter;

/// The run installed on a controller.
pub(super) struct ActiveRun {
    /// The write half of the run's future. Taken, never overwritten, so the
    /// displaced completion reaches transition delivery.
    pub(super) completer: RunCompleter,
    pub(super) mode: RunMode,
    pub(super) settle: Settle,
}

/// What a run samples each frame. A repeat cannot also be a simulation.
pub(super) enum RunMode {
    /// From the start to the target value over `duration`, eased by `curve`
    /// (linear when `None`).
    Timed {
        curve: Option<Rc<dyn Curve>>,
        duration: Duration,
    },
    /// A repeat, sampled as a pure function of its elapsed time.
    Repeat(RepeatRun),
    /// A physical or generated simulation.
    Simulation(SimulationRun),
}

/// Whether a run waits for frames or settles on its next delivery.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Settle {
    /// It advances as frames sample it.
    Sampled,
    /// It settles on its next delivery: a motion already at rest, or a run
    /// admitted while its controller delivers.
    Immediate,
}

impl ActiveRun {
    /// Hand the mode's opaque sources to `retired` and return the completer
    /// for the caller to complete or cancel.
    pub(super) fn retire(self, retired: &mut RetiredSources) -> RunCompleter {
        match self.mode {
            RunMode::Timed {
                curve: Some(curve), ..
            } => retired.curve = Some(Opaque::new(curve)),
            RunMode::Simulation(simulation) => retired.simulation = Some(Opaque::new(simulation)),
            RunMode::Timed { curve: None, .. } | RunMode::Repeat(_) => {}
        }
        self.completer
    }
}
