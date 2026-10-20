//! Validating a recording before anything is replayed (ADR-0215 §1, §3).

use std::io::BufRead;

use flui_runtime::{DecisionRow, InputRecord};

use super::format::{RecordingError, RecordingHeader, RecordingReader};

/// A recording read through its `End` row and found replayable: no `Gap`
/// marker, no redacted row. Nothing reaches a runtime until this holds, so a
/// damaged recording dispatches nothing at all.
#[derive(Debug, Clone)]
pub struct ReplayableRecording {
    header: RecordingHeader,
    rows: Vec<InputRecord>,
}

impl ReplayableRecording {
    /// Read the whole recording behind `reader` and check it.
    ///
    /// # Errors
    ///
    /// [`RecordingError::NonReplayable`] at the first redacted row or `Gap`
    /// marker, at a row after the `End` row, or past the last line of a
    /// recording with no `End` row (a failing tap, a crash, a truncated
    /// file); and the reader's own errors.
    pub fn read<R: BufRead>(mut reader: RecordingReader<R>) -> Result<Self, RecordingError> {
        let mut rows = Vec::new();
        let mut ended = false;
        for record in reader.by_ref() {
            let record = record?;
            if ended || record.redacted {
                return Err(RecordingError::NonReplayable { line: record.line });
            }
            match record.record {
                InputRecord::Gap { .. } => {
                    return Err(RecordingError::NonReplayable { line: record.line });
                }
                InputRecord::End { .. } => ended = true,
                row => rows.push(row),
            }
        }
        if !ended {
            return Err(RecordingError::NonReplayable {
                line: reader.next_line(),
            });
        }
        Ok(Self {
            header: reader.header().clone(),
            rows,
        })
    }

    /// The recording's header.
    #[must_use]
    pub fn header(&self) -> &RecordingHeader {
        &self.header
    }

    /// The rows to replay, in order, without the `End` row.
    #[must_use]
    pub fn rows(&self) -> &[InputRecord] {
        &self.rows
    }

    /// The arena decisions the recording holds, in order: what a replay must
    /// reproduce.
    #[must_use]
    pub fn decisions(&self) -> Vec<DecisionRow> {
        self.rows
            .iter()
            .filter_map(|row| match row {
                InputRecord::Decision(decision) => Some(decision.clone()),
                _ => None,
            })
            .collect()
    }
}
