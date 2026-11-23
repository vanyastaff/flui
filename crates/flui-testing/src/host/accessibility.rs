//! The accessibility bridge a headless host plays for each window.

#[cfg(doc)]
use super::HeadlessHost;

use std::sync::atomic::{AtomicBool, Ordering};

use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, AssistiveFocusListener,
    PlatformAccessibility,
};
use parking_lot::Mutex;
use rustc_hash::FxHashSet;

/// The accessibility bridge of a [`HeadlessHost`]'s window: assistive
/// technology attaches when a test asks, published updates are folded into
/// the tree an adapter would hold, and the action listener the UI runtime
/// registers is handed to a test that plays the adapter.
#[derive(Default)]
// `Mutex` and atomic fields: `PlatformAccessibility` is `Send + Sync`.
pub(super) struct HeadlessAccessibility {
    active: AtomicBool,
    activation: Mutex<Option<AccessibilityActivationListener>>,
    pub(super) action: Mutex<Option<AccessibilityActionListener>>,
    pub(super) assistive_focus: Mutex<Option<AssistiveFocusListener>>,
    pub(super) published: Mutex<Option<accesskit::TreeUpdate>>,
}

impl HeadlessAccessibility {
    /// Attach assistive technology: the listener the UI runtime registered flips
    /// the presentation's semantics flag, which the next frame reconciles
    /// onto the pipeline.
    pub(super) fn attach(&self) {
        self.active.store(true, Ordering::Relaxed);
        let listener = self.activation.lock().clone();
        if let Some(listener) = listener {
            listener(true);
        }
    }
}

impl HeadlessAccessibility {
    /// Detach assistive technology: the listener clears the presentation's
    /// semantics flag, and the next frame stops collecting unless an agent
    /// still holds the tree.
    pub(super) fn detach(&self) {
        self.active.store(false, Ordering::Relaxed);
        let listener = self.activation.lock().clone();
        if let Some(listener) = listener {
            listener(false);
        }
    }
}

impl PlatformAccessibility for HeadlessAccessibility {
    /// Fold `update` into the published tree as an adapter does: an update
    /// carrying tree data replaces it, any other replaces the nodes it names,
    /// adds the rest, and drops those no longer reachable from the root.
    fn publish(&self, update: accesskit::TreeUpdate) {
        let mut published = self.published.lock();
        match published.as_mut() {
            Some(tree) if update.tree.is_none() => {
                for (id, node) in update.nodes {
                    match tree.nodes.iter_mut().find(|(known, _)| *known == id) {
                        Some(slot) => slot.1 = node,
                        None => tree.nodes.push((id, node)),
                    }
                }
                tree.focus = update.focus;
                // An adapter drops the nodes no longer reachable from the
                // root, so the kept tree tracks the live one instead of
                // accumulating every node ever published.
                if let Some(root) = tree.tree.as_ref().map(|data| data.root) {
                    let mut reachable = FxHashSet::<accesskit::NodeId>::from_iter([root]);
                    let mut pending = vec![root];
                    while let Some(id) = pending.pop() {
                        if let Some((_, node)) = tree.nodes.iter().find(|(known, _)| *known == id) {
                            for &child in node.children() {
                                if reachable.insert(child) {
                                    pending.push(child);
                                }
                            }
                        }
                    }
                    tree.nodes.retain(|(id, _)| reachable.contains(id));
                }
            }
            _ => *published = Some(update),
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn set_activation_listener(&self, listener: AccessibilityActivationListener) {
        *self.activation.lock() = Some(listener);
    }

    fn set_action_listener(&self, listener: AccessibilityActionListener) {
        *self.action.lock() = Some(listener);
    }

    /// A test plays the screen reader, so the cursor is reported: through
    /// [`HeadlessHost::move_assistive_focus`].
    fn assistive_focus_support(&self) -> flui_semantics::platform::AssistiveFocusSupport {
        flui_semantics::platform::AssistiveFocusSupport::Reported
    }

    fn set_assistive_focus_listener(&self, listener: AssistiveFocusListener) {
        *self.assistive_focus.lock() = Some(listener);
    }
}
