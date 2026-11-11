//! [`FocusHost`]: one focus node hosted by one element.
//!
//! The lifecycle wiring of a focus node is the same wherever the node lives:
//! it is configured from the widget, attached under the nearest enclosing
//! focus node, moved when that node changes, replaced when the widget names
//! another node, and detached when the element goes. [`Focus`] owns one
//! `FocusHost`, and so does every other element that hosts a focus node of
//! its own (ADR-0322 §1); none of them repeats the wiring.
//!
//! The host owns no element and no render object. Its owner forwards the
//! lifecycle hooks to it, tells it how the node is measured
//! ([`FocusHost::init`]) and publishes the node to the subtree
//! ([`FocusHost::provide`]).
//!
//! [`Focus`]: super::focus::Focus

use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::Subscription;
use flui_interaction::focus::{
    FocusAttachment, FocusCause, FocusManager, FocusNode, FocusNodeRegistration,
    FocusTraversalOverrides, KeyEventHandler, KeyEventResult, RectProvider,
};
use flui_platform::keyboard::KeyEvent;
use flui_view::prelude::*;
use flui_view::{Provided, RebuildHandle};

use super::actions::FocusRecord;
use super::focus::{
    FocusChangeHandler, FocusParent, enclosing_focus_parent, record_focus_context,
    request_autofocus,
};
use super::focus_reveal::{FocusRevealScope, SharedRevealTarget};

/// A key handler as a focus widget stores it: it decides the event's
/// routing, and may write signals through the `cx` it receives.
pub(crate) type FocusKeyHandler = Rc<dyn Fn(&mut EventCx<'_>, &KeyEvent) -> KeyEventResult>;

/// What the callbacks a [`FocusHost`] installs on its node read, and the
/// host writes: one allocation shared by the host, the node's listener and
/// its key handler.
///
/// It names the node weakly and the key handler names it weakly, so nothing
/// installed on a node keeps the node, or the host's callbacks, alive past
/// the host.
struct Shared {
    /// Set first in `init`; the focus-edge listener and the key handler
    /// open their writes from it (ADR-0086). A key event cannot reach the
    /// node before `init` attaches it, so the cell is set by then.
    writer: OnceCell<WriterSource>,
    /// The node the listener reads, which a live replacement moves before
    /// the focus tree announces it.
    observed_node: RefCell<Weak<FocusNode>>,
    /// Last `has_focus` value for the currently observed node.
    was_focused: Cell<bool>,
    /// Inherited-provider revision advanced by node notifications.
    node_revision: Cell<u64>,
    /// The current `on_focus_change` handler: the installed listener reads
    /// the *latest* one, so `update` writes here rather than reinstalling the
    /// listener. A handler captured by value would keep firing the one from
    /// the build that mounted it.
    on_focus_change: RefCell<Option<FocusChangeHandler>>,
    /// The current `on_visible_focus_change` handler, read the same way.
    on_visible_focus_change: RefCell<Option<FocusChangeHandler>>,
    /// Where the node's shown focus is revealed (`focus_reveal.rs`).
    reveal: RevealSlot,
}

/// Wrap a key handler into the node's shape, opening its write from the
/// host's writer source. A handler that outlives its host (an external node
/// another host adopted) ignores the event.
fn key_handler(handler: &FocusKeyHandler, shared: &Rc<Shared>) -> KeyEventHandler {
    let handler = Rc::clone(handler);
    let shared = Rc::downgrade(shared);
    Rc::new(move |event| {
        let Some(shared) = shared.upgrade() else {
            return KeyEventResult::Ignored;
        };
        let Some(writer) = shared.writer.get() else {
            tracing::warn!("Focus: a key event reached a node whose widget has not initialized");
            return KeyEventResult::Ignored;
        };
        writer.write(|cx| handler(cx, event))
    })
}

/// Which side is authoritative for the configuration stored on a focus node.
///
/// An external node is always *hosted* by the widget — attached, reparented,
/// and detached with it — but it can follow either configuration policy. A
/// managed external node lets the widget manage explicitly supplied
/// attributes. An external source leaves every node attribute under the
/// caller's control.
#[derive(Clone)]
pub(crate) enum FocusNodeOwnership {
    Internal,
    ManagedExternal(Rc<FocusNode>),
    ExternalSource(Rc<FocusNode>),
}

/// What a widget asks of the focus node its element hosts.
#[derive(Clone)]
pub(crate) struct FocusConfig {
    pub(crate) node_ownership: FocusNodeOwnership,
    pub(crate) autofocus: bool,
    pub(crate) can_request_focus: Option<bool>,
    pub(crate) skip_traversal: Option<bool>,
    pub(crate) descendants_are_focusable: Option<bool>,
    pub(crate) on_focus_change: Option<FocusChangeHandler>,
    pub(crate) on_visible_focus_change: Option<FocusChangeHandler>,
    pub(crate) on_key_event: Option<FocusKeyHandler>,
    pub(crate) debug_label: Option<&'static str>,
    pub(crate) traversal_overrides: Option<FocusTraversalOverrides>,
}

impl FocusConfig {
    /// A widget-owned node with every attribute at its default.
    pub(crate) fn new() -> Self {
        Self {
            node_ownership: FocusNodeOwnership::Internal,
            autofocus: false,
            can_request_focus: None,
            skip_traversal: None,
            descendants_are_focusable: None,
            on_focus_change: None,
            on_visible_focus_change: None,
            on_key_event: None,
            debug_label: None,
            traversal_overrides: None,
        }
    }

    fn sync_traversal_overrides(
        &self,
        node: &Rc<FocusNode>,
        registration: &mut Option<FocusNodeRegistration>,
    ) {
        if matches!(self.node_ownership, FocusNodeOwnership::ExternalSource(_)) {
            if let Some(previous) = registration.take() {
                previous.relinquish();
            }
        } else {
            *registration = self
                .traversal_overrides
                .as_ref()
                .map(|overrides| node.register_traversal_overrides(overrides.clone()));
        }
    }

    /// The node the host will drive: the external one, or a fresh one.
    ///
    /// A fresh node is the host's alone: once the host lets it go, a
    /// request a stale callback still makes on it answers
    /// [`Stale`](flui_interaction::FocusRequestOutcome::Stale) rather than
    /// waiting for an attachment that will never come.
    fn make_node(&self) -> Rc<FocusNode> {
        match &self.node_ownership {
            FocusNodeOwnership::Internal => {
                let node = FocusNode::with_debug_label(self.debug_label.unwrap_or("Focus"));
                flui_interaction::__runtime::mark_widget_owned(&node);
                node
            }
            FocusNodeOwnership::ManagedExternal(node)
            | FocusNodeOwnership::ExternalSource(node) => Rc::clone(node),
        }
    }

    /// Apply this configuration's node-attribute policy.
    ///
    /// Internal nodes receive a complete configuration, including defaults.
    /// A regular external node receives only explicit overrides, preserving
    /// caller state for omitted properties. A source-of-truth external node
    /// is never mutated. The generation-checked registration carries
    /// ownership across a managed-external rebuild that drops an override:
    /// the handler remains installed by design, but cleanup cannot erase a
    /// value written later by the external owner.
    fn configure(
        &self,
        node: &Rc<FocusNode>,
        key_handler_registration: &mut Option<FocusNodeRegistration>,
        shared: &Rc<Shared>,
    ) {
        match &self.node_ownership {
            FocusNodeOwnership::Internal => {
                node.set_can_request_focus(self.can_request_focus.unwrap_or(true));
                node.set_skip_traversal(self.skip_traversal.unwrap_or(false));
                node.set_descendants_are_focusable(self.descendants_are_focusable.unwrap_or(true));
                if let Some(handler) = &self.on_key_event {
                    *key_handler_registration =
                        Some(node.register_on_key_event(key_handler(handler, shared)));
                } else {
                    key_handler_registration.take();
                    node.clear_on_key_event();
                }
            }
            FocusNodeOwnership::ManagedExternal(_) => {
                if let Some(can_request_focus) = self.can_request_focus {
                    node.set_can_request_focus(can_request_focus);
                }
                if let Some(skip_traversal) = self.skip_traversal {
                    node.set_skip_traversal(skip_traversal);
                }
                if let Some(descendants_are_focusable) = self.descendants_are_focusable {
                    node.set_descendants_are_focusable(descendants_are_focusable);
                }
                if let Some(handler) = &self.on_key_event {
                    *key_handler_registration =
                        Some(node.register_on_key_event(key_handler(handler, shared)));
                }
            }
            FocusNodeOwnership::ExternalSource(_) => {
                if let Some(registration) = key_handler_registration.take() {
                    registration.relinquish();
                }
            }
        }
    }
}

/// What a shown focus on a hosted node is revealed through: the nearest
/// reveal scope above the element and the node's live geometry, both acquired
/// in lifecycle hooks, and whether the node showed the primary focus at the
/// last notification.
#[derive(Default)]
struct RevealSlot {
    target: RefCell<Option<SharedRevealTarget>>,
    rect: RefCell<Option<RectProvider>>,
    shown: Cell<bool>,
}

impl RevealSlot {
    /// The node took the shown primary focus: hand its measured rectangle to
    /// the scope above. Nothing to reveal before the node's first layout.
    /// Neither cell stays borrowed while the scope runs.
    fn reveal(&self) {
        let target = self.target.borrow().clone();
        let provider = self.rect.borrow().clone();
        let (Some(target), Some(provider)) = (target, provider) else {
            return;
        };
        if let Some(rect) = provider() {
            target.reveal(rect);
        }
    }
}

/// One focus node and its place in the presentation's focus tree, for the
/// lifetime of the element that owns this host.
pub(crate) struct FocusHost {
    traversal_registration: Option<FocusNodeRegistration>,
    /// What the callbacks installed on the node read.
    shared: Rc<Shared>,
    node: Rc<FocusNode>,
    /// Whether `node` is the one this host made for itself. A refused
    /// replacement leaves the hosted node behind the configuration, so an
    /// update compares the configuration with this, not with the previous
    /// one.
    owns_node: bool,
    /// Generation-checked ownership of the key handler this host installed.
    ///
    /// External source-of-truth nodes keep caller-owned handlers intact.
    /// Managed external nodes may retain an explicitly installed handler when
    /// a later configuration omits the override, so ownership is stateful
    /// rather than inferred from only the latest configuration.
    key_handler_registration: Option<FocusNodeRegistration>,
    /// The exact presentation-local manager acquired from the mounting build
    /// owner. Listener removal must go back to this same instance.
    focus_manager: Option<Rc<FocusManager>>,
    /// Generation-checked ownership of this host's attachment. Holding the
    /// token prevents stale lifecycle callbacks from detaching a newer host.
    /// Shared so the assistive focus request can tell whether this host
    /// still owns the node before it requests focus.
    attachment: Option<Rc<FocusAttachment>>,
    /// The node this one currently hangs under; `dependencies_changed`
    /// moves it when the provider changes.
    parent: Option<Rc<FocusNode>>,
    /// The live geometry source, retained so a replacement node receives the
    /// same one without reacquiring lifecycle-only context.
    rect_provider: Option<RectProvider>,
    /// Generation-checked ownership of the geometry source installed on the
    /// current node.
    rect_provider_registration: Option<FocusNodeRegistration>,
    /// The `Actions` chain visible at the element, recorded on the node so a
    /// `Shortcuts` above resolves intents from here (ADR-0079).
    action_chain: Option<FocusRecord>,
    /// Generation-checked ownership of that record on the current node.
    context_registration: Option<FocusNodeRegistration>,
    /// Lifecycle-acquired rebuild capability used by the node subscription.
    rebuild_handle: Option<RebuildHandle>,
    /// Listener installed on the current node. It drives both inherited
    /// dependents and the optional focus-edge callbacks.
    focus_subscription: Option<Subscription>,
    /// Captured at `new`: `init` has no configuration.
    autofocus: bool,
    /// One-shot latch: whether this host has already attempted its
    /// autofocus request.
    /// Set the moment the attempt is made, win or lose (an already-focused
    /// sibling can still make the attempt lose), so a later update that
    /// merely re-asserts the same `autofocus` value does not re-request.
    did_autofocus: bool,
}

impl FocusHost {
    /// Makes or takes the node and configures it. Nothing is acquired that
    /// [`dispose`](Self::dispose) would have to release: the node is not
    /// attached until [`init`](Self::init).
    pub(crate) fn new(config: &FocusConfig) -> Self {
        let node = config.make_node();
        let shared = Rc::new(Shared {
            writer: OnceCell::new(),
            observed_node: RefCell::new(Rc::downgrade(&node)),
            was_focused: Cell::new(false),
            node_revision: Cell::new(0),
            on_focus_change: RefCell::new(config.on_focus_change.clone()),
            on_visible_focus_change: RefCell::new(config.on_visible_focus_change.clone()),
            reveal: RevealSlot::default(),
        });
        let mut key_handler_registration = None;
        config.configure(&node, &mut key_handler_registration, &shared);
        let mut traversal_registration = None;
        config.sync_traversal_overrides(&node, &mut traversal_registration);
        Self {
            shared,
            node,
            owns_node: matches!(config.node_ownership, FocusNodeOwnership::Internal),
            key_handler_registration,
            traversal_registration,
            focus_manager: None,
            attachment: None,
            parent: None,
            rect_provider: None,
            rect_provider_registration: None,
            action_chain: None,
            context_registration: None,
            rebuild_handle: None,
            focus_subscription: None,
            autofocus: config.autofocus,
            did_autofocus: false,
        }
    }

    /// The hosted node.
    pub(crate) fn node(&self) -> &Rc<FocusNode> {
        &self.node
    }

    fn manager(&self) -> &Rc<FocusManager> {
        self.focus_manager
            .as_ref()
            .expect("BUG: a focus host was used before init installed its focus manager")
    }

    /// The rebuild-on-focus-change listener: descendants that read the node's
    /// state during build stay current, and the edge callbacks fire.
    fn add_focus_listener(&self, node: &Rc<FocusNode>) -> Subscription {
        let rebuild = self
            .rebuild_handle
            .as_ref()
            .expect("BUG: a focus listener was installed before init captured the rebuild handle")
            .clone();
        let shared = Rc::clone(&self.shared);
        let writer = shared
            .writer
            .get()
            .expect("BUG: a focus listener was installed before init acquired the writer source")
            .clone();
        node.subscribe(move || {
            let next_revision = shared
                .node_revision
                .get()
                .checked_add(1)
                .expect("BUG: Focus inherited revision exhausted");
            shared.node_revision.set(next_revision);
            rebuild.schedule(flui_view::RebuildReason::StateChange);

            // The host holds the node it observes; a notification that
            // arrives after the host let it go has nothing to report.
            let Some(node) = shared.observed_node.borrow().upgrade() else {
                return;
            };
            let now_focused = node.has_focus();
            let now_shown = node.has_visible_focus();
            drop(node);
            if shared.was_focused.replace(now_focused) != now_focused {
                // FocusNode clones its listener list before dispatch, so
                // user code may synchronously move focus. Clone the
                // callback out of the RefCell before invoking it for the
                // same re-entrancy reason.
                let handler = shared.on_focus_change.borrow().clone();
                if let Some(handler) = handler {
                    writer.write(|cx| handler(cx, now_focused));
                }
            }
            // Recorded before user code runs, so a notification it causes
            // compares against this one's value.
            let was_shown = shared.reveal.shown.replace(now_shown);
            if was_shown != now_shown {
                let handler = shared.on_visible_focus_change.borrow().clone();
                if let Some(handler) = handler {
                    writer.write(|cx| handler(cx, now_shown));
                }
            }
            // Shown focus is never left where the user cannot see it
            // (WCAG 2.4.11): on the edge into it, the scopes above bring the
            // node into view.
            if now_shown && !was_shown {
                shared.reveal.reveal();
            }
        })
    }

    /// Record the bindings in force at the element on the node (ADR-0079):
    /// the owner's `Actions` chain and platform convention. The owner calls
    /// it when its own bindings change between dependency changes.
    pub(crate) fn record(&mut self, record: Option<FocusRecord>) {
        record_focus_context(
            &self.node,
            record,
            &mut self.action_chain,
            &mut self.context_registration,
        );
    }

    fn install_focus_listener(&mut self) {
        if self.focus_subscription.is_some() {
            return;
        }
        self.shared.was_focused.set(self.node.has_focus());
        self.shared.reveal.shown.set(self.node.has_visible_focus());
        self.focus_subscription = Some(self.add_focus_listener(&self.node));
    }

    /// A one-shot autofocus attempt, made the first time `autofocus` is (or
    /// becomes) `true` and never repeated: `did_autofocus` latches whether or
    /// not the attempt won the focus. Only when the enclosing scope has
    /// nothing focused yet does the request land. Synchronous — FLUI has no
    /// end-of-frame focus batch.
    ///
    /// Derives the nearest scope from `parent` (kept current by `init` and
    /// `dependencies_changed`) rather than an ambient `BuildContext` lookup,
    /// so it can run from [`update`](Self::update) too, which has no context.
    fn try_autofocus(&mut self) {
        if self.did_autofocus || !self.autofocus {
            return;
        }
        self.did_autofocus = true;
        request_autofocus(self.parent.as_ref(), self.manager(), &self.node);
    }

    /// Listen, attach, autofocus — in that order, so a focus request queued
    /// on an external node before mount is observed when attach fulfills it.
    ///
    /// `rect_provider` measures the node: traversal order and the reveal on
    /// focus read it. The owner builds it from whatever it lays out — an
    /// anchor over its child, or its own render object.
    /// `record` is what [`record`](Self::record) takes: the bindings in force
    /// at the element when it mounts.
    pub(crate) fn init(
        &mut self,
        ctx: &dyn LifecycleContext,
        rect_provider: RectProvider,
        record: Option<FocusRecord>,
    ) {
        self.shared
            .writer
            .set(ctx.writer_source())
            .expect("BUG: a focus host is initialized once");
        self.focus_manager = Some(ctx.focus_manager());
        self.rebuild_handle = Some(ctx.rebuild_handle());
        let parent = enclosing_focus_parent(ctx);
        self.install_focus_listener();
        let rect_provider_registration =
            self.node.register_rect_provider(Rc::clone(&rect_provider));
        *self.shared.reveal.rect.borrow_mut() = Some(Rc::clone(&rect_provider));
        *self.shared.reveal.target.borrow_mut() = FocusRevealScope::of(ctx);
        self.rect_provider = Some(rect_provider);
        self.rect_provider_registration = Some(rect_provider_registration);
        self.attachment = Some(Rc::new(parent.attach_node(&self.node).expect(
            "BUG: a focus host could not attach its node to the enclosing focus tree",
        )));
        self.parent = Some(parent);
        self.record(record);

        self.try_autofocus();
    }

    /// The owner's `did_change_dependencies`: when the enclosing focus node
    /// changed, move the hosted node — with focus — under the new parent, via
    /// ADR-0026's adopt.
    pub(crate) fn dependencies_changed(
        &mut self,
        ctx: &dyn LifecycleContext,
        record: Option<FocusRecord>,
    ) {
        let parent = enclosing_focus_parent(ctx);
        if self
            .parent
            .as_ref()
            .is_none_or(|held| !Rc::ptr_eq(held, &parent))
        {
            self.attachment
                .as_ref()
                .expect("BUG: a mounted focus host must retain its FocusAttachment")
                .reparent(&parent)
                .expect("BUG: a focus host could not reparent within its presentation focus tree");
            self.parent = Some(parent);
        }
        self.record(record);
        *self.shared.reveal.target.borrow_mut() = FocusRevealScope::of(ctx);
    }

    /// The owner's `did_update_view`: follow a new configuration.
    pub(crate) fn update(&mut self, config: &FocusConfig) {
        // Install the current callbacks before any node replacement can emit
        // a focus edge. The node subscription always reads these cells.
        self.shared
            .on_focus_change
            .borrow_mut()
            .clone_from(&config.on_focus_change);
        self.shared
            .on_visible_focus_change
            .borrow_mut()
            .clone_from(&config.on_visible_focus_change);

        // Against the hosted node, not the previous configuration: a
        // replacement refused earlier left the node behind the one it was
        // asked for.
        let node_changed = match &config.node_ownership {
            FocusNodeOwnership::Internal => !self.owns_node,
            FocusNodeOwnership::ManagedExternal(node)
            | FocusNodeOwnership::ExternalSource(node) => !Rc::ptr_eq(node, &self.node),
        };

        if node_changed {
            let replacement = config.make_node();
            let mut replacement_key_handler_registration = None;
            let mut replacement_traversal_registration = None;
            config.sync_traversal_overrides(&replacement, &mut replacement_traversal_registration);
            config.configure(
                &replacement,
                &mut replacement_key_handler_registration,
                &self.shared,
            );
            let replacement_rect_provider_registration = self
                .rect_provider
                .as_ref()
                .map(|provider| replacement.register_rect_provider(Rc::clone(provider)));
            let replacement_context_registration = self
                .action_chain
                .as_ref()
                .map(|record| record.register_on(&replacement));
            let replacement_focus_subscription = self.add_focus_listener(&replacement);

            // Observe the replacement before the core transaction delivers
            // its stable-tree notification. Keep the previous `has_focus`
            // bit: when a focused descendant moves with the subtree, both old
            // and replacement nodes have focus and no synthetic edge fires.
            let previous = std::mem::replace(
                &mut *self.shared.observed_node.borrow_mut(),
                Rc::downgrade(&replacement),
            );
            let attachment = self
                .attachment
                .take()
                .expect("BUG: a mounted focus host must retain its FocusAttachment");
            let replacement_attachment = match attachment.replace_node(&replacement) {
                Ok(replacement_attachment) => replacement_attachment,
                Err(error) => {
                    // Refused before anything changed — a focus listener is
                    // running. Keep hosting the current node; the next update
                    // compares against it and tries the replacement again.
                    // Dropping the replacement's registrations clears only
                    // what this host installed on it.
                    tracing::warn!(
                        %error,
                        "Focus kept its node: the replacement was refused during a focus \
                         notification"
                    );
                    *self.shared.observed_node.borrow_mut() = previous;
                    self.attachment = Some(attachment);
                    drop((
                        replacement_focus_subscription,
                        replacement_key_handler_registration,
                        replacement_traversal_registration,
                        replacement_rect_provider_registration,
                        replacement_context_registration,
                    ));
                    self.autofocus = config.autofocus;
                    self.try_autofocus();
                    return;
                }
            };

            // The completed transaction made the old token stale and retired
            // the old node, so ancillary state can now be removed without
            // touching a newer host.
            self.key_handler_registration.take();
            self.traversal_registration.take();
            self.rect_provider_registration.take();
            self.context_registration.take();
            let previous_subscription = self
                .focus_subscription
                .replace(replacement_focus_subscription);
            self.node = replacement;
            self.owns_node = matches!(config.node_ownership, FocusNodeOwnership::Internal);
            self.key_handler_registration = replacement_key_handler_registration;
            self.traversal_registration = replacement_traversal_registration;
            self.rect_provider_registration = replacement_rect_provider_registration;
            self.context_registration = replacement_context_registration;
            self.attachment = Some(Rc::new(replacement_attachment));
            drop(previous_subscription);
        } else {
            // Re-sync flags and handlers from the latest configuration.
            config.sync_traversal_overrides(&self.node, &mut self.traversal_registration);
            config.configure(&self.node, &mut self.key_handler_registration, &self.shared);
        }

        self.autofocus = config.autofocus;
        // The changed-`autofocus` guard is `try_autofocus`'s own latch: an
        // update that flips `autofocus` from `false` to `true` makes the one
        // still-unattempted request; one that repeats an already-`true`
        // value does nothing.
        self.try_autofocus();
    }

    /// The owner's `dispose`: detach the node and release what this host
    /// installed on it.
    pub(crate) fn dispose(&mut self) {
        // An external node outlives its host: it must not keep measuring a
        // dead element or calling a host-owned key handler. Only the current
        // generation owns those registrations: a stale host must not erase
        // state installed by a newer host of the same external node.
        let owns_attachment = self
            .attachment
            .as_ref()
            .is_some_and(|attachment| attachment.is_attached());
        if owns_attachment {
            self.key_handler_registration.take();
            self.traversal_registration.take();
            self.rect_provider_registration.take();
            self.context_registration.take();
        } else if let Some(registration) = self.key_handler_registration.take() {
            // A superseded attachment has no authority to mutate this node.
            registration.relinquish();
        }
        if !owns_attachment && let Some(registration) = self.rect_provider_registration.take() {
            registration.relinquish();
        }
        if !owns_attachment && let Some(registration) = self.traversal_registration.take() {
            registration.relinquish();
        }
        if !owns_attachment && let Some(registration) = self.context_registration.take() {
            registration.relinquish();
        }
        drop(self.focus_subscription.take());
        self.shared.reveal.target.borrow_mut().take();
        self.shared.reveal.rect.borrow_mut().take();
        // The generation-checked attachment is the sole detach authority:
        // an old host cannot detach a node that a newer host adopted.
        if let Some(attachment) = self.attachment.take() {
            let _ = attachment.detach();
        }
        self.parent = None;
        self.rebuild_handle = None;
        self.focus_manager = None;
    }

    /// Publishes the hosted node as the parent of the focus nodes under the
    /// owner. The revision follows the node's notifications, so a dependent
    /// that reads the node's state in `build` rebuilds with it.
    pub(crate) fn provide(&self, values: &mut Provided<'_>) {
        values.value(FocusParent {
            parent: Rc::clone(&self.node),
            revision: self.shared.node_revision.get(),
        });
    }

    /// The focus request assistive technology makes on the node, or `None`
    /// when another host has adopted the node: only the current attachment
    /// may focus it, so this element neither advertises nor performs a
    /// request on the adopter's behalf. The request checks the attachment
    /// again when it runs.
    ///
    /// Assistive technology moving focus is keyboard-like focus: the
    /// indicator shows, as it does after Tab.
    pub(crate) fn assistive_focus_request(&self) -> Option<impl Fn() + 'static> {
        let attachment = self.attachment.as_ref().filter(|a| a.is_attached())?;
        let node = Rc::clone(&self.node);
        let attachment = Rc::downgrade(attachment);
        Some(move || {
            if attachment.upgrade().is_some_and(|a| a.is_attached()) {
                let _ = node.request_focus_with(FocusCause::Assistive);
            }
        })
    }
}
