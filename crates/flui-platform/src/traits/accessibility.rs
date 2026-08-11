//! Platform accessibility capability.
//!
//! [`PlatformAccessibility`] is reached through
//! [`PlatformWindow::accessibility`](super::window::PlatformWindow::accessibility),
//! the same fallible `Option<Arc<dyn _>>` discovery used by
//! [`PlatformTextInput`](super::text_input::PlatformTextInput) (ADR-0030) and
//! [`PlatformHaptics`](super::haptics::PlatformHaptics) (ADR-0031). A backend
//! with no accessibility integration returns `None` rather than every
//! `PlatformWindow` implementor inheriting methods it cannot honor.
//!
//! # It speaks AccessKit, never semantics types
//!
//! `flui-platform` is layer 2 and `flui-semantics` is layer 3, so this crate
//! cannot name a `SemanticsNode`. That is not a workaround — it is what forces
//! the translation to happen on the producing side, where the tree and its
//! stable identities live. What crosses this seam is an
//! [`accesskit::TreeUpdate`], already translated.
//!
//! # Both directions
//!
//! - **Out** — [`publish`](PlatformAccessibility::publish) hands the platform a
//!   tree.
//! - **In** — assistive technology attaches, detaches, and requests actions.
//!   Those arrive through listeners registered by the composition root, because
//!   a layer-2 crate has nothing to call.
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
/// build a semantics tree until something is listening, mirroring Flutter's
/// `ensureSemantics` refcount. Assembly costs a tree walk per frame, so leaving
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
/// The request also carries [`ActionRequest::target_tree`], which is what will
/// address the right presentation once more than one window publishes a tree.
pub type AccessibilityActionListener = Arc<dyn Fn(ActionRequest) + Send + Sync>;

/// Platform capability for exposing one window's accessibility tree.
pub trait PlatformAccessibility: Send + Sync {
    /// Hand the platform the current tree.
    ///
    /// Implementations are expected to skip the work when no assistive
    /// technology is attached. `update` is borrowed rather than owned precisely
    /// so an inactive implementation can drop it without paying for a clone —
    /// [`accesskit::TreeUpdate`] is `Clone`, so an active one clones inside its
    /// own active-check.
    fn publish(&self, update: &TreeUpdate);

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
}
