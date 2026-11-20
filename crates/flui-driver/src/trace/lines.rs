//! Lines of an untrusted JSON Lines file, each read only up to its bound: a line past it is
//! named before more than the bound is held in memory.

use std::io::{self, BufRead, Read as _};

/// What one read of a [`BoundedLines`] found.
#[derive(Debug)]
pub(crate) enum LineRead {
    /// A line: its bytes without the line ending, how many bytes it took with it, and whether it
    /// had its ending (the last line of a file a writer stopped in the middle of has none).
    Line {
        bytes: Vec<u8>,
        len: usize,
        terminated: bool,
    },
    /// A line longer than the bound: nothing past the bound was kept.
    TooLong,
    /// The end of the file.
    End,
}

/// The lines of a reader, each at most `max` bytes without its ending.
#[derive(Debug)]
pub(crate) struct BoundedLines<R> {
    reader: R,
    max: usize,
}

impl<R: BufRead> BoundedLines<R> {
    pub(crate) fn new(reader: R, max: usize) -> Self {
        Self { reader, max }
    }

    /// The next line.
    ///
    /// # Errors
    ///
    /// When the reader fails.
    pub(crate) fn next_line(&mut self) -> io::Result<LineRead> {
        let mut bytes = Vec::new();
        // The bound, a `\r` and the `\n`: a line of exactly `max` bytes still fits.
        let limit = u64::try_from(self.max.saturating_add(2)).unwrap_or(u64::MAX);
        let len = (&mut self.reader)
            .take(limit)
            .read_until(b'\n', &mut bytes)?;
        if len == 0 {
            return Ok(LineRead::End);
        }
        let terminated = bytes.last() == Some(&b'\n');
        if terminated {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.len() > self.max || (!terminated && len == self.max.saturating_add(2)) {
            return Ok(LineRead::TooLong);
        }
        Ok(LineRead::Line {
            bytes,
            len,
            terminated,
        })
    }
}
