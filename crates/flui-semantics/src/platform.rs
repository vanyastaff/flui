//! The platform accessibility capability a backend implements.
//!
//! [`PlatformAccessibility`] is defined here, beside the AccessKit translation
//! that produces what it carries, and implemented by the backends in
//! `flui-native`. A window offers it through
//! `flui_native::HostWindow::accessibility`, the
//! same fallible `Option<Arc<dyn _>>` discovery used for text input
//! (ADR-0030) and haptics (ADR-0031): a backend with no accessibility
//! integration returns `None` rather than every window inheriting methods it
//! cannot honor. It is on the host-side subtrait, not on `PlatformWindow`,
//! because its signatures are AccessKit's and the window contract names none
//! (ADR-0082 §1).
//!
//! It lives in this crate rather than the backend crate so the frame runtime
//! that holds and wires a window's bridge can name it without linking a
//! platform backend (ADR-0082 §2, ADR-0083).
//!
//! # It speaks AccessKit, never semantics types
//!
//! No [`SemanticsNode`](crate::SemanticsNode) crosses this seam, although this
//! crate could name one: the translation happens on the producing side, where
//! the tree and its stable identities live, so what a backend receives is an
//! [`accesskit::TreeUpdate`], already translated, and a backend needs to know
//! nothing of the semantics tree.
//!
//! # Both directions
//!
//! - **Out** — [`publish`](PlatformAccessibility::publish) hands the platform a
//!   tree.
//! - **In** — assistive technology attaches, detaches, and requests actions.
//!   Those arrive through listeners registered by the composition root, because
//!   a backend has nothing to call.
//!
//! The inbound `NodeId`s are the same stable `AccessibilityNodeId` values the
//! published tree carried, which is what lets a composition root route an
//! action straight to `SemanticsOwner::resolve_action` with no translation
//! table in between.

use std::sync::Arc;

use accesskit::{ActionRequest, TreeUpdate};

/// Notified when assistive technology attaches (`true`) or detaches (`false`).
///
/// A composition root uses this to drive semantics assembly: FLUI does not
/// build a semantics tree until something is listening (a refcount of
/// interested listeners). Assembly costs a tree walk per frame, so leaving
/// it on unconditionally would charge every user for a feature almost none of
/// them have enabled.
pub type AccessibilityActivationListener = Arc<dyn Fn(bool) + Send + Sync>;

/// Receives an action requested by assistive technology.
///
/// [`ActionRequest::target_node`] is the stable node id from the last published
/// tree. It may name a node that has since been removed — assistive technology
/// acts on the snapshot it last read — so a handler resolves rather than
/// assumes, and treats a miss as a graceful drop.
///
/// The request also carries [`ActionRequest::target_tree`]. A window publishes one tree,
/// [`TreeId::ROOT`](accesskit::TreeId::ROOT), and the presentation the request is stamped
/// with decides which window it reaches; a request for any other tree is refused when it is
/// decoded ([`ActionDecodeError::UnknownTree`](crate::ActionDecodeError::UnknownTree)).
pub type AccessibilityActionListener = Arc<dyn Fn(ActionRequest) + Send + Sync>;

/// Receives where the assistive technology's own cursor (VoiceOver,
/// TalkBack) is: the stable id of the node it reads, or `None` once it leaves
/// the window's tree. Distinct from input focus, which the application owns
/// and publishes as the tree's `focus`.
pub type AssistiveFocusListener = Arc<dyn Fn(Option<accesskit::NodeId>) + Send + Sync>;

/// Whether an adapter tells where the assistive technology's cursor is.
///
/// A widget that should act on it, such as a carousel that stops moving
/// while a screen reader reads it, asks first: where the platform keeps the
/// cursor to itself, it relies on input focus and hover instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum AssistiveFocusSupport {
    /// The adapter calls its [`AssistiveFocusListener`] each time the cursor moves.
    Reported,
    /// The platform, or FLUI's adapter for it, does not say where the cursor
    /// is ([gap 36](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#36-accessibility-bridge-fidelity-properties-and-announcements)).
    #[default]
    Unreported,
}

/// Platform capability for exposing one window's accessibility tree.
pub trait PlatformAccessibility: Send + Sync {
    /// Hand the platform an update — incremental or self-contained.
    ///
    /// `SemanticsOwner::flush` publishes incrementally: most updates carry
    /// only the nodes that changed, with `TreeUpdate::tree` set to `None`.
    /// An update whose `tree` metadata is present is the producer's promise
    /// that it is **self-contained** (the initializing publish, a root
    /// change, and every reconnect-driven `send_full_tree`) — the only kind
    /// an implementation may retain to answer a late-activating screen
    /// reader on its own.
    ///
    /// Taken **by value** so an implementation can move it into the
    /// platform's own update call instead of cloning. An implementation with
    /// nothing attached simply drops it, which costs nothing the caller had
    /// not already spent building it.
    fn publish(&self, update: TreeUpdate);

    /// Whether assistive technology is currently attached.
    ///
    /// Advisory rather than a gate: it can go stale between the check and the
    /// call, so [`publish`](Self::publish) re-checks. Useful for diagnostics
    /// and for a composition root deciding whether assembly is worth starting.
    fn is_active(&self) -> bool;

    /// Register the attach/detach listener, replacing any previous one.
    fn set_activation_listener(&self, listener: AccessibilityActivationListener);

    /// Register the inbound-action listener, replacing any previous one.
    fn set_action_listener(&self, listener: AccessibilityActionListener);

    /// Whether this adapter tells where the assistive technology's cursor is.
    /// Each adapter states it: a host that starts reporting it changes this
    /// answer, and the platform parity table with it.
    fn assistive_focus_support(&self) -> AssistiveFocusSupport;

    /// Register the listener told where the assistive technology's cursor is,
    /// replacing any previous one. An adapter whose
    /// [`assistive_focus_support`](Self::assistive_focus_support) is
    /// [`Unreported`](AssistiveFocusSupport::Unreported) never calls it.
    fn set_assistive_focus_listener(&self, listener: AssistiveFocusListener) {
        drop(listener);
    }
}
