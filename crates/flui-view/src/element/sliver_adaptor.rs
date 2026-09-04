//! `SliverAdaptorElement<R>` — element-tree backend shared by every lazy
//! multi-box sliver.
//!
//! # What this is
//!
//! Flutter's `SliverMultiBoxAdaptorElement` is the element responsible for
//! lazily building and disposing the children of a `RenderSliverMultiBoxAdaptor`
//! (and its subclasses `RenderSliverList` / `RenderSliverGrid`). FLUI splits
//! this responsibility across two crates AND generalizes it over the render
//! object family via one trait, [`LazyMultiBoxRender`]:
//!
//! - **Render half** (`flui-objects`): a concrete `RenderSliver` implementor
//!   (`RenderSliverList`, `RenderSliverGrid`, …) — emits build requests via
//!   `SliverLayoutContext::request_child_build` for absent slots, and emits
//!   `emit_retain_band` for eviction.
//! - **Element half** (this module): [`SliverAdaptorElement<R>`] — registered
//!   as a `ChildManager` in `BuildOwner`; receives the post-layout requests
//!   and retain-bands via `service_child_requests` and drives `SparseChildren`
//!   to build or evict lazy children.
//!
//! A render object joins this family by implementing [`LazyMultiBoxRender`]
//! (its `Config` associated type carries whatever configuration knob the view
//! exposes — a per-item extent estimate, a grid delegate, …) and pairing it
//! with a `pub type Alias = SliverMultiBoxAdaptor<TheRenderObject>;` — no new
//! manager, behavior, or element type is needed. [`SliverList`] and
//! [`SliverGrid`] are the two production instances today.
//!
//! # Lifecycle
//!
//! 1. **mount**: [`SliverAdaptorBehavior::on_mount`] creates the render object
//!    (via the inner `RenderBehavior`) and then registers
//!    `Arc::clone(&self.manager)` in `BuildOwner::child_manager_registry` keyed
//!    by the sliver's `RenderId`. Registration happens in the adaptor's own
//!    `on_mount`, not in the generic `behavior.rs:789` site, because that
//!    generic site has no way to reach this element's child-manager.
//! 2. **service**: `BuildOwner::service_child_requests` drains the
//!    `PipelineOwner`'s pending buffers, groups by `RenderId`, and calls
//!    [`SliverAdaptorManager::service`] — which evicts out-of-band children
//!    via `SparseChildren::retain_band` and builds new ones via
//!    `SparseChildren::ensure`.
//! 3. **unmount**: [`SliverAdaptorBehavior::on_unmount`] pushes all live
//!    sparse children to `owner.push_inactive` — necessary because the host
//!    element's own `child_ids` stays empty, so the normal dense-unmount walk
//!    cannot reach them — then unregisters the manager, then removes the
//!    render object. `finalize_tree` finds the lazy children's descendants via
//!    each sparse child's own `child_ids`.
//!
//! # Invariant: host `child_ids` stays empty
//!
//! `build_into_views` returns an empty `Vec` so the dense reconciler in
//! `build_scope` never touches the lazy children. The lazy children live only
//! in `SparseChildren::by_logical_index`; they are managed solely by
//! `service_child_requests`.

use std::{collections::HashMap, marker::PhantomData, rc::Rc, sync::Arc};

use flui_foundation::{ElementId, RenderId, ViewKey};
use flui_objects::{RenderSliverFixedExtentList, RenderSliverGrid, RenderSliverList};
use flui_rendering::{
    parent_data::SliverMultiBoxAdaptorParentData, pipeline::PipelineCell, protocol::SliverProtocol,
    traits::RenderSliver,
};
use parking_lot::Mutex;

use super::sparse_children::{ReconcileSource, build_item_or_error};
use super::{
    Variable,
    behavior::{ElementBehavior, RenderBehavior},
    child_manager::ChildManager,
    generic::ElementCore,
    sparse_children::SparseChildren,
    unified::Element,
};
use crate::{
    BoxedView, ElementOwner,
    tree::ElementTree,
    view::{RenderView, View},
};

/// A delegate's key → index callback (Flutter's `findChildIndexCallback`).
pub(crate) type FindIndexByKey = Rc<dyn Fn(&dyn ViewKey) -> Option<usize>>;

/// A delegate's item factory: the view at a logical index, `None` past the end.
pub(crate) type ItemBuilder = Rc<dyn Fn(usize) -> Option<BoxedView>>;

// ============================================================================
// LAZY MULTI-BOX RENDER TRAIT
// ============================================================================

/// A render object that [`SliverMultiBoxAdaptor<R>`] can drive as a lazy,
/// element-built multi-child sliver.
///
/// Implementing this trait is the entire cost of joining the lazy-adaptor
/// family: no new `ChildManager`, `ElementBehavior`, or element type alias is
/// needed — the crate-private `SliverAdaptorManager<R>` and
/// `SliverAdaptorBehavior<R>` are generic over any `R: LazyMultiBoxRender`.
///
/// `RenderSliverList` and `RenderSliverGrid` (both in `flui-objects`) are
/// the two production implementors, paired with the [`SliverList`] and
/// [`SliverGrid`] view aliases respectively.
pub trait LazyMultiBoxRender:
    RenderSliver<Arity = Variable, ParentData = SliverMultiBoxAdaptorParentData> + Send + Sync + 'static
{
    /// The render-object-specific configuration the view carries — a
    /// per-item extent estimate for a list, a grid delegate for a grid, …
    type Config: Clone + std::fmt::Debug + 'static;

    /// Label naming this render family, used only for `Debug`/log output
    /// (the view's `Debug` impl and [`ElementBehavior::debug_kind`]).
    const KIND: &'static str;

    /// Construct a new render object over `item_count` items configured by
    /// `config`. Mirrors the render object's own `new` constructor.
    fn create(config: &Self::Config, item_count: usize) -> Self;

    /// Apply a changed `config` to an existing render object.
    ///
    /// The adaptor's [`RenderView::update_render_object`] ORs the result with
    /// `set_item_count`'s impact and an unconditional
    /// [`RenderUpdateImpact::LAYOUT`](flui_rendering::RenderUpdateImpact::LAYOUT) —
    /// the builder closure is opaque and reconciliation cannot compare its
    /// behavior, so every replacement conservatively refreshes resident
    /// children and relayouts them.
    fn update(&mut self, config: &Self::Config) -> flui_rendering::RenderUpdateImpact;

    /// The render object's current item count (the data-source length as
    /// last told).
    fn item_count(&self) -> usize;

    /// Update the known item count. Call when the data-source length changes.
    fn set_item_count(&mut self, item_count: usize) -> flui_rendering::RenderUpdateImpact;
}

/// Whether an update must re-consult the delegate for the resident children.
///
/// A builder delegate always must: it is an opaque closure that may read
/// state the adaptor cannot see, so a reused `Rc` says nothing about its
/// output (Flutter's `SliverChildBuilderDelegate.shouldRebuild` is always
/// `true`). A static delegate's output is a function of its identity, so two
/// adaptors over the same [`StaticChildren`] (same builder and key callback
/// `Rc`s, same count) are the same delegate (`SliverChildListDelegate.
/// shouldRebuild` is `children != oldDelegate.children`). Config changes
/// reach the render object through `update_render_object` and need no
/// resident refresh.
fn delegate_changed<R: LazyMultiBoxRender>(
    old: &SliverMultiBoxAdaptor<R>,
    new: &SliverMultiBoxAdaptor<R>,
) -> bool {
    match (&old.static_children, &new.static_children) {
        (Some(a), Some(b)) => !Rc::ptr_eq(a, b) || old.item_count != new.item_count,
        _ => true,
    }
}

// ============================================================================
// STATIC CHILDREN — the delegate behind every `list` constructor
// ============================================================================

/// A fixed `Vec<BoxedView>` served by index, with the key → index map that
/// lets a keyed child whose data moved be found without a callback.
///
/// Flutter's `SliverChildListDelegate` (`widgets/scroll_delegate.dart`):
/// `build` answers `None` out of range, `findIndexByKey` consults a lazily
/// filled `_keyToIndex`, and two delegates compare by the identity of their
/// `children`. The map here is built on the first lookup, keyed by
/// `key_hash` and decided by `key_eq` inside the bucket; the adaptor's
/// reconcile hands the callback the item's own key (the per-item wrapper's
/// salt is stripped before the call).
pub struct StaticChildren {
    children: Vec<BoxedView>,
    key_to_index: std::cell::OnceCell<HashMap<u64, Vec<usize>>>,
    /// Applied to each child as it is built (a per-item repaint boundary,
    /// say). The key map reads the raw children, so a wrapper that salts
    /// the key stays matchable through the unsalted callback key.
    map: Option<Rc<dyn Fn(BoxedView) -> BoxedView>>,
    /// The builder and key callback handed to adaptors, created once: two
    /// adaptors built over one delegate carry the same `Rc`s, which is what
    /// the adaptor's update compares to decide whether the residents need
    /// a refresh (Flutter's `SliverChildListDelegate.shouldRebuild` is
    /// `children != oldDelegate.children` — identity).
    delegate_pair: std::cell::OnceCell<(ItemBuilder, FindIndexByKey)>,
}

impl StaticChildren {
    /// Wrap `children` as a shared delegate.
    #[must_use]
    pub fn new(children: Vec<BoxedView>) -> Rc<Self> {
        Rc::new(Self {
            children,
            key_to_index: std::cell::OnceCell::new(),
            map: None,
            delegate_pair: std::cell::OnceCell::new(),
        })
    }

    /// Wrap `children` as a shared delegate whose every built item passes
    /// through `map` first.
    #[must_use]
    pub fn mapped(
        children: Vec<BoxedView>,
        map: impl Fn(BoxedView) -> BoxedView + 'static,
    ) -> Rc<Self> {
        Rc::new(Self {
            children,
            key_to_index: std::cell::OnceCell::new(),
            map: Some(Rc::new(map)),
            delegate_pair: std::cell::OnceCell::new(),
        })
    }

    /// The raw children, as handed in (before any mapping).
    #[must_use]
    pub fn children(&self) -> &[BoxedView] {
        &self.children
    }

    /// The number of children.
    #[must_use]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Whether there are no children.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// The child at `index` (mapped, if a mapper was given), or `None` past
    /// the end.
    #[must_use]
    pub fn build(&self, index: usize) -> Option<BoxedView> {
        let child = self.children.get(index).cloned()?;
        Some(match &self.map {
            Some(map) => map(child),
            None => child,
        })
    }

    /// The index of the first child carrying `key`.
    #[must_use]
    pub fn find_index_by_key(&self, key: &dyn ViewKey) -> Option<usize> {
        let map = self.key_to_index.get_or_init(|| {
            let mut map: HashMap<u64, Vec<usize>> = HashMap::new();
            for (index, child) in self.children.iter().enumerate() {
                if let Some(child_key) = child.0.key() {
                    map.entry(child_key.key_hash()).or_default().push(index);
                }
            }
            map
        });
        map.get(&key.key_hash())?.iter().copied().find(|&index| {
            self.children[index]
                .0
                .key()
                .is_some_and(|child_key| child_key.key_eq(key))
        })
    }

    /// The builder and key callback an adaptor takes — the same pair every
    /// time for one delegate, so adaptors built over it compare as unchanged.
    fn delegate_pair(self: &Rc<Self>) -> (ItemBuilder, FindIndexByKey) {
        let (builder, find) = self.delegate_pair.get_or_init(|| {
            // The closures hold a weak reference: a strong one would keep
            // the delegate alive through its own cache, a cycle nothing
            // could break.
            let weak = Rc::downgrade(self);
            let builder: ItemBuilder = {
                let weak = weak.clone();
                Rc::new(move |index: usize| weak.upgrade().and_then(|this| this.build(index)))
            };
            let find: FindIndexByKey = Rc::new(move |key: &dyn ViewKey| {
                weak.upgrade().and_then(|this| this.find_index_by_key(key))
            });
            (builder, find)
        });
        (Rc::clone(builder), Rc::clone(find))
    }
}

impl std::fmt::Debug for StaticChildren {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticChildren")
            .field("len", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<R: LazyMultiBoxRender> SliverMultiBoxAdaptor<R> {
    /// Serve `children` through this adaptor: built by index, keyed children
    /// found by the delegate's key map.
    fn over_static_children(mut self, children: &Rc<StaticChildren>) -> Self {
        let (builder, find) = children.delegate_pair();
        self.static_children = Some(Rc::clone(children));
        self.item_count = ItemCount::Exact(children.len());
        self.builder = builder;
        self.find_index_by_key = Some(find);
        self
    }
}

// ============================================================================
// VIEW CONFIG
// ============================================================================

/// View configuration for a lazy multi-box sliver adaptor element, generic
/// over the render object family `R`.
///
/// Holds the item count, the render-object-specific `config`, and the item
/// builder. The element this view creates wraps `R` (the render half) and
/// owns a crate-private `SliverAdaptorManager<R>` that services
/// `ChildManager::service` calls post-layout.
///
/// [`SliverList`] and [`SliverGrid`] are type aliases over this struct;
/// their inherent constructors (`SliverList::new`, `SliverGrid::new`, …)
/// live in their own `impl` blocks below.
///
/// # Invariant: no dense children
///
/// [`has_children`](RenderView::has_children) returns `false` so
/// `build_into_views` returns an empty `Vec`. The dense reconciler must
/// never touch the lazy children — they are managed by `SparseChildren`
/// via `BuildOwner::service_child_requests`.
pub struct SliverMultiBoxAdaptor<R: LazyMultiBoxRender> {
    /// Render-object-specific configuration (the per-item extent estimate
    /// for a list, the grid delegate for a grid, …).
    pub(crate) config: R::Config,
    /// The [`StaticChildren`] delegate `builder` serves, when it is one:
    /// the adaptor keeps it alive (its closures hold only weak references)
    /// and compares it by identity on update. `None` for an opaque builder
    /// closure, which may read state the adaptor cannot see and is
    /// re-consulted on every update (Flutter's
    /// `SliverChildBuilderDelegate.shouldRebuild` is always `true`).
    pub(crate) static_children: Option<Rc<StaticChildren>>,
    /// Total number of items in the data source.
    pub(crate) item_count: ItemCount,
    /// Given a logical index, produces the item's view. Returns `None` when
    /// the index is past the end of the data source.
    pub(crate) builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    /// Maps an item's key to its current index in the data source, so a
    /// keyed child whose data moved *out of the resident band* is still
    /// found and its state kept — Flutter's `findChildIndexCallback`. Keyed
    /// moves *within* the band need no callback: the reconcile matches
    /// residents by key on its own.
    pub(crate) find_index_by_key: Option<FindIndexByKey>,
}

impl<R: LazyMultiBoxRender> Clone for SliverMultiBoxAdaptor<R> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            static_children: self.static_children.clone(),
            item_count: self.item_count,
            builder: Rc::clone(&self.builder),
            find_index_by_key: self.find_index_by_key.clone(),
        }
    }
}

impl<R: LazyMultiBoxRender> std::fmt::Debug for SliverMultiBoxAdaptor<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(R::KIND)
            .field("item_count", &self.item_count)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<R: LazyMultiBoxRender> SliverMultiBoxAdaptor<R> {
    /// An adaptor over any [`LazyMultiBoxRender`]: `config` is the render
    /// object's own knob, `item_count` the data source's length, `builder`
    /// the item factory (`None` past the end).
    ///
    /// This is how a render object outside this crate joins the lazy child
    /// lifecycle — implement the trait, construct the adaptor, and the
    /// element tree builds, retains, evicts and reconciles its children
    /// exactly as it does for [`SliverList`] and [`SliverGrid`]. (The
    /// aliases keep their own `new`; an inherent `new` on the generic type
    /// would collide with them.)
    #[must_use]
    pub fn with_config(
        config: R::Config,
        item_count: impl Into<ItemCount>,
        builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    ) -> Self {
        let item_count = item_count.into();
        Self {
            config,
            static_children: None,
            item_count,
            builder,
            find_index_by_key: None,
        }
    }

    /// The item count to build a render object with, probing the builder when
    /// the caller declared [`ItemCount::Unknown`].
    fn resolved_item_count(&self) -> usize {
        match self.item_count {
            ItemCount::Exact(count) => count,
            ItemCount::Unknown => probe_item_count(&*self.builder),
        }
    }

    /// Install the key → index callback (see the field doc).
    ///
    /// Shared by every lazy multi-box adaptor — the callback's shape does
    /// not depend on the render object family.
    #[must_use]
    pub fn find_index_by_key(
        mut self,
        find: impl Fn(&dyn ViewKey) -> Option<usize> + 'static,
    ) -> Self {
        self.find_index_by_key = Some(Rc::new(find));
        self
    }
}

// ============================================================================
// RenderView impl — generic over the render object family
// ============================================================================

impl<R: LazyMultiBoxRender> RenderView for SliverMultiBoxAdaptor<R> {
    type Protocol = SliverProtocol;
    type RenderObject = R;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        // `Unknown` is resolved HERE, at mount, not when the view value is
        // built. A view is reconstructed on every rebuild, so probing in its
        // constructor would repeat `2·log₂(n)` builder calls every frame —
        // and a builder with side effects would see them.
        R::create(&self.config, self.resolved_item_count())
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // A changed delegate (builder or key callback) is detected by the
        // behavior's `on_view_updated`, which sees both views: it flags the
        // resident refresh and marks this render object for layout. Here only
        // the render-side knobs speak.
        let count_impact = match self.item_count {
            // The declaration still names an exact length: push it, so a data
            // source that shrank or grew is honoured.
            ItemCount::Exact(count) => render_object.set_item_count(count),
            // Unknown: resolved at mount, and re-probed by `on_view_updated`
            // when the delegate actually changes. Re-probing here instead
            // would run the search on every rebuild, which is the cost this
            // whole arrangement exists to avoid.
            ItemCount::Unknown => flui_rendering::RenderUpdateImpact::NONE,
        };
        count_impact | render_object.update(&self.config)
    }

    /// Invariant: no dense children — the dense reconciler must not touch
    /// lazy children.
    fn has_children(&self) -> bool {
        false
    }

    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {
        // No dense children to visit — this element only hosts lazy/sparse children.
    }
}

// ============================================================================
// View impl — creates a SliverAdaptorElement<R> with the shared behavior
// ============================================================================

impl<R: LazyMultiBoxRender> View for SliverMultiBoxAdaptor<R> {
    fn create_element(&self) -> crate::element::ElementKind {
        // Creates the adaptor element with the custom behavior instead of the
        // generic `RenderBehavior::new()` produced by `impl_render_view!`.
        // This is required so on_mount registers the ChildManager — which the
        // generic RenderBehavior does not do; that registration must happen
        // in this element's own on_mount instead. Routes through the
        // `RenderVariable` variant via the blanket impl below.
        crate::element::ElementKind::RenderVariable(Box::new(SliverAdaptorElement::<R>::new(
            self,
            SliverAdaptorBehavior::<R>::new(self),
        )))
    }
}

// `SliverMultiBoxAdaptor<R>` uses a custom adaptor behavior (not the generic
// `RenderBehavior`), so it needs its own `RenderElementBase<Variable>` tag to
// route into `ElementKind::RenderVariable`; the `RenderBehavior` blanket impl
// in `element/kind.rs` does not cover this behavior.
impl<R: LazyMultiBoxRender> crate::element::RenderElementBase<Variable> for SliverAdaptorElement<R> where
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<SliverProtocol>>>
{
}

// ============================================================================
// MANAGER
// ============================================================================

/// The `ChildManager` implementation shared by every live lazy multi-box
/// adaptor element, generic over the render object family `R`.
///
/// Holds the `SparseChildren` bookkeeping, the host element id, and the item
/// builder. Called by `BuildOwner::service_child_requests` after each layout
/// pass; not reachable from any other path (single-threaded call site).
pub(crate) struct SliverAdaptorManager<R: LazyMultiBoxRender> {
    /// Sparse logical-index → ElementId map for built children.
    sparse_children: SparseChildren,
    /// The element id of the adaptor host element. `None` until `on_mount`
    /// stamps it; the host is always mounted before `service` runs.
    host_element_id: Option<ElementId>,
    /// Item factory. `Rc` so it's shared with [`SliverMultiBoxAdaptor<R>`]
    /// and the behavior without cloning the closure.
    builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    /// The view's key → index callback, if any (see
    /// [`SliverMultiBoxAdaptor::find_index_by_key`]).
    find_index_by_key: Option<FindIndexByKey>,
    /// The sliver's render id, for clamping its item count when the builder
    /// declines an index below it (the data source shrank).
    render_id: Option<RenderId>,
    /// Set by [`SliverAdaptorBehavior::on_view_updated`] whenever the parent
    /// hands this element a new view; consumed (and cleared) by the next
    /// `service` call, which re-consults `builder` for every currently-
    /// resident index via `SparseChildren::reconcile`. Mirrors Flutter's
    /// `SliverChildBuilderDelegate.shouldRebuild => true` default
    /// (`widgets/scroll_delegate.dart`, tag `3.44.0`): a delegate change
    /// re-builds every resident child, not only newly-visible ones.
    ///
    /// The "next `service` call" is guaranteed to land in the SAME frame as
    /// the view update, on two legs that must both stay unconditional: this
    /// adaptor's render update includes layout because its builder delegate
    /// conservatively rebuilds resident children, and every `R`'s
    /// `perform_layout` emits its retain band on every layout pass (list) or
    /// every exit path (grid: empty grid, window-past-end, and the normal
    /// path) — so the frame's `service_child_requests` pass never takes its
    /// empty early-return after a sliver view update. An early-out added to
    /// either leg turns this flag into deferred-forever work.
    needs_resident_refresh: bool,
    /// Ties this manager to its render object family without storing an `R`
    /// value — the manager never owns a render object, only reads/writes one
    /// through `pipeline` by `render_id`.
    _render: PhantomData<R>,
}

impl<R: LazyMultiBoxRender> std::fmt::Debug for SliverAdaptorManager<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverAdaptorManager")
            .field("built_children", &self.sparse_children.len())
            .field("host_element_id", &self.host_element_id)
            .field("render_id", &self.render_id)
            .field("needs_resident_refresh", &self.needs_resident_refresh)
            .finish_non_exhaustive()
    }
}

impl<R: LazyMultiBoxRender> ChildManager for SliverAdaptorManager<R> {
    fn service(
        &mut self,
        requested_indices: &[usize],
        retain_first: usize,
        retain_last: usize,
        tree: &mut ElementTree,
        owner: &mut ElementOwner<'_>,
        pipeline: &PipelineCell,
    ) -> bool {
        let Some(host) = self.host_element_id else {
            // service called before mount: programming-contract violation;
            // warn loudly but do not panic (production robustness).
            tracing::warn!("SliverAdaptorManager::service called before host element was mounted");
            return false;
        };

        // Reconcile against the (possibly just-updated) delegate BEFORE
        // evicting by band: a keyed resident whose old index falls outside
        // the new band but whose data moved inside it is relocated by the
        // reconcile, and only then does the band eviction judge it — by its
        // new index. Evicting first destroyed such a resident and the
        // request pass mounted fresh state (Flutter runs `performRebuild`'s
        // remap before `collectGarbage` for the same reason).
        let refresh_did_work = if self.needs_resident_refresh {
            self.needs_resident_refresh = false;
            let outcome = self.sparse_children.reconcile(
                ReconcileSource {
                    builder: &*self.builder,
                    find_index_by_key: self.find_index_by_key.as_deref(),
                    item_count: self.item_count(pipeline),
                    retain_band: (retain_first, retain_last),
                },
                host,
                tree,
                owner,
                pipeline,
            );
            let clamped = outcome
                .end_reached_at
                .is_some_and(|end| self.clamp_render_item_count(end, pipeline));
            outcome.did_work || clamped
        } else {
            false
        };
        // Then evict what the retain band no longer covers. An index that
        // falls outside the band and was also requested (a mid-scroll jump)
        // is correctly evicted then not rebuilt.
        let retain_did_work =
            self.sparse_children
                .retain_band(retain_first, retain_last, tree, owner);
        // Build each requested index that is (a) within the retain band and
        // (b) not already built. We check first to avoid calling the builder
        // for already-present indices (idempotency without closure overhead)
        // and to accurately track whether any new child was mounted.
        let mut any_new_build = false;
        let mut reached_end_at: Option<usize> = None;
        for &logical_index in requested_indices {
            if logical_index < retain_first || logical_index >= retain_last {
                // Fell outside the band we just retained — skip.
                continue;
            }
            if reached_end_at.is_some_and(|end| logical_index >= end) {
                continue;
            }
            if self.sparse_children.get(logical_index).is_some() {
                // Already built — no work needed.
                continue;
            }
            match build_item_or_error(&*self.builder, logical_index) {
                Some(view) => {
                    self.sparse_children.ensure(
                        logical_index,
                        view.0.as_ref(),
                        host,
                        tree,
                        owner,
                        pipeline,
                    );
                    any_new_build = true;
                }
                None => {
                    // The builder declined: the data source ends here. The
                    // render object's count follows, so the next pass reports
                    // the real extent and the viewport clamps (Flutter's
                    // `childCount` / `addInitialChild` failing → max extent).
                    reached_end_at =
                        Some(reached_end_at.map_or(logical_index, |end| end.min(logical_index)));
                }
            }
        }
        let count_clamped = reached_end_at
            .is_some_and(|end_index| self.clamp_render_item_count(end_index, pipeline));
        retain_did_work || refresh_did_work || any_new_build || count_clamped
    }

    fn forget_child(&mut self, child: ElementId) {
        self.sparse_children.forget(child);
    }
}

impl<R: LazyMultiBoxRender> SliverAdaptorManager<R> {
    /// The render object's current item count (the manager's view of the
    /// data source length).
    fn item_count(&self, pipeline: &PipelineCell) -> usize {
        let Some(render_id) = self.render_id else {
            return usize::MAX;
        };
        pipeline.with(|owner| {
            owner
                .render_tree()
                .get(render_id)
                .and_then(|node| node.downcast_render_object::<R>())
                .map_or(usize::MAX, R::item_count)
        })
    }

    /// Clamp the render object's item count to `end_index` when the builder
    /// declined that index. Returns whether the count changed.
    fn clamp_render_item_count(&mut self, end_index: usize, pipeline: &PipelineCell) -> bool {
        let Some(render_id) = self.render_id else {
            return false;
        };
        pipeline.with_mut(|owner| {
            let Some(render_object) = owner
                .render_tree_mut()
                .get_mut(render_id)
                .and_then(|node| node.downcast_render_object_mut::<R>())
            else {
                return false;
            };
            let impact = if end_index < render_object.item_count() {
                render_object.set_item_count(end_index)
            } else {
                flui_rendering::RenderUpdateImpact::NONE
            };
            owner.apply_render_update_impact(render_id, impact);
            !impact.is_none()
        })
    }
}

// ============================================================================
// BEHAVIOR
// ============================================================================

/// `ElementBehavior` for the lazy multi-box adaptor element, generic over the
/// render object family `R`.
///
/// Wraps [`RenderBehavior<SliverMultiBoxAdaptor<R>>`] (which handles
/// render-object creation and removal) and additionally:
/// - **mount**: stamps `host_element_id` on the manager and registers it in
///   `BuildOwner::child_manager_registry` keyed by the sliver's `RenderId`.
/// - **unmount**: pushes live sparse children to the inactive queue (needed
///   because the host's own `child_ids` stays empty, so the normal
///   dense-unmount walk can't reach them) and unregisters from the registry.
///
/// Registration happens in the adaptor's own `on_mount`, not in the generic
/// `behavior.rs:789` site, because that generic site has no way to reach
/// this element's child-manager.
pub(crate) struct SliverAdaptorBehavior<R: LazyMultiBoxRender> {
    /// Handles the render object's creation / update / removal.
    inner: RenderBehavior<SliverMultiBoxAdaptor<R>>,
    /// Shared manager; Arc lets `on_mount` insert a clone into the registry
    /// without moving out of `self`.
    manager: Arc<Mutex<SliverAdaptorManager<R>>>,
}

impl<R: LazyMultiBoxRender> std::fmt::Debug for SliverAdaptorBehavior<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverAdaptorBehavior")
            .field("render_id", &self.inner.render_id)
            .field("manager", &*self.manager.lock())
            .finish()
    }
}

impl<R: LazyMultiBoxRender> SliverAdaptorBehavior<R> {
    fn new(view: &SliverMultiBoxAdaptor<R>) -> Self {
        Self {
            inner: RenderBehavior::new(),
            manager: Arc::new(Mutex::new(SliverAdaptorManager {
                sparse_children: SparseChildren::new(),
                host_element_id: None,
                builder: Rc::clone(&view.builder),
                find_index_by_key: view.find_index_by_key.clone(),
                render_id: None,
                needs_resident_refresh: false,
                _render: PhantomData,
            })),
        }
    }
}

impl<R: LazyMultiBoxRender> ElementBehavior<SliverMultiBoxAdaptor<R>, Variable>
    for SliverAdaptorBehavior<R>
where
    flui_rendering::storage::RenderNode:
        From<Box<dyn flui_rendering::traits::RenderObject<SliverProtocol>>>,
{
    fn debug_kind(&self) -> &'static str {
        R::KIND
    }

    /// Returns empty — the dense reconciler must not touch lazy children.
    ///
    /// The inner `RenderBehavior::build_into_views` also returns empty
    /// because `SliverMultiBoxAdaptor::has_children() = false`; we forward
    /// for the `should_build` guard and `clear_dirty` side effect.
    fn build_into_views(
        &mut self,
        core: &mut ElementCore<SliverMultiBoxAdaptor<R>, Variable>,
        owner: &mut ElementOwner<'_>,
    ) -> Vec<Box<dyn View>> {
        self.inner.build_into_views(core, owner)
    }

    /// Creates the render object, registers the manager, and stamps
    /// `host_element_id` on the manager for later `service` calls.
    fn on_mount(
        &mut self,
        core: &mut ElementCore<SliverMultiBoxAdaptor<R>, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        // Step 1: create the render object via the inner RenderBehavior.
        self.inner.on_mount(core, owner);

        // Step 2: stamp the host element id on the manager now that the element
        // is slab-stamped (set_self_id fires before on_mount in ElementTree::insert).
        if let Some(self_id) = core.self_id() {
            self.manager.lock().host_element_id = Some(self_id);
        } else {
            tracing::warn!(
                "SliverAdaptorBehavior::on_mount: no self_id stamped — \
                 ChildManager service will be a no-op"
            );
        }

        // Step 3: register the manager keyed by the sliver's RenderId. This
        // registration belongs here, not in generic behavior.rs:789, since
        // only this behavior knows about the child-manager registry. The
        // manager's own `render_id` is stamped here too — independent of
        // whether `self_id` was available above — so a manager that IS
        // registered always has the `render_id` its `item_count` /
        // `clamp_render_item_count` need; those two half-states must not
        // travel together.
        match self.inner.render_id {
            Some(render_id) => {
                self.manager.lock().render_id = Some(render_id);
                owner.register_child_manager(
                    render_id,
                    Arc::clone(&self.manager) as Arc<Mutex<dyn ChildManager>>,
                );
                tracing::debug!(
                    ?render_id,
                    "SliverAdaptorBehavior: registered child manager"
                );
            }
            None => {
                // Happens when there is no PipelineOwner in scope (e.g. in
                // a pure-element test). `service_child_requests` will find no
                // entry for this sliver and skip it gracefully.
                tracing::warn!(
                    "SliverAdaptorBehavior::on_mount: no render_id yet (no PipelineOwner) — \
                     child manager not registered"
                );
            }
        }
    }

    /// Pushes live sparse children to the inactive queue (the host's own
    /// `child_ids` stays empty, so they're unreachable by the normal
    /// dense-unmount walk), unregisters the manager, and removes the render
    /// object.
    fn on_unmount(
        &mut self,
        core: &mut ElementCore<SliverMultiBoxAdaptor<R>, Variable>,
        owner: &mut ElementOwner<'_>,
    ) {
        // The host's `child_ids` stays empty by design, so `finalize_tree`'s
        // `collect_elements_to_unmount` cannot reach the lazy children via the
        // normal dense walk. Push each sparse child to the inactive queue at
        // a sentinel depth so `finalize_tree` unmounts them and recurses into
        // their own `child_ids` for descendants.
        //
        // Sentinel depth=1: an approximation. `finalize_tree` sorts deepest-
        // first; using 1 means lazy children appear near the top of the order.
        // This is safe because each sparse child is an independent subtree; the
        // only ordering contract finalize_tree has is parent-before-children
        // WITHIN a single subtree, which `collect_elements_to_unmount` already
        // enforces via pre-order + reverse-sweep.
        {
            let manager = self.manager.lock();
            // NOTE: these are pushed WITHOUT `deactivate()`, unlike the
            // canonical caller in `ElementTree::remove` — `on_unmount` has no
            // tree handle, only `ElementOwner`. So a sparse child sits in the
            // inactive queue while its `Lifecycle` is still `Active`, and
            // `ElementOwner::is_inactive` reports queue membership, not
            // lifecycle.
            //
            // That is sound ONLY because these pushes are unmount-only (the
            // host is being torn down; `finalize_tree` takes them to `Defunct`)
            // and because the lazy path registers no GlobalKey, so a sparse
            // child can never be a `retake_inactive_global_key` candidate —
            // that function activates from `Inactive` and would trip
            // `can_activate()`. If key attachment ever lands on sparse
            // children, they must be deactivated before being queued.
            for (_logical_index, child_id) in manager.sparse_children.iter_built() {
                owner.push_inactive(child_id, 1);
            }
        }

        // Unregister from the child-manager registry so no future
        // `service_child_requests` call hits a stale entry.
        if let Some(render_id) = self.inner.render_id {
            owner.unregister_child_manager(render_id);
            tracing::debug!(
                ?render_id,
                "SliverAdaptorBehavior: unregistered child manager"
            );
        }
        self.manager.lock().render_id = None;

        // Remove the render object via the inner behavior.
        self.inner.on_unmount(core, owner);
    }

    fn on_update(
        &mut self,
        core: &ElementCore<SliverMultiBoxAdaptor<R>, Variable>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        self.inner.on_update(core, owner);
    }

    fn on_view_updated(
        &mut self,
        core: &ElementCore<SliverMultiBoxAdaptor<R>, Variable>,
        old_view: &SliverMultiBoxAdaptor<R>,
        owner: &mut ElementOwner<'_>,
    ) {
        // NOTE: `item_count`/`config` do NOT travel through this call —
        // `RenderBehavior` has no `on_view_updated` override, so this hits
        // the empty trait default. They reach the render object via this
        // behavior's `on_update` delegation (`RenderBehavior::on_update` →
        // `RenderView::update_render_object` → `set_item_count` / `update`),
        // a separate, already-working path this fix does not touch.
        self.inner.on_view_updated(core, old_view, owner);

        // Refresh the stored builder and flag the resident children for
        // re-consultation on the next `service` call — see
        // `SliverAdaptorManager::needs_resident_refresh`'s doc comment for
        // the Flutter contract this mirrors and why it is needed at all
        // (`SparseChildren::ensure` is otherwise idempotent for an
        // already-built index, so without this an already-resident child
        // would show stale content forever across a `pump_widget` root-swap
        // that changes the backing item list/builder).
        if !delegate_changed(old_view, core.view()) {
            // Same builder, same key callback, same count: the residents
            // cannot read differently — Flutter's `SliverChildListDelegate.
            // shouldRebuild` (`children != oldDelegate.children`) says the
            // same for a list handed over unchanged; a builder delegate is a
            // fresh closure per build and never compares equal, exactly as
            // `SliverChildBuilderDelegate.shouldRebuild` is always true.
            return;
        }
        let mut manager = self.manager.lock();
        manager.builder = Rc::clone(&core.view().builder);
        manager
            .find_index_by_key
            .clone_from(&core.view().find_index_by_key);
        manager.needs_resident_refresh = true;
        drop(manager);

        // A changed delegate under `ItemCount::Unknown` may mean a changed
        // length, and nothing else can discover a GROWN one: the manager's
        // clamp only ever shrinks the count (it fires when a requested index
        // returns `None`), so a paged feed that gained a page — the case this
        // variant's own doc names — would never have its new indices
        // requested.
        //
        // The check is ONE builder call, not a re-search. `delegate_changed`
        // is true on every rebuild for a builder delegate (a fresh closure
        // never compares equal, exactly as `SliverChildBuilderDelegate.
        // shouldRebuild` is always true), so re-probing here unconditionally
        // would restore the per-rebuild search this design exists to avoid.
        // Asking the builder for the index one past the end is O(1) and
        // answers the only question that matters: `None` means the length is
        // unchanged or shrank, and the clamp already handles shrinking;
        // `Some` means it grew, and only then is a full search worth running.
        if core.view().item_count == ItemCount::Unknown
            && let (Some(render_id), Some(pipeline)) = (self.inner.render_id, core.pipeline_owner())
        {
            let builder = &*core.view().builder;
            let current = pipeline.with(|pipeline_owner| {
                pipeline_owner
                    .render_tree()
                    .get(render_id)
                    .and_then(|node| node.downcast_render_object::<R>())
                    .map(R::item_count)
            });
            if let Some(current) = current
                && let Some(grown) = regrown_item_count(current, builder)
            {
                pipeline.with_mut(|pipeline_owner| {
                    if let Some(render_object) = pipeline_owner
                        .render_tree_mut()
                        .get_mut(render_id)
                        .and_then(|node| node.downcast_render_object_mut::<R>())
                    {
                        let impact = render_object.set_item_count(grown);
                        pipeline_owner.apply_render_update_impact(render_id, impact);
                    }
                });
            }
        }

        // The refresh runs in the service pass after a layout: a delegate
        // swap with an unchanged count and config would otherwise leave the
        // sliver clean and the residents stale until something else laid it
        // out.
        if let (Some(render_id), Some(pipeline)) = (self.inner.render_id, core.pipeline_owner()) {
            pipeline.with_mut(|pipeline_owner| pipeline_owner.mark_needs_layout(render_id));
        }
    }

    fn render_id(&self) -> Option<RenderId> {
        self.inner.render_id()
    }
    fn hosts_sparse_children(&self) -> bool {
        true
    }
}

// ============================================================================
// TYPE ALIAS
// ============================================================================

/// Element type for the lazy multi-box adaptor, generic over the render
/// object family `R`.
///
/// Wraps `R` (via [`SliverAdaptorBehavior<R>`]) and owns a
/// [`SliverAdaptorManager<R>`] registered in `BuildOwner`'s
/// `child_manager_registry`. Post-layout, `BuildOwner::service_child_requests`
/// drives the manager to build or evict lazy children.
///
/// External consumers create adaptor elements through
/// [`SliverMultiBoxAdaptor::create_element`](View::create_element) (or
/// [`ListView::builder`](crate::BuildContext)) — not through this alias
/// directly — so `pub(crate)` is sufficient.
pub(crate) type SliverAdaptorElement<R> =
    Element<SliverMultiBoxAdaptor<R>, Variable, SliverAdaptorBehavior<R>>;

// ============================================================================
// SLIVER LIST — RenderSliverList as a LazyMultiBoxRender
// ============================================================================

/// Render-object-specific configuration for [`RenderSliverList`] under the
/// generic [`SliverMultiBoxAdaptor`] adaptor.
///
/// A named, `#[non_exhaustive]` struct rather than a bare `f32`: downstream
/// code constructs it through [`ListConfig::new`] and reads its fields, so a
/// future knob can be added without breaking a caller — a struct literal
/// would break on any new field, `Default` or not, which is why literals are
/// not part of the contract.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ListConfig {
    /// Default per-item extent (logical pixels), used to seed the virtualizer
    /// until real measurements arrive from laid-out children.
    pub item_extent_estimate: f32,
}

impl ListConfig {
    /// A list config with the given per-item extent estimate.
    #[must_use]
    pub const fn new(item_extent_estimate: f32) -> Self {
        Self {
            item_extent_estimate,
        }
    }
}

impl LazyMultiBoxRender for RenderSliverList {
    type Config = ListConfig;

    const KIND: &'static str = "SliverList";

    fn create(config: &Self::Config, item_count: usize) -> Self {
        RenderSliverList::new(item_count, config.item_extent_estimate)
    }

    fn update(&mut self, config: &Self::Config) -> flui_rendering::RenderUpdateImpact {
        self.set_default_extent_estimate(config.item_extent_estimate)
    }

    fn item_count(&self) -> usize {
        RenderSliverList::item_count(self)
    }

    fn set_item_count(&mut self, item_count: usize) -> flui_rendering::RenderUpdateImpact {
        RenderSliverList::set_item_count(self, item_count)
    }
}

/// The new length of an [`ItemCount::Unknown`] source that has grown past
/// `current`, or `None` when it has not.
///
/// Costs **one** builder call in the common case: the index just past the
/// known end either exists (the source grew, and only then is a full
/// [`probe_item_count`] search worth running) or does not (unchanged, or
/// shrank — which the adaptor's own clamp already handles). That bound is the
/// point. `on_view_updated` runs this on every rebuild, because a builder
/// delegate is a fresh closure each build and so always compares as changed,
/// so anything heavier here restores the per-rebuild search that resolving at
/// mount exists to avoid.
///
/// An already-unbounded `current` short-circuits to `None` without calling the
/// builder at all. The sentinel cannot grow; an endless builder would answer
/// `Some` at that index and send every rebuild through the full search; and
/// asking user code for index `usize::MAX` is a plausible way to overflow
/// arithmetic inside it.
fn regrown_item_count(
    current: usize,
    builder: &dyn Fn(usize) -> Option<BoxedView>,
) -> Option<usize> {
    if current == usize::MAX {
        return None;
    }
    super::sparse_children::build_item_or_error(builder, current)
        .is_some()
        .then(|| probe_item_count(builder))
}

/// How many items a lazy sliver has, when the caller knows.
///
/// [`Unknown`](ItemCount::Unknown) is for a source whose length only its
/// builder can answer — a paged feed, a generator. It is resolved once, by
/// probing the builder, rather than discovered a band at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCount {
    /// The caller knows the length. Always prefer this: it costs nothing,
    /// where [`Unknown`](Self::Unknown) costs `2·log₂(n)` builder calls once.
    Exact(usize),
    /// Only the builder knows. It must answer `None` for every index past the
    /// end and `Some` for every index before it — a builder that is `None` in
    /// the middle of its range truncates there.
    ///
    /// The length is searched for once at mount. After that each rebuild costs
    /// **one** builder call — asking whether the index just past the known end
    /// exists — and only a `Some` there, meaning the source actually grew,
    /// escalates to another search. A builder with side effects therefore sees
    /// `2·log₂(n)` calls at mount, one per rebuild, and another `2·log₂(n)`
    /// on each growth.
    ///
    /// One exception, and it is observable to a side-effectful builder: a
    /// source the initial search classified as *endless* — every index answers
    /// `Some`, so the length is `usize::MAX` — costs **zero** calls per
    /// rebuild. It cannot grow, so there is nothing to ask about, and asking
    /// would mean handing user code the index `usize::MAX`.
    Unknown,
}

impl From<usize> for ItemCount {
    /// `usize::MAX` becomes [`Unknown`](ItemCount::Unknown), not
    /// `Exact(usize::MAX)`.
    ///
    /// That value was the sentinel ADR-0053 recorded for "only the builder
    /// knows the length", so translating it is what it always meant. Mapping
    /// it to an exact count instead would keep every existing caller
    /// compiling *and* keep them on the old behaviour — advertising a sentinel
    /// extent and converging through later clamps — which is precisely the
    /// defect being fixed. A list that genuinely holds `usize::MAX` items
    /// cannot be materialised anyway; the window bounds it either way.
    fn from(count: usize) -> Self {
        if count == usize::MAX {
            Self::Unknown
        } else {
            Self::Exact(count)
        }
    }
}

/// Resolve an [`ItemCount::Unknown`] by probing `builder` for its end.
///
/// Doubling search for an upper bound, then bisection — Flutter's
/// `SliverMultiBoxAdaptorElement.childCount`, which does the same walk. Two
/// differences, both deliberate:
///
/// * **It probes, it does not build.** Flutter's search calls `_build`, which
///   inflates a widget inside a `buildScope` for every probe; the reference's
///   own doc tells callers to supply a count "to avoid the cost of searching".
///   Here the builder returns a view *value* and nothing is mounted, so the
///   search costs `2·log₂(n)` closure calls and the allocations they make.
/// * **The overflow case cannot throw.** Flutter raises a `FlutterError` when
///   even `i64::MAX` yields a child. A builder that answers `Some` for every
///   index is infinite, which is a legitimate thing to write, so the search
///   reports `usize::MAX` and lets the caller's window bound it — the same
///   answer the unbounded-window path already gives.
///
/// Every probe goes through `build_item_or_error`, the same boundary the
/// adaptor's own builder calls use, so a panicking builder does not unwind out
/// of the mount and abort the whole sliver. A panic counts the index as
/// **present**, because that is what the boundary does with it: the item
/// exists and renders the registered error view. Truncating the list at a
/// panicking index instead would silently shorten it because one row threw,
/// which is the worse failure — and the search's own upper bound still caps a
/// builder that panics everywhere.
///
/// The view a probe builds is discarded. A builder with side effects per index
/// therefore sees calls for indices that are never mounted, which is true of
/// Flutter's search too and is why both document supplying the count as the
/// cheaper option.
#[must_use]
pub fn probe_item_count(builder: &dyn Fn(usize) -> Option<BoxedView>) -> usize {
    // `lo` is a count known to be reachable (every index below it exists);
    // `hi` is a count known to be past the end, once the loop settles.
    let exists =
        |index: usize| super::sparse_children::build_item_or_error(builder, index).is_some();
    if !exists(0) {
        return 0;
    }
    let mut lo: usize = 1;
    let mut hi: usize = 2;
    while exists(hi - 1) {
        lo = hi;
        // On overflow, clamp the upper bound to `usize::MAX` and let the
        // bisection below decide. Returning `usize::MAX` here instead would
        // misreport every finite source whose length happens to sit in the
        // top half of the range: at this point only index `hi - 1` has been
        // shown to exist, and nothing above it has been tested at all.
        hi = hi.saturating_mul(2);
        if hi == usize::MAX && exists(usize::MAX - 1) {
            // Even the last representable index yields a child, so the source
            // is unbounded in any sense that matters. Flutter raises a
            // `FlutterError` here; an always-`Some` builder is a legitimate
            // infinite source, so this reports the sentinel the
            // unbounded-window path already handles.
            return usize::MAX;
        }
    }
    // Invariant: index `lo - 1` exists, index `hi - 1` does not.
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if exists(mid - 1) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The canonical lazy-sliver adaptor over [`RenderSliverList`].
///
/// See [`SliverMultiBoxAdaptor`]'s type-level doc for the shared lifecycle;
/// this alias's own constructors below (`new`, `separated`, `list`) are the
/// public surface `flui-widgets`' `ListView` builds on.
pub type SliverList = SliverMultiBoxAdaptor<RenderSliverList>;

impl SliverList {
    /// Construct a new lazy-sliver adaptor view configuration.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent_estimate` is not finite and positive — a zero or
    /// negative estimate seeds the virtualizer with an invalid band width.
    pub fn new(
        item_count: impl Into<ItemCount>,
        item_extent_estimate: f32,
        builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    ) -> Self {
        assert!(
            item_extent_estimate.is_finite() && item_extent_estimate > 0.0,
            "item_extent_estimate must be finite and positive, got {item_extent_estimate}",
        );
        SliverMultiBoxAdaptor::with_config(
            ListConfig::new(item_extent_estimate),
            item_count,
            builder,
        )
    }

    /// Construct a lazy-sliver adaptor that interleaves `item_count` items
    /// with separators placed between them.
    ///
    /// Mirrors Flutter's `SliverList.separated` named constructor
    /// (`widgets/sliver.dart` `SliverList.separated`, tag `3.44.0`): even
    /// logical indices delegate to `item_builder(index / 2)`, odd logical
    /// indices to `separator_builder((index - 1) / 2)`. The effective child
    /// count is `2 * item_count - 1` for `item_count > 0`, and `0` when
    /// `item_count` is `0` — Flutter's own `math.max(0, itemCount * 2 - 1)`.
    ///
    /// This is an inherent `SliverList` constructor, not a `flui-widgets`
    /// wrapper type, because `.separated` produces the exact same
    /// `SliverList` view FLUI already has — just a different interleaving
    /// builder — mirroring how Flutter's own `.builder`/`.separated`/`.list`
    /// all construct one `SliverList` widget class.
    ///
    /// # Panics
    ///
    /// Panics under the same condition as [`SliverList::new`]
    /// (`item_extent_estimate` must be finite and positive), and when
    /// `item_count` is large enough that the interleaved child count
    /// `2 * item_count - 1` overflows `usize`.
    pub fn separated(
        item_count: usize,
        item_extent_estimate: f32,
        item_builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
        separator_builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    ) -> Self {
        let child_count = item_count
            .checked_mul(2)
            .expect("BUG: item_count so large the interleaved child count overflows usize")
            .saturating_sub(1);
        let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |index: usize| {
            // Out-of-range consultation answers `None` before either
            // user builder runs — `SliverChildBuilderDelegate.build`'s own
            // index guard (`widgets/scroll_delegate.dart`, tag `3.44.0`).
            if index >= child_count {
                return None;
            }
            if index.is_multiple_of(2) {
                (item_builder)(index / 2)
            } else {
                // Flutter's `SliverList.separated` asserts the separator
                // builder returns a widget; a `None` here would silently
                // truncate the list at the first separator instead.
                let separator = (separator_builder)((index - 1) / 2);
                debug_assert!(
                    separator.is_some(),
                    "separator_builder must return a view for every in-range index"
                );
                separator
            }
        });
        Self::new(child_count, item_extent_estimate, builder)
    }

    /// Construct a lazy-sliver adaptor over a fixed list of pre-built child
    /// views.
    ///
    /// Mirrors Flutter's `SliverList.list` named constructor
    /// (`widgets/sliver.dart` `SliverList.list`, tag `3.44.0`), backed by
    /// `SliverChildListDelegate`: logical index `i` serves `children[i]`.
    ///
    /// FLUI's lazy-adaptor protocol may re-consult the builder for an
    /// already-resident index (`SparseChildren::refresh_resident`, driven by
    /// `SliverAdaptorManager`'s internal `needs_resident_refresh` flag), so
    /// an owned `Vec<BoxedView>` cannot be handed out by value more than
    /// once. Each call instead clones the stored [`BoxedView`] — a real,
    /// deep `dyn_clone` of the underlying view (`BoxedView`'s own `Clone`
    /// impl, `crates/flui-view/src/view/into_view.rs`), not a shared handle
    /// — which mirrors Flutter's own semantics: `SliverChildListDelegate.build`
    /// hands back the same immutable `Widget` value on every call, and
    /// FLUI's clone reproduces an equivalent view every time.
    ///
    /// # Panics
    ///
    /// Panics under the same condition as [`SliverList::new`].
    pub fn list(item_extent_estimate: f32, children: Vec<BoxedView>) -> Self {
        Self::over(item_extent_estimate, &StaticChildren::new(children))
    }

    /// The same as [`Self::list`] over an already shared delegate: two views
    /// built over one `Rc` compare as the same delegate on update, so the
    /// residents are not refreshed (Flutter's `shouldRebuild` by identity).
    #[must_use]
    pub fn over(item_extent_estimate: f32, children: &Rc<StaticChildren>) -> Self {
        Self::new(0, item_extent_estimate, Rc::new(|_| None)).over_static_children(children)
    }
}

// ============================================================================
// SLIVER GRID — RenderSliverGrid as a LazyMultiBoxRender
// ============================================================================

impl LazyMultiBoxRender for RenderSliverGrid {
    type Config = Arc<dyn flui_rendering::delegates::SliverGridDelegate>;

    const KIND: &'static str = "SliverGrid";

    fn create(config: &Self::Config, item_count: usize) -> Self {
        RenderSliverGrid::new(Arc::clone(config), item_count)
    }

    fn update(&mut self, config: &Self::Config) -> flui_rendering::RenderUpdateImpact {
        self.set_grid_delegate(Arc::clone(config))
    }

    fn item_count(&self) -> usize {
        RenderSliverGrid::item_count(self)
    }

    fn set_item_count(&mut self, item_count: usize) -> flui_rendering::RenderUpdateImpact {
        RenderSliverGrid::set_item_count(self, item_count)
    }
}

/// The canonical lazy-grid adaptor over [`RenderSliverGrid`].
///
/// See [`SliverMultiBoxAdaptor`]'s type-level doc for the shared lifecycle;
/// this alias's own constructor below is the public surface `flui-widgets`'
/// `GridView` builds on.
pub type SliverGrid = SliverMultiBoxAdaptor<RenderSliverGrid>;

impl SliverGrid {
    /// Constructs a new lazy-grid view configuration.
    pub fn new(
        grid_delegate: Arc<dyn flui_rendering::delegates::SliverGridDelegate>,
        item_count: usize,
        builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    ) -> Self {
        SliverMultiBoxAdaptor::with_config(grid_delegate, item_count, builder)
    }

    /// A grid over a fixed list of children, served lazily by index with the
    /// delegate's key map (Flutter's `SliverGrid` with a
    /// `SliverChildListDelegate`).
    #[must_use]
    pub fn list(
        grid_delegate: Arc<dyn flui_rendering::delegates::SliverGridDelegate>,
        children: Vec<BoxedView>,
    ) -> Self {
        Self::over(grid_delegate, &StaticChildren::new(children))
    }

    /// [`Self::list`] over an already shared delegate.
    #[must_use]
    pub fn over(
        grid_delegate: Arc<dyn flui_rendering::delegates::SliverGridDelegate>,
        children: &Rc<StaticChildren>,
    ) -> Self {
        Self::new(grid_delegate, 0, Rc::new(|_| None)).over_static_children(children)
    }
}

// ============================================================================
// SLIVER FIXED EXTENT LIST — RenderSliverFixedExtentList as a LazyMultiBoxRender
// ============================================================================

/// The one knob of a fixed-extent list: every child's main-axis extent.
///
/// `#[non_exhaustive]` with a constructor, like [`ListConfig`]: a future knob
/// is additive through [`FixedExtentConfig::new`], never through a literal.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct FixedExtentConfig {
    /// Main-axis extent every child is laid out to (logical pixels).
    pub item_extent: f32,
}

impl FixedExtentConfig {
    /// A config with the given per-child extent.
    #[must_use]
    pub const fn new(item_extent: f32) -> Self {
        Self { item_extent }
    }
}

impl LazyMultiBoxRender for RenderSliverFixedExtentList {
    type Config = FixedExtentConfig;

    const KIND: &'static str = "SliverFixedExtentList";

    fn create(config: &Self::Config, item_count: usize) -> Self {
        RenderSliverFixedExtentList::new(config.item_extent, item_count)
    }

    fn update(&mut self, config: &Self::Config) -> flui_rendering::RenderUpdateImpact {
        self.set_item_extent(config.item_extent)
    }

    fn item_count(&self) -> usize {
        RenderSliverFixedExtentList::item_count(self)
    }

    fn set_item_count(&mut self, item_count: usize) -> flui_rendering::RenderUpdateImpact {
        RenderSliverFixedExtentList::set_item_count(self, item_count)
    }
}

/// A lazily built list whose children all share one main-axis extent: the
/// index math needs no measurement, so any offset is a multiplication.
/// Flutter's `SliverFixedExtentList`.
pub type SliverFixedExtentList = SliverMultiBoxAdaptor<RenderSliverFixedExtentList>;

impl SliverFixedExtentList {
    /// A fixed-extent list of `item_count` children built on demand.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    #[must_use]
    pub fn new(
        item_extent: f32,
        item_count: usize,
        builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    ) -> Self {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and positive, got {item_extent}",
        );
        SliverMultiBoxAdaptor::with_config(FixedExtentConfig::new(item_extent), item_count, builder)
    }

    /// A fixed-extent list over a fixed list of children, served lazily by
    /// index with the delegate's key map.
    #[must_use]
    pub fn list(item_extent: f32, children: Vec<BoxedView>) -> Self {
        Self::over(item_extent, &StaticChildren::new(children))
    }

    /// [`Self::list`] over an already shared delegate.
    #[must_use]
    pub fn over(item_extent: f32, children: &Rc<StaticChildren>) -> Self {
        Self::new(item_extent, 0, Rc::new(|_| None)).over_static_children(children)
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use std::any::TypeId;
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_foundation::{ElementId, RenderId};
    use flui_objects::RenderSizedBox;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    use flui_types::geometry::px;

    use super::*;
    use crate::view::RenderView;
    use crate::{BuildOwner, ElementTree};

    // -------------------------------------------------------------------------
    // Shared test fixture — minimal item view used as a list placeholder.
    // Defined at module level to satisfy `clippy::items_after_statements`.
    // -------------------------------------------------------------------------

    #[derive(Clone)]
    pub(super) struct ItemView;

    impl RenderView for ItemView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;
        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(48.0)), Some(px(48.0)))
        }
        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for ItemView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// A second, distinct view type — same render shape, different `TypeId` —
    /// so a refresh whose new builder returns this instead of [`ItemView`]
    /// exercises the incompatible-type (evict + remount) branch.
    #[derive(Clone)]
    struct OtherItemView;

    impl RenderView for OtherItemView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;
        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(48.0)), Some(px(48.0)))
        }
        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _: &mut Self::RenderObject,
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    impl View for OtherItemView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    fn make_builder(item_count: usize) -> Rc<dyn Fn(usize) -> Option<BoxedView>> {
        Rc::new(move |idx: usize| {
            if idx < item_count {
                Some(BoxedView(Box::new(ItemView)))
            } else {
                None
            }
        })
    }

    // -------------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------------

    /// `SliverList::new` panics on a zero extent estimate.
    #[test]
    fn new_panics_on_zero_estimate() {
        let builder = make_builder(10);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SliverList::new(10, 0.0, builder)
        }));
        assert!(result.is_err(), "zero estimate must panic");
    }

    /// `SliverList::new` panics on a negative extent estimate.
    #[test]
    fn new_panics_on_negative_estimate() {
        let builder = make_builder(10);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SliverList::new(10, -1.0, builder)
        }));
        assert!(result.is_err(), "negative estimate must panic");
    }

    /// Valid construction sets all fields and enforces the no-dense-children
    /// invariant.
    #[test]
    fn new_succeeds_with_valid_parameters() {
        let builder = make_builder(100);
        let view = SliverList::new(100, 48.0, builder);
        assert_eq!(view.item_count, ItemCount::Exact(100));
        assert!((view.config.item_extent_estimate - 48.0).abs() < f32::EPSILON);
        assert!(
            !view.has_children(),
            "adaptor view must have no dense children"
        );
    }

    /// Builder is called with the expected index; returns `Some` for valid
    /// indices and `None` for out-of-range.
    #[test]
    fn builder_returns_some_for_valid_index_and_none_for_out_of_range() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |idx: usize| {
            call_count_clone.fetch_add(1, Ordering::Relaxed);
            if idx < 5 {
                Some(BoxedView(Box::new(ItemView)))
            } else {
                None
            }
        });

        let view = SliverList::new(5, 48.0, Rc::clone(&builder));
        assert!(
            !view.has_children(),
            "adaptor view must report no dense children"
        );
        assert!((view.builder)(3).is_some());
        assert!((view.builder)(5).is_none());
        assert_eq!(call_count.load(Ordering::Relaxed), 2);
    }

    // -------------------------------------------------------------------------
    // `SliverList::separated`
    // -------------------------------------------------------------------------

    /// `SliverList::separated` reports the interleaved child count:
    /// `2 * item_count - 1` for `item_count > 0`.
    #[test]
    fn separated_reports_interleaved_child_count() {
        let view = SliverList::separated(3, 48.0, make_builder(3), make_builder(3));
        assert_eq!(
            view.item_count,
            ItemCount::Exact(5),
            "2*3-1 = 5 interleaved logical slots"
        );
    }

    /// `SliverList::separated` with zero items produces zero children —
    /// Flutter's own `math.max(0, itemCount * 2 - 1)` clamps rather than
    /// underflowing.
    #[test]
    fn separated_with_zero_items_has_zero_children() {
        let view = SliverList::separated(0, 48.0, make_builder(0), make_builder(0));
        assert_eq!(view.item_count, ItemCount::Exact(0));
    }

    /// `SliverList::separated` maps even logical indices to item indices
    /// `0, 1, 2, ...` in order and odd logical indices to separator indices
    /// `0, 1, ...` in order — the exact arithmetic Flutter's
    /// `SliverList.separated` uses (`index.isEven ? index ~/ 2 : (index - 1)
    /// ~/ 2`, `widgets/sliver.dart`, tag `3.44.0`).
    #[test]
    fn separated_maps_logical_index_to_item_and_separator_index_correctly() {
        let item_indices: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let item_indices_probe = Rc::clone(&item_indices);
        let item_builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |idx: usize| {
            item_indices_probe.borrow_mut().push(idx);
            Some(BoxedView(Box::new(ItemView)))
        });

        let separator_indices: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let separator_indices_probe = Rc::clone(&separator_indices);
        let separator_builder: Rc<dyn Fn(usize) -> Option<BoxedView>> =
            Rc::new(move |idx: usize| {
                separator_indices_probe.borrow_mut().push(idx);
                Some(BoxedView(Box::new(ItemView)))
            });

        let view = SliverList::separated(3, 48.0, item_builder, separator_builder);
        let ItemCount::Exact(child_count) = view.item_count else {
            panic!("separated declares an exact interleaved child count");
        };
        for logical_index in 0..child_count {
            (view.builder)(logical_index);
        }

        assert_eq!(
            *item_indices.borrow(),
            vec![0, 1, 2],
            "even logical indices 0,2,4 must map to item indices 0,1,2 in order"
        );
        assert_eq!(
            *separator_indices.borrow(),
            vec![0, 1],
            "odd logical indices 1,3 must map to separator indices 0,1 in order"
        );
    }

    /// `SliverList::separated` panics on a non-positive extent estimate,
    /// same as [`SliverList::new`].
    #[test]
    fn separated_panics_on_zero_estimate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SliverList::separated(3, 0.0, make_builder(3), make_builder(3))
        }));
        assert!(result.is_err(), "zero estimate must panic");
    }

    // -------------------------------------------------------------------------
    // `SliverList::list`
    // -------------------------------------------------------------------------

    /// `SliverList::list` reports the children count and serves each index
    /// from the stored list; an out-of-range index returns `None`.
    #[test]
    fn list_serves_children_by_index_and_reports_their_count() {
        let children = vec![
            BoxedView(Box::new(ItemView)),
            BoxedView(Box::new(OtherItemView)),
        ];
        let view = SliverList::list(48.0, children);

        assert_eq!(view.item_count, ItemCount::Exact(2));
        assert!((view.builder)(0).is_some());
        assert!((view.builder)(1).is_some());
        assert!(
            (view.builder)(2).is_none(),
            "an index past the end of the list must return None"
        );
    }

    /// `SliverList::list`'s builder can be called more than once for the
    /// same index without panicking or exhausting the list — the shape
    /// `SparseChildren::refresh_resident` needs when it re-consults the
    /// builder for an already-resident child.
    #[test]
    fn list_builder_can_be_called_more_than_once_for_the_same_index() {
        let children = vec![BoxedView(Box::new(ItemView))];
        let view = SliverList::list(48.0, children);

        assert!((view.builder)(0).is_some());
        assert!((view.builder)(0).is_some());
        assert!((view.builder)(0).is_some());
    }

    /// `SliverList::list` panics on a non-positive extent estimate, same as
    /// [`SliverList::new`].
    #[test]
    fn list_panics_on_zero_estimate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SliverList::list(0.0, vec![BoxedView(Box::new(ItemView))])
        }));
        assert!(result.is_err(), "zero estimate must panic");
    }

    /// `SliverList` is `Clone` (required by `View` + `RenderView`).
    /// A render object outside this crate joins by implementing the trait
    /// and constructing the adaptor through the generic constructor — the
    /// aliases' constructors are conveniences, not the only door.
    #[test]
    fn generic_constructor_builds_an_adaptor_element() {
        let view = SliverMultiBoxAdaptor::<RenderSliverList>::with_config(
            ListConfig::new(48.0),
            3,
            make_builder(3),
        );
        assert_eq!(view.item_count, ItemCount::Exact(3));
        assert!(matches!(
            view.create_element(),
            crate::element::ElementKind::RenderVariable(_)
        ));
    }

    /// The identity rule: a builder delegate is re-consulted on every update
    /// even when its `Rc` is reused (its output may depend on state the
    /// adaptor cannot see); a static delegate handed over unchanged is not.
    #[test]
    fn delegate_changed_distinguishes_builder_from_static_delegates() {
        let builder = make_builder(3);
        let with_builder = SliverList::new(3, 48.0, Rc::clone(&builder));
        let with_same_builder = SliverList::new(3, 48.0, builder);
        assert!(
            delegate_changed(&with_builder, &with_same_builder),
            "a reused builder closure still refreshes"
        );

        let children = StaticChildren::new(vec![
            BoxedView(Box::new(ItemView)),
            BoxedView(Box::new(ItemView)),
        ]);
        let over_children = SliverList::over(48.0, &children);
        let over_same_children = SliverList::over(48.0, &children);
        assert!(
            !delegate_changed(&over_children, &over_same_children),
            "two adaptors over one static delegate are the same delegate"
        );
        let other = StaticChildren::new(vec![BoxedView(Box::new(ItemView))]);
        let over_other = SliverList::over(48.0, &other);
        assert!(
            delegate_changed(&over_children, &over_other),
            "a different static delegate refreshes"
        );
    }

    /// The pair a static delegate hands out is created once, so identity
    /// survives across adaptor builds — and holds no strong reference back
    /// to the delegate.
    #[test]
    fn static_delegate_pair_is_stable_and_does_not_leak() {
        let children = StaticChildren::new(vec![BoxedView(Box::new(ItemView))]);
        let (b1, f1) = children.delegate_pair();
        let (b2, f2) = children.delegate_pair();
        assert!(Rc::ptr_eq(&b1, &b2) && Rc::ptr_eq(&f1, &f2));
        assert!(b1(0).is_some());
        let weak = Rc::downgrade(&children);
        drop(children);
        assert!(
            weak.upgrade().is_none(),
            "the cached closures must not keep the delegate alive"
        );
        assert!(
            b1(0).is_none(),
            "a closure outliving its delegate answers None"
        );
    }

    #[test]
    fn view_is_clone() {
        let builder = make_builder(10);
        let view = SliverList::new(10, 48.0, builder);
        let cloned = view.clone();
        assert_eq!(cloned.item_count, ItemCount::Exact(10));
        assert!((cloned.config.item_extent_estimate - 48.0).abs() < f32::EPSILON);
    }

    /// `create_element` produces a `SliverAdaptorElement<RenderSliverList>`
    /// (the view type id round-trips through the `dyn ElementBase`
    /// interface).
    ///
    /// Specifically: `view_type_id() == TypeId::of::<SliverList>()`, NOT
    /// `TypeId::of::<SliverAdaptorElement<RenderSliverList>>()` or any
    /// internal adaptor name. This is the identity the reconciler checks in
    /// `can_update_by_id` — if it were wrong, the element would be torn down
    /// and rebuilt on every parent rebuild that produces a new `SliverList`
    /// view (BLOCKER 1).
    #[test]
    fn create_element_produces_adaptor_element() {
        let builder = make_builder(10);
        let view = SliverList::new(10, 48.0, builder);
        let element = view.create_element();
        assert_eq!(element.element().view_type_id(), TypeId::of::<SliverList>());
    }

    // =========================================================================
    // Helper: minimal tree wired to a PipelineOwner, for service + round-trip.
    // =========================================================================

    /// Mount a render-bearing `ItemView` root wired to a fresh `PipelineOwner`.
    /// Returns `(tree, build_owner, pipeline, host_element_id)`.
    fn host_tree() -> (ElementTree, BuildOwner, PipelineCell, ElementId) {
        let pipeline = PipelineCell::new(PipelineOwner::new());
        let mut build_owner = BuildOwner::new();
        let mut tree = ElementTree::new();
        let host = tree.mount_root_with_pipeline_owner(
            &ItemView,
            Some(pipeline.clone()),
            &mut build_owner.element_owner_mut(),
        );
        (tree, build_owner, pipeline, host)
    }

    /// Construct a bare `SliverAdaptorManager<RenderSliverList>` for direct
    /// unit-testing (bypassing the behavior's `on_mount` wiring).
    fn list_manager(host: ElementId, item_count: usize) -> SliverAdaptorManager<RenderSliverList> {
        SliverAdaptorManager {
            sparse_children: SparseChildren::new(),
            host_element_id: Some(host),
            builder: make_builder(item_count),
            find_index_by_key: None,
            render_id: None,
            needs_resident_refresh: false,
            _render: PhantomData,
        }
    }

    /// Construct a bare `SliverAdaptorManager<RenderSliverGrid>` for
    /// direct unit-testing (bypassing the behavior's `on_mount` wiring).
    fn grid_manager(host: ElementId, item_count: usize) -> SliverAdaptorManager<RenderSliverGrid> {
        SliverAdaptorManager {
            sparse_children: SparseChildren::new(),
            host_element_id: Some(host),
            builder: make_builder(item_count),
            find_index_by_key: None,
            render_id: None,
            needs_resident_refresh: false,
            _render: PhantomData,
        }
    }

    // =========================================================================
    // Test gap 6a: `ChildManager::service` bool-return unit tests.
    // =========================================================================

    /// `ChildManager::service` must return `false` when no children are evicted
    /// and no new children are built — the quiescence signal that prevents
    /// `service_child_requests` from calling `mark_needs_layout` and therefore
    /// issuing another layout pass on an already-settled sliver.
    #[test]
    fn service_returns_false_when_no_work_done() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        // Manager with no pre-built children; no requested indices; full retain
        // band [0, usize::MAX) ≡ keep everything.
        let mut manager = list_manager(host, 5);

        let did_work = manager.service(
            &[],        // no children requested
            0,          // retain_first
            usize::MAX, // retain_last — nothing is out-of-band
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            !did_work,
            "service with no evictions and no builds must return false (quiescence gate)"
        );
    }

    /// `ChildManager::service` must return `true` when it builds at least one
    /// new child. `true` tells `service_child_requests` to call
    /// `mark_needs_layout` so the sliver lays out the freshly-built children.
    #[test]
    fn service_returns_true_when_children_are_built() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = list_manager(host, 5);

        // Request index 0, retain band [0, 1): service must build item 0.
        let did_work = manager.service(
            &[0],
            0,
            1,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            did_work,
            "service that builds at least one child must return true"
        );
        assert!(
            manager.sparse_children.get(0).is_some(),
            "the requested child must be present in SparseChildren after service"
        );
    }

    /// `ChildManager::service` must return `true` when it evicts at least one
    /// child that has scrolled outside the retain band — the off-band
    /// eviction path.
    #[test]
    fn service_returns_true_when_children_are_evicted() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = list_manager(host, 5);

        // Seed two pre-built children at indices 0 and 1.
        manager.service(
            &[0, 1],
            0,
            2,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        assert_eq!(
            manager.sparse_children.len(),
            2,
            "pre-condition: 2 children built"
        );

        // Retain band [5, 10): both pre-built children (0, 1) are out-of-band.
        let did_work = manager.service(
            &[],
            5,
            10,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            did_work,
            "service that evicts at least one child must return true"
        );
        assert_eq!(
            manager.sparse_children.len(),
            0,
            "all out-of-band children must be evicted"
        );
    }

    // =========================================================================
    // `needs_resident_refresh` → `refresh_resident`: the builder-staleness fix.
    // =========================================================================

    /// After the item builder is swapped and `needs_resident_refresh` is set,
    /// the next `service` re-consults the NEW builder for every resident index
    /// and, when the result is the same view type, updates the existing child
    /// in place — preserving its `ElementId` (identity/state) rather than
    /// evicting and remounting. The flag is consumed exactly once.
    #[test]
    fn refresh_resident_updates_in_place_and_consumes_flag() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = list_manager(host, 3);

        // Seed a resident child at index 0.
        manager.service(
            &[0],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        let before = manager
            .sparse_children
            .get(0)
            .expect("index 0 resident after seed");

        // Swap in a fresh (same-type) builder that counts its calls, and flag
        // the residents for refresh.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_probe = Arc::clone(&calls);
        let refreshed: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |idx: usize| {
            calls_probe.fetch_add(1, Ordering::Relaxed);
            (idx < 3).then(|| BoxedView(Box::new(ItemView)))
        });
        manager.builder = refreshed;
        manager.needs_resident_refresh = true;

        manager.service(
            &[],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            calls.load(Ordering::Relaxed) >= 1,
            "refresh must re-consult the new builder for the resident index"
        );
        assert_eq!(
            manager.sparse_children.get(0),
            Some(before),
            "a same-type refresh must update in place, preserving the ElementId"
        );
        assert!(
            !manager.needs_resident_refresh,
            "the refresh flag must be consumed exactly once"
        );
    }

    /// When the swapped-in builder returns a DIFFERENT view type for a
    /// resident index, `refresh_resident` evicts the stale child and remounts
    /// a fresh one — matching Flutter's remount-on-incompatible-type behavior.
    /// The resident `ElementId` changes.
    #[test]
    fn refresh_resident_remounts_on_type_change() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = list_manager(host, 3);

        manager.service(
            &[0],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        let before = manager
            .sparse_children
            .get(0)
            .expect("index 0 resident after seed");

        // New builder returns a different concrete type at the same index.
        let remounting: Rc<dyn Fn(usize) -> Option<BoxedView>> =
            Rc::new(|idx: usize| (idx < 3).then(|| BoxedView(Box::new(OtherItemView))));
        manager.builder = remounting;
        manager.needs_resident_refresh = true;

        manager.service(
            &[],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        let after = manager
            .sparse_children
            .get(0)
            .expect("index 0 still resident after refresh remount");
        assert_ne!(
            after, before,
            "an incompatible-type refresh must evict and remount, changing the ElementId"
        );
        assert!(
            !manager.needs_resident_refresh,
            "the refresh flag must be consumed exactly once"
        );
    }

    // =========================================================================
    // `needs_resident_refresh` → `refresh_resident`: the grid sister fix.
    // Mirrors the two `refresh_resident_*` tests above exactly, driving
    // `SliverAdaptorManager<RenderSliverGrid>::service` instead of the
    // list manager's — confirming by construction that the shared, generic
    // manager behaves identically for both render families.
    // =========================================================================

    /// After the item builder is swapped and `needs_resident_refresh` is set,
    /// the next `service` re-consults the NEW builder for every resident index
    /// and, when the result is the same view type, updates the existing child
    /// in place — preserving its `ElementId` (identity/state) rather than
    /// evicting and remounting. The flag is consumed exactly once.
    #[test]
    fn grid_refresh_resident_updates_in_place_and_consumes_flag() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = grid_manager(host, 3);

        // Seed a resident child at index 0.
        manager.service(
            &[0],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        let before = manager
            .sparse_children
            .get(0)
            .expect("index 0 resident after seed");

        // Swap in a fresh (same-type) builder that counts its calls, and flag
        // the residents for refresh.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_probe = Arc::clone(&calls);
        let refreshed: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |idx: usize| {
            calls_probe.fetch_add(1, Ordering::Relaxed);
            (idx < 3).then(|| BoxedView(Box::new(ItemView)))
        });
        manager.builder = refreshed;
        manager.needs_resident_refresh = true;

        manager.service(
            &[],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        assert!(
            calls.load(Ordering::Relaxed) >= 1,
            "refresh must re-consult the new builder for the resident index"
        );
        assert_eq!(
            manager.sparse_children.get(0),
            Some(before),
            "a same-type refresh must update in place, preserving the ElementId"
        );
        assert!(
            !manager.needs_resident_refresh,
            "the refresh flag must be consumed exactly once"
        );
    }

    /// When the swapped-in builder returns a DIFFERENT view type for a
    /// resident index, `refresh_resident` evicts the stale child and remounts
    /// a fresh one — matching Flutter's remount-on-incompatible-type behavior.
    /// The resident `ElementId` changes.
    #[test]
    fn grid_refresh_resident_remounts_on_type_change() {
        let (mut tree, mut build_owner, pipeline, host) = host_tree();

        let mut manager = grid_manager(host, 3);

        manager.service(
            &[0],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
        let before = manager
            .sparse_children
            .get(0)
            .expect("index 0 resident after seed");

        // New builder returns a different concrete type at the same index.
        let remounting: Rc<dyn Fn(usize) -> Option<BoxedView>> =
            Rc::new(|idx: usize| (idx < 3).then(|| BoxedView(Box::new(OtherItemView))));
        manager.builder = remounting;
        manager.needs_resident_refresh = true;

        manager.service(
            &[],
            0,
            usize::MAX,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );

        let after = manager
            .sparse_children
            .get(0)
            .expect("index 0 still resident after refresh remount");
        assert_ne!(
            after, before,
            "an incompatible-type refresh must evict and remount, changing the ElementId"
        );
        assert!(
            !manager.needs_resident_refresh,
            "the refresh flag must be consumed exactly once"
        );
    }

    // =========================================================================
    // Test gap 6b: register/unregister round-trip via element lifecycle.
    // =========================================================================

    /// Mounting a `SliverList` element must register its `ChildManager` in the
    /// `BuildOwner`'s registry (keyed by the sliver's `RenderId`), and unmounting
    /// it must remove that entry. This end-to-end path exercises
    /// `SliverAdaptorBehavior::on_mount` → `ElementOwner::register_child_manager`
    /// and `on_unmount` → `ElementOwner::unregister_child_manager`.
    #[test]
    fn child_manager_registered_on_mount_and_unregistered_on_unmount() {
        let (mut tree, mut build_owner, _pipeline, host) = host_tree();

        let sliver = SliverList::new(5, 48.0, make_builder(5));

        // Mount: `on_mount` must register the ChildManager.
        let sliver_id = tree.insert(&sliver, host, 0, &mut build_owner.element_owner_mut());

        // The element's render node carries the RenderId used as the registry key.
        let sliver_render_id: Option<RenderId> =
            tree.get(sliver_id).and_then(|n| n.element().render_id());
        let sliver_render_id =
            sliver_render_id.expect("SliverList element must have a render node after mount");

        {
            let registry = build_owner.child_manager_registry.lock();
            assert!(
                registry.contains_key(&sliver_render_id),
                "ChildManager must be registered in the BuildOwner registry after on_mount"
            );
        }

        // Unmount: `on_unmount` must unregister the ChildManager.
        tree.remove_subtree(
            sliver_id,
            &mut build_owner.element_owner_mut(),
            crate::tree::SubtreeRemoval::DeactivateKeyed,
        );

        {
            let registry = build_owner.child_manager_registry.lock();
            assert!(
                !registry.contains_key(&sliver_render_id),
                "ChildManager must be removed from the BuildOwner registry after on_unmount"
            );
        }
    }
}

#[cfg(test)]
mod probe_tests {
    use super::{ItemCount, probe_item_count};
    use crate::BoxedView;
    use std::cell::RefCell;

    /// A builder over `0..len` that records every index it was asked for.
    fn counting(len: usize, calls: &RefCell<Vec<usize>>) -> impl Fn(usize) -> Option<BoxedView> {
        move |index| {
            calls.borrow_mut().push(index);
            (index < len).then(|| BoxedView(Box::new(super::tests::ItemView)))
        }
    }

    #[test]
    fn the_probe_finds_the_exact_end_for_every_small_length() {
        // Exhaustive over the range where the doubling and bisection phases
        // interleave differently — off-by-one here is the whole risk.
        for len in 0..=64 {
            let calls = RefCell::new(Vec::new());
            assert_eq!(
                probe_item_count(&counting(len, &calls)),
                len,
                "probe disagreed for len {len}",
            );
        }
    }

    #[test]
    fn the_probe_is_logarithmic_not_linear() {
        let calls = RefCell::new(Vec::new());
        let len = 100_000;
        assert_eq!(probe_item_count(&counting(len, &calls)), len);
        let made = calls.borrow().len();
        // Doubling to pass the end, then bisecting: about 2·log2(n). The bound
        // is what distinguishes this from a walk — a linear scan would make
        // 100_001 calls and still pass an equality assertion on the result.
        assert!(
            made < 4 * (usize::BITS - len.leading_zeros()) as usize,
            "probe made {made} builder calls for {len} items; expected O(log n)",
        );
    }

    #[test]
    fn the_growth_check_costs_one_call_and_none_at_all_when_unbounded() {
        use super::regrown_item_count;

        let calls = RefCell::new(Vec::new());
        // Unchanged: one call, no growth.
        assert_eq!(regrown_item_count(4, &counting(4, &calls)), None);
        assert_eq!(
            calls.borrow().as_slice(),
            &[4],
            "the check asks for exactly the index past the end"
        );

        // Grown: the one call finds a child, and only then does it search.
        let calls = RefCell::new(Vec::new());
        assert_eq!(regrown_item_count(4, &counting(9, &calls)), Some(9));
        assert!(
            calls.borrow().len() > 1,
            "a grown source escalates to the full search"
        );

        // Already unbounded: no builder call at all. An endless builder would
        // answer `Some` at `usize::MAX` and send every rebuild through the
        // whole search — and being asked for that index is its own hazard.
        let calls = RefCell::new(Vec::new());
        assert_eq!(
            regrown_item_count(usize::MAX, &counting(usize::MAX, &calls)),
            None
        );
        assert!(
            calls.borrow().is_empty(),
            "an unbounded source must not be asked anything: {:?}",
            calls.borrow()
        );
    }

    #[test]
    fn a_panicking_builder_does_not_unwind_out_of_the_probe() {
        // Every probe goes through the same boundary the adaptor's own builder
        // calls use, so a panic becomes the registered error view rather than
        // an unwind out of `create_render_object` that would abort mounting
        // the whole sliver — the per-item recovery `ItemCount::Unknown` must
        // not quietly disable.
        //
        // A panic counts the index as PRESENT, which is what the boundary does
        // with it: truncating the list because one row threw would silently
        // shorten it, and the error view is displayable at that index.
        //
        // Index 3 is chosen because the search actually visits it. For a
        // 5-item source the probe asks for 0, 1, 3, 7, 5, 4 — a panic at 2
        // would never be reached and the test would pass with the boundary
        // removed, which the first version of it did.
        let builder = |index: usize| -> Option<BoxedView> {
            assert!(index != 3, "boom at index 3");
            (index < 5).then(|| BoxedView(Box::new(super::tests::ItemView)))
        };
        assert_eq!(
            probe_item_count(&builder),
            5,
            "a panic at one index must neither escape nor truncate the search"
        );
    }

    #[test]
    fn an_endless_builder_reports_unbounded_instead_of_erroring() {
        // Flutter raises a `FlutterError` here. An always-`Some` builder is a
        // legitimate infinite source, so this reports the sentinel the
        // unbounded-window path already handles.
        let endless = |_: usize| Some(BoxedView(Box::new(super::tests::ItemView)));
        assert_eq!(probe_item_count(&endless), usize::MAX);
    }

    #[test]
    fn item_count_is_copy_and_compares_by_value() {
        assert_eq!(ItemCount::Exact(3), ItemCount::Exact(3));
        assert_ne!(ItemCount::Exact(3), ItemCount::Unknown);
    }
}
