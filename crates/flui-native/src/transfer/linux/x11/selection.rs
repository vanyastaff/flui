//! The ICCCM `INCR` protocol beneath the X11 clipboard and XDND.
//!
//! This module owns no selection, offer, text encoding, payload or completion.
//! [`SelectionRead`] turns property replies into bounded protocol progress;
//! [`SelectionWrite`] numbers the chunks of one announced payload. The feature
//! adapters keep their ownership, queues, payload storage and error vocabulary.

#![forbid(unsafe_code)]

use std::ops::Range;

use web_time::Instant;
use x11rb::protocol::xproto::{self, GetPropertyReply};

/// The most bytes an `INCR` reader reserves from its peer's announcement.
/// Chunks, not the announcement, prove that any more room is needed.
pub(crate) const RESERVE_CAP: usize = 4 * 1024 * 1024;

/// Why a selection read was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadRefusal {
    /// The peer did not make the next protocol step by the read's deadline.
    Timeout,
    /// The announced or accumulated payload is past the caller's limit.
    TooLarge {
        /// The announced or accumulated size, including bytes not read.
        actual: u64,
        /// The caller's byte limit.
        limit: u64,
    },
    /// The peer's reply is not a valid next step of `INCR`.
    Protocol,
}

/// One step of reading a selection property.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadProgress {
    /// An `INCR` announcement or nonterminal chunk was accepted.
    Progress,
    /// The whole property or the terminal empty chunk completed the read.
    Done(Vec<u8>),
    /// The read was refused; its requestor must be retired by the adapter.
    Refused(ReadRefusal),
}

/// The bounded read of one selection property, whole or through `INCR`.
#[derive(Debug)]
pub(crate) struct SelectionRead {
    limit: u64,
    reserve_cap: usize,
    deadline: Instant,
    incremental: Option<Vec<u8>>,
}

impl SelectionRead {
    /// Read at most `limit` bytes, reserving at most `reserve_cap` from an
    /// announcement, and refuse a peer still busy at `deadline`.
    pub(crate) fn new(limit: u64, reserve_cap: usize, deadline: Instant) -> Self {
        Self {
            limit,
            reserve_cap,
            deadline,
            incremental: None,
        }
    }

    /// The next `GetProperty` request's length in 32-bit units. One unit past
    /// the room left makes an oversize property visible in `bytes_after`
    /// without reading or allocating the rest of it.
    pub(crate) fn request_words(&self) -> u32 {
        let room = self.limit.saturating_sub(self.received());
        u32::try_from(room / 4 + 1).unwrap_or(u32::MAX)
    }

    /// Whether an `INCR` announcement has been accepted.
    pub(crate) fn is_incremental(&self) -> bool {
        self.incremental.is_some()
    }

    /// The read's deadline.
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Read the first property reply of a conversion. `incr` is the `INCR`
    /// atom on this connection.
    pub(crate) fn first(&mut self, reply: GetPropertyReply, incr: xproto::Atom) -> ReadProgress {
        if self.incremental.is_some() {
            return ReadProgress::Refused(ReadRefusal::Protocol);
        }
        if reply.type_ == incr {
            let Some(announced) = reply.value32().and_then(|mut value| value.next()) else {
                return ReadProgress::Refused(ReadRefusal::Protocol);
            };
            let announced = u64::from(announced);
            if announced > self.limit {
                return self.too_large(announced);
            }
            let reserve = usize::try_from(announced)
                .unwrap_or(usize::MAX)
                .min(self.reserve_cap);
            // Deleting the announcement asked for the first chunk. Its bytes,
            // not this untrusted size, justify every later allocation.
            self.incremental = Some(Vec::with_capacity(reserve));
            return ReadProgress::Progress;
        }
        self.whole(reply)
    }

    /// Read one `INCR` chunk. An empty chunk with no unread bytes completes
    /// the transfer; it never appends or grows the accumulated buffer.
    pub(crate) fn chunk(&mut self, reply: GetPropertyReply) -> ReadProgress {
        let Some(bytes) = self.incremental.as_mut() else {
            return ReadProgress::Refused(ReadRefusal::Protocol);
        };
        let actual = measured(bytes.len(), &reply);
        if actual > self.limit {
            return self.too_large(actual);
        }
        if reply.bytes_after != 0 {
            return ReadProgress::Refused(ReadRefusal::Protocol);
        }
        if reply.value.is_empty() {
            let bytes = std::mem::take(bytes);
            return ReadProgress::Done(bytes);
        }
        append(bytes, &reply.value, self.limit);
        ReadProgress::Progress
    }

    /// Give a peer that made progress until `deadline` for its next step.
    /// Clipboard reads keep their original whole-read deadline; XDND uses
    /// this for its existing per-answer timeout.
    pub(crate) fn refresh(&mut self, deadline: Instant) {
        self.deadline = deadline;
    }

    /// The refusal to deliver if the read is due at `now`.
    pub(crate) fn expire(&self, now: Instant) -> Option<ReadRefusal> {
        (self.deadline <= now).then_some(ReadRefusal::Timeout)
    }

    /// The bytes already held, as a `u64` for checked protocol arithmetic.
    fn received(&self) -> u64 {
        self.incremental
            .as_ref()
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
    }

    /// Refuse a measured payload past this read's limit.
    fn too_large(&self, actual: u64) -> ReadProgress {
        ReadProgress::Refused(ReadRefusal::TooLarge {
            actual,
            limit: self.limit,
        })
    }

    /// Finish or refuse a whole, non-`INCR` property.
    fn whole(&self, reply: GetPropertyReply) -> ReadProgress {
        let actual = measured(0, &reply);
        if actual > self.limit {
            self.too_large(actual)
        } else if reply.bytes_after != 0 {
            ReadProgress::Refused(ReadRefusal::Protocol)
        } else {
            ReadProgress::Done(reply.value)
        }
    }

    /// The reserved bytes, inspected by the allocation regression cases.
    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.incremental.as_ref().map_or(0, Vec::capacity)
    }
}

/// The size a reply reports, including bytes the server did not send.
fn measured(held: usize, reply: &GetPropertyReply) -> u64 {
    u64::try_from(held)
        .unwrap_or(u64::MAX)
        .saturating_add(u64::try_from(reply.value.len()).unwrap_or(u64::MAX))
        .saturating_add(u64::from(reply.bytes_after))
}

/// Append a chunk whose total size was already checked. Past the reserved
/// room the buffer grows exactly, then geometrically, never past `limit`.
fn append(bytes: &mut Vec<u8>, chunk: &[u8], limit: u64) {
    let needed = bytes.len().saturating_add(chunk.len());
    if needed > bytes.capacity() {
        let limit = usize::try_from(limit).unwrap_or(usize::MAX);
        let grown = bytes
            .capacity()
            .saturating_mul(2)
            .clamp(needed, limit.max(needed));
        bytes.reserve_exact(grown - bytes.len());
    }
    bytes.extend_from_slice(chunk);
}

/// The protocol state of one outgoing `INCR` transfer.
///
/// The payload itself stays in the feature adapter. This value only knows
/// where to write it, its announced length and which range comes next.
#[derive(Debug)]
pub(crate) struct SelectionWrite {
    requestor: xproto::Window,
    property: xproto::Atom,
    target: xproto::Atom,
    len: usize,
    written: usize,
    ended: bool,
    deadline: Instant,
}

impl SelectionWrite {
    /// Announce `len` bytes to `requestor`'s `property` as `target`, with the
    /// first chunk due by `deadline`. X11's announcement is one `u32`.
    pub(crate) fn new(
        requestor: xproto::Window,
        property: xproto::Atom,
        target: xproto::Atom,
        len: u32,
        deadline: Instant,
    ) -> Self {
        Self {
            requestor,
            property,
            target,
            len: len as usize,
            written: 0,
            ended: false,
            deadline,
        }
    }

    /// Whether this write answers a deletion of `property` on `requestor`.
    pub(crate) fn matches(&self, requestor: xproto::Window, property: xproto::Atom) -> bool {
        self.requestor == requestor && self.property == property
    }

    /// The requestor whose property event mask the adapter manages.
    pub(crate) fn requestor(&self) -> xproto::Window {
        self.requestor
    }

    /// The property's target atom, used as each chunk's type.
    pub(crate) fn target(&self) -> xproto::Atom {
        self.target
    }

    /// Whether the terminal empty chunk has been written. The next deletion
    /// retires the writer; no further payload range follows.
    pub(crate) fn ended(&self) -> bool {
        self.ended
    }

    /// Take the next bounded payload range and move the chunk deadline to
    /// `deadline`. The last call returns `len..len` and marks the terminal
    /// empty chunk written.
    pub(crate) fn next_chunk(&mut self, chunk: usize, deadline: Instant) -> Range<usize> {
        debug_assert!(!self.ended, "BUG: a finished INCR writer is retired");
        let start = self.written.min(self.len);
        let end = start.saturating_add(chunk.max(1)).min(self.len);
        self.written = end;
        self.ended = start == self.len;
        self.deadline = deadline;
        start..end
    }

    /// The next chunk's deadline.
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Whether no deletion arrived by `now`.
    pub(crate) fn expired(&self, now: Instant) -> bool {
        self.deadline <= now
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Instant, ReadProgress, ReadRefusal, SelectionRead, SelectionWrite, xproto};

    const INCR: xproto::Atom = 41;
    const TARGET: xproto::Atom = 42;

    fn at(seconds: u64) -> Instant {
        Instant::now() + Duration::from_secs(seconds)
    }

    fn reply(value: &[u8], bytes_after: u32) -> x11rb::protocol::xproto::GetPropertyReply {
        x11rb::protocol::xproto::GetPropertyReply {
            format: 8,
            sequence: 0,
            length: 0,
            type_: TARGET,
            bytes_after,
            value_len: u32::try_from(value.len()).expect("a small test property"),
            value: value.to_vec(),
        }
    }

    fn announcement(announced: u32) -> x11rb::protocol::xproto::GetPropertyReply {
        x11rb::protocol::xproto::GetPropertyReply {
            format: 32,
            type_: INCR,
            value_len: 1,
            ..reply(&announced.to_ne_bytes(), 0)
        }
    }

    /// Whole properties, `INCR`, and the terminal empty chunk each have one
    /// protocol outcome; a deadline refuses either phase as a typed timeout.
    #[test]
    fn selection_read_progress_table() {
        let mut whole = SelectionRead::new(3, 4, at(10));
        assert_eq!(
            whole.first(reply(b"abc", 0), INCR),
            ReadProgress::Done(b"abc".to_vec())
        );

        let mut incremental = SelectionRead::new(3, 4, at(10));
        assert_eq!(
            incremental.first(announcement(3), INCR),
            ReadProgress::Progress
        );
        assert!(incremental.is_incremental());
        assert_eq!(incremental.chunk(reply(b"abc", 0)), ReadProgress::Progress);
        assert_eq!(
            incremental.chunk(reply(b"", 0)),
            ReadProgress::Done(b"abc".to_vec())
        );

        let mut empty = SelectionRead::new(0, 4, at(10));
        assert_eq!(empty.first(announcement(0), INCR), ReadProgress::Progress);
        assert_eq!(empty.chunk(reply(b"", 0)), ReadProgress::Done(Vec::new()));

        let mut pending = SelectionRead::new(3, 4, at(10));
        assert_eq!(pending.first(announcement(3), INCR), ReadProgress::Progress);
        for read in [SelectionRead::new(3, 4, at(10)), pending] {
            assert_eq!(read.expire(at(9)), None);
            assert_eq!(read.expire(at(10)), Some(ReadRefusal::Timeout));
        }
    }

    /// A 64 MiB announcement reserves only the common cap, and both an
    /// announcement and chunks that lie about their size are refused by
    /// measurement rather than trusted or copied past the limit.
    #[test]
    fn selection_read_bound_table() {
        let mut announced = SelectionRead::new(64 * 1024 * 1024, 16, at(10));
        assert_eq!(
            announced.first(announcement(64 * 1024 * 1024), INCR),
            ReadProgress::Progress
        );
        assert_eq!(
            announced.capacity(),
            16,
            "the announcement is not a reservation order"
        );

        let mut past = SelectionRead::new(10, 16, at(10));
        assert_eq!(
            past.first(announcement(64 * 1024 * 1024), INCR),
            ReadProgress::Refused(ReadRefusal::TooLarge {
                actual: 64 * 1024 * 1024,
                limit: 10,
            })
        );
        assert_eq!(past.capacity(), 0);

        let mut lying = SelectionRead::new(10, 16, at(10));
        assert_eq!(lying.first(announcement(1), INCR), ReadProgress::Progress);
        assert_eq!(lying.chunk(reply(b"123456", 0)), ReadProgress::Progress);
        assert_eq!(
            lying.chunk(reply(b"abcdef", 0)),
            ReadProgress::Refused(ReadRefusal::TooLarge {
                actual: 12,
                limit: 10,
            })
        );
        assert_eq!(lying.capacity(), 6, "the refused chunk was not appended");

        let mut whole = SelectionRead::new(10, 16, at(10));
        assert_eq!(
            whole.first(reply(b"123456", 5), INCR),
            ReadProgress::Refused(ReadRefusal::TooLarge {
                actual: 11,
                limit: 10,
            })
        );
    }

    /// A writer emits bounded ranges, ends only after the explicit empty
    /// chunk, and carries a deadline without owning any payload storage.
    #[test]
    fn selection_write_range_table() {
        for (len, chunk, ranges) in [
            (0, 2, vec![0..0]),
            (1, 2, vec![0..1, 1..1]),
            (5, 2, vec![0..2, 2..4, 4..5, 5..5]),
            (4, 8, vec![0..4, 4..4]),
        ] {
            let mut write = SelectionWrite::new(7, 8, TARGET, len, at(10));
            assert!(write.matches(7, 8));
            assert!(!write.matches(7, 9));
            assert_eq!(write.target(), TARGET);
            for range in ranges {
                assert!(!write.expired(at(9)));
                let deadline = at(20);
                assert_eq!(write.next_chunk(chunk, deadline), range, "len {len}");
                assert_eq!(write.deadline(), deadline);
            }
            assert!(write.ended(), "len {len} wrote its terminal chunk");
            assert!(write.expired(at(20)));
        }
    }
}
