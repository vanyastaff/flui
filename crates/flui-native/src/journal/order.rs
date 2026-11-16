//! The order and the bounds of a native recording's rows: the order its reader checks from
//! the rows alone, how much of a line it reads, and where its writer cuts it (ADR-0215 §4).

use std::io::{BufRead, Read};

use flui_protocol::native_recording as wire;

use super::codec::{CloseAnswer, CloseCause, CodecError, Row};
use super::store::RecordedStore;

/// The order a live stream writes its rows in, which a reader checks from the
/// rows alone: nothing after a cut; a window's close requests and close in an
/// order its delivery produces; and a text store lock only on a store a focus
/// row named, with its document recorded before it relies on it.
///
/// A recording that joined its stream late ([`Header::joined_late`]) begins at
/// the join: its first row follows rows it never saw. A rule is relaxed only
/// for context that would have come before that point, and only until the
/// recording has seen the context itself: a lock on a store no focus row in the
/// recording named (the store was focused before the join), and a first close
/// row that follows a grant the recording did not see. A store focused after
/// the join, and the close rows after the first, are held to every rule.
///
/// [`Header::joined_late`]: super::codec::Header::joined_late
pub(super) struct RowOrder {
    joined_late: bool,
    cut: bool,
    /// A close row was read: from here the close rules hold in full.
    close_seen: bool,
    /// A close request was granted (answered `close`, or by no handler).
    granted: bool,
    closed: bool,
    /// The stores the recording knows, and whether it holds each one's
    /// document now.
    stores: std::collections::BTreeMap<RecordedStore, bool>,
}

impl RowOrder {
    pub(super) const fn new(joined_late: bool) -> Self {
        Self {
            joined_late,
            cut: false,
            close_seen: false,
            granted: false,
            closed: false,
            stores: std::collections::BTreeMap::new(),
        }
    }

    /// Whether the rows before this one may hold context the recording never
    /// saw: it joined late, and this is the first row of its kind.
    const fn before_join(&self, seen: bool) -> bool {
        self.joined_late && !seen
    }

    /// Take `row`, or say why no live stream writes it here.
    pub(super) fn admit(&mut self, row: &Row) -> Result<(), String> {
        if self.cut {
            return Err(format!("a `{}` row follows the cut", row.name()));
        }
        match row {
            Row::Cut(_) => self.cut = true,
            Row::CloseRequest { answer, .. } => {
                if self.before_join(self.close_seen) && *answer == CloseAnswer::Closing {
                    // A request was granted before the join.
                    self.granted = true;
                }
                self.close_seen = true;
                if self.closed {
                    return Err("a close request follows the close".to_owned());
                }
                // Once a request was granted, every later one is `closing`,
                // and only then.
                if (*answer == CloseAnswer::Closing) != self.granted {
                    return Err(if self.granted {
                        "a request is asked after an earlier one was granted".to_owned()
                    } else {
                        "a request answered `closing` follows no granted request".to_owned()
                    });
                }
                if matches!(answer, CloseAnswer::Close | CloseAnswer::NoHandler) {
                    self.granted = true;
                }
            }
            Row::Closed(cause) => {
                if self.before_join(self.close_seen) && *cause == CloseCause::GrantedRequest {
                    // The request it closes for was granted before the join.
                    self.granted = true;
                }
                self.close_seen = true;
                if self.closed {
                    return Err("the window closes a second time".to_owned());
                }
                match cause {
                    CloseCause::GrantedRequest if !self.granted => {
                        return Err("a close for a granted request follows none".to_owned());
                    }
                    CloseCause::NoRequest if self.granted => {
                        return Err("a close asked nothing after a request was granted".to_owned());
                    }
                    _ => {}
                }
                self.closed = true;
            }
            Row::StoreFocus(Some(focus)) => {
                if self.stores.insert(focus.store, false).is_some() {
                    return Err(format!(
                        "store {} is focused a second time",
                        focus.store.get()
                    ));
                }
            }
            Row::StoreLock { lock, .. } => {
                if !self.stores.contains_key(&lock.store) {
                    if !self.joined_late {
                        return Err(format!(
                            "a lock on store {}, which no focus row named",
                            lock.store.get()
                        ));
                    }
                    // Focused before the join: whatever the journal held of
                    // its document then is out of the recording's sight.
                    self.stores.insert(lock.store, true);
                }
                let documented = self
                    .stores
                    .get_mut(&lock.store)
                    .expect("BUG: the store was inserted above");
                if lock.protected || lock.incomplete {
                    // The journal records the document again at the next lock.
                    *documented = false;
                } else if lock.opened.is_some() {
                    *documented = true;
                } else if !*documented {
                    return Err(format!(
                        "a lock on store {} before the recording holds its document",
                        lock.store.get()
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// How many bytes a writer keeps free for the [`wire::Row::Cut`] line it may
/// end a recording with: the longest such line and its newline, with room.
const CUT_LINE_ROOM: usize = 64;

/// The bytes and rows a recording has taken so far, against the bounds its
/// reader holds it to: every row while they all fit, otherwise the rows that
/// fit with room after the last for the `cut` row that replaces the next. The
/// in-process recorder keeps one as rows arrive and `NativeRecording::write`
/// one as it writes, under the same rule.
#[derive(Debug, Clone, Copy)]
pub(super) struct CutBudget {
    bytes: usize,
    rows: usize,
}

impl CutBudget {
    /// A recording whose header takes `header` bytes.
    pub(super) const fn after_header(header: usize) -> Self {
        Self {
            bytes: header,
            rows: 0,
        }
    }

    /// Take a row of `line` bytes, or say why the recording is cut in its
    /// place. `follows` says whether another row may come after it, which
    /// needs room for the cut that could replace that one.
    pub(super) fn admit(&mut self, line: usize, follows: bool) -> Result<(), wire::CutReason> {
        // The rows this one makes, and the cut a row after it would need.
        if self.rows + 1 + usize::from(follows) > wire::MAX_ROWS {
            return Err(wire::CutReason::Rows);
        }
        let room = if follows { CUT_LINE_ROOM } else { 0 };
        let bytes = self.bytes.saturating_add(line);
        if bytes.saturating_add(room) > wire::MAX_RECORDING_BYTES {
            return Err(wire::CutReason::Bytes);
        }
        self.bytes = bytes;
        self.rows += 1;
        Ok(())
    }
}

/// The bytes `value` takes as a line of a recording, its newline included,
/// counted without keeping the line. A value the format does not mirror
/// counts none here: the write that reaches it fails.
#[cfg(all(debug_assertions, feature = "native-capture"))]
pub(super) fn line_bytes<T: serde::Serialize>(value: Result<T, CodecError>) -> usize {
    /// A writer that keeps only how many bytes it was given.
    struct Counted(usize);

    impl std::io::Write for Counted {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let Ok(value) = value else {
        return 0;
    };
    // The newline is the first byte counted.
    let mut counted = Counted(1);
    serde_json::to_writer(&mut counted, &value).map_or(0, |()| counted.0)
}

/// Read line `line` into `buffer`, replacing what it held: how many bytes
/// were read, zero at the end. A line longer than [`wire::MAX_LINE_BYTES`]
/// is refused once that many bytes and one more were read, so no longer line
/// is ever held.
pub(super) fn read_bounded_line(
    input: &mut impl BufRead,
    buffer: &mut String,
    line: u64,
) -> Result<usize, CodecError> {
    // The bytes are read, measured and only then decoded: a line cut at the
    // bound may end inside a character, which is too long, not malformed.
    let mut bytes = std::mem::take(buffer).into_bytes();
    bytes.clear();
    // The bound, its line ending ("\r\n") and one byte that shows it overran.
    let most = u64::try_from(wire::MAX_LINE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(3);
    let read = Read::take(&mut *input, most).read_until(b'\n', &mut bytes)?;
    let content = bytes
        .iter()
        .rposition(|byte| !matches!(byte, b'\n' | b'\r'))
        .map_or(0, |last| last + 1);
    if content > wire::MAX_LINE_BYTES {
        return Err(CodecError::LineTooLong {
            line,
            limit: wire::MAX_LINE_BYTES,
        });
    }
    *buffer = String::from_utf8(bytes).map_err(|error| {
        CodecError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })?;
    Ok(read)
}
