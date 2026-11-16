//! The order a native recording's rows come in, which its reader checks from
//! the rows alone (ADR-0215 §4).

use super::codec::{CloseAnswer, CloseCause, Row};
use super::store::RecordedStore;

/// The order a live stream writes its rows in, which a reader checks from the
/// rows alone: nothing after a cut; a window's close requests and close in an
/// order its delivery produces; and a text store lock only on a store a focus
/// row named, with its document recorded before it relies on it, unless the
/// recording says it joined late.
pub(super) struct RowOrder {
    joined_late: bool,
    cut: bool,
    /// A close request was granted (answered `close`, or by no handler).
    granted: bool,
    closed: bool,
    /// The stores focus rows named, and whether the recording holds each
    /// one's document now.
    stores: std::collections::BTreeMap<RecordedStore, bool>,
}

impl RowOrder {
    pub(super) const fn new(joined_late: bool) -> Self {
        Self {
            joined_late,
            cut: false,
            granted: false,
            closed: false,
            stores: std::collections::BTreeMap::new(),
        }
    }

    /// Take `row`, or say why no live stream writes it here.
    pub(super) fn admit(&mut self, row: &Row) -> Result<(), String> {
        if self.cut {
            return Err(format!("a `{}` row follows the cut", row.name()));
        }
        match row {
            Row::Cut(_) => self.cut = true,
            Row::CloseRequest { answer, .. } => {
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
                let Some(documented) = self.stores.get_mut(&lock.store) else {
                    if self.joined_late {
                        return Ok(());
                    }
                    return Err(format!(
                        "a lock on store {}, which no focus row named",
                        lock.store.get()
                    ));
                };
                if lock.protected || lock.incomplete {
                    // The journal records the document again at the next lock.
                    *documented = false;
                } else if lock.opened.is_some() {
                    *documented = true;
                } else if !*documented && !self.joined_late {
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
