use crate::events::PointerUpdate;

/// Bound retained history independently of how long delivery is deferred.
const MAX_COALESCED_HISTORY: usize = 100;

/// Preserve real samples in arrival order; predictions belong only to the
/// newest observation and must never be promoted into measured history.
pub(crate) fn prepend_motion_history(newer: &mut PointerUpdate, older: &mut PointerUpdate) {
    let mut history = std::mem::take(&mut older.coalesced);
    history.push(older.current.clone());
    history.append(&mut newer.coalesced);
    let excess = history.len().saturating_sub(MAX_COALESCED_HISTORY);
    history.drain(..excess);
    newer.coalesced = history;
}
