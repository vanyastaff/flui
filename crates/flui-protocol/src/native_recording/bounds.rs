//! The bounds a native recording is held to, and the count that holds a writer and a reader to
//! the same ones.
//!
//! A recording is untrusted input to whoever reads it, and every writer of one writes for that
//! reader: the in-process recorder and `NativeRecording::write` in `flui-native`, and
//! `flui record --native`, which writes a live stream's lines as they arrive. Each keeps a
//! [`Budget`] and asks it before every row it writes ([`Budget::admit_row`]); the reader asks
//! its own for every line it read ([`Budget::read_line`]). Both answer from the same limits, so
//! no writer writes a recording its reader refuses.

use std::fmt;

use super::CutReason;

/// How long one line of a native recording may be, in bytes, its newline excluded: a reader
/// refuses a longer line before it parses it. A [`StoreLock`](super::StoreLock) row within its
/// two bounds fits.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// How many rows one native recording may hold: a reader refuses the row past it.
pub const MAX_ROWS: usize = 1 << 20;

/// How many bytes one native recording may hold, its lines and their endings together: a reader
/// refuses the line that would pass it, as it reads, before it parses or keeps that line. The two
/// bounds above alone admit about a tebibyte held at once; the largest recording a device run has
/// written is a few mebibytes, so this leaves room for long sessions and keeps what one read
/// holds within what a test host and a CI runner have.
pub const MAX_RECORDING_BYTES: usize = 64 * 1024 * 1024;

/// How many bytes a writer keeps free for the [`Row::Cut`](super::Row::Cut) line it may end a
/// recording with: the longest such line and its newline, with room.
const CUT_LINE_ROOM: usize = 64;

/// The bound of a native recording a line would pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Overrun {
    /// The line is longer than [`MAX_LINE_BYTES`].
    LineBytes,
    /// The header would grow longer than [`MAX_LINE_BYTES`] to declare the devices the row reads.
    HeaderBytes,
    /// The line would be past the [`MAX_ROWS`] rows a recording holds.
    Rows,
    /// The line would take the recording past [`MAX_RECORDING_BYTES`].
    RecordingBytes,
}

impl Overrun {
    /// The limit passed, in the unit its constant names.
    #[must_use]
    pub const fn limit(self) -> usize {
        match self {
            Self::LineBytes | Self::HeaderBytes => MAX_LINE_BYTES,
            Self::Rows => MAX_ROWS,
            Self::RecordingBytes => MAX_RECORDING_BYTES,
        }
    }

    /// The reason the `cut` row gives when a writer ends the recording in this line's place: the
    /// recording is full, so nothing after the line fits either. `None` for a line that does not
    /// fit on its own ([`LineBytes`](Self::LineBytes), [`HeaderBytes`](Self::HeaderBytes)), which
    /// a writer refuses without ending the recording.
    #[must_use]
    pub const fn cut_reason(self) -> Option<CutReason> {
        match self {
            Self::Rows => Some(CutReason::Rows),
            Self::RecordingBytes => Some(CutReason::Bytes),
            Self::LineBytes | Self::HeaderBytes => None,
        }
    }
}

impl fmt::Display for Overrun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let limit = self.limit();
        match self {
            Self::LineBytes => write!(f, "a line is longer than {limit} bytes"),
            Self::HeaderBytes => write!(
                f,
                "the header would be longer than {limit} bytes to declare the row's devices"
            ),
            Self::Rows => write!(f, "a native recording holds at most {limit} rows"),
            Self::RecordingBytes => write!(f, "a native recording holds at most {limit} bytes"),
        }
    }
}

impl std::error::Error for Overrun {}

/// What one native recording has taken of its bounds: its header line, the bytes of the lines
/// after it, each with its line ending, and how many there are.
///
/// Every count saturates, so no input overflows it into a recording that seems to fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    header: usize,
    body: usize,
    rows: usize,
}

impl Budget {
    /// A recording whose header line takes `header` bytes, its line ending included.
    #[must_use]
    pub const fn after_header(header: usize) -> Self {
        Self {
            header,
            body: 0,
            rows: 0,
        }
    }

    /// The bytes taken, the header's included.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.header.saturating_add(self.body)
    }

    /// The lines taken after the header.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.rows
    }

    /// Take a row a writer is about to write: its line of `line` bytes, newline included, and the
    /// `declared` bytes its header grows by to declare the devices the row reads first (zero for a
    /// writer whose header already declares every row's devices). `follows` says whether a row may
    /// come after this one, which a live stream always may: then room stays for the `cut` row a
    /// writer ends the recording with in that one's place, so a writer that admits every row it
    /// writes, and ends the recording with a `cut` row at the first it is refused, never writes
    /// past a bound.
    ///
    /// # Errors
    ///
    /// The [`Overrun`] the row would make, checked in this order: its line past
    /// [`MAX_LINE_BYTES`], the header past it, the rows past [`MAX_ROWS`] (counting the `cut`
    /// row's place when one may follow), the bytes past [`MAX_RECORDING_BYTES`]. A refused row
    /// takes nothing.
    pub fn admit_row(
        &mut self,
        line: usize,
        declared: usize,
        follows: bool,
    ) -> Result<(), Overrun> {
        let header = self.header.saturating_add(declared);
        if line.saturating_sub(1) > MAX_LINE_BYTES {
            return Err(Overrun::LineBytes);
        }
        if header.saturating_sub(1) > MAX_LINE_BYTES {
            return Err(Overrun::HeaderBytes);
        }
        if self.rows.saturating_add(1 + usize::from(follows)) > MAX_ROWS {
            return Err(Overrun::Rows);
        }
        let body = self.body.saturating_add(line);
        let room = if follows { CUT_LINE_ROOM } else { 0 };
        if header.saturating_add(body).saturating_add(room) > MAX_RECORDING_BYTES {
            return Err(Overrun::RecordingBytes);
        }
        self.header = header;
        self.body = body;
        self.rows += 1;
        Ok(())
    }

    /// Take a line a reader read after the header, `line` bytes with its ending. The line's own
    /// length is the reader's to check as it reads, before it holds more than [`MAX_LINE_BYTES`]
    /// of it.
    ///
    /// # Errors
    ///
    /// [`Overrun::RecordingBytes`] when the line takes the recording past
    /// [`MAX_RECORDING_BYTES`], then [`Overrun::Rows`] when it is past [`MAX_ROWS`]. A refused
    /// line takes nothing.
    pub fn read_line(&mut self, line: usize) -> Result<(), Overrun> {
        let body = self.body.saturating_add(line);
        if self.header.saturating_add(body) > MAX_RECORDING_BYTES {
            return Err(Overrun::RecordingBytes);
        }
        if self.rows >= MAX_ROWS {
            return Err(Overrun::Rows);
        }
        self.body = body;
        self.rows += 1;
        Ok(())
    }
}
