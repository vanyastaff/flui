//! The order and the bounds of a native recording's rows: the order its reader checks from
//! the rows alone, and where its writer cuts it (ADR-0215 §4).

use flui_protocol::native_recording as wire;

use super::codec::{CloseAnswer, CloseCause, Row};
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

/// How many of `lines` a writer writes after a header of `header` bytes, and
/// why it cuts the rest: every row while they all fit, otherwise the rows that
/// fit with room after the last for the `cut` row that replaces the next.
pub(super) fn cut_point(header: usize, lines: &[Vec<u8>]) -> (usize, Option<wire::CutReason>) {
    let mut written = header;
    for (index, line) in lines.iter().enumerate() {
        let follows = index + 1 < lines.len();
        // The rows this one makes, and the cut a row after it would need.
        let rows = index + 1 + usize::from(follows);
        if rows > wire::MAX_ROWS {
            return (index, Some(wire::CutReason::Rows));
        }
        let room = if follows { CUT_LINE_ROOM } else { 0 };
        if written + line.len() + room > wire::MAX_RECORDING_BYTES {
            return (index, Some(wire::CutReason::Bytes));
        }
        written += line.len();
    }
    (lines.len(), None)
}
