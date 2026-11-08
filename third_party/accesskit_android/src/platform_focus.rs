// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file) or the MIT license (found in
// the LICENSE-MIT file), at your option.

use accesskit::{NodeId, TreeId};
use std::{
    collections::VecDeque,
    fmt::{Debug, Formatter},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

/// Told when the assistive technology's own cursor (TalkBack's) moves to a
/// node of the tree or leaves it. This is not the tree's focus, which the
/// application owns and publishes.
pub trait PlatformFocusHandler {
    /// The cursor moved to `node`, or left the tree for `None`.
    fn platform_focus_changed(&mut self, node: Option<(NodeId, TreeId)>);
}

type FocusChange = Option<(NodeId, TreeId)>;
type Handler = Box<dyn PlatformFocusHandler + Send>;

#[derive(Default)]
struct State {
    /// Taken by the outermost delivery. A re-entrant or concurrent
    /// notification queues its change instead of calling the handler itself.
    handler: Option<Handler>,
    pending: VecDeque<FocusChange>,
}

/// The platform focus handler and the changes made while it runs. The handler
/// is taken out of the state during a call, so no lock is held around
/// application code and a panic cannot poison the queue.
pub(crate) struct PlatformFocus {
    state: Mutex<State>,
}

impl PlatformFocus {
    pub(crate) fn new(handler: impl 'static + PlatformFocusHandler + Send) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                handler: Some(Box::new(handler)),
                pending: VecDeque::new(),
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // No application code runs while this lock is held, so poisoning would
        // come from a panic in an internal assertion. The queued transitions
        // and handler remain coherent and must not become unreachable.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn notify(&self, node: FocusChange) {
        let handler = {
            let mut state = self.lock();
            state.pending.push_back(node);
            let Some(handler) = state.handler.take() else {
                return;
            };
            handler
        };
        Delivery {
            focus: self,
            handler: Some(handler),
        }
        .deliver();
    }
}

impl Debug for PlatformFocus {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformFocus").finish_non_exhaustive()
    }
}

/// Owns the handler for one outermost delivery. On ordinary completion the
/// queue check and handler restoration are one locked step, so a concurrent
/// notification cannot strand a change between them. On unwind, `Drop`
/// restores the handler and leaves changes queued before the panic for the
/// next notification, matching the iOS dispatcher's retry boundary.
struct Delivery<'a> {
    focus: &'a PlatformFocus,
    handler: Option<Handler>,
}

impl Delivery<'_> {
    fn deliver(mut self) {
        loop {
            let next = {
                let mut state = self.focus.lock();
                match state.pending.pop_front() {
                    Some(next) => next,
                    None => {
                        state.handler = self.handler.take();
                        return;
                    }
                }
            };
            self.handler
                .as_mut()
                .expect("a platform focus delivery owns its handler")
                .platform_focus_changed(next);
        }
    }
}

impl Drop for Delivery<'_> {
    fn drop(&mut self) {
        if self.handler.is_some() {
            self.focus.lock().handler = self.handler.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use super::*;

    const ROOT: NodeId = NodeId(1);
    const CHILD: NodeId = NodeId(2);
    const OTHER: NodeId = NodeId(3);

    struct ReentrantHandler {
        focus: Weak<PlatformFocus>,
        seen: Arc<Mutex<Vec<FocusChange>>>,
        entered: bool,
        first: bool,
        panic_first: bool,
    }

    impl PlatformFocusHandler for ReentrantHandler {
        fn platform_focus_changed(&mut self, node: FocusChange) {
            assert!(!self.entered, "the handler must not be re-entered");
            self.entered = true;
            self.seen.lock().unwrap().push(node);
            if self.first {
                self.first = false;
                let focus = self.focus.upgrade().unwrap();
                focus.notify(None);
                focus.notify(Some((CHILD, TreeId::ROOT)));
                if self.panic_first {
                    self.entered = false;
                    panic!("the test handler fails after queueing two changes");
                }
            }
            self.entered = false;
        }
    }

    fn reentrant_focus(panic_first: bool) -> (Arc<PlatformFocus>, Arc<Mutex<Vec<FocusChange>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let focus = Arc::new_cyclic(|focus| {
            let handler = ReentrantHandler {
                focus: Weak::clone(focus),
                seen: Arc::clone(&seen),
                entered: false,
                first: true,
                panic_first,
            };
            PlatformFocus {
                state: Mutex::new(State {
                    handler: Some(Box::new(handler)),
                    pending: VecDeque::new(),
                }),
            }
        });
        (focus, seen)
    }

    #[test]
    fn reentrant_changes_are_delivered_in_order_without_nesting() {
        let (focus, seen) = reentrant_focus(false);

        focus.notify(Some((ROOT, TreeId::ROOT)));

        assert_eq!(
            *seen.lock().unwrap(),
            [
                Some((ROOT, TreeId::ROOT)),
                None,
                Some((CHILD, TreeId::ROOT)),
            ]
        );
    }

    #[test]
    fn a_panic_keeps_later_changes_for_the_next_delivery() {
        let (focus, seen) = reentrant_focus(true);

        #[expect(
            clippy::disallowed_methods,
            reason = "the test pins recovery after a platform-focus handler panic"
        )]
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            focus.notify(Some((ROOT, TreeId::ROOT)));
        }));
        assert!(result.is_err());
        focus.notify(Some((OTHER, TreeId::ROOT)));

        assert_eq!(
            *seen.lock().unwrap(),
            [
                Some((ROOT, TreeId::ROOT)),
                None,
                Some((CHILD, TreeId::ROOT)),
                Some((OTHER, TreeId::ROOT)),
            ]
        );
    }
}
