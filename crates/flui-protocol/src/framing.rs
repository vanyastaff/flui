//! How a byte transport frames the protocol (ADR-0238 §2): newline-delimited JSON lines of at
//! most [`MAX_LINE`] bytes, and binary payloads as frames of their own.
//!
//! A result that carries a payload announces it with a [`BlobRef`](crate::BlobRef); right
//! after that result's line the payload follows as one frame: a [`BlobHeader`] of
//! [`BlobHeader::SIZE`] bytes, then exactly `len` bytes. A reader checks `len` against its own
//! limit before reading or allocating. The BiDi transport has no binary frame and carries the
//! payload as a base64 string instead.

use std::fmt;

/// The longest line either side reads, in bytes, without its newline.
pub const MAX_LINE: usize = 1 << 20;

/// A payload's number on its connection, counted from 1 by the side that sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(transparent)
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(inline))]
pub struct BlobId(u64);

impl BlobId {
    /// Payload `id`.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The header in front of a payload: its [`BlobId`] and its length, each a little-endian
/// `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct BlobHeader {
    /// The payload the result announced.
    pub id: BlobId,
    /// How many bytes follow the header.
    pub len: u64,
}

impl BlobHeader {
    /// The header's size on the wire, in bytes.
    pub const SIZE: usize = 16;

    /// The header of payload `id`, `len` bytes long.
    #[must_use]
    pub const fn new(id: BlobId, len: u64) -> Self {
        Self { id, len }
    }

    /// The header's bytes.
    #[must_use]
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0_u8; Self::SIZE];
        bytes[..8].copy_from_slice(&self.id.get().to_le_bytes());
        bytes[8..].copy_from_slice(&self.len.to_le_bytes());
        bytes
    }

    /// The header in `bytes`, whose payload a reader takes only when it is at most
    /// `max_len` bytes long.
    ///
    /// # Errors
    ///
    /// [`FramingError::PayloadTooLong`] when the header announces more than `max_len` bytes,
    /// and [`FramingError::ZeroId`] for id `0`, which no sender uses.
    pub fn decode(bytes: &[u8; Self::SIZE], max_len: u64) -> Result<Self, FramingError> {
        let mut id = [0_u8; 8];
        id.copy_from_slice(&bytes[..8]);
        let mut len = [0_u8; 8];
        len.copy_from_slice(&bytes[8..]);
        let id = u64::from_le_bytes(id);
        let len = u64::from_le_bytes(len);
        if id == 0 {
            return Err(FramingError::ZeroId);
        }
        if len > max_len {
            return Err(FramingError::PayloadTooLong { len, max_len });
        }
        Ok(Self::new(BlobId::new(id), len))
    }
}

/// Splits a byte stream into the protocol's lines, with no I/O of its own: a transport
/// [pushes](Self::push) what each read returned and [takes](Self::next_line) the whole lines.
///
/// A line ends at `\n`; a `\r` before it is dropped. A line longer than the reader's limit,
/// without its newline, is refused as soon as the buffer shows it, without waiting for its end.
/// After that the stream cannot be framed again: the transport closes it.
#[derive(Debug, Clone)]
pub struct LineReader {
    buffer: Vec<u8>,
    /// How much of the buffer is known to hold no newline.
    scanned: usize,
    max_len: usize,
}

impl LineReader {
    /// A reader that takes lines of at most `max_len` bytes, such as [`MAX_LINE`].
    #[must_use]
    pub const fn new(max_len: usize) -> Self {
        Self {
            buffer: Vec::new(),
            scanned: 0,
            max_len,
        }
    }

    /// Adds what the transport read.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next whole line without its newline, or `None` while the buffer holds no whole line.
    ///
    /// # Errors
    ///
    /// [`FramingError::LineTooLong`] once the line being read is longer than the limit.
    pub fn next_line(&mut self) -> Result<Option<Vec<u8>>, FramingError> {
        let too_long = FramingError::LineTooLong {
            max_len: self.max_len,
        };
        let Some(found) = self.buffer[self.scanned..]
            .iter()
            .position(|byte| *byte == b'\n')
        else {
            self.scanned = self.buffer.len();
            return if self.buffer.len() > self.max_len {
                Err(too_long)
            } else {
                Ok(None)
            };
        };
        let end = self.scanned + found;
        if end > self.max_len {
            return Err(too_long);
        }
        let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
        self.scanned = 0;
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Ok(Some(line))
    }

    /// How many bytes are buffered and not yet taken as a line.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

/// Why a line or a frame header was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FramingError {
    /// A line ran past the reader's limit before its newline.
    LineTooLong {
        /// The reader's limit, in bytes.
        max_len: usize,
    },
    /// The header announced a payload longer than the reader takes.
    PayloadTooLong {
        /// The announced length.
        len: u64,
        /// The reader's limit.
        max_len: u64,
    },
    /// The header named payload `0`.
    ZeroId,
}

impl fmt::Display for FramingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineTooLong { max_len } => {
                write!(f, "a line is longer than the limit of {max_len} bytes")
            }
            Self::PayloadTooLong { len, max_len } => {
                write!(f, "a payload of {len} bytes is over the limit of {max_len}")
            }
            Self::ZeroId => f.write_str("a payload header named payload 0"),
        }
    }
}

impl std::error::Error for FramingError {}
