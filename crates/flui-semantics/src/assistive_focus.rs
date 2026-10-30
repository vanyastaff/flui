//! Where the assistive technology's own cursor is (VoiceOver, TalkBack), as
//! the subtrees it is in: each node's [`AssistiveFocusHandler`]s are told
//! `true` when the cursor enters the node's subtree and `false` when it
//! leaves.
//!
//! The cursor is the platform's, not the application's: it is reported by an
//! adapter whose [`AssistiveFocusSupport`](crate::AssistiveFocusSupport) is
//! `Reported`, never published. Input focus is a separate thing the
//! application moves.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

use crate::{AccessibilityNodeId, AssistiveFocusHandler, SemanticsOwner};

/// The handlers told the cursor is in their subtree, kept by the pipeline
/// across the semantics owners it makes, so that every `true` a handler was
/// told is followed by a `false`, whatever the tree did in between.
#[derive(Default)]
pub struct AssistiveFocus {
    node: Option<AccessibilityNodeId>,
    within: Vec<AssistiveFocusHandler>,
}

impl std::fmt::Debug for AssistiveFocus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssistiveFocus")
            .field("node", &self.node)
            .field("within", &self.within.len())
            .finish()
    }
}

impl AssistiveFocus {
    /// The node the cursor was last reported on, if it is on one.
    #[must_use]
    pub fn node(&self) -> Option<AccessibilityNodeId> {
        self.node
    }

    /// Move the cursor to `node` of `owner`'s tree, or off the tree for
    /// `None`. A node `owner` does not hold, or no owner at all, leaves every
    /// subtree. The handlers of the subtrees left are told `false`, then
    /// those entered `true`, when the returned change is invoked.
    pub fn move_to(
        &mut self,
        node: Option<AccessibilityNodeId>,
        owner: Option<&SemanticsOwner>,
    ) -> AssistiveFocusChange {
        let entered_chain = match (node, owner) {
            (Some(node), Some(owner)) => owner.assistive_focus_chain(node),
            _ => Vec::new(),
        };
        let mut calls = Vec::new();
        for handler in &self.within {
            if !entered_chain.iter().any(|kept| Rc::ptr_eq(kept, handler)) {
                calls.push((Rc::clone(handler), false));
            }
        }
        for handler in &entered_chain {
            if !self.within.iter().any(|held| Rc::ptr_eq(held, handler)) {
                calls.push((Rc::clone(handler), true));
            }
        }
        self.node = node;
        self.within = entered_chain;
        AssistiveFocusChange { calls }
    }
}

/// The handlers a move of the cursor tells, with what each is told, held to
/// run once no borrow of the pipeline or the tree is held.
#[must_use = "a change tells its handlers only when invoked"]
pub struct AssistiveFocusChange {
    calls: Vec<(AssistiveFocusHandler, bool)>,
}

impl std::fmt::Debug for AssistiveFocusChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssistiveFocusChange")
            .field(
                "calls",
                &self
                    .calls
                    .iter()
                    .map(|(_, within)| *within)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl AssistiveFocusChange {
    /// Whether the move tells no handler anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    /// Tell each handler, those left before those entered. A handler that
    /// panics does not keep the rest from being told; the first panic is
    /// resumed once all have been. After a failure the remaining handlers
    /// are kept rather than dropped, as every retirement after a failure is
    /// (ADR-0127).
    pub fn invoke(self) {
        let mut first = None;
        for (handler, within) in self.calls {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| handler(within))) {
                if first.is_none() {
                    first = Some(payload);
                } else {
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
            }
            if first.is_some() {
                std::mem::forget(handler);
            } else if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(handler))) {
                first = Some(payload);
            }
        }
        if let Some(payload) = first {
            resume_unwind(payload);
        }
    }
}
