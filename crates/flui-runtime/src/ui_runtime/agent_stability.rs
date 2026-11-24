//! What the actionability check remembers between calls, so it can say whether
//! an element holds still (R11: "stable").
//!
//! The check compares an element's bounds with those the previous call saw. A
//! comparison made with no frame between the two says nothing about whether
//! the element moves, so one counts only when a terminal frame has run since
//! the earlier call, or when the presentation has no work pending that could
//! still move the element. Owner thread only.

use rustc_hash::FxHashMap;

use flui_protocol::{geometry::LogicalRect, tree::ElementRef};

use super::agent_held::MAX_HELD_ACTS;
use crate::epoch::TreeRevision;

/// The bounds one element had when the check last read it.
#[derive(Debug, Clone, Copy)]
struct Seen {
    revision: TreeRevision,
    rect: LogicalRect,
    /// When the check last read it, in the table's own count of reads.
    read: u64,
}

/// The bounds of the elements the check read lately, bounded in size.
#[derive(Debug, Default)]
pub(crate) struct AgentStability {
    seen: FxHashMap<ElementRef, Seen>,
    /// How many reads the table has taken.
    reads: u64,
}

impl AgentStability {
    /// The most elements remembered: the two each held act reads at every
    /// frame boundary (a gesture's target and a drag's drop target, a scroll's
    /// scrollable and its match) for as many acts as a presentation holds,
    /// and room for one more element per act that a driver polls beside them.
    /// A full table forgets the element read longest ago, so an element read
    /// at every boundary stays while the elements read between two boundaries
    /// fit.
    const MAX: usize = 2 * MAX_HELD_ACTS + MAX_HELD_ACTS;

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
        self.reads = self.reads.saturating_add(1);
        let read = self.reads;
        let now = Seen {
            revision,
            rect,
            read,
        };
        let Some(before) = self.seen.get_mut(&element) else {
            if self.seen.len() >= Self::MAX {
                self.forget_oldest();
            }
            self.seen.insert(element, now);
            return None;
        };
        if revision > before.revision || idle {
            let earlier = before.rect;
            *before = now;
            Some(earlier)
        } else {
            // Read again within the frame: it stays, its bounds those of the earlier read.
            before.read = read;
            None
        }
    }

    /// Forget the element read longest ago.
    fn forget_oldest(&mut self) {
        let oldest = self
            .seen
            .iter()
            .min_by_key(|(_, seen)| seen.read)
            .map(|(element, _)| *element);
        if let Some(oldest) = oldest {
            self.seen.remove(&oldest);
        }
    }
}
