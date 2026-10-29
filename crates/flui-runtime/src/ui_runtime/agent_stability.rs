//! What the actionability check remembers between calls, so it can say whether
//! an element holds still (R11: "stable").
//!
//! The check compares an element's bounds with those the previous call saw. A
//! comparison made with no frame between the two says nothing about whether
//! the element moves, so one counts only when a terminal frame has run since
//! the earlier call, or when the presentation has no work pending that could
//! still move the element. Owner thread only.

use std::collections::HashMap;

use flui_protocol::{ElementRef, LogicalRect};

use crate::epoch::TreeRevision;

/// The bounds one element had when the check last read it.
#[derive(Debug, Clone, Copy)]
struct Seen {
    revision: TreeRevision,
    rect: LogicalRect,
}

/// The bounds of the elements the check read lately, bounded in size.
#[derive(Debug, Default)]
pub(crate) struct AgentStability {
    seen: HashMap<ElementRef, Seen>,
}

impl AgentStability {
    /// The most elements remembered: a driver polls one element at a time, so
    /// a handful is plenty, and a full table starts again rather than grow.
    const MAX: usize = 64;

    /// Record that `element` has `rect` now (`None`: it is not attached), at
    /// `revision` of the presentation's terminal frames, and answer the
    /// bounds it had when it was last read if they can be compared: a frame
    /// has run since (`revision` moved), or nothing is pending (`idle`).
    pub(crate) fn observe(
        &mut self,
        element: ElementRef,
        revision: TreeRevision,
        idle: bool,
        rect: Option<LogicalRect>,
    ) -> Option<LogicalRect> {
        let Some(rect) = rect else {
            self.seen.remove(&element);
            return None;
        };
        let now = Seen { revision, rect };
        let Some(before) = self.seen.get(&element).copied() else {
            if self.seen.len() >= Self::MAX {
                self.seen.clear();
            }
            self.seen.insert(element, now);
            return None;
        };
        if revision > before.revision || idle {
            self.seen.insert(element, now);
            Some(before.rect)
        } else {
            None
        }
    }
}
