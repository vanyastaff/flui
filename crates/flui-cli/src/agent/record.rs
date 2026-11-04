//! A running app's input recording, written as a recording file (ADR-0288 §11).
//!
//! `record.start` answers the recording's header, and each row then arrives as a `record`
//! event; [`record`] writes the header and the rows as the input recording's JSON Lines
//! (ADR-0215 §3), the format `flui-testing`'s `RecordingReader` replays. Each line is flushed
//! as it is written, so a recording cut short keeps every row that arrived.
//!
//! A recording ends with its `end` row: when the caller asks it to stop (`record.stop`, whose
//! answer comes once the `end` row went out), or when the app ends it, because the window it
//! was started on closed or the runtime went. Either way `record.stop` then answers what it
//! came to, since the app holds a recording that ended by itself until it is named.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use flui_protocol::Event;
use flui_protocol::input_recording::Row;
use flui_protocol::method::{Method, RecordStart, RecordStop};
use flui_protocol::{RecordEvent, RecordParams, RecordStopParams, RecordSummary, Recording};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::client::{Connection, Failure};

/// How often a recording that is waiting for rows looks at whether it was asked to stop.
const POLL: Duration = Duration::from_millis(100);

/// Why a recording ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// The caller stopped it.
    Stopped,
    /// The app ended it: the window it was started on closed, or the runtime went.
    ByApp,
}

/// A recording that ended with its `end` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recorded {
    /// What the app says it came to.
    pub(crate) summary: RecordSummary,
    /// Why it ended.
    pub(crate) ending: Ending,
}

/// Why a recording did not end with its `end` row.
#[derive(Debug)]
pub(crate) enum RecordFailure {
    /// `record.start` failed; nothing was written.
    Start(Failure),
    /// The conversation broke after the header was written: what is written holds `rows`
    /// rows and no `end` row.
    Broken {
        /// The rows written, as [`RecordSummary::rows`] counts them.
        rows: u64,
        /// What broke.
        why: Failure,
    },
    /// A line could not be written.
    Write(io::Error),
}

/// Records `params` over `connection` into `out` until `stop` answers true or the app ends the
/// recording.
pub(crate) fn record(
    connection: &mut Connection,
    params: &RecordParams,
    out: &mut impl Write,
    stop: impl Fn() -> bool,
) -> Result<Recorded, RecordFailure> {
    // The connection is this recording's alone, so every row that arrives before the answer
    // is one of its own: the rows come after the header they follow.
    let mut early = Vec::new();
    let started: Recording = call::<RecordStart>(connection, params, |event| {
        if let Event::Record(event) = event {
            early.push(*event);
        }
    })
    .map_err(RecordFailure::Start)?;
    let ours = started.recording;
    line(out, &started.header)?;

    let mut file = Written {
        out,
        ours,
        rows: 0,
        ended: false,
    };
    for event in early {
        file.take(&event)?;
    }
    while !file.ended && !stop() {
        match connection.next_event(Instant::now() + POLL) {
            Ok(Some(Event::Record(event))) => file.take(&event)?,
            Ok(_) => {}
            Err(why) => {
                return Err(RecordFailure::Broken {
                    rows: file.rows,
                    why,
                });
            }
        }
    }
    let ending = if file.ended {
        Ending::ByApp
    } else {
        Ending::Stopped
    };

    // The rows admitted before the stop, and the `end` row, come before its answer.
    let mut written = Ok(());
    let stopped = call::<RecordStop>(connection, &RecordStopParams::new(ours), |event| {
        if let (Ok(()), Event::Record(event)) = (&written, event) {
            written = file.take(&event);
        }
    });
    written?;
    let summary = stopped.map_err(|why| RecordFailure::Broken {
        rows: file.rows,
        why,
    })?;
    if !file.ended {
        return Err(RecordFailure::Broken {
            rows: file.rows,
            why: Failure::Transport("the app answered the stop before the recording's end".into()),
        });
    }
    Ok(Recorded { summary, ending })
}

/// The rows of one recording, as they are written.
struct Written<'a, W: Write> {
    out: &'a mut W,
    ours: u64,
    /// The rows written, as [`RecordSummary::rows`] counts them: the header, the `gap` rows
    /// and the `end` row aside.
    rows: u64,
    /// Whether the `end` row was written.
    ended: bool,
}

impl<W: Write> Written<'_, W> {
    /// Writes `event`'s row when it is one of this recording's and its `end` row is not
    /// written yet.
    fn take(&mut self, event: &RecordEvent) -> Result<(), RecordFailure> {
        if event.recording != self.ours || self.ended {
            return Ok(());
        }
        line(self.out, &event.row)?;
        match event.row {
            Row::End { .. } => self.ended = true,
            Row::Gap { .. } => {}
            _ => self.rows += 1,
        }
        Ok(())
    }
}

/// Calls `M` with `params`, handing `event` each event that arrives before its answer.
fn call<M>(
    connection: &mut Connection,
    params: &M::Params,
    event: impl FnMut(Event),
) -> Result<M::Output, Failure>
where
    M: Method,
    M::Params: Serialize,
    M::Output: DeserializeOwned,
{
    let params = serde_json::to_value(params)
        .map_err(|e| Failure::Unsent(format!("{} did not serialize: {e}", M::NAME)))?;
    let result = connection.request_seeing(M::NAME, params, event)?;
    serde_json::from_value::<M::Output>(result)
        .map_err(|e| Failure::Transport(format!("the app's {} answer is malformed: {e}", M::NAME)))
}

/// Writes `value` as one line and flushes it.
fn line(out: &mut impl Write, value: &impl Serialize) -> Result<(), RecordFailure> {
    serde_json::to_writer(&mut *out, value)
        .map_err(io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush())
        .map_err(RecordFailure::Write)
}
