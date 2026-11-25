//! Test support for the protocol's transport (ADR-0243), behind the `testing` feature.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::rc::Rc;

/// What a [`ScriptedPipe`] answers next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PipeAnswer {
    /// A read returns this many bytes (zeros); a write takes this many.
    Bytes(usize),
    /// The call is interrupted before it moves a byte (`Interrupted`), to be made again.
    Interrupted,
    /// The call finds nothing to read or no room (`WouldBlock`), as a polled stream answers.
    WouldBlock,
    /// The call fails as on a closed pipe (`BrokenPipe`).
    Closed,
}

impl PipeAnswer {
    /// The answer to a call that offered `len` bytes.
    fn answer(self, len: usize) -> io::Result<usize> {
        match self {
            Self::Bytes(count) => Ok(count.min(len)),
            Self::Interrupted => Err(io::Error::from(io::ErrorKind::Interrupted)),
            Self::WouldBlock => Err(io::Error::from(io::ErrorKind::WouldBlock)),
            Self::Closed => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
        }
    }
}

/// A pipe that answers each read and each write from its script, read and written through
/// `&ScriptedPipe` as a local endpoint's stream is, and records how many bytes each write
/// offered and how many flushes reached it. Its clones share one script and one record, so a
/// test keeps a clone of a pipe it hands to a wrapper.
///
/// # Panics
///
/// A read or a write past its script panics: the script names every call a case makes.
#[derive(Clone, Debug, Default)]
pub struct ScriptedPipe {
    script: Rc<RefCell<Script>>,
}

#[derive(Debug, Default)]
struct Script {
    reads: VecDeque<PipeAnswer>,
    writes: VecDeque<PipeAnswer>,
    offered: Vec<usize>,
    flushes: usize,
}

impl ScriptedPipe {
    /// A pipe that answers its reads with `reads` and its writes with `writes`, in order.
    #[must_use]
    pub fn new(
        reads: impl IntoIterator<Item = PipeAnswer>,
        writes: impl IntoIterator<Item = PipeAnswer>,
    ) -> Self {
        Self {
            script: Rc::new(RefCell::new(Script {
                reads: reads.into_iter().collect(),
                writes: writes.into_iter().collect(),
                offered: Vec::new(),
                flushes: 0,
            })),
        }
    }

    /// How many bytes each write offered, in order.
    #[must_use]
    pub fn offered(&self) -> Vec<usize> {
        self.script.borrow().offered.clone()
    }

    /// How many flushes reached the pipe.
    #[must_use]
    pub fn flushes(&self) -> usize {
        self.script.borrow().flushes
    }

    /// Whether every scripted read and write was made.
    #[must_use]
    pub fn is_spent(&self) -> bool {
        let script = self.script.borrow();
        script.reads.is_empty() && script.writes.is_empty()
    }
}

impl Read for &ScriptedPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let answer = self
            .script
            .borrow_mut()
            .reads
            .pop_front()
            .expect("BUG: the script names every read");
        let count = answer.answer(buf.len())?;
        buf[..count].fill(0);
        Ok(count)
    }
}

impl Write for &ScriptedPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut script = self.script.borrow_mut();
        script.offered.push(buf.len());
        let answer = script
            .writes
            .pop_front()
            .expect("BUG: the script names every write");
        answer.answer(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.script.borrow_mut().flushes += 1;
        Ok(())
    }
}
