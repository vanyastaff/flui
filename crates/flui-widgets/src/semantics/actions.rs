//! Owner-local action handlers of a [`Semantics`](super::Semantics) node.
//!
//! The handlers stay in the owner's interaction lane as one table per node
//! (`EventActions`); the semantics configuration carries only a ticket to it,
//! and `deliver` resolves that ticket when an assistive technology invokes an
//! action.

use std::rc::Rc;

use flui_interaction::pointer::{LocalPayloadTarget, resolve_local_payload};
use flui_semantics::{ActionCall, SemanticsAction};
use flui_view::{EventContext, WriterSource};

use crate::support::ValueCallback;

/// One owner-local action handler: the dispatch's `EventContext` and the call,
/// payload included.
pub(super) type EventActionHandler = Rc<dyn Fn(&mut EventContext<'_>, ActionCall)>;

/// The owner-local action handlers one [`Semantics`](super::Semantics) registered, in
/// registration order: one per action (a later registration replaces an
/// earlier one, as `SemanticsConfiguration::add_action` does), and any number
/// of labelled custom actions.
#[derive(Clone, Default)]
pub(super) struct EventActions {
    /// Two are held inline: a control advertises its activation and, when
    /// it takes focus, the focus request.
    pub(super) handlers: smallvec::SmallVec<[(SemanticsAction, EventActionHandler); 2]>,
    pub(super) custom: Vec<CustomEntry>,
    /// Told when the assistive technology's cursor enters (`true`) or leaves
    /// the node's subtree.
    pub(super) assistive_focus: Option<ValueCallback<bool>>,
}

/// One labelled custom action, offered while its gate (if any) holds.
#[derive(Clone)]
pub(super) struct CustomEntry {
    pub(super) label: String,
    pub(super) handler: EventActionHandler,
    pub(super) enabled: Option<Rc<dyn Fn() -> bool>>,
}

impl CustomEntry {
    pub(super) fn is_offered(&self) -> bool {
        self.enabled.as_ref().is_none_or(|enabled| enabled())
    }
}

impl EventActions {
    pub(super) fn insert(&mut self, action: SemanticsAction, handler: EventActionHandler) {
        match self.handlers.iter_mut().find(|(known, _)| *known == action) {
            Some((_, slot)) => *slot = handler,
            None => self.handlers.push((action, handler)),
        }
    }

    pub(super) fn get(&self, action: SemanticsAction) -> Option<EventActionHandler> {
        self.handlers
            .iter()
            .find(|(known, _)| *known == action)
            .map(|(_, handler)| Rc::clone(handler))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.handlers.is_empty() && self.custom.is_empty() && self.assistive_focus.is_none()
    }

    pub(super) fn actions(&self) -> impl Iterator<Item = SemanticsAction> + '_ {
        self.handlers.iter().map(|(action, _)| *action)
    }
}

impl std::fmt::Debug for EventActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventActions")
            .field("handlers", &self.actions().collect::<Vec<_>>())
            .field("assistive_focus", &self.assistive_focus.is_some())
            .field(
                "custom",
                &self
                    .custom
                    .iter()
                    .map(|entry| &entry.label)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// What a mounted [`Semantics`](super::Semantics) keeps in the owner's interaction lane: its
/// action table, the writer each handler's `EventContext` is opened from, and how
/// many custom actions the configuration declared before the table's own, so
/// a custom action's id finds its handler.
pub(super) struct SemanticsActionCell {
    pub(super) writer: WriterSource,
    pub(super) actions: EventActions,
    pub(super) custom_base: usize,
}

/// Tell the node whose table `target` names that the assistive technology's
/// cursor entered (`true`) or left its subtree. A table gone, or one that no
/// longer asks, drops the news: the node is no longer there to hear it.
pub(super) fn deliver_assistive_focus(target: LocalPayloadTarget, within: bool) {
    let Ok(payload) = resolve_local_payload(target) else {
        tracing::debug!(
            within,
            "an assistive cursor move reached a node that is gone"
        );
        return;
    };
    let Ok(cell) = payload.downcast::<SemanticsActionCell>() else {
        tracing::error!("BUG: a semantics action ticket resolved to a payload of another type");
        return;
    };
    let Some(handler) = cell.actions.assistive_focus.clone() else {
        return;
    };
    cell.writer.write(|ctx| handler(ctx, within));
}

/// Run `action`'s owner-local handler for the node whose table `target`
/// names.
///
/// Reached through the `Send + Sync` handler the configuration advertises,
/// which the semantics owner invokes on the owner thread while the UI runtime is
/// entered. Every failure is a dropped action, never a panic: the platform
/// asked for something the tree can no longer do.
pub(super) fn deliver(target: LocalPayloadTarget, call: ActionCall) {
    let action = call.action();
    let payload = match resolve_local_payload(target) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                ?error,
                ?action,
                "a semantics action was dropped: its handler is owner-local and runs only \
                 inside its ui_runtime"
            );
            return;
        }
    };
    let Ok(cell) = payload.downcast::<SemanticsActionCell>() else {
        tracing::error!(
            ?action,
            "BUG: a semantics action ticket resolved to a payload of another type"
        );
        return;
    };
    let handler = if let ActionCall::CustomAction(id) = &call {
        // The id is the one the node's configuration gave the action; the
        // widget's own follow any the configuration declared before them.
        // A gated-off action is refused here too: a request may name one the
        // platform was shown before its enablement last changed.
        id.index()
            .checked_sub(cell.custom_base)
            .and_then(|index| cell.actions.custom.get(index))
            .filter(|entry| entry.is_offered())
            .map(|entry| Rc::clone(&entry.handler))
    } else {
        cell.actions.get(action)
    };
    let Some(handler) = handler else {
        tracing::debug!(
            ?action,
            "a semantics action arrived after its handler was removed; dropped"
        );
        return;
    };
    cell.writer.write(|ctx| handler(ctx, call));
}
