//! The user-activity guard on a device: `getevent` watched for input the lane did not make
//! ([`super::getevent::Watch`]). An emulator needs none: nobody holds it.

use std::time::{Duration, Instant};

use super::adb::{AdbStream, lane_error};
use super::getevent::Watch;
use crate::error::Result;
use crate::lane::{ActivityGuard, ForeignInput};

/// What a guard reports once it stopped watching: it can no longer tell the person's input from
/// the lane's, so the run ends.
pub const STOPPED_WATCHING: &str = "the getevent guard stopped watching";

/// How long arming waits for `getevent` to name its first device.
pub const ARM_WITHIN: Duration = Duration::from_secs(5);

/// How long arming reads the lines that came while the guard was disarmed, until none is
/// waiting.
pub const BACKLOG_WITHIN: Duration = Duration::from_secs(1);

/// How long a look for foreign input before an event reads the lines waiting.
pub const LOOK_WITHIN: Duration = Duration::from_millis(100);

/// The guard over a device: see the [module docs](self).
pub struct GeteventGuard {
    stream: Box<dyn AdbStream>,
    watch: Watch,
    stopped: Option<String>,
}

impl GeteventGuard {
    /// A guard reading `stream`, a running `getevent -l` (`super::getevent::WATCH`).
    #[must_use]
    pub fn new(stream: Box<dyn AdbStream>) -> Self {
        Self {
            stream,
            watch: Watch::new(),
            stopped: None,
        }
    }

    /// Read the lines that already came until none is waiting, or until `within` has passed: a
    /// device whose sensors print without pause is read for that long and no longer.
    fn drain(&mut self, within: Duration) {
        let deadline = Instant::now() + within;
        while self.stopped.is_none() && Instant::now() < deadline {
            match self.stream.next_record(Duration::ZERO) {
                Ok(Some(line)) => self.watch.read(&line),
                Ok(None) => return,
                Err(error) => self.stopped = Some(error.to_string()),
            }
        }
    }
}

impl ActivityGuard for GeteventGuard {
    fn arm(&mut self) -> Result<()> {
        self.drain(BACKLOG_WITHIN);
        let deadline = Instant::now() + ARM_WITHIN;
        while !self.watch.started() && self.stopped.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(lane_error(format!(
                    "getevent named no input device within {} s, so the guard cannot watch",
                    ARM_WITHIN.as_secs()
                )));
            }
            match self
                .stream
                .next_record(left.min(Duration::from_millis(100)))
            {
                Ok(Some(line)) => self.watch.read(&line),
                Ok(None) => {}
                Err(error) => self.stopped = Some(error.to_string()),
            }
        }
        if let Some(why) = &self.stopped {
            return Err(lane_error(format!("{STOPPED_WATCHING}: {why}")));
        }
        // What came between two runs (the person's touches while the lane was idle) is read and
        // forgotten here: only input after the guard armed ends a run.
        self.drain(BACKLOG_WITHIN);
        self.watch.arm();
        Ok(())
    }

    fn foreign_input(&mut self) -> Option<ForeignInput> {
        self.drain(LOOK_WITHIN);
        if let Some(why) = &self.stopped {
            return Some(ForeignInput::new(format!("{STOPPED_WATCHING}: {why}")));
        }
        self.watch.foreign().map(ForeignInput::new)
    }

    fn disarm(&mut self) {
        self.watch.disarm();
    }
}

impl std::fmt::Debug for GeteventGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeteventGuard")
            .field("watch", &self.watch)
            .field("stopped", &self.stopped)
            .finish_non_exhaustive()
    }
}
