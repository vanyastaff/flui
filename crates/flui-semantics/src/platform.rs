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
//! - **In** — assistive technology attaches, detaches, requests actions, and
//!   may move its own cursor. Those arrive through the composition root's one
//!   [`AccessibilityHandler`], attached and revoked as a single contract.
//!
//! The inbound `NodeId`s are the same stable `AccessibilityNodeId` values the
//! published tree carried, which is what lets a composition root route an
//! action straight to `SemanticsOwner::resolve_action` with no translation
//! table in between.

use std::sync::Arc;

use accesskit::{ActionRequest, TreeUpdate};

/// The composition root's inbound side of one accessibility attachment.
///
/// Methods may run on an adapter's OS thread. They only transport the event;
/// the composition root delivers actions and cursor moves at its next owner
/// turn. Every method is required so an adapter cannot silently lose an event
/// by omitting a registration.
pub trait AccessibilityHandler: Send + Sync + 'static {
    /// Assistive technology attached (`true`) or detached (`false`).
    ///
    /// The composition root uses this to drive semantics assembly: FLUI does
    /// not build a semantics tree until something is listening.
    fn activation_changed(&self, active: bool);

    /// Assistive technology requested an action.
    ///
    /// [`ActionRequest::target_node`] is the stable node id from the last
    /// published tree. It may name a node that has since been removed, so the
    /// owner resolves rather than assumes, and treats a miss as a graceful
    /// drop. [`ActionRequest::target_tree`] must be
    /// [`TreeId::ROOT`](accesskit::TreeId::ROOT); another tree is refused when
    /// the request is decoded.
    fn action_requested(&self, request: ActionRequest);

    /// The assistive technology's own cursor moved to `node`, or left the
    /// window's tree for `None`. Distinct from input focus, which the
    /// application owns and publishes as the tree's `focus`.
    fn assistive_focus_changed(&self, node: Option<accesskit::NodeId>);
}

/// Whether an adapter tells where the assistive technology's cursor is.
///
/// A widget that should act on it, such as a carousel that stops moving
/// while a screen reader reads it, asks first: where the platform keeps the
/// cursor to itself, it relies on input focus and hover instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum AssistiveFocusSupport {
    /// The adapter calls [`AccessibilityHandler::assistive_focus_changed`]
    /// each time the cursor moves.
    Reported,
    /// The platform, or FLUI's adapter for it, does not say where the cursor
    /// is ([gap 36](https://github.com/vanyastaff/flui/blob/main/docs/gaps.md#36-accessibility-bridge-fidelity-properties-and-announcements)).
    #[default]
    Unreported,
}

/// How an adapter tells assistive technology that the innermost route of
/// the window's tree changed (ADR-0204 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RouteChangeSupport {
    /// The adapter conveys a [`RouteChange`] through the platform's own
    /// notice of a new screen or pane, told through
    /// [`PlatformAccessibility::route_changed`].
    Native,
    /// The platform has no such notice: the semantics owner speaks the new
    /// route's name as a polite announcement in the update that shows it.
    #[default]
    Announced,
}

/// The immutable delivery capabilities of one accessibility adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct AccessibilityCapabilities {
    assistive_focus: AssistiveFocusSupport,
    route_changes: RouteChangeSupport,
}

impl AccessibilityCapabilities {
    /// No assistive-cursor reporting; route changes are announced in the tree.
    pub const NONE: Self = Self::new(
        AssistiveFocusSupport::Unreported,
        RouteChangeSupport::Announced,
    );

    /// Capabilities of one adapter, returned when its handler attaches.
    #[must_use]
    pub const fn new(
        assistive_focus: AssistiveFocusSupport,
        route_changes: RouteChangeSupport,
    ) -> Self {
        Self {
            assistive_focus,
            route_changes,
        }
    }

    /// Whether the adapter reports the assistive technology's cursor.
    #[must_use]
    pub const fn assistive_focus(self) -> AssistiveFocusSupport {
        self.assistive_focus
    }

    /// How the adapter conveys an innermost route change.
    #[must_use]
    pub const fn route_changes(self) -> RouteChangeSupport {
        self.route_changes
    }
}

/// The adapter state observed atomically with a handler's attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct AccessibilityAttachment {
    active: bool,
    capabilities: AccessibilityCapabilities,
}

impl AccessibilityAttachment {
    /// An attachment snapshot made by an adapter.
    #[must_use]
    pub const fn new(active: bool, capabilities: AccessibilityCapabilities) -> Self {
        Self {
            active,
            capabilities,
        }
    }

    /// Whether assistive technology had already attached. If so, the
    /// composition root enables assembly from this snapshot rather than
    /// waiting for a transition that happened before registration.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.active
    }

    /// What the attached adapter can deliver.
    #[must_use]
    pub const fn capabilities(self) -> AccessibilityCapabilities {
        self.capabilities
    }
}

/// A second handler tried to attach before the first was detached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an accessibility handler is already attached")]
pub struct AccessibilityAlreadyAttached;

/// How the innermost route of a published tree changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RouteChangeKind {
    /// A route covered the one before it, which is still in the tree.
    Pushed,
    /// The route before it left the tree, and this route, published under it
    /// before, is the innermost again.
    Popped,
    /// The route before it left the tree, and this one is new.
    Replaced,
}

/// The innermost route of a published tree changed: the node that scopes it,
/// its name, and how it changed. Raised once per flush that changes it, never
/// for the first route a tree publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteChange {
    route: accesskit::NodeId,
    name: Option<String>,
    kind: RouteChangeKind,
}

impl RouteChange {
    pub(crate) fn new(
        route: accesskit::NodeId,
        name: Option<String>,
        kind: RouteChangeKind,
    ) -> Self {
        Self { route, name, kind }
    }

    /// The published node that scopes the route.
    #[must_use]
    pub fn route(&self) -> accesskit::NodeId {
        self.route
    }

    /// The route's name: the label of the node that names it, or a modal
    /// dialog's own name. `None` for a route nothing names.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// How the route changed.
    #[must_use]
    pub fn kind(&self) -> RouteChangeKind {
        self.kind
    }
}

/// Told each [`RouteChange`] once the update that shows it is delivered, by
/// a semantics owner whose adapter conveys route changes
/// [natively](RouteChangeSupport::Native).
pub type RouteChangeSink = Arc<dyn Fn(RouteChange) + Send + Sync>;

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

    /// Attach the composition root's one inbound handler.
    ///
    /// The returned snapshot carries the initial activation state and the
    /// adapter's capabilities, so there is no registration gap or second
    /// source for those facts. A second attachment is refused without
    /// replacing the live handler. After [`detach_handler`](Self::detach_handler),
    /// a new presentation may attach one.
    ///
    /// # Errors
    ///
    /// [`AccessibilityAlreadyAttached`] while another handler is attached.
    fn attach(
        &self,
        handler: Arc<dyn AccessibilityHandler>,
    ) -> Result<AccessibilityAttachment, AccessibilityAlreadyAttached>;

    /// Withdraw the attached handler. A later call is a no-op.
    ///
    /// The handler is removed under the adapter's state lock and destroyed
    /// after that lock is released. An event whose handler was already cloned
    /// may finish; later events have no handler to call.
    fn detach_handler(&self);

    /// Convey `change` through the platform's notice of a new screen or pane,
    /// after the update that shows the route was published. Called only when
    /// the attachment capabilities say [`Native`](RouteChangeSupport::Native).
    /// An adapter without a native notice explicitly drops the change.
    fn route_changed(&self, change: RouteChange);
}
