//! SemanticsOwner - Manages the semantics tree lifecycle
//!
//! The SemanticsOwner coordinates updates to the semantics tree and
//! sends updates to the platform accessibility services.

use std::{rc::Weak, sync::Arc};

use flui_foundation::SemanticsId;
use flui_protocol::TreeRevision;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use thiserror::Error;

use crate::{
    action::{ActionCall, Reveal, SemanticsAction, SemanticsActionHandler, SemanticsActionRequest},
    identity::AccessibilityNodeId,
    node::SemanticsNode,
    relation::SemanticsAnchor,
    tree::SemanticsTree,
};

// ============================================================================
// CALLBACK TYPE
// ============================================================================

/// Callback for semantics updates.
///
/// Called when the semantics tree changes and needs to be sent to the platform.
///
/// The payload is a [`TreeUpdate`](crate::TreeUpdate) — the shape a platform
/// accessibility adapter consumes directly — rather than FLUI's own node type.
/// Two reasons, and the second is structural:
///
/// - The platform capability speaks accesskit types only, because
///   `flui-native` (layer 2) cannot depend on `flui-semantics` (layer 3).
///   Translating on the producing side is what keeps that edge absent.
/// - The payload must be keyed by the stable `AccessibilityNodeId` an adapter
///   publishes and routes actions back through — never by `SemanticsId`,
///   which is an arena position in a tree the pipeline rebuilds every pass.
///   (A per-node payload keyed on `SemanticsId` used to live here and was
///   removed for exactly that reason.) See [`crate::tree_to_update`].
pub type SemanticsUpdateCallback = Arc<dyn Fn(&crate::TreeUpdate) + Send + Sync>;

// ============================================================================
// ACTION RESOLUTION
// ============================================================================

/// Why an accessibility action could not be resolved against the current tree.
///
/// Platform routers intentionally treat these outcomes as graceful drops:
/// assistive technologies may act on an older snapshot after a node has been
/// removed or its actions changed. Keeping the reason typed makes that
/// forgiving behavior observable without turning stale platform input into a
/// panic.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SemanticsActionError {
    /// No rooted semantics result is currently available.
    #[error("no rooted semantics tree is available")]
    SemanticsUnavailable,

    /// The stable identity from the platform snapshot is no longer live.
    #[error("accessibility node {node_id} is no longer present")]
    NodeNotFound {
        /// Stable platform-facing node identity.
        node_id: AccessibilityNodeId,
    },

    /// A malformed tree exposes the same stable identity more than once.
    #[error("accessibility node identity {node_id} resolves to multiple live nodes")]
    AmbiguousNode {
        /// Duplicated platform-facing node identity.
        node_id: AccessibilityNodeId,
    },

    /// The current node no longer exposes the requested action.
    #[error("accessibility node {node_id} does not expose action {action:?}")]
    UnsupportedAction {
        /// Stable platform-facing node identity.
        node_id: AccessibilityNodeId,
        /// Action absent from the node's effective action mask.
        action: SemanticsAction,
    },

    /// The node publishes as disabled, so it takes no user action. Focus,
    /// blur and reveal still reach it: they move the viewport or the
    /// cursor, not the control.
    #[error("accessibility node {node_id} is disabled")]
    Disabled {
        /// Stable platform-facing node identity.
        node_id: AccessibilityNodeId,
    },

    /// The requested numeric value is absent, malformed, or outside the current range.
    #[error("accessibility node {node_id} refused an invalid numeric value")]
    InvalidNumericValue {
        /// Stable platform-facing node identity.
        node_id: AccessibilityNodeId,
    },

    /// The owning presentation has begun or completed teardown; actions are
    /// refused regardless of whether the node itself still resolves.
    #[error("presentation is closing or closed; accessibility action refused")]
    PresentationClosed,
}

/// A resolved action whose handler has been cloned out of the semantics tree.
///
/// Resolution and invocation are deliberately separate. A caller may resolve
/// this value while borrowing the presentation's pipeline owner, release that
/// borrow, and only then invoke or retire the snapshot. Reentrant handlers and
/// callback destructors can therefore reach the same owner without overlapping
/// a tree borrow.
#[must_use = "resolved semantics actions must be invoked or intentionally dropped"]
pub struct SemanticsActionInvocation {
    node_id: AccessibilityNodeId,
    action: SemanticsAction,
    delivery: Option<ActionDelivery>,
}

enum ActionDelivery {
    Direct {
        call: ActionCall,
        handler: SemanticsActionHandler,
    },
    Reveal {
        path: Vec<Weak<()>>,
        steps: Vec<(SemanticsActionHandler, Reveal)>,
    },
}

fn retire_reveal_handler(
    handler: SemanticsActionHandler,
    first: &mut Option<Box<dyn std::any::Any + Send>>,
    unwinding: bool,
) {
    // User-owned captures are an independent failure boundary, including when
    // an intentionally dropped snapshot is their last owner.
    if unwinding || first.is_some() {
        std::mem::forget(handler);
    } else if let Err(payload) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(handler)))
    {
        *first = Some(payload);
    }
}

impl Drop for SemanticsActionInvocation {
    fn drop(&mut self) {
        if let Some(ActionDelivery::Reveal { steps, .. }) = self.delivery.take() {
            let mut first = None;
            let unwinding = std::thread::panicking();
            for (handler, _) in steps {
                retire_reveal_handler(handler, &mut first, unwinding);
            }
            if let Some(payload) = first {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

impl std::fmt::Debug for SemanticsActionInvocation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SemanticsActionInvocation")
            .field("node_id", &self.node_id)
            .field("action", &self.action)
            .field("delivery", &"<resolved callbacks>")
            .finish()
    }
}

impl SemanticsActionInvocation {
    /// Stable identity of the node whose handler was resolved.
    #[inline]
    #[must_use]
    pub const fn node_id(&self) -> AccessibilityNodeId {
        self.node_id
    }

    /// Action passed to the handler.
    #[inline]
    #[must_use]
    pub const fn action(&self) -> SemanticsAction {
        self.action
    }

    /// Invoke the cloned handler.
    ///
    /// No semantics-tree borrow is held while user code runs.
    pub fn invoke(mut self) {
        match self
            .delivery
            .take()
            .expect("BUG: action delivery is consumed once")
        {
            ActionDelivery::Direct { call, handler } => handler(call),
            ActionDelivery::Reveal { path, steps } => {
                let mut first = None;
                let unwinding = std::thread::panicking();
                for (handler, reveal) in steps {
                    if !unwinding && path.iter().all(|member| member.strong_count() != 0) {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handler(ActionCall::ScrollIntoView(Some(reveal)));
                        }));
                        if let Err(payload) = result {
                            if first.is_none() {
                                first = Some(payload);
                            } else {
                                flui_foundation::panic::retain_opaque_payload(payload);
                            }
                        }
                    }
                    retire_reveal_handler(handler, &mut first, unwinding);
                }
                if let Some(payload) = first {
                    std::panic::resume_unwind(payload);
                }
            }
        }
    }
}

// ============================================================================
// SEMANTICS OWNER
// ============================================================================

/// Manages the semantics tree lifecycle and platform updates.
///
/// SemanticsOwner is responsible for:
/// 1. Managing the semantics tree
/// 2. Tracking dirty nodes that need updates
/// 3. Flushing updates to the platform accessibility services
/// 4. Producing immutable full-tree snapshots for adapter handoff
///
/// # Responsibilities
///
/// - Owns the semantics tree for a render tree
/// - Manages update lifecycle (mark dirty → flush)
/// - Sends updates to platform channel
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::RenderId;
/// use flui_semantics::{
///     SemanticsConfiguration, SemanticsNode, SemanticsOwner, SemanticsRole, TreeUpdate,
/// };
///
/// // The platform callback receives each translated update.
/// let mut owner = SemanticsOwner::new(Arc::new(|update: &TreeUpdate| {
///     assert!(!update.nodes.is_empty());
/// }));
///
/// let mut button = SemanticsConfiguration::new();
/// button.set_label("Submit");
/// button.set_role(SemanticsRole::Button);
/// let id = owner.insert(
///     SemanticsNode::new()
///         .with_source_render_id(RenderId::new(1))
///         .with_config(button),
/// );
/// owner.set_root(Some(id));
///
/// assert_eq!(owner.flush(), 1);
/// ```
pub struct SemanticsOwner {
    /// The semantics tree.
    tree: SemanticsTree,

    /// Platform callback for sending updates.
    callback: Option<SemanticsUpdateCallback>,

    /// Whether semantics is enabled.
    enabled: bool,

    /// What the last update told the platform, keyed by stable identity.
    ///
    /// `None` until the first publish (or after
    /// [`Self::schedule_full_publish`] deliberately forgets it): the next
    /// flush is then a self-contained full update. While `Some`, a flush
    /// diffs each dirty node's translation against this mirror and publishes
    /// only what actually changed — which is what keeps a rebuild-everything
    /// assembly pass (flui-semantics ARCHITECTURE.md, semantics assembly) from republishing an entire tree because one
    /// checkbox toggled.
    published: Option<PublishedState>,

    /// Forces the next flush to publish the complete tree even if the diff
    /// would be empty. Set by [`Self::send_full_tree`] and
    /// [`Self::schedule_full_publish`]; cleared once a full update is
    /// actually delivered (an unrooted tree leaves it pending, exactly like
    /// the dirty bits it travels with).
    full_publish_pending: bool,

    /// Physical pixels per logical pixel of the window this tree is
    /// published to, carried by the published root as a scale transform
    /// (see [`Self::set_device_pixel_ratio`]).
    device_pixel_ratio: f64,

    /// Arena ids of the nodes currently declaring themselves focused, maintained
    /// incrementally so a flush derives focus in O(dirty) instead of
    /// re-scanning every node:
    ///
    /// - a dirty node examined during an incremental flush is inserted or
    ///   removed by what its config now says;
    /// - every full publish rebuilds the set from a whole-tree scan (the
    ///   full path is O(tree) anyway);
    /// - dead entries are dropped at resolution time by a liveness check
    ///   over the (tiny) set itself.
    ///
    /// Keyed by **arena id, not stable id**, so an unpublishable claimant
    /// still counts: ambiguity must match what the full path
    /// (`tree_to_update`'s `focused_node`) would derive — two claimants
    /// mean the root, even when only one of them could ever be published.
    /// Addressability is resolved at publish time instead: a single
    /// claimant with no stable identity also falls back to the root,
    /// exactly like the full path's failed `stable_id` lookup.
    ///
    /// Arena-id keying survives slot reuse because a recycled slot is a
    /// fresh insert, fresh inserts are dirty, and dirty nodes are always
    /// re-examined before resolution — a stale entry can only refer to a
    /// vacant slot, which the liveness check drops. A node whose focus
    /// flag changes without its dirty bit is invisible here — the
    /// identical staleness contract the payload diff itself has.
    focus_claimants: FxHashSet<SemanticsId>,

    /// The nodes declaring anchors and the nodes holding relations,
    /// maintained like [`Self::focus_claimants`] so an incremental flush can
    /// resolve relations without scanning the arena (ADR-0205).
    relations: RelationIndex,

    /// How many arena entries the last flush inspected as publish/diff
    /// candidates — the scaling oracle for the O(dirty) claim: a
    /// single-node change must inspect one entry, not the arena.
    ///
    /// One definition on both paths: a full publish inspects every arena
    /// entry (`tree_to_update` iterates the whole slab and pays each
    /// node's translation before the addressability check discards an
    /// unpublishable one), so it reports `tree.len()`; an incremental
    /// flush reports the dirty set's live, still-dirty survivors —
    /// including unpublishable ones, whose focus claims it must examine —
    /// plus every anchored node it resolves relations against and every clean
    /// relation source it re-translates, so an incremental
    /// flush is O(dirty + anchors + relation sources) and says so.
    #[cfg(any(test, feature = "testing"))]
    examined_last_flush: usize,

    /// What publications found wrong with what authors declared and the frame
    /// driver has not yet reported, oldest first.
    diagnostics: std::collections::VecDeque<SemanticsDiagnostic>,

    /// How many publications have changed what the platform was told (see
    /// [`Self::revision`]).
    revision: TreeRevision,

    /// The identifier clashes the last check found, so each is reported once
    /// for as long as it lasts (debug builds only).
    #[cfg(debug_assertions)]
    reported_clashes: Vec<DuplicateIdentifier>,
    /// The presentation's announcement queue and announcer nodes, published
    /// as the last children of the root (ADR-0204 §4).
    announcements: Option<crate::Announcements>,
}

/// An author error a publication found: published as declared, or left out,
/// and reported by the frame driver.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SemanticsDiagnostic {
    /// An attribute the node's role does not support, left out of the update.
    InapplicableAttribute(InapplicableAttribute),
    /// Siblings that share an identifier, published as written.
    DuplicateIdentifier(DuplicateIdentifier),
}

/// A role-dependent attribute a published node declared and its role does
/// not support, left out of the update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct InapplicableAttribute {
    /// The node that declared it.
    pub node: AccessibilityNodeId,
    /// The node's declared role.
    pub role: crate::SemanticsRole,
    /// The attribute the role does not support.
    pub attribute: crate::applicability::RoleAttribute,
}

/// Siblings in the published tree, as assistive technology sees it, that share
/// an identifier, which must be unique among siblings (ADR-0240 §4). Found by
/// debug builds after a publication that reshaped the tree or changed an
/// identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DuplicateIdentifier {
    /// The identifier they share.
    pub identifier: Arc<str>,
    /// Their parent.
    pub parent: AccessibilityNodeId,
    /// The siblings carrying it, in tree order.
    pub siblings: Vec<AccessibilityNodeId>,
}

/// Reports `diagnostic` as a warning. Called by the frame driver with no
/// borrow of the pipeline held, since a `tracing` subscriber is user code.
pub fn report_diagnostic(diagnostic: &SemanticsDiagnostic) {
    match diagnostic {
        SemanticsDiagnostic::InapplicableAttribute(finding) => tracing::warn!(
            node = ?finding.node,
            role = %finding.role,
            attribute = finding.attribute.aria_name(),
            "a semantics attribute its role does not support is not published"
        ),
        SemanticsDiagnostic::DuplicateIdentifier(clash) => tracing::warn!(
            identifier = &*clash.identifier,
            parent = ?clash.parent,
            siblings = ?clash.siblings,
            "siblings share a semantics identifier, which must be unique among siblings"
        ),
    }
}

/// The inapplicable attributes `node` declares, when it is published.
fn inapplicable_of(node: &SemanticsNode) -> impl Iterator<Item = InapplicableAttribute> + '_ {
    let attrs = node.config().attributes();
    node.accessibility_id().into_iter().flat_map(move |id| {
        crate::applicability::inapplicable(attrs).map(move |attribute| InapplicableAttribute {
            node: id,
            role: attrs.effective_role(),
            attribute,
        })
    })
}

/// One custom action as the last delivered update offered it, kept so a
/// request is checked against the action's identity rather than its label.
struct PublishedCustomAction {
    /// The handler's allocation, held weakly: identity without keeping the
    /// handler's captures alive, and without letting its address be reused.
    handler: std::sync::Weak<dyn Fn(ActionCall) + Send + Sync>,
    own_id: crate::CustomActionId,
    label: smol_str::SmolStr,
}

impl PublishedCustomAction {
    /// Whether `declared` is this action: the same handler, the same id its
    /// own configuration gave it and the same label.
    fn is(&self, declared: &crate::DeclaredCustomAction<'_>) -> bool {
        self.own_id == declared.own_id
            && self.label == declared.label
            && std::ptr::addr_eq(self.handler.as_ptr(), Arc::as_ptr(declared.handler))
    }
}

/// What a delivered update offered on a node beyond its AccessKit payload,
/// which names neither a custom action's handler nor which handler a value
/// write reaches.
#[derive(Default)]
struct OfferRecord {
    /// The node's custom actions, in id order.
    custom: SmallVec<[PublishedCustomAction; 2]>,
    /// Whether a text write was read as a number ([`reads_text_as_number`]).
    text_as_number: bool,
}

impl OfferRecord {
    /// What `node` offers, as its update publishes it.
    fn of(node: &SemanticsNode) -> Self {
        let config = node.config();
        Self {
            custom: config
                .offered_custom_actions()
                .map(|declared| PublishedCustomAction {
                    handler: Arc::downgrade(declared.handler),
                    own_id: declared.own_id,
                    label: declared.label.into(),
                })
                .collect(),
            text_as_number: reads_text_as_number(config),
        }
    }

    fn is_empty(&self) -> bool {
        self.custom.is_empty() && !self.text_as_number
    }
}

/// Whether a `SetValue` text sent to this node reaches its numeric handler: a
/// range with no text handler. UI Automation writes through the `Value`
/// pattern, as a string, whenever the range also shows text
/// (`accesskit_windows` 0.35), and such a range is published writable when
/// only its numeric handler exists.
fn reads_text_as_number(config: &crate::SemanticsConfiguration) -> bool {
    config.effective_actions_as_bits() & SemanticsAction::SetValue.value() == 0
        && crate::applicability::retain_applicable(config.attributes())
            .numeric_range
            .is_some()
}

/// What the last delivered update showed of the node a request names: what
/// the adapter can have offered, so the most a request may ask of the current
/// node.
#[derive(Clone, Copy)]
enum Delivered<'a> {
    /// No update is on record (none published yet, or a reconnect forgot
    /// what the adapter holds): a request is checked against the current
    /// tree alone.
    Unknown,
    /// The node as delivered and what else it offered, if anything.
    Node(&'a accesskit::Node, Option<&'a OfferRecord>),
}

impl Delivered<'_> {
    /// Whether the delivered node offered what a request for `action` asks.
    fn offers(self, action: SemanticsAction) -> bool {
        match self {
            Self::Unknown => true,
            Self::Node(node, _) => crate::accesskit_translation::offered_as(action)
                .is_some_and(|offered| node.supports_action(offered)),
        }
    }

    fn is_hidden(self) -> bool {
        matches!(self, Self::Node(node, _) if node.is_hidden())
    }

    /// Whether the adapter holds the node as disabled.
    fn is_disabled(self) -> bool {
        matches!(self, Self::Node(node, _) if node.is_disabled())
    }

    fn range(self) -> Option<(f64, f64)> {
        match self {
            Self::Unknown => None,
            Self::Node(node, _) => node.min_numeric_value().zip(node.max_numeric_value()),
        }
    }

    /// Whether a text write to the delivered node reached its numeric
    /// handler, or `None` with no update on record.
    fn reads_text_as_number(self) -> Option<bool> {
        match self {
            Self::Unknown => None,
            Self::Node(_, record) => Some(record.is_some_and(|record| record.text_as_number)),
        }
    }

    /// Whether the delivered node showed a numeric range admitting `value`.
    fn range_admits(self, value: f64) -> bool {
        matches!(self, Self::Unknown)
            || self
                .range()
                .is_some_and(|(min, max)| value >= min && value <= max)
    }

    /// Whether custom action `id` is the action delivered under it: the same
    /// handler, the same id its own configuration gave it and the same label.
    fn custom_action_is(
        self,
        id: crate::CustomActionId,
        declared: &crate::DeclaredCustomAction<'_>,
    ) -> bool {
        match self {
            Self::Unknown => true,
            Self::Node(_, record) => record
                .and_then(|record| record.custom.get(id.index()))
                .is_some_and(|action| action.is(declared)),
        }
    }
}

/// Mirror of the adapter-visible tree as of the last delivered update.
///
/// The map holds the translated [`accesskit::Node`] per published
/// [`accesskit::NodeId`] — equality against a fresh translation is the diff.
/// Entries are pruned when their node leaves the arena, so a node that is
/// removed and later returns with identical content is correctly republished
/// (the adapter dropped it in between; the mirror must forget it too).
struct PublishedState {
    nodes: FxHashMap<accesskit::NodeId, accesskit::Node>,
    root: accesskit::NodeId,
    focus: accesskit::NodeId,
    /// What each published node offered beyond its payload; a node with
    /// nothing to record has no entry.
    offers: FxHashMap<accesskit::NodeId, OfferRecord>,
}

impl PublishedState {
    /// Mirror of a self-contained full update the adapter was just given.
    ///
    /// Consumes the update (callers are done delivering it by reference) so
    /// the mirror is built by moving the translated nodes, not re-cloning
    /// every label and property the adapter just received — on the full
    /// publish path that clone was the single largest added cost.
    fn mirror_of(update: crate::TreeUpdate) -> Self {
        Self {
            root: update
                .tree
                .as_ref()
                .map(|tree| tree.root)
                .expect("BUG: a full update always carries tree metadata"),
            focus: update.focus,
            nodes: update.nodes.into_iter().collect(),
            offers: FxHashMap::default(),
        }
    }

    /// Records what every published node in `tree` offered, after a full
    /// update delivered them.
    fn record_offers(&mut self, tree: &SemanticsTree) {
        // The same rooted, addressable walk the full update was built from:
        // an unattached alias of a published identity records nothing.
        self.offers = tree
            .rooted_nodes()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|sid| {
                let node = tree.get(sid)?;
                let id = accesskit::NodeId(node.accessibility_id()?.as_u64());
                let record = OfferRecord::of(node);
                (!record.is_empty()).then_some((id, record))
            })
            .collect();
    }

    /// Records what one delivered node now offers.
    fn update_offers(&mut self, id: accesskit::NodeId, record: OfferRecord) {
        if record.is_empty() {
            self.offers.remove(&id);
        } else {
            self.offers.insert(id, record);
        }
    }

    /// Forgets a node the adapter dropped.
    fn forget(&mut self, id: accesskit::NodeId) {
        self.nodes.remove(&id);
        self.offers.remove(&id);
    }
}

/// Which nodes declare anchors and which hold relations (ADR-0205).
///
/// Maintained exactly as the focus claimants are: a dirty node examined by an
/// incremental flush updates its entries, a full publish rebuilds both from a
/// whole-tree scan, and dead entries are dropped by a liveness check before
/// resolution. A target's identity can change without its sources being
/// dirty, so an incremental flush re-translates every relation source and lets
/// the mirror diff drop the unchanged ones.
#[derive(Default)]
struct RelationIndex {
    anchored: FxHashMap<SemanticsId, SmallVec<[SemanticsAnchor; 1]>>,
    sources: FxHashSet<SemanticsId>,
}

impl RelationIndex {
    fn scan(tree: &SemanticsTree) -> Self {
        let mut index = Self::default();
        for (sid, node) in tree.iter() {
            index.observe(sid, node);
        }
        index
    }

    /// Records what `node`'s configuration now declares.
    fn observe(&mut self, sid: SemanticsId, node: &SemanticsNode) {
        let config = node.config();
        if config.anchors().is_empty() {
            self.anchored.remove(&sid);
        } else {
            self.anchored
                .insert(sid, config.anchors().iter().cloned().collect());
        }
        if config.relations().is_empty() {
            self.sources.remove(&sid);
        } else {
            self.sources.insert(sid);
        }
    }

    fn retain_live(&mut self, tree: &SemanticsTree) {
        self.anchored.retain(|&sid, _| tree.contains(sid));
        self.sources.retain(|&sid| tree.contains(sid));
    }

    /// The nodes each anchor resolves to.
    fn anchor_index(&self, tree: &SemanticsTree) -> crate::accesskit_translation::AnchorIndex {
        let mut index = crate::accesskit_translation::AnchorIndex::default();
        for (&sid, anchors) in &self.anchored {
            let Some(node) = tree.get(sid) else {
                continue;
            };
            let Some(identity) = node.accessibility_id() else {
                continue;
            };
            let target = crate::accesskit_translation::AnchorTarget::of(node, identity);
            for anchor in anchors {
                index
                    .entry(anchor.clone())
                    .or_default()
                    .push(target.clone());
            }
        }
        index
    }
}

impl std::fmt::Debug for SemanticsOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsOwner")
            .field("tree", &self.tree)
            .field("callback", &self.callback.as_ref().map(|_| "<callback>"))
            .field("enabled", &self.enabled)
            .field(
                "published",
                &self.published.as_ref().map(|state| state.nodes.len()),
            )
            .field("full_publish_pending", &self.full_publish_pending)
            .field("focus_claimants", &self.focus_claimants.len())
            .finish_non_exhaustive()
    }
}

impl SemanticsOwner {
    /// Creates a new SemanticsOwner with a platform callback.
    pub fn new(callback: SemanticsUpdateCallback) -> Self {
        Self {
            tree: SemanticsTree::new(),
            callback: Some(callback),
            enabled: true,
            published: None,
            full_publish_pending: false,
            focus_claimants: FxHashSet::default(),
            relations: RelationIndex::default(),
            device_pixel_ratio: 1.0,
            #[cfg(any(test, feature = "testing"))]
            examined_last_flush: 0,
            diagnostics: std::collections::VecDeque::new(),
            revision: TreeRevision::default(),
            #[cfg(debug_assertions)]
            reported_clashes: Vec::new(),
            announcements: None,
        }
    }

    /// Creates a new SemanticsOwner without a callback (for testing).
    ///
    /// **Testing only** — gated on `#[cfg(any(test, feature = "testing"))]`.
    /// Production code constructs through [`Self::new`] which requires a
    /// platform callback; a no-callback owner is a scaffolding-only
    /// convenience.
    #[cfg(any(test, feature = "testing"))]
    pub fn new_without_callback() -> Self {
        Self {
            tree: SemanticsTree::new(),
            callback: None,
            enabled: true,
            published: None,
            full_publish_pending: false,
            focus_claimants: FxHashSet::default(),
            relations: RelationIndex::default(),
            device_pixel_ratio: 1.0,
            #[cfg(any(test, feature = "testing"))]
            examined_last_flush: 0,
            diagnostics: std::collections::VecDeque::new(),
            revision: TreeRevision::default(),
            #[cfg(debug_assertions)]
            reported_clashes: Vec::new(),
            announcements: None,
        }
    }

    /// Creates a SemanticsOwner with pre-allocated capacity.
    pub fn with_capacity(capacity: usize, callback: SemanticsUpdateCallback) -> Self {
        Self {
            tree: SemanticsTree::with_capacity(capacity),
            callback: Some(callback),
            enabled: true,
            published: None,
            full_publish_pending: false,
            focus_claimants: FxHashSet::default(),
            relations: RelationIndex::default(),
            device_pixel_ratio: 1.0,
            #[cfg(any(test, feature = "testing"))]
            examined_last_flush: 0,
            diagnostics: std::collections::VecDeque::new(),
            revision: TreeRevision::default(),
            #[cfg(debug_assertions)]
            reported_clashes: Vec::new(),
            announcements: None,
        }
    }

    /// Replaces the platform callback on a live owner.
    ///
    /// For the composition root that learns its platform bridge *after* this
    /// owner was created — an owner lazily constructed by the pipeline on
    /// enablement, wired to the real adapter once one exists.
    ///
    /// A live owner may already be **clean**: [`Self::flush`] gates on dirty
    /// nodes, so without help the new callback would first hear from us only
    /// on the next tree change — a late-wired adapter would present nothing
    /// until the user did something. So a rooted current tree is published
    /// through the new callback immediately; translation is snapshot-shaped,
    /// so this says exactly what a flush would say.
    ///
    /// The swap also resets the published-state mirror to exactly what the
    /// new callback was just told: the previous adapter's knowledge is
    /// irrelevant to this one, and diffing future flushes against it would
    /// withhold nodes the new adapter has never seen.
    ///
    /// If the new callback panics on that update, nothing is replaced: the
    /// previous callback stays installed and the mirror still describes what
    /// its adapter holds, so action resolution keeps checking against it.
    pub fn set_callback(&mut self, callback: SemanticsUpdateCallback) {
        if self.enabled
            && let Some((update, announced)) = self.full_update(None)
        {
            callback(&update);
            self.commit_announcements(announced.as_ref());
            let mut mirror = PublishedState::mirror_of(update);
            mirror.record_offers(&self.tree);
            self.published = Some(mirror);
            // Same wholesale reset a full publish performs: claimants
            // re-seeded from the tree, stale removal notices discarded.
            self.focus_claimants = Self::scan_focus_claimants(&self.tree);
            self.relations = RelationIndex::scan(&self.tree);
            let _ = self.tree.take_removed_accessibility_ids();
        } else {
            // The new adapter was told nothing.
            self.published = None;
        }
        self.callback = Some(callback);
    }

    // ========== Enabled State ==========

    /// Returns whether semantics is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enables semantics.
    pub fn enable(&mut self) {
        self.enabled = true;
        if let Some(announcements) = &self.announcements {
            announcements.set_listening(true);
        }
    }

    /// Disables semantics.
    ///
    /// When disabled, no updates are sent to the platform.
    pub fn disable(&mut self) {
        self.enabled = false;
        if let Some(announcements) = &self.announcements {
            announcements.set_listening(false);
        }
    }

    // ========== Tree Access ==========

    /// Returns a reference to the semantics tree.
    #[inline]
    pub fn tree(&self) -> &SemanticsTree {
        &self.tree
    }

    /// The current tree as an AccessKit `TreeUpdate`.
    ///
    /// This is the shape both an OS accessibility adapter and a query-by-role
    /// test harness consume, so publishing and asserting cannot drift apart:
    /// what a screen reader is told is exactly what a test can inspect.
    ///
    /// `focus` names the node the platform should treat as focused; a `None`,
    /// or an id not present in the tree, falls back to the root, because
    /// AccessKit requires a valid focus target.
    ///
    /// Returns `None` before the first assembly pass, when the tree has no
    /// root and no applicable update exists.
    #[must_use]
    pub fn to_accesskit_tree_update(
        &self,
        focus: Option<SemanticsId>,
    ) -> Option<accesskit::TreeUpdate> {
        self.full_update(focus).map(|(update, _)| update)
    }

    /// Returns a mutable reference to the semantics tree.
    #[inline]
    pub fn tree_mut(&mut self) -> &mut SemanticsTree {
        &mut self.tree
    }

    /// What the last delivered update showed of `node_id`, or `None` when an
    /// update is on record and did not carry the node.
    fn delivered(&self, node_id: AccessibilityNodeId) -> Option<Delivered<'_>> {
        let Some(state) = self.published.as_ref() else {
            return Some(Delivered::Unknown);
        };
        let id = accesskit::NodeId(node_id.as_u64());
        let node = state.nodes.get(&id)?;
        Some(Delivered::Node(node, state.offers.get(&id)))
    }

    /// The rooted node that publishes under `node_id`.
    ///
    /// One arena entry under the identity is unambiguous, so the lookup
    /// checks only that the entry is rooted: its ancestors up to the root
    /// are all addressable and list each other. Several entries, or a climb
    /// that fails, need the whole tree, because an
    /// unrooted one never counts toward ambiguity.
    fn locate(
        &self,
        root: SemanticsId,
        node_id: AccessibilityNodeId,
    ) -> Result<(SemanticsId, &SemanticsNode), SemanticsActionError> {
        let not_found = SemanticsActionError::NodeNotFound { node_id };
        let found = match *self.tree.nodes_with_accessibility_id(node_id) {
            [] => Err(not_found),
            // Links that are not reciprocal fail the climb, and only the walk
            // knows whether the root still reaches the entry.
            [only] => self
                .rooted(root, only)
                .map_or_else(|| self.locate_by_walk(root, node_id), Ok),
            _ => self.locate_by_walk(root, node_id),
        };
        debug_assert_eq!(
            found.as_ref().map(|(id, _)| *id),
            self.locate_by_walk(root, node_id)
                .as_ref()
                .map(|(id, _)| *id),
            "BUG: the indexed lookup must agree with the full walk",
        );
        found
    }

    /// `id` and its node when every node from `id` up to `root` is
    /// addressable, the way the full publication reaches it.
    fn rooted(&self, root: SemanticsId, id: SemanticsId) -> Option<(SemanticsId, &SemanticsNode)> {
        let node = self.tree.get(id)?;
        let mut current = id;
        // A cycle never reaches the root; the arena size bounds the climb.
        for _ in 0..=self.tree.len() {
            let ancestor = self.tree.get(current)?;
            ancestor.accessibility_id()?;
            if current == root {
                return Some((id, node));
            }
            // The walk reaches a node through its parent's child list, so
            // the climb accepts a link only when that list names the child.
            let parent = ancestor.parent()?;
            if !self.tree.get(parent)?.children().contains(&current) {
                return None;
            }
            current = parent;
        }
        None
    }

    /// The same lookup by visiting every rooted node. Orphaned arena entries
    /// were never exported and must not remain actionable. The visited set
    /// also makes malformed repeated edges and cycles finite; snapshot
    /// validation reports those structural errors separately.
    fn locate_by_walk(
        &self,
        root: SemanticsId,
        node_id: AccessibilityNodeId,
    ) -> Result<(SemanticsId, &SemanticsNode), SemanticsActionError> {
        let mut pending = SmallVec::<[SemanticsId; 32]>::new();
        let mut visited = FxHashSet::default();
        let mut resolved = None;
        pending.push(root);

        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            // An unaddressable node is not published, and neither is its
            // subtree: the walk stops there, as the full update does.
            let Some(node) = self
                .tree
                .get(id)
                .filter(|node| node.accessibility_id().is_some())
            else {
                continue;
            };
            pending.extend(node.children().iter().rev().copied());

            if node.accessibility_id() != Some(node_id) {
                continue;
            }
            if resolved.is_some() {
                return Err(SemanticsActionError::AmbiguousNode { node_id });
            }
            resolved = Some((id, node));
        }
        resolved.ok_or(SemanticsActionError::NodeNotFound { node_id })
    }

    /// Resolves a platform request against the current rooted semantics tree,
    /// within what the last delivered update offered.
    ///
    /// A request is admitted only when both agree: the current node can serve
    /// it (its handlers are the only ones there are to run), and the node as
    /// last delivered offered it (the request can only mean what assistive
    /// technology was shown). So after a failed publication an action only the
    /// delivered node offered, and one only the undelivered node offers, are
    /// both refused until a publication succeeds. With no update on record
    /// (none published yet, or a reconnect forgot it) the current tree alone
    /// decides; a node an update on record did not carry is not found.
    ///
    /// The lookup uses the stable accessibility identity exported in the
    /// latest snapshot. It never interprets that value as a rebuild-local
    /// [`SemanticsId`]. Only effective actions are routable, so
    /// `blocks_user_actions` applies identically to snapshot export and input
    /// dispatch.
    ///
    /// An `Expand` or `Collapse` request is admitted only for the transition
    /// the node's expanded state allows, whichever handler serves it. To an
    /// expandable node that registers neither discrete action but has a click
    /// handler, it resolves to that handler with [`SemanticsAction::Click`]. A
    /// numeric setter is refused unless its value lies in the node's range. A
    /// `SetValue` text to a range without a text handler resolves to its
    /// numeric handler when the text parses as a finite number. A custom
    /// action runs its own handler, and only while its id still names the
    /// action the last publication showed under it.
    ///
    /// The returned invocation owns callback snapshots and must be invoked
    /// after the outer pipeline owner borrow has been released. Descendant
    /// reveal snapshots refuse removed, replaced or reparented geometry. Their
    /// published scroll-position basis lets receiving scrollables rebase
    /// reentrant requests without repeating cached movement.
    pub fn resolve_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<SemanticsActionInvocation, SemanticsActionError> {
        let root = self
            .tree
            .root()
            .ok_or(SemanticsActionError::SemanticsUnavailable)?;

        let (target_id, node) = self.locate(root, request.node_id)?;
        let not_found = SemanticsActionError::NodeNotFound {
            node_id: request.node_id,
        };
        // A node the adapter does not hold offered it nothing.
        let delivered = self.delivered(request.node_id).ok_or(not_found)?;
        let requested = request.action();
        let unsupported = SemanticsActionError::UnsupportedAction {
            node_id: request.node_id,
            action: requested,
        };
        if !delivered.offers(requested) {
            return Err(unsupported);
        }
        let config = node.config();
        // The node as publication shows it: a state its role does not support
        // was not offered, so no request is resolved through it.
        let published = crate::applicability::retain_applicable(config.attributes());
        // A disabled control takes no user action, whoever asks: the agent
        // surface refuses it the same way. Focus, blur and reveal move the
        // cursor or the viewport and stay available.
        if (published.states.enabled == Some(false) || delivered.is_disabled())
            && !matches!(
                requested,
                SemanticsAction::Focus | SemanticsAction::Blur | SemanticsAction::ScrollIntoView
            )
        {
            return Err(SemanticsActionError::Disabled {
                node_id: request.node_id,
            });
        }
        let actions = config.effective_actions_as_bits();
        // A hidden node is not revealed, whichever handler would serve it.
        if requested == SemanticsAction::ScrollIntoView
            && (config.is_hidden() || delivered.is_hidden())
        {
            return Err(unsupported);
        }
        if requested == SemanticsAction::ScrollIntoView
            && actions & requested.value() != 0
            && config.action_handler(requested).is_none()
        {
            let mut path = Vec::new();
            let mut steps = Vec::new();
            let mut current = Some(target_id);
            let mut target_rect = node.reveal_rect();
            let mut ancestors = FxHashSet::default();
            while let Some(id) = current {
                if !ancestors.insert(id) {
                    break;
                }
                let Some(ancestor) = self.tree.get(id) else {
                    break;
                };
                let Some(member) = self.tree.membership(id) else {
                    break;
                };
                path.push(member);
                if id != target_id
                    && ancestor.config().effective_actions_as_bits()
                        & SemanticsAction::ScrollIntoView.value()
                        != 0
                    && let Some(handler) = ancestor
                        .config()
                        .action_handler(SemanticsAction::ScrollIntoView)
                {
                    let viewport_rect = ancestor.reveal_rect();
                    steps.push((
                        Arc::clone(handler),
                        Reveal {
                            target_rect,
                            viewport_rect,
                            scroll_position: ancestor.config().scroll_position(),
                        },
                    ));
                    target_rect = viewport_rect;
                }
                if id == root {
                    if !steps.is_empty() {
                        return Ok(SemanticsActionInvocation {
                            node_id: request.node_id,
                            action: requested,
                            delivery: Some(ActionDelivery::Reveal { path, steps }),
                        });
                    }
                    break;
                }
                current = ancestor.parent();
            }
            return Err(unsupported);
        }
        // Expand and collapse are transitions, offered only from the state
        // that allows them; a request for the state the node already
        // publishes (or from a node without one) reaches no handler. The
        // delivered node offered only the transition its own state allowed,
        // so both states must allow this one.
        if matches!(
            requested,
            SemanticsAction::Expand | SemanticsAction::Collapse
        ) && crate::action::disclosure_transition(published.states.expanded) != Some(requested)
        {
            return Err(unsupported);
        }
        let mut call = request.call;
        // A click-only expandable node receives the transition its current
        // state allows through its click handler.
        if crate::action::tap_disclosure_transition(actions, published.states.expanded)
            == Some(requested)
        {
            call = ActionCall::Click;
        }
        // A string written to a range with no text handler is offered to its
        // numeric handler as a number. The delivered node decided which
        // handler the platform's text write was meant for, so a text whose
        // routing changed since is refused rather than sent to the other one.
        if let ActionCall::SetValue(_) = &call
            && delivered
                .reads_text_as_number()
                .is_some_and(|delivered| delivered != reads_text_as_number(config))
        {
            return Err(unsupported);
        }
        if let ActionCall::SetValue(text) = &call
            && reads_text_as_number(config)
            && let Ok(value) = text.trim().parse::<f64>()
            && value.is_finite()
        {
            call = ActionCall::SetNumericValue(value);
        }
        let routed = call.action();
        let handler = if let ActionCall::CustomAction(id) = call {
            // An id is a place in the node's list, so a request from an older
            // publication runs only while that place still holds the action
            // the platform was shown.
            let Some(declared) = config.custom_action(id) else {
                return Err(unsupported);
            };
            if !delivered.custom_action_is(id, &declared) {
                return Err(unsupported);
            }
            // The handler hears the id its own configuration gave the action,
            // not the one a merge published it under.
            call = ActionCall::CustomAction(declared.own_id);
            Arc::clone(declared.handler)
        } else {
            let Some(handler) = (actions & routed.value() != 0)
                .then(|| config.action_handler(routed))
                .flatten()
                .map(Arc::clone)
            else {
                return Err(unsupported);
            };
            handler
        };

        // The value must lie in the range the platform was shown as well as
        // the one the handler now serves.
        if let ActionCall::SetNumericValue(value) = call
            && !(published
                .numeric_range
                .is_some_and(|range| range.contains(value))
                && delivered.range_admits(value))
        {
            return Err(SemanticsActionError::InvalidNumericValue {
                node_id: request.node_id,
            });
        }

        Ok(SemanticsActionInvocation {
            node_id: request.node_id,
            action: routed,
            delivery: Some(ActionDelivery::Direct { call, handler }),
        })
    }

    // ========== Root Management ==========

    /// Get the root SemanticsNode ID.
    #[inline]
    pub fn root(&self) -> Option<SemanticsId> {
        self.tree.root()
    }

    /// Set the root SemanticsNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<SemanticsId>) {
        self.tree.set_root(root);
    }

    // ========== Node Management ==========

    /// Inserts a SemanticsNode into the tree.
    pub fn insert(&mut self, node: SemanticsNode) -> SemanticsId {
        self.tree.insert(node)
    }

    /// Returns a reference to a SemanticsNode.
    #[inline]
    pub fn get(&self, id: SemanticsId) -> Option<&SemanticsNode> {
        self.tree.get(id)
    }

    /// Returns a mutable reference to a SemanticsNode.
    #[inline]
    pub fn get_mut(&mut self, id: SemanticsId) -> Option<&mut SemanticsNode> {
        self.tree.get_mut(id)
    }

    /// Removes a SemanticsNode from the tree (cascades to all descendants).
    ///
    /// Delegates to [`SemanticsTree::remove`](crate::tree::SemanticsTree::remove).
    /// For non-cascading removal,
    /// reach into [`SemanticsTree::remove_shallow`](crate::tree::SemanticsTree::remove_shallow) via
    /// [`Self::tree`] / [`Self::tree_mut`].
    pub fn remove(&mut self, id: SemanticsId) -> Option<SemanticsNode> {
        self.tree.remove(id)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.tree.clear();
    }

    /// Disposes of the SemanticsOwner.
    ///
    /// This clears all nodes, removes the callback, and disables semantics.
    /// After calling dispose, the owner should not be used.
    ///
    /// # Effects
    ///
    /// - Clears the semantics tree
    /// - Removes all listeners
    /// - Releases resources
    pub fn dispose(&mut self) {
        self.tree.clear();
        self.callback = None;
        self.enabled = false;
        if let Some(announcements) = &self.announcements {
            announcements.set_listening(false);
        }
        self.published = None;
        self.full_publish_pending = false;
        self.focus_claimants.clear();
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent SemanticsNode.
    pub fn add_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        self.tree.add_child(parent_id, child_id);
    }

    /// Removes a child from a parent SemanticsNode.
    pub fn remove_child(&mut self, parent_id: SemanticsId, child_id: SemanticsId) {
        self.tree.remove_child(parent_id, child_id);
    }

    // ========== Dirty Tracking ==========

    /// Returns true if the next [`Self::flush`] would have work to do —
    /// dirty nodes, or a scheduled full publish
    /// ([`Self::schedule_full_publish`]) that is pending on an otherwise
    /// clean tree. This predicate mirrors `flush`'s own gate exactly: a
    /// frame loop consulting a narrower one would never deliver the
    /// reconnect update.
    pub fn needs_flush(&self) -> bool {
        self.enabled
            && (self.tree.has_dirty_nodes()
                || self.full_publish_pending
                || self.has_pending_announcements())
    }

    // ========== Announcements ==========

    /// Installs the presentation's announcement queue: its two nodes publish
    /// as the last children of the root from the next flush, which is a full
    /// one so the adapter receives them.
    pub fn set_announcements(&mut self, announcements: crate::Announcements) {
        announcements.set_listening(self.enabled);
        self.announcements = Some(announcements);
        self.full_publish_pending = true;
    }

    /// Queues `message` for the next flush, which speaks it through the
    /// announcer node of `urgency`. Use from inside a frame; application code
    /// holds an [`Announcer`](crate::Announcer), which also requests the frame.
    ///
    /// # Errors
    /// [`AnnounceError::PresentationClosed`](crate::AnnounceError::PresentationClosed)
    /// when no announcement queue is installed: there is no node to speak
    /// through.
    pub fn announce(
        &mut self,
        message: &str,
        urgency: crate::Urgency,
    ) -> Result<(), crate::AnnounceError> {
        let announcements = self
            .announcements
            .as_ref()
            .ok_or(crate::AnnounceError::PresentationClosed)?;
        announcements.enqueue(message, urgency);
        Ok(())
    }

    fn has_pending_announcements(&self) -> bool {
        self.announcements
            .as_ref()
            .is_some_and(crate::Announcements::has_pending)
    }

    /// The announcer nodes for `prepared`, polite then assertive: static text
    /// whose value is the message, live at its urgency, with no area.
    fn announcer_nodes(
        announcements: &crate::Announcements,
        prepared: &crate::announce::PreparedAnnouncements,
    ) -> [(accesskit::NodeId, accesskit::Node); 2] {
        let node = |urgency: crate::Urgency, value: &str| {
            let mut node = accesskit::Node::new(accesskit::Role::Label);
            node.set_live(match urgency {
                crate::Urgency::Assertive => accesskit::Live::Assertive,
                crate::Urgency::Polite => accesskit::Live::Polite,
            });
            node.set_live_atomic();
            node.set_value(value);
            node.set_bounds(accesskit::Rect::ZERO);
            (
                accesskit::NodeId(announcements.node_id(urgency).as_u64()),
                node,
            )
        };
        [
            node(crate::Urgency::Polite, &prepared.polite),
            node(crate::Urgency::Assertive, &prepared.assertive),
        ]
    }

    /// Appends the announcer nodes to `root`'s children.
    fn adopt_announcers(announcements: &crate::Announcements, root: &mut accesskit::Node) {
        for urgency in [crate::Urgency::Polite, crate::Urgency::Assertive] {
            root.push_child(accesskit::NodeId(announcements.node_id(urgency).as_u64()));
        }
    }

    /// Marks a specific node as dirty.
    pub fn mark_dirty(&mut self, id: SemanticsId) {
        if let Some(node) = self.tree.get_mut(id) {
            node.mark_dirty();
        }
    }

    // ========== Flush to Platform ==========

    /// Publishes what changed — and only what changed — to the platform.
    ///
    /// The observable contract: an idle frame publishes nothing and returns in
    /// O(1); only nodes whose content actually changed serialize into the
    /// update. The *mechanism* is a diff because FLUI rebuilds the semantics
    /// arena every assembly pass (flui-semantics ARCHITECTURE.md, semantics assembly)
    /// rather than mutating persistent nodes: a rebuild marks every node dirty, so the dirty bit alone cannot
    /// say what changed. The diff therefore compares each dirty node's
    /// translation, keyed by its stable [`AccessibilityNodeId`], against a
    /// private mirror of the last delivered update — payload equality is
    /// authoritative, dirty bits only bound how much is re-examined.
    ///
    /// Three shapes come out of one call:
    ///
    /// - **Clean tree** — O(1) early return, no translation, no callback.
    /// - **First publish / [`Self::schedule_full_publish`] pending / root
    ///   identity changed** — a self-contained full update carrying
    ///   [`accesskit::TreeUpdate::tree`] metadata. `tree: Some` is this
    ///   owner's promise that the update stands alone (the Linux bridge
    ///   retains exactly these to answer a late-activating screen reader).
    /// - **Otherwise** — an incremental update: changed nodes only,
    ///   `tree: None`. Structure changes ride along for free because a
    ///   node's children list is part of its payload — the parent whose
    ///   children changed is itself a changed node, which is also how an
    ///   adapter learns a removed child is gone. If nothing survives the
    ///   diff and focus is unchanged, no update is delivered at all.
    ///
    /// An unrooted tree publishes nothing and stays dirty, so the first
    /// rooted flush retries (unchanged from the pre-diff behavior; see the
    /// comment inside the private full-publish path).
    ///
    /// Returns how many nodes the delivered update carried — the whole tree
    /// for a full publish, the changed nodes for an incremental one, and 0
    /// when nothing was delivered. The pipeline's phase counters sum it.
    pub fn flush(&mut self) -> usize {
        if !self.needs_flush() {
            return 0;
        }
        // While two live nodes share an identity, a diff could carry the
        // unattached one's payload under the published id. The full path
        // publishes the rooted nodes and refuses only when two of those
        // alias; refused, the tree stays dirty and the first flush after the
        // defect clears delivers it.
        let needs_full = self.tree.has_aliased_identities()
            || self.full_publish_pending
            || match (&self.published, self.current_root_id()) {
                // A changed root identity is a different tree to the
                // adapter; a diff cannot express that transition, so it
                // escalates to a self-contained full update.
                (Some(state), Some(root)) => state.root != root,
                // Nothing published yet — or no addressable root (both
                // paths publish nothing and stay dirty; route through the
                // full path for one exit).
                _ => true,
            };

        if needs_full {
            self.publish_full()
        } else {
            self.publish_incremental()
        }
    }

    /// The stable identity the current root would publish under.
    fn current_root_id(&self) -> Option<accesskit::NodeId> {
        self.tree
            .root()
            .and_then(|root| self.tree.get(root))
            .and_then(SemanticsNode::accessibility_id)
            .map(|id| accesskit::NodeId(id.as_u64()))
    }

    /// The whole rooted tree as one update, the announcer nodes included, and
    /// the announcements it would deliver. `None` while the tree has no root.
    fn full_update(
        &self,
        focus: Option<SemanticsId>,
    ) -> Option<(
        accesskit::TreeUpdate,
        Option<crate::announce::PreparedAnnouncements>,
    )> {
        let mut update = crate::accesskit_translation::tree_to_scaled_update(
            &self.tree,
            focus,
            self.device_pixel_ratio,
        )?;
        let prepared = self.announcements.as_ref().map(|announcements| {
            let prepared = announcements.prepare();
            let root = update
                .tree
                .as_ref()
                .expect("BUG: a full update names its root")
                .root;
            if let Some((_, node)) = update.nodes.iter_mut().find(|(id, _)| *id == root) {
                Self::adopt_announcers(announcements, node);
            }
            update
                .nodes
                .extend(Self::announcer_nodes(announcements, &prepared));
            prepared
        });
        Some((update, prepared))
    }

    /// Removes from the queue what a delivered update carried.
    fn commit_announcements(&self, delivered: Option<&crate::announce::PreparedAnnouncements>) {
        if let (Some(announcements), Some(delivered)) = (&self.announcements, delivered) {
            announcements.commit(delivered);
        }
    }

    /// Publishes the complete rooted tree and resets the mirror to it.
    /// Returns the number of nodes delivered (0 when there is no root yet).
    fn publish_full(&mut self) -> usize {
        // Translate before touching the callback so the borrow of `self.tree`
        // ends first.
        let Some((update, announced)) = self.full_update(None) else {
            // No root yet — nothing an adapter could apply. Leave the tree
            // dirty (and any full publish pending) so the next flush retries
            // once assembly has rooted it, rather than silently swallowing
            // the first real update.
            //
            // A tree that *loses* its root after publishing takes this path
            // too, and nothing withdraws the tree already sent: the adapter
            // keeps presenting it. Withdrawal is the adapter's deactivation
            // signal rather than an empty update — `TreeUpdate` has no
            // representation for "no tree" — so it is defined with the
            // adapter lifecycle rather than invented here without one. See
            // the teardown item in the Linux bridge issue.
            return 0;
        };

        // Clone-and-release: cloning the `Arc` out of `self.callback`
        // decouples the invocation from any lock the owner may hold, so the
        // callback never runs while one is held.
        let callback = self.callback.as_ref().map(Arc::clone);
        if let Some(callback) = callback {
            callback(&update);
        }
        // Delivered: a panic above leaves the messages queued for the retry.
        self.commit_announcements(announced.as_ref());

        let published = update.nodes.len();
        self.revision = TreeRevision(self.revision.0.saturating_add(1));
        let mut mirror = PublishedState::mirror_of(update);
        mirror.record_offers(&self.tree);
        self.published = Some(mirror);
        self.full_publish_pending = false;
        self.focus_claimants = Self::scan_focus_claimants(&self.tree);
        self.relations = RelationIndex::scan(&self.tree);
        // The mirror was rebuilt wholesale; pending removal notices predate
        // it and would only prune entries the rebuild already resolved.
        let _ = self.tree.take_removed_accessibility_ids();
        #[cfg(any(test, feature = "testing"))]
        {
            self.examined_last_flush = self.tree.len();
        }
        // Diagnostics follow node changes, not publication: a reconnect
        // republishes every node, and a clean one was reported when it changed.
        let findings: Vec<SemanticsDiagnostic> = self
            .tree
            .rooted_nodes()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|sid| self.tree.get(sid))
            .filter(|node| node.is_dirty())
            .flat_map(inapplicable_of)
            .map(SemanticsDiagnostic::InapplicableAttribute)
            .collect();
        self.queue_diagnostics(findings);
        #[cfg(debug_assertions)]
        self.check_identifier_clashes();
        self.tree.mark_all_clean();
        published
    }

    /// Queues each identifier clash in the delivered tree, as assistive
    /// technology filters it, that the last check did not find: a clash is
    /// reported once for as long as it lasts, and again if it comes back.
    ///
    /// Debug builds only, after a publication that reshaped the tree (always
    /// a full one) or changed an identifier, since the check reads the whole
    /// tree. A clash that only a change of clipping brings into or out of view
    /// is found by the next such publication; an agent read
    /// ([`Self::read_wire`]) always reports the tree it reads.
    #[cfg(debug_assertions)]
    fn check_identifier_clashes(&mut self) {
        let Some(state) = self.published.as_ref() else {
            return;
        };
        let identified = state
            .nodes
            .values()
            .filter(|node| node.author_id().is_some());
        if identified.take(2).count() < 2 {
            self.reported_clashes.clear();
            return;
        }
        let update = crate::TreeUpdate {
            nodes: state
                .nodes
                .iter()
                .map(|(id, node)| (*id, node.clone()))
                .collect(),
            tree: Some(accesskit::TreeInfo::new(state.root)),
            tree_id: accesskit::TreeId::ROOT,
            focus: state.focus,
        };
        // The delivered tree passed the full publish's shape checks; a
        // consumer panic here is a defect the adapter meets first.
        let Ok(tree) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            accesskit_consumer::Tree::new(update, true)
        })) else {
            return;
        };
        let clashes = crate::agent::identifier_clashes(tree.state());
        let new: Vec<SemanticsDiagnostic> = clashes
            .iter()
            .filter(|clash| !self.reported_clashes.contains(clash))
            .cloned()
            .map(SemanticsDiagnostic::DuplicateIdentifier)
            .collect();
        self.reported_clashes = clashes;
        self.queue_diagnostics(new);
    }

    /// Every node currently claiming the focused flag — publishable or not,
    /// because ambiguity counts all claimants — the full-scan seed for the
    /// incrementally-maintained [`Self::focus_claimants`] set.
    fn scan_focus_claimants(tree: &SemanticsTree) -> FxHashSet<SemanticsId> {
        tree.iter()
            .filter(|(_, node)| node.config().is_focused())
            .map(|(sid, _)| sid)
            .collect()
    }

    /// Diffs dirty nodes against the mirror and publishes only the changes.
    ///
    /// Cost is O(dirty + removed), not O(arena): the tree's maintained
    /// dirty set bounds what is re-translated, its removal log bounds the
    /// mirror prune, and focus comes from the incrementally-maintained
    /// claimant set — no step sweeps every live node. (An earlier shape of
    /// this method walked the whole arena per flush to collect a live-id
    /// set and derive focus; that walk was the last O(tree) cost on the
    /// publish path.)
    ///
    /// Returns the number of changed nodes delivered (0 when the diff is
    /// empty and focus is unchanged, so nothing is delivered).
    fn publish_incremental(&mut self) -> usize {
        let state = self
            .published
            .as_mut()
            .expect("BUG: incremental publish requires a prior published state");

        // Mirror entries whose nodes left the arena, from the tree's removal
        // log. The diff treats them as absent: otherwise a node that is
        // removed and later returns with identical content would diff as
        // "unchanged" against a mirror the adapter no longer agrees with — the
        // adapter drops it when its parent is republished without it — and
        // never be re-sent. They leave the mirror only once an update is
        // delivered: until then the adapter still holds them, and action
        // resolution reads the mirror as what it holds. The liveness re-check
        // protects a node that returned: its removal notice is stale, not its
        // entry.
        let pruned: FxHashSet<accesskit::NodeId> = self
            .tree
            .take_removed_accessibility_ids()
            .into_iter()
            .filter(|&raw| !self.tree.is_accessibility_id_live(raw))
            .map(accesskit::NodeId)
            .collect();
        // Constant-time membership: every diffed node, anchor target,
        // relation source and focus claimant is looked up here.
        let held = |id: &accesskit::NodeId| {
            if pruned.contains(id) {
                None
            } else {
                state.nodes.get(id)
            }
        };

        #[cfg(any(test, feature = "testing"))]
        let mut examined = 0_usize;

        let dirty: SmallVec<[SemanticsId; 16]> = self.tree.dirty_ids().collect();
        // Relations resolve against what the dirty nodes now declare, so the
        // index is brought up to date before anything is translated.
        for &sid in &dirty {
            if let Some(node) = self.tree.get(sid).filter(|node| node.is_dirty()) {
                self.relations.observe(sid, node);
            }
        }
        self.relations.retain_live(&self.tree);
        let mut anchors = self.relations.anchor_index(&self.tree);
        // A relation names only nodes the adapter holds: an anchored node the
        // last publication did not carry (an unattached one) is no target, or
        // the consumer would resolve an id it never received.
        for targets in anchors.values_mut() {
            targets.retain(|target| held(&target.id).is_some());
        }
        // Resolving relations reads every anchored node: counted, so the
        // oracle reports this pass's real cost.
        #[cfg(any(test, feature = "testing"))]
        {
            examined += self.relations.anchored.len();
        }

        let mut changed: Vec<(accesskit::NodeId, accesskit::Node)> = Vec::new();
        // What each examined node now offers beyond its payload, recorded once
        // the update carrying it is delivered (or found unchanged).
        let mut offer_updates: Vec<(accesskit::NodeId, OfferRecord)> = Vec::new();
        let mut findings: Vec<SemanticsDiagnostic> = Vec::new();
        for sid in dirty {
            // The set is a conservative superset: skip members that died
            // since marking or whose bit a caller cleaned through a borrow.
            let Some(node) = self.tree.get(sid) else {
                continue;
            };
            if !node.is_dirty() {
                continue;
            }
            #[cfg(any(test, feature = "testing"))]
            {
                examined += 1;
            }

            // Focus bookkeeping counts every claimant, publishable or not
            // — ambiguity must match the full path, which counts them all.
            if node.config().is_focused() {
                self.focus_claimants.insert(sid);
            } else {
                self.focus_claimants.remove(&sid);
            }

            let Some(identity) = node.accessibility_id() else {
                // Unaddressable: never published, nothing to diff. Same
                // skip rule as `tree_to_update`.
                continue;
            };
            let id = accesskit::NodeId(identity.as_u64());
            offer_updates.push((id, OfferRecord::of(node)));
            let Some(data) = self.tree.publish_view(node) else {
                continue;
            };
            findings.extend(inapplicable_of(node).map(SemanticsDiagnostic::InapplicableAttribute));
            let mut translated = crate::accesskit_translation::to_published_node(
                &data,
                id == state.root,
                self.device_pixel_ratio,
                &anchors,
            );
            if id == state.root
                && let Some(announcements) = &self.announcements
            {
                Self::adopt_announcers(announcements, &mut translated);
            }
            if held(&id) != Some(&translated) {
                changed.push((id, translated));
            }
        }

        // A relation's target can change identity while its source stays
        // clean, so every clean source is re-translated and diffed too.
        for &sid in &self.relations.sources {
            let Some(node) = self.tree.get(sid).filter(|node| !node.is_dirty()) else {
                continue;
            };
            #[cfg(any(test, feature = "testing"))]
            {
                examined += 1;
            }
            let Some(data) = self.tree.publish_view(node) else {
                continue;
            };
            let id = accesskit::NodeId(data.id.as_u64());
            // A source the last publication did not carry (an unattached node)
            // is not in the tree the adapter holds; re-translating it would
            // publish a node no parent lists.
            if held(&id).is_none() {
                continue;
            }
            let mut translated = crate::accesskit_translation::to_published_node(
                &data,
                id == state.root,
                self.device_pixel_ratio,
                &anchors,
            );
            if id == state.root
                && let Some(announcements) = &self.announcements
            {
                Self::adopt_announcers(announcements, &mut translated);
            }
            if held(&id) != Some(&translated) {
                changed.push((id, translated));
            }
        }

        // An announcement changes only its node's value.
        let announced = self.announcements.as_ref().map(|announcements| {
            let prepared = announcements.prepare();
            for (id, node) in Self::announcer_nodes(announcements, &prepared) {
                if held(&id) != Some(&node) {
                    changed.push((id, node));
                }
            }
            prepared
        });

        #[cfg(any(test, feature = "testing"))]
        {
            self.examined_last_flush = examined;
        }

        // The diff carries content changes only. A node the adapter does not
        // hold yet, or one whose children changed, reshapes the tree, and an
        // edge built through the public node APIs can leave a node under two
        // parents or in a cycle: the full publish validates the whole rooted
        // shape before anything reaches the adapter.
        let reshaped = changed.iter().any(|(id, node)| {
            held(id).is_none_or(|published| published.children() != node.children())
        });
        if reshaped {
            return self.publish_full();
        }
        // Resolution mirrors `tree_to_update`'s `focused_node` + stable-id
        // lookup exactly: dead claimants are dropped (the set is tiny, so
        // the liveness sweep is O(claimants)); one survivor wins if it is
        // publishable and falls back to the root if not; several warn and
        // fall back to the root.
        let tree = &self.tree;
        self.focus_claimants.retain(|&sid| tree.contains(sid));
        // Only claimants the adapter holds count: an unattached node marked
        // focused must not make a published one ambiguous.
        let mut claimants = self.focus_claimants.iter().filter_map(|&sid| {
            tree.get(sid)
                .and_then(SemanticsNode::accessibility_id)
                .map(|identity| accesskit::NodeId(identity.as_u64()))
                .filter(|id| held(id).is_some())
        });
        let focus = match (claimants.next(), claimants.next()) {
            (Some(single), None) => single,
            (Some(_), Some(_)) => {
                tracing::warn!(
                    "semantics tree has more than one focused node; publishing the root"
                );
                state.root
            }
            (None, _) => state.root,
        };

        if changed.is_empty() && focus == state.focus {
            // Everything the dirty bits pointed at translated identically —
            // a rebuild that reproduced the same tree. The adapter hears
            // nothing; the dirt is simply consumed. A rebuilt handler behind
            // an unchanged action is still that action.
            for id in &pruned {
                state.forget(*id);
            }
            for (id, record) in offer_updates {
                state.update_offers(id, record);
            }
            self.tree.mark_all_clean();
            self.queue_diagnostics(findings);
            return 0;
        }

        let update = crate::TreeUpdate {
            nodes: changed,
            tree: None,
            tree_id: accesskit::TreeId::ROOT,
            focus,
        };

        let callback = self.callback.as_ref().map(Arc::clone);
        if let Some(callback) = callback {
            callback(&update);
        }
        // Delivered: a panic above leaves the messages queued for the retry.
        if let (Some(announcements), Some(announced)) = (&self.announcements, &announced) {
            announcements.commit(announced);
        }

        #[cfg(debug_assertions)]
        let identifiers_changed = update.nodes.iter().any(|(id, node)| {
            state.nodes.get(id).map(accesskit::Node::author_id) != Some(node.author_id())
        });
        // The mirror describes delivery, not the attempted update. A failed
        // callback leaves the dirty nodes available to retry unchanged input.
        for (id, node) in &update.nodes {
            state.nodes.insert(*id, node.clone());
        }
        for id in &pruned {
            state.forget(*id);
        }
        for (id, record) in offer_updates {
            state.update_offers(id, record);
        }
        state.focus = focus;

        self.tree.mark_all_clean();
        self.queue_diagnostics(findings);
        #[cfg(debug_assertions)]
        if identifiers_changed {
            self.check_identifier_clashes();
        }
        self.revision = TreeRevision(self.revision.0.saturating_add(1));
        update.nodes.len()
    }

    /// How many arena entries the last flush inspected — the scaling
    /// oracle for the O(dirty) publish claim. A full publish inspects
    /// every arena entry; an incremental one only the dirty set's live
    /// survivors. See the field doc for why "inspected" is the honest
    /// unit on both paths.
    #[cfg(any(test, feature = "testing"))]
    pub fn examined_last_flush(&self) -> usize {
        self.examined_last_flush
    }

    /// Queues what a publication left unpublished, once per node and
    /// attribute within it. Nothing is reported here: a flush runs inside the
    /// pipeline's checkout and the frame driver's locks, and a `tracing`
    /// subscriber is user code that may reach either, so the driver takes the
    /// queue ([`Self::take_diagnostics`]) and reports it once they are
    /// released. A finding from an earlier publication that is still queued
    /// is a separate change: it is not merged with a new one.
    fn queue_diagnostics(&mut self, findings: impl IntoIterator<Item = SemanticsDiagnostic>) {
        let queued = self.diagnostics.len();
        for finding in findings {
            if !self
                .diagnostics
                .range(queued..)
                .any(|known| *known == finding)
            {
                self.diagnostics.push_back(finding);
            }
        }
    }

    /// Takes every diagnostic not yet reported, oldest first. The pipeline
    /// moves them onto itself after each flush, so they outlive this owner,
    /// and its frame driver reports them ([`report_diagnostic`]) with no
    /// lock held.
    #[must_use]
    pub fn take_diagnostics(&mut self) -> Vec<SemanticsDiagnostic> {
        self.diagnostics.drain(..).collect()
    }

    /// Forces the next flush to publish a self-contained full update, even
    /// if no node is dirty.
    ///
    /// The reconnect primitive: when assistive technology (re)activates, the
    /// adapter's state is unknown — it may have forgotten everything — so the
    /// mirror is forgotten with it. The composition root pairs this with
    /// re-seeding the assembly pass so a flush actually runs.
    pub fn schedule_full_publish(&mut self) {
        self.published = None;
        self.full_publish_pending = true;
    }

    /// Sets how many physical pixels the window has per logical pixel.
    ///
    /// The tree stays in logical pixels; the ratio reaches the platform as
    /// a scale transform on the published root, because AccessKit adapters
    /// read bounds in physical pixels. A changed ratio marks the root dirty,
    /// so the next [`Self::flush`] republishes the root with the new
    /// transform (the descendants inherit it and are not resent).
    ///
    /// A non-finite or non-positive ratio is ignored, keeping the previous
    /// one: it would publish degenerate or non-finite bounds.
    pub fn set_device_pixel_ratio(&mut self, device_pixel_ratio: f64) {
        if !(device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0) {
            tracing::warn!(
                device_pixel_ratio,
                "ignoring a non-finite or non-positive device pixel ratio for semantics"
            );
            return;
        }
        #[expect(
            clippy::float_cmp,
            reason = "an unchanged ratio is bit-identical, not approximately equal"
        )]
        if device_pixel_ratio == self.device_pixel_ratio {
            return;
        }
        self.device_pixel_ratio = device_pixel_ratio;
        if let Some(root) = self.tree.root() {
            self.mark_dirty(root);
        }
    }

    /// The device pixel ratio the published root is scaled by.
    #[must_use]
    pub fn device_pixel_ratio(&self) -> f64 {
        self.device_pixel_ratio
    }

    /// The revision of the tree the platform was last told: it grows by one at
    /// every publication that delivered something, so two readings compare by
    /// revision (`Match::revision`, `Actionability::revision`).
    ///
    /// A flush that finds nothing changed, or no root yet, leaves it as it was.
    #[must_use]
    pub fn revision(&self) -> TreeRevision {
        self.revision
    }

    /// Forces a full tree update, now.
    ///
    /// Publishes the complete tree, bypassing the incremental diff. Use when accessibility services reconnect or
    /// request the full tree.
    pub fn send_full_tree(&mut self) {
        if !self.enabled {
            return;
        }

        self.full_publish_pending = true;
        self.flush();
    }
}

#[cfg(any(test, feature = "testing"))]
impl Default for SemanticsOwner {
    fn default() -> Self {
        Self::new_without_callback()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {

    use flui_foundation::RenderId;

    use super::*;
    use crate::{AccessibilityNodeId, ActionCall, SemanticsActionRequest};

    #[test]
    fn action_resolution_rejects_orphaned_and_stale_nodes() {
        let root_render_id = RenderId::new(1);
        let orphan_render_id = RenderId::new(2);
        let stale_render_id = RenderId::new(99);
        let mut owner = SemanticsOwner::new_without_callback();
        let root = owner.insert(SemanticsNode::new().with_source_render_id(root_render_id));
        let mut orphan = SemanticsNode::new().with_source_render_id(orphan_render_id);
        orphan
            .config_mut()
            .add_action(SemanticsAction::Click, Arc::new(|_| {}));
        let _orphan = owner.insert(orphan);
        owner.set_root(Some(root));

        for target in [
            AccessibilityNodeId::from(orphan_render_id),
            AccessibilityNodeId::from(stale_render_id),
        ] {
            assert_eq!(
                owner
                    .resolve_action(SemanticsActionRequest::new(target, ActionCall::Click))
                    .expect_err("nodes absent from the rooted snapshot are stale"),
                SemanticsActionError::NodeNotFound { node_id: target },
            );
        }
    }

    /// The scaling oracle counts the anchored nodes an incremental flush
    /// resolves and the clean relation sources it re-translates, so a flush
    /// doing O(anchors + sources) work cannot report O(dirty).
    #[test]
    fn an_incremental_flush_counts_the_relation_work_it_does() {
        let mut owner = SemanticsOwner::new(Arc::new(|_: &crate::TreeUpdate| {}));
        let root = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        let anchor = crate::SemanticsAnchor::new();
        let mut title = SemanticsNode::new().with_source_render_id(RenderId::new(2));
        title.config_mut().set_label("Title");
        title.config_mut().add_anchor(anchor.clone());
        let title = owner.insert(title);
        let mut sources = Vec::new();
        for index in 3..6 {
            let mut source = SemanticsNode::new().with_source_render_id(RenderId::new(index));
            source.config_mut().set_label("Dialog");
            source
                .config_mut()
                .add_relation(crate::SemanticsRelation::LabelledBy, anchor.clone());
            sources.push(owner.insert(source));
        }
        owner.add_child(root, title);
        for &source in &sources {
            owner.add_child(root, source);
        }
        owner.set_root(Some(root));
        owner.flush();

        owner.mark_dirty(title);
        owner.flush();
        assert_eq!(
            owner.examined_last_flush(),
            1 + 1 + sources.len(),
            "one dirty node, the anchored title and every clean relation source"
        );
    }

    /// An attribute the declared role does not support is reported once per
    /// change of its node: by the flush that publishes the change, not by a
    /// later flush that does not touch it.
    #[test]
    fn an_inapplicable_attribute_is_reported_once_per_change() {
        let mut owner = SemanticsOwner::new(Arc::new(|_: &crate::TreeUpdate| {}));
        let render = RenderId::new(3);
        let button = |label: &str| {
            let mut node = SemanticsNode::new().with_source_render_id(render);
            node.config_mut().set_role(crate::SemanticsRole::Button);
            node.config_mut().set_label(label);
            node.config_mut().set_selected(true);
            node
        };
        let root = owner.insert(button("Send"));
        owner.set_root(Some(root));
        owner.flush();
        let expected = [SemanticsDiagnostic::InapplicableAttribute(
            InapplicableAttribute {
                node: AccessibilityNodeId::from(render),
                role: crate::SemanticsRole::Button,
                attribute: crate::RoleAttribute::Selected,
            },
        )];
        assert_eq!(
            owner.take_diagnostics(),
            expected,
            "the full publish reports it"
        );

        owner.flush();
        assert_eq!(
            owner.take_diagnostics(),
            [],
            "an idle flush reports nothing"
        );

        owner.schedule_full_publish();
        owner.flush();
        owner.send_full_tree();
        assert_eq!(
            owner.take_diagnostics(),
            [],
            "a reconnect republishes the node without reporting it again"
        );

        owner.tree_mut().replace_node(root, button("Send now"));
        owner.flush();
        assert_eq!(
            owner.take_diagnostics(),
            expected,
            "the flush that publishes a change reports it again"
        );
    }

    /// A request is resolved against the node as publication shows it: a
    /// paragraph's expanded state is not published, so an `Expand` request
    /// does not fall back to its click handler.
    #[test]
    fn a_state_the_role_does_not_support_routes_no_action() {
        let clicks = Arc::new(parking_lot::Mutex::new(0_u32));
        let counter = Arc::clone(&clicks);
        let mut owner = SemanticsOwner::new(Arc::new(|_: &crate::TreeUpdate| {}));
        let render = RenderId::new(3);
        let node = |role| {
            let mut node = SemanticsNode::new().with_source_render_id(render);
            node.config_mut().set_role(role);
            node.config_mut().set_expanded(false);
            let counter = Arc::clone(&counter);
            node.config_mut().add_action(
                SemanticsAction::Click,
                Arc::new(move |_| *counter.lock() += 1),
            );
            node
        };
        let root = owner.insert(node(crate::SemanticsRole::Button));
        owner.set_root(Some(root));
        owner.flush();
        let expand =
            SemanticsActionRequest::new(AccessibilityNodeId::from(render), ActionCall::Expand);
        owner
            .resolve_action(expand.clone())
            .expect("a collapsed button expands through its click")
            .invoke();

        owner
            .tree_mut()
            .replace_node(root, node(crate::SemanticsRole::Paragraph));
        owner.flush();
        assert!(
            owner.resolve_action(expand).is_err(),
            "a paragraph offers no expand"
        );
        assert_eq!(*clicks.lock(), 1);
    }

    /// Queued diagnostics are taken once, oldest first; a finding still
    /// queued and the same finding from a newer change are two reports.
    #[test]
    fn a_queued_diagnostic_and_a_newer_change_are_two_reports() {
        let mut owner = SemanticsOwner::new(Arc::new(|_: &crate::TreeUpdate| {}));
        let selected_button = |index: usize| {
            let mut node = SemanticsNode::new().with_source_render_id(RenderId::new(index));
            node.config_mut().set_role(crate::SemanticsRole::Button);
            node.config_mut().set_selected(true);
            node
        };
        let finding = |index: usize| {
            SemanticsDiagnostic::InapplicableAttribute(InapplicableAttribute {
                node: AccessibilityNodeId::from(RenderId::new(index)),
                role: crate::SemanticsRole::Button,
                attribute: crate::RoleAttribute::Selected,
            })
        };
        let root = owner.insert(SemanticsNode::new().with_source_render_id(RenderId::new(1)));
        let first = owner.insert(selected_button(2));
        let second = owner.insert(selected_button(3));
        owner.add_child(root, first);
        owner.add_child(root, second);
        owner.set_root(Some(root));
        owner.flush();
        assert_eq!(owner.take_diagnostics(), [finding(2), finding(3)]);
        assert_eq!(owner.take_diagnostics(), [], "taken once");

        owner.flush();
        assert_eq!(owner.take_diagnostics(), [], "an idle flush queues nothing");

        owner.tree_mut().replace_node(second, selected_button(3));
        owner.flush();
        owner.tree_mut().replace_node(second, selected_button(3));
        owner.flush();
        assert_eq!(
            owner.take_diagnostics(),
            [finding(3), finding(3)],
            "the queued finding stays, and the newer change is its own report"
        );
    }

    /// Each custom action runs its own handler. A request whose id is past the
    /// node's list, or names a place that now holds another action than the
    /// one last published there, is refused rather than run.
    #[test]
    fn custom_actions_run_their_own_handler_until_stale() {
        let ran = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let handler = |name: &'static str| -> crate::SemanticsActionHandler {
            let ran = Arc::clone(&ran);
            Arc::new(move |_| ran.lock().push(name))
        };
        let mut owner = SemanticsOwner::new(Arc::new(|_: &crate::TreeUpdate| {}));
        let render = RenderId::new(3);
        let mut node = SemanticsNode::new().with_source_render_id(render);
        let archive = node
            .config_mut()
            .add_custom_action("Archive", handler("archive"));
        let pin = node.config_mut().add_custom_action("Pin", handler("pin"));
        let root = owner.insert(node);
        owner.set_root(Some(root));
        owner.flush();
        let target = AccessibilityNodeId::from(render);
        let request = |id| SemanticsActionRequest::new(target, ActionCall::CustomAction(id));
        let refused = Some(SemanticsActionError::UnsupportedAction {
            node_id: target,
            action: SemanticsAction::CustomAction,
        });

        for id in [pin, archive] {
            owner
                .resolve_action(request(id))
                .expect("a published custom action resolves")
                .invoke();
        }
        assert_eq!(
            *ran.lock(),
            ["pin", "archive"],
            "each id runs its own handler"
        );
        assert_eq!(
            owner
                .resolve_action(request(crate::CustomActionId::at(2)))
                .err(),
            refused,
            "an id past the list names nothing"
        );

        // The list changes order before the platform hears of it: the first
        // place now holds Pin, so a request for the Archive it was shown there
        // is stale.
        let mut reordered = SemanticsNode::new().with_source_render_id(render);
        reordered
            .config_mut()
            .add_custom_action("Pin", handler("pin"));
        reordered
            .config_mut()
            .add_custom_action("Archive", handler("archive"));
        owner.tree_mut().replace_node(root, reordered);
        assert_eq!(
            owner.resolve_action(request(archive)).err(),
            refused,
            "a place that now holds another action is stale"
        );
        owner.flush();
        owner
            .resolve_action(request(archive))
            .expect("after publication the id names what the platform shows")
            .invoke();
        assert_eq!(ran.lock().last(), Some(&"pin"));

        // Two actions with one label are told apart by identity, not text:
        // swapping them before the platform hears of it leaves the label at
        // each place unchanged, and the request is still stale.
        let mut twins = SemanticsNode::new().with_source_render_id(render);
        twins
            .config_mut()
            .add_custom_action("Delete", handler("first"));
        twins
            .config_mut()
            .add_custom_action("Delete", handler("second"));
        let swapped = {
            let mut node = SemanticsNode::new().with_source_render_id(render);
            let config = twins.config();
            for id in [crate::CustomActionId::at(1), crate::CustomActionId::at(0)] {
                let declared = config.custom_action(id).expect("declared");
                node.config_mut()
                    .add_custom_action(declared.label, Arc::clone(declared.handler));
            }
            node
        };
        owner.tree_mut().replace_node(root, twins);
        owner.flush();
        owner.tree_mut().replace_node(root, swapped);
        assert_eq!(
            owner.resolve_action(request(archive)).err(),
            refused,
            "a same-labelled action from another handler is not the one published"
        );

        // A blocked node offers no custom action, so the delivered update
        // published none: unblocking it before the next flush does not make a
        // request for an id the platform never showed resolve.
        let mut blocked = SemanticsNode::new().with_source_render_id(render);
        blocked
            .config_mut()
            .add_custom_action("Archive", handler("archive"));
        blocked.config_mut().set_blocks_user_actions(true);
        let unblocked = {
            let mut node = blocked.clone();
            node.config_mut().set_blocks_user_actions(false);
            node
        };
        owner.tree_mut().replace_node(root, blocked);
        owner.flush();
        owner.tree_mut().replace_node(root, unblocked);
        assert_eq!(
            owner.resolve_action(request(archive)).err(),
            refused,
            "an action the last update did not offer is not run"
        );
    }
}
