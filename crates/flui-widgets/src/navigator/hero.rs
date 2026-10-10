//! The `Hero` view, its per-route registry, and the handle a `HeroController` drives.
//!
//! `Hero` is public; its registry, handle and tag
//! storage are not public API (nameable only through the doc-hidden, temporary
//! `__test_access`, ADR-0083 §4). A `Hero` registers with its route, can be *told* to show a
//! placeholder, and exposes the signed-off customization hooks:
//! `create_rect_tween`, `flight_shuttle_builder`, and FLUI's state-preserving
//! `placeholder`.
//!
//! # Registration, not an element walk
//!
//! Finding a route's heroes by walking its element subtree, testing each widget for
//! `Hero` and downcasting to its state is not possible here: a downcast from
//! `&dyn View` is exactly the view-type smuggling FLUI rules out, and an element walk
//! from an observer callback is exactly what a previous change deliberately removed.
//!
//! So the direction is inverted. Each `Hero` **registers itself** with the nearest
//! enclosing [`HeroScope`] in `init_state` and deregisters in `dispose`. The registry
//! is owned by the route (`ModalInner`), reachable by `RouteId` through the
//! navigator's modal registry, and the controller reads it as pure data. No
//! `GlobalKey`, no tree re-entry, no downcast.
//!
//! Two consequences, both recorded:
//!
//! * **Registration order, not tree order.** The registry is filled in mount order,
//!   which for a static subtree matches depth-first tree order, and for a dynamic one
//!   does not. Nothing in the flight algorithm depends on the order — it looks tags
//!   up, never iterates positionally.
//! * **A `Hero` under a nested `Navigator` registers with its own route**, because it
//!   finds *its* nearest `HeroScope`. A hero inside a nested `Navigator`'s current
//!   `PageRoute` must still be visible to an outer flight, which is what
//!   [`NestedHeroSource`] provides: a nested `Navigator` publishes it on the
//!   nearest enclosing route from its own `build` (resynced every time, so a
//!   `GlobalKey` reparent under a different route is never missed), and
//!   [`HeroRegistry::all_heroes`] resolves it recursively, without an element walk.
//!   `HeroControllerScope::none` still blocks a nested navigator's own
//!   *auto-default controller* (so it flies no heroes on its own pushes/pops), but it
//!   does not gate this visibility hook.
//!
//! **A `GlobalKey`-reparented nested `Navigator`'s re-sync is verified in
//! isolation, not end-to-end.** `NavigatorState::sync_nested_hero_registration`
//! (`navigator.rs`) re-resolves and republishes on every `build`, and hand-traced
//! instrumentation confirmed it re-points at the new route immediately after a
//! reparent. A `flui_widgets`-level regression test built on top of that (reparent,
//! then push a further route to force a flight measurement) surfaced what looks
//! like a **pre-existing, separate** timing gap: `ctx.get::<HeroScope, _>` can
//! transiently answer with the wrong route's registry across a few builds while a
//! *recently-migrated* route is itself being covered by yet another push — a
//! symptom a plain (non-reparented) three-route chain with the same double-cover
//! shape did not reproduce, so it reads as an inherited-ambient-resolution timing
//! issue in `flui-view`, not a defect in this fix's own diff. No end-to-end
//! `GlobalKey`-reparent-with-a-hero test ships as a result — see the fuller note on
//! `NavigatorState::sync_nested_hero_registration` (`navigator.rs`).
//!
//! # Duplicate tags
//!
//! A duplicate tag does not panic: it is a *caller*
//! mistake, and [`PANIC-POLICY`](../../../../docs/PANIC-POLICY.md) reserves panics
//! for framework invariants. FLUI logs and keeps the **first** — "last wins" would
//! make the surviving hero depend on mount order.

use super::lifecycle::Terminal;
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_animation::{Animatable, Animation, ArcCurve, Curve, Curves};
use flui_foundation::geometry::Rect;
use flui_foundation::geometry::Size;
use flui_foundation::{RenderId, ViewKey};
use flui_objects::SubtreeAnchor;
use flui_rendering::pipeline::PipelineCell;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, impl_inherited_view};
use parking_lot::Mutex;

use super::hero_controller::FlightDirection;
use crate::__private::AnchoredBox;
use crate::{Offstage, SizedBox, Stack, TickerMode};

/// Builds the [`RectTween`](flui_animation::RectTween)-like path a hero's shuttle
/// follows. The default is a linear `RectTween`. Erased and `Rc`-shared because hero customization is UI-owner
/// local, including the returned mapping and its captures.
pub(crate) type RectTweenFactory = Rc<dyn Fn(Rect, Rect) -> Box<dyn Animatable<Value = Rect>>>;

/// Builds the widget shown in flight instead of the default (a fresh copy of the
/// destination hero's child). The builder receives the flight animation, the
/// direction, and the source and destination hero child views directly (FLUI cannot
/// hand out the two foreign `BuildContext`s a flight would otherwise involve).
pub(crate) type ShuttleBuilder = Rc<
    dyn Fn(&std::rc::Rc<dyn Animation<f64>>, FlightDirection, &BoxedView, &BoxedView) -> BoxedView,
>;

/// Builds the widget left in the hero's place while it is in flight. It is
/// state-preserving: it takes only the frozen [`Size`], never the child, so it
/// *cannot* drop the child — the real child stays offstage and its state survives.
pub(crate) type PlaceholderBuilder = Rc<dyn Fn(Size) -> BoxedView>;

/// What identifies a hero across two routes, compared by value.
///
/// Backed by [`ViewKey`], the framework's existing reconciliation-key trait: it
/// already provides value equality (`key_eq`) and hashing (`key_hash`) across erased
/// key types, which is precisely what a tag is. **No `dyn Any`, no downcast** — this
/// type never calls `ViewKey::as_any`, so no view type is ever downcast.
#[derive(Clone)]
pub struct HeroTag(Arc<dyn ViewKey>);

impl HeroTag {
    /// Tag a hero with any [`ViewKey`] — `ValueKey<&str>`, `ValueKey<u64>`, a domain
    /// newtype. Accepts anything the framework already knows how to compare.
    pub fn new(key: impl ViewKey) -> Self {
        Self(Arc::new(key))
    }
}

impl PartialEq for HeroTag {
    fn eq(&self, other: &Self) -> bool {
        self.0.key_eq(other.0.as_ref())
    }
}

impl Eq for HeroTag {}

impl Hash for HeroTag {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // `key_hash` is the trait's own hash, so two keys that `key_eq` agree on hash
        // alike — the `Hash`/`Eq` contract, delegated rather than re-derived.
        state.write_u64(self.0.key_hash());
    }
}

impl fmt::Debug for HeroTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.debug_fmt(f)
    }
}

// ============================================================================
// The registry
// ============================================================================

/// Every [`Hero`] mounted inside one route, by tag, built by registration rather
/// than by an element walk.
///
/// Cloneable and `'static`: the route owns one, the [`HeroScope`] hands clones to its
/// descendants, and the controller reads it through [`ModalHandle`]. The lock is
/// private and never escapes — every accessor copies or clones out.
///
/// [`ModalHandle`]: super::modal_route::ModalHandle
#[derive(Default)]
pub struct HeroRegistry {
    heroes: Terminal<Arc<super::lifecycle::TerminalMap<HeroTag, HeroHandle>>>,
    /// Nested `Navigator`s that publish a cross-flight visibility hook here.
    /// There is no element walk to reach one, so each nested `Navigator`
    /// registers itself with the nearest enclosing route instead.
    /// Empty for the common case of no nested navigator inside this route.
    nested: Terminal<Arc<super::lifecycle::TerminalVec<NestedHeroSource>>>,
}

impl Clone for HeroRegistry {
    fn clone(&self) -> Self {
        Self {
            heroes: Terminal::new(Arc::clone(&self.heroes)),
            nested: Terminal::new(Arc::clone(&self.nested)),
        }
    }
}

impl Drop for HeroRegistry {
    fn drop(&mut self) {
        let heroes = self.heroes.withdraw();
        let nested = self.nested.withdraw();
        drop((heroes, nested));
    }
}

impl HeroRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish a nested `Navigator`'s cross-flight visibility hook. Called from
    /// that `Navigator`'s own `build` (re-synced every time, so a `GlobalKey`
    /// reparent under a different route is not missed); the hook is stored, not
    /// invoked, so registering does not itself resolve anything about the nested
    /// stack.
    pub(crate) fn register_nested(&self, source: NestedHeroSource) {
        self.nested.lock().push(source);
    }

    /// Withdraw a nested `Navigator`'s hook, matched by identity — the mirror of
    /// [`register_nested`](Self::register_nested), called from that
    /// `Navigator`'s `dispose` and whenever it re-publishes elsewhere.
    pub(crate) fn deregister_nested(&self, source: &NestedHeroSource) {
        let mut nested = std::mem::take(&mut *self.nested.lock());
        nested.retain(|existing| !existing.is(source));
        let _prev = std::mem::replace(&mut *self.nested.lock(), nested);
    }

    /// Every hero visible for a flight through this route: this route's own,
    /// plus — recursively — whatever each registered nested `Navigator`
    /// publishes for its own current top route.
    ///
    /// A hero found in a nested `Navigator` is invited iff its route is current
    /// and a `PageRoute` — exactly the condition a [`NestedHeroSource`] encodes
    /// before it resolves to anything at all (see
    /// [`NavigatorHandle`](super::navigator::NavigatorHandle)'s registration).
    ///
    /// This route's own heroes win a tag shared with a nested route's — the
    /// same first-registered-wins call [`register`](Self::register) makes
    /// locally, extended across the registry boundary since there is no
    /// cross-registry visit order to arbitrate by instead.
    ///
    /// The nested sources are snapshotted out of the lock **before** any of them
    /// is resolved: `resolve` runs a caller-supplied closure that reads a
    /// `NavigatorHandle`'s own locks (history, route registries), and a nested
    /// `Navigator` could — through a future recursive shape or a caller-supplied
    /// hook — resolve back into this same registry. Holding `self.nested`'s lock
    /// across that call would risk exactly the lock-held-over-a-handle-querying-
    /// closure deadlock class the `pop_until`/`push_and_remove_until` fix closed.
    pub(crate) fn all_heroes(&self) -> HashMap<HeroTag, HeroHandle> {
        let mut all = self.heroes.lock().clone();
        let nested_sources: Vec<NestedHeroSource> = self.nested.lock().clone();
        for nested in &nested_sources {
            let Some(registry) = nested.resolve() else {
                continue;
            };
            for (tag, handle) in registry.all_heroes() {
                all.entry(tag).or_insert(handle);
            }
        }
        all
    }

    /// Register `handle` under `tag`, keeping the **first** registration.
    ///
    /// Returns whether it was accepted. See the module docs for why a duplicate
    /// tag logs and first-wins rather than panicking.
    fn register(&self, tag: HeroTag, handle: HeroHandle) -> bool {
        let mut heroes = self.heroes.lock();
        if heroes.contains_key(&tag) {
            tracing::warn!(
                ?tag,
                "two Hero views share one tag within a single route subtree; the \
                 second is ignored. Within each PageRoute subtree, each Hero must \
                 have a unique tag."
            );
            return false;
        }
        heroes.insert(tag, handle);
        true
    }

    /// Remove `tag`, but only if it still names `handle`.
    ///
    /// The identity check is what makes a *rejected* duplicate harmless: when it
    /// unmounts it must not evict the hero that won the tag. `Arc::ptr_eq`, not tag
    /// equality, is the question being asked.
    fn deregister(&self, tag: &HeroTag, handle: &HeroHandle) {
        let mut heroes = self.heroes.lock();
        if heroes.get(tag).is_some_and(|held| held.is(handle)) {
            heroes.remove(tag);
        }
    }

    /// The handle registered under `tag`, cloned out. Test-facing: production
    /// matching goes through `all_heroes`, which also reaches a
    /// nested `Navigator`'s heroes.
    #[must_use]
    pub fn get(&self, tag: &HeroTag) -> Option<HeroHandle> {
        self.heroes.lock().get(tag).cloned()
    }

    /// Every registered tag, cloned out. The caller matches these against another
    /// route's registry; nothing here depends on the order.
    pub(crate) fn tags(&self) -> Vec<HeroTag> {
        self.heroes.lock().keys().cloned().collect()
    }

    /// How many heroes are registered. Test-facing.
    #[must_use]
    #[expect(
        clippy::len_without_is_empty,
        reason = "a test-facing count; nothing asks whether the registry is empty"
    )]
    pub fn len(&self) -> usize {
        self.heroes.lock().len()
    }

    /// Whether both handles name the same registry — the identity
    /// `NavigatorState::sync_nested_hero_registration` checks each build, so an
    /// unchanged enclosing route costs one `Arc::ptr_eq` instead of a
    /// deregister/register round trip.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.heroes, &other.heroes)
    }
}

impl fmt::Debug for HeroRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroRegistry")
            .field("tags", &self.tags())
            .finish()
    }
}

/// A nested `Navigator`'s cross-flight visibility hook, registered with the
/// nearest enclosing route's [`HeroScope`] from that `Navigator`'s own `build`,
/// resynced every time so a `GlobalKey` reparent is never missed.
///
/// Heroes are registered rather than found by walking the element tree, so the
/// nested `Navigator` publishes this closure rather than being discovered by a
/// walk. Resolving it answers "is the hero's route current and a `PageRoute`"
/// for that navigator's *current* top route in one step: `None` when the top
/// route is not a `PageRoute`, is unmounted, or the navigator has none.
#[derive(Clone)]
pub(crate) struct NestedHeroSource {
    query: Rc<dyn Fn() -> Option<HeroRegistry>>,
}

impl NestedHeroSource {
    pub(crate) fn new(query: impl Fn() -> Option<HeroRegistry> + 'static) -> Self {
        Self {
            query: Rc::new(query),
        }
    }

    fn resolve(&self) -> Option<HeroRegistry> {
        (self.query)()
    }

    /// Whether both handles name the same registration — the identity
    /// [`HeroRegistry::deregister_nested`] matches on, mirroring
    /// [`HeroHandle::is`].
    fn is(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.query, &other.query)
    }
}

impl fmt::Debug for NestedHeroSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NestedHeroSource").finish_non_exhaustive()
    }
}

/// Provides a route's [`HeroRegistry`] to the heroes inside it.
///
/// The ambient-lookup pattern `VsyncScope` already uses: an
/// `InheritedView` a descendant reads **once**, in `init_state`. It never notifies —
/// the registry handle is fixed for the scope's lifetime — so a `Hero` never rebuilds
/// because of it.
///
/// This replaces both an element walk and a "which navigator owns this hero"
/// check: a hero registers with the route it is lexically inside, and can reach
/// no other.
pub struct HeroScope {
    registry: Terminal<HeroRegistry>,
    child: Terminal<BoxedView>,
}

impl Clone for HeroScope {
    fn clone(&self) -> Self {
        Self {
            registry: Terminal::new(self.registry.clone()),
            child: Terminal::new(self.child.clone()),
        }
    }
}

impl Drop for HeroScope {
    fn drop(&mut self) {
        let registry = self.registry.withdraw();
        let child = self.child.withdraw();
        drop((registry, child));
    }
}

impl HeroScope {
    /// Publish `registry` to the heroes in `child`'s subtree.
    pub fn new(registry: HeroRegistry, child: impl IntoView) -> Self {
        Self {
            registry: Terminal::new(registry),
            child: Terminal::new(BoxedView(Box::new(child.into_view()))),
        }
    }

    /// The registry heroes below this scope register with — what a nested
    /// `Navigator` reads, from its own `build`, to publish a [`NestedHeroSource`]
    /// on the route hosting it.
    pub(crate) fn registry(&self) -> HeroRegistry {
        self.registry.clone()
    }
}

impl fmt::Debug for HeroScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroScope")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

impl InheritedView for HeroScope {
    type Data = HeroRegistry;

    fn data(&self) -> &Self::Data {
        &self.registry
    }

    fn child(&self) -> &dyn View {
        &*self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        false
    }
}

impl_inherited_view!(HeroScope);

// ============================================================================
// The handle
// ============================================================================

/// The mutable half of a mounted [`Hero`], shared with whoever holds a
/// [`HeroHandle`].
struct HeroInner {
    tag: Terminal<HeroTag>,
    /// The hero's own render node, published on `attach` and cleared on `detach` —
    /// the same mechanism `RenderSubtreeAnchor` uses. This is how a hero finds its
    /// own render object — `BuildContext::find_render_object` walks strict
    /// *ancestors* and cannot answer it.
    anchor: SubtreeAnchor,
    /// The frozen placeholder size. `Some` iff in flight.
    placeholder: Mutex<Option<Size>>,
    /// Whether the placeholder keeps the real child offstage.
    include_child: AtomicBool,
    /// The render tree, so `start_flight` can read its own committed size.
    owner: Mutex<Option<PipelineCell>>,
    /// `setState`. Acquired in `init_state`, fired from a post-frame callback —
    /// never from `build`/layout/paint.
    rebuild: Mutex<Option<RebuildHandle>>,
    /// The hero's current child, for the flight shuttle to inflate afresh.
    ///
    /// The default shuttle is the *destination* hero's child, built anew in the
    /// overlay. Nothing is reparented, so this is a `BoxedView` clone,
    /// kept current through `did_update_view`.
    shuttle_child: Terminal<Mutex<BoxedView>>,
    /// The `create_rect_tween` factory, or `None` for the linear default.
    /// Read by the controller when it builds a flight.
    rect_factory: Mutex<Option<RectTweenFactory>>,
    /// The `flight_shuttle_builder`, or `None` for the default
    /// shuttle. Read by the controller when it builds a flight.
    shuttle_builder: Mutex<Option<ShuttleBuilder>>,
    /// The flight's forward easing. The default is `Curves::FastOutSlowIn`.
    curve: Terminal<Mutex<ArcCurve>>,
    /// The reverse easing, or `None` for [`curve`](Self::curve) flipped.
    reverse_curve: Mutex<Option<ArcCurve>>,
    /// Whether the ambient [`HeroMode`] allows this hero to fly — the AND of the `enabled` flags
    /// of every enclosing scope, sampled each build. `true` with no scope above.
    /// A disabled hero is still registered; the measurement pass skips it.
    hero_mode_enabled: AtomicBool,
    /// Defaults to `false`; a pair flies during a gesture-driven transition only
    /// when **both** ends opt in.
    transition_on_user_gestures: AtomicBool,
}

impl Drop for HeroInner {
    fn drop(&mut self) {
        let tag = self.tag.withdraw();
        let owner = Terminal::new(self.owner.get_mut().take());
        let rebuild = Terminal::new(self.rebuild.get_mut().take());
        let child = self.shuttle_child.withdraw();
        let factory = Terminal::new(self.rect_factory.get_mut().take());
        let builder = Terminal::new(self.shuttle_builder.get_mut().take());
        let curve = self.curve.withdraw();
        let reverse = Terminal::new(self.reverse_curve.get_mut().take());
        drop((tag, owner, rebuild, child, factory, builder, curve, reverse));
    }
}

/// An owned, `'static` capability to drive one mounted [`Hero`].
///
/// The same pattern seen elsewhere: a `HeroController` can never hold `&mut HeroState`
/// — nothing can — so the state that a flight mutates lives behind this handle.
#[derive(Clone)]
pub struct HeroHandle {
    inner: Arc<HeroInner>,
}

impl HeroHandle {
    #[cfg(test)]
    pub(super) fn test_handle(view: &Hero) -> Self {
        Self::new(view)
    }

    /// A handle whose hero is laid out at 10×10 in its own render tree, so
    /// [`start_flight`](Self::start_flight) can freeze it. The cell keeps the
    /// tree alive.
    #[cfg(test)]
    pub(super) fn test_laid_out(view: &Hero) -> (Self, PipelineCell) {
        use flui_rendering::pipeline::PipelineOwner;
        use flui_rendering::prelude::BoxConstraints;
        let handle = Self::new(view);
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        owner.set_root_render_object(Box::new(flui_objects::RenderSubtreeAnchor::new(
            handle.inner.anchor.clone(),
        )));
        owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(10.0, 10.0))));
        let mut owner = owner.into_layout();
        owner.run_layout().expect("a lone anchor lays out");
        let cell = PipelineCell::new(owner.into_idle());
        *handle.inner.owner.lock() = Some(cell.clone());
        (handle, cell)
    }

    /// A handle over `view`'s current configuration; [`ViewState::did_update_view`]
    /// keeps the view-derived halves current afterwards.
    fn new(view: &Hero) -> Self {
        Self {
            inner: Arc::new(HeroInner {
                tag: Terminal::new(view.tag.clone()),
                anchor: SubtreeAnchor::new(),
                placeholder: Mutex::new(None),
                include_child: AtomicBool::new(true),
                owner: Mutex::new(None),
                rebuild: Mutex::new(None),
                shuttle_child: Terminal::new(Mutex::new(view.child.clone())),
                rect_factory: Mutex::new(view.rect_factory.clone()),
                shuttle_builder: Mutex::new(view.shuttle_builder.clone()),
                curve: Terminal::new(Mutex::new(view.curve.clone())),
                reverse_curve: Mutex::new(view.reverse_curve.clone()),
                hero_mode_enabled: AtomicBool::new(true),
                transition_on_user_gestures: AtomicBool::new(view.transition_on_user_gestures),
            }),
        }
    }

    /// Whether both handles name the same mounted hero.
    fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether both handles name the same mounted hero — the "same tag" vs "same
    /// hero" distinction the duplicate-tag contract and the flight-divert logic both
    /// turn on.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        self.is(other)
    }

    pub(crate) fn tag(&self) -> &HeroTag {
        &self.inner.tag
    }

    /// The hero's render node, or `None` before it attaches and after it detaches.
    ///
    /// Resolving to `Some` says nothing about layout — `attach` runs during build.
    /// Ask [`PipelineOwner::box_size`](flui_rendering::pipeline::PipelineOwner::box_size)
    /// for geometry.
    #[must_use]
    pub fn render_id(&self) -> Option<RenderId> {
        self.inner.anchor.get()
    }

    /// The frozen placeholder size — `Some` exactly while in flight.
    #[must_use]
    pub fn placeholder_size(&self) -> Option<Size> {
        *self.inner.placeholder.lock()
    }

    /// What the flight's shuttle should show: a fresh inflation of this hero's child.
    ///
    /// This is the destination hero's child as-is; there is no `MediaQuery`
    /// padding compensation between the two heroes.
    pub(crate) fn shuttle_child(&self) -> BoxedView {
        self.inner.shuttle_child.lock().clone()
    }

    /// This hero's `create_rect_tween` factory, if it set one.
    pub(crate) fn rect_factory(&self) -> Option<RectTweenFactory> {
        self.inner.rect_factory.lock().clone()
    }

    /// This hero's `flight_shuttle_builder`, if it set one.
    pub(crate) fn shuttle_builder(&self) -> Option<ShuttleBuilder> {
        self.inner.shuttle_builder.lock().clone()
    }

    /// This hero's forward flight curve.
    pub(crate) fn curve(&self) -> ArcCurve {
        self.inner.curve.lock().clone()
    }

    /// This hero's reverse flight curve, if it set one. `None` means "the
    /// forward curve, flipped".
    pub(crate) fn reverse_curve(&self) -> Option<ArcCurve> {
        self.inner.reverse_curve.lock().clone()
    }

    /// Whether the ambient [`HeroMode`] allows this hero to fly.
    pub(crate) fn hero_mode_enabled(&self) -> bool {
        self.inner.hero_mode_enabled.load(Ordering::Relaxed)
    }

    /// Whether this hero opts into a gesture-driven (e.g. edge swipe-back)
    /// flight. `false` by default.
    pub(crate) fn transition_on_user_gestures(&self) -> bool {
        self.inner
            .transition_on_user_gestures
            .load(Ordering::Relaxed)
    }

    /// Whether an in-flight hero keeps its child offstage inside the placeholder.
    #[must_use]
    pub fn includes_child(&self) -> bool {
        self.inner.include_child.load(Ordering::Relaxed)
    }

    /// The hero's bounding box in `ancestor`'s coordinate space, or `None` when it is
    /// unmounted, not laid out, or not a descendant of `ancestor`.
    ///
    /// The box is the hero's size transformed into `ancestor`'s space. A missing
    /// size is an `Option` rather than an assertion — a hero on an unbuilt route
    /// is a routine `None`, not a broken invariant.
    #[must_use]
    pub fn bounding_box_in(&self, ancestor: RenderId) -> Option<Rect> {
        let render_id = self.render_id()?;
        let owner = self.inner.owner.lock().clone()?;
        owner.with(|owner| {
            let size = owner.box_size(render_id)?;
            let transform = owner.transform_to(render_id, ancestor)?;
            Some(transform.transform_rect(&Rect::from_ltwh(0.0, 0.0, size.width, size.height)))
        })
    }

    /// Freeze the hero at its committed size and rebuild it as a placeholder.
    ///
    /// Returns the captured size, or `None` when the hero has no committed layout to
    /// freeze — such a hero simply does not fly.
    ///
    /// `include_child_in_placeholder` is `true` for the *from* hero of a push and
    /// `false` otherwise: the source subtree is preserved offstage so its
    /// state survives the flight, while the destination's is not yet needed.
    pub fn start_flight(&self, include_child_in_placeholder: bool) -> Option<Size> {
        let render_id = self.render_id()?;
        let size = {
            let owner = self.inner.owner.lock().clone()?;
            owner.with(|owner| owner.box_size(render_id))?
        };

        self.inner
            .include_child
            .store(include_child_in_placeholder, Ordering::Relaxed);
        *self.inner.placeholder.lock() = Some(size);
        self.request_rebuild();
        Some(size)
    }

    /// Drop the placeholder and show the child again. Safe to call on a hero that
    /// is not in flight.
    ///
    /// `keep_placeholder` leaves it frozen — used when a flight ends by being
    /// diverted into another.
    pub fn end_flight(&self, keep_placeholder: bool) {
        {
            let mut placeholder = self.inner.placeholder.lock();
            if keep_placeholder || placeholder.is_none() {
                return;
            }
            *placeholder = None;
        }
        self.request_rebuild();
    }

    /// Schedules a rebuild. Inert on an unmounted hero.
    fn request_rebuild(&self) {
        if let Some(rebuild) = self.inner.rebuild.lock().as_ref() {
            rebuild.schedule(flui_view::RebuildReason::AnimationTick);
        }
    }
}

impl fmt::Debug for HeroHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroHandle")
            .field("tag", &self.inner.tag)
            .field("render_id", &self.render_id())
            .field("placeholder", &self.placeholder_size())
            .finish()
    }
}

// ============================================================================
// The view
// ============================================================================

/// Marks a subtree as a hero: the thing that flies between two routes.
///
/// A subtree that animates between two routes when it appears in both under the same
/// tag.
///
/// A `HeroController` must observe the `Navigator` for flights to run; a bare
/// `Navigator` now installs a default one, and `HeroControllerScope` customizes or
/// disables that. The public surface includes the baseline `tag`/`child` plus the
/// customization hooks: [`create_rect_tween`](Self::create_rect_tween),
/// [`flight_shuttle_builder`](Self::flight_shuttle_builder), FLUI's
/// state-preserving [`placeholder`](Self::placeholder), and the flight easing
/// [`curve`](Self::curve) / [`reverse_curve`](Self::reverse_curve). A subtree is
/// grounded with [`HeroMode`]. Cross-navigator flights work — a hero inside a nested
/// `Navigator`'s current `PageRoute` matches an outer route's hero of the same tag.
/// [`transition_on_user_gestures`](Self::transition_on_user_gestures) opts a hero
/// into a gesture-driven (edge swipe-back) flight; `false` by default.
pub struct Hero {
    tag: Terminal<HeroTag>,
    child: Terminal<BoxedView>,
    rect_factory: Option<RectTweenFactory>,
    shuttle_builder: Option<ShuttleBuilder>,
    placeholder: Option<PlaceholderBuilder>,
    curve: Terminal<ArcCurve>,
    reverse_curve: Option<ArcCurve>,
    transition_on_user_gestures: bool,
}

impl Clone for Hero {
    fn clone(&self) -> Self {
        Self {
            tag: Terminal::new(self.tag.clone()),
            child: Terminal::new(self.child.clone()),
            rect_factory: self.rect_factory.clone(),
            shuttle_builder: self.shuttle_builder.clone(),
            placeholder: self.placeholder.clone(),
            curve: Terminal::new(self.curve.clone()),
            reverse_curve: self.reverse_curve.clone(),
            transition_on_user_gestures: self.transition_on_user_gestures,
        }
    }
}

impl Drop for Hero {
    fn drop(&mut self) {
        let tag = self.tag.withdraw();
        let child = self.child.withdraw();
        let factory = Terminal::new(self.rect_factory.take());
        let builder = Terminal::new(self.shuttle_builder.take());
        let placeholder = Terminal::new(self.placeholder.take());
        let curve = self.curve.withdraw();
        let reverse = Terminal::new(self.reverse_curve.take());
        drop((tag, child, factory, builder, placeholder, curve, reverse));
    }
}

impl Hero {
    /// A hero identified by `tag`. Any [`ViewKey`] works — `ValueKey::new("photo")`,
    /// a domain newtype — and two heroes fly together iff their tags compare equal.
    pub fn new(tag: impl ViewKey, child: impl IntoView) -> Self {
        Self {
            tag: Terminal::new(HeroTag::new(tag)),
            child: Terminal::new(BoxedView(Box::new(child.into_view()))),
            rect_factory: None,
            shuttle_builder: None,
            placeholder: None,
            curve: Terminal::new(ArcCurve::new(Curves::FastOutSlowIn)),
            reverse_curve: None,
            transition_on_user_gestures: false,
        }
    }

    /// The easing the flight animation runs with in the forward direction; the
    /// default is `Curves::FastOutSlowIn`. A push eases on the **destination**
    /// hero's curve, a pop on the **source** hero's.
    #[must_use]
    pub fn curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        *self.curve = ArcCurve::new(curve);
        self
    }

    /// The easing for the reverse direction: when unset, [`curve`](Self::curve)
    /// flipped.
    #[must_use]
    pub fn reverse_curve(mut self, curve: impl Curve + Send + Sync + 'static) -> Self {
        self.reverse_curve = Some(ArcCurve::new(curve));
        self
    }

    /// Whether this hero participates in a **gesture-driven** transition
    /// (e.g. an edge swipe-back), as opposed to only a programmatic push/pop.
    ///
    /// A pair flies during a gesture-driven transition only when **both**
    /// ends opt in; a hero left out this way is explicitly un-hidden if a
    /// prior flight had frozen it. Defaults to `false`.
    #[must_use]
    pub fn transition_on_user_gestures(mut self, enabled: bool) -> Self {
        self.transition_on_user_gestures = enabled;
        self
    }

    /// Shape the path the hero's shuttle flies along: `factory(begin, end)` returns
    /// the tween the flight interpolates as its animation runs 0→1. The default is
    /// a linear [`RectTween`](flui_animation::RectTween). When both this and the
    /// [`HeroController`](super::hero_controller::HeroController)'s default are set,
    /// this one wins.
    /// Reversing an airborne flight between the same heroes retraces the selected
    /// mapping, retaining its factory and evaluating it at mirrored progress.
    #[must_use]
    pub fn create_rect_tween<F, A>(mut self, factory: F) -> Self
    where
        F: Fn(Rect, Rect) -> A + 'static,
        A: Animatable<Value = Rect> + 'static,
    {
        self.rect_factory = Some(Rc::new(move |begin, end| {
            Box::new(factory(begin, end)) as Box<dyn Animatable<Value = Rect>>
        }));
        self
    }

    /// Replace the default in-flight widget. The builder receives the flight
    /// `animation`, the `direction`, and the source and destination hero child
    /// views — not two foreign `BuildContext`s, which FLUI has no way to hand out.
    /// When both heroes of a pair supply one, the destination's wins.
    #[must_use]
    pub fn flight_shuttle_builder<F, V>(mut self, builder: F) -> Self
    where
        F: Fn(&std::rc::Rc<dyn Animation<f64>>, FlightDirection, &BoxedView, &BoxedView) -> V
            + 'static,
        V: IntoView,
    {
        self.shuttle_builder = Some(Rc::new(move |animation, direction, from, to| {
            BoxedView(Box::new(
                builder(animation, direction, from, to).into_view(),
            ))
        }));
        self
    }

    /// Show a custom widget in the hero's place while it is in flight, **without**
    /// losing the child's state.
    ///
    /// The closure takes only the frozen [`Size`] the space must
    /// hold; it never receives the child, so it cannot drop it. FLUI keeps the real
    /// child offstage at a constant tree position, so its state survives the flight with
    /// no `GlobalKey`. The default (no placeholder) leaves an empty box of that size.
    #[must_use]
    pub fn placeholder<F, V>(mut self, builder: F) -> Self
    where
        F: Fn(Size) -> V + 'static,
        V: IntoView,
    {
        self.placeholder = Some(Rc::new(move |size| {
            BoxedView(Box::new(builder(size).into_view()))
        }));
        self
    }
}

impl fmt::Debug for Hero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hero")
            .field("tag", &self.tag)
            .finish_non_exhaustive()
    }
}

impl View for Hero {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Hero {
    type State = HeroState;

    fn create_state(&self) -> Self::State {
        HeroState {
            handle: Terminal::new(HeroHandle::new(self)),
            registry: None,
        }
    }
}

/// The state behind [`Hero`]. `pub` only because `StatefulView::State` requires
/// it (as `NavigatorState` is); **not** re-exported, so it is reachable only as
/// `<Hero as StatefulView>::State` and carries no public API of its own.
pub struct HeroState {
    handle: Terminal<HeroHandle>,
    /// The route's registry, resolved once from the ambient [`HeroScope`]. `None` for
    /// a `Hero` mounted outside any route, which is inert rather than an error.
    registry: Option<HeroRegistry>,
}

impl Drop for HeroState {
    fn drop(&mut self) {
        let handle = self.handle.withdraw();
        let registry = Terminal::new(self.registry.take());
        drop((handle, registry));
    }
}

impl std::fmt::Debug for HeroState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeroState")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl ViewState<Hero> for HeroState {
    /// Keep the handle's view-derived config current: the shuttle source, the rect-tween
    /// factory, the shuttle builder, and the flight curves are all read at flight
    /// start, i.e. from the *latest* `Hero` configuration.
    fn did_update_view(&mut self, _old: &Hero, new_view: &Hero) {
        self.handle
            .inner
            .shuttle_child
            .lock()
            .clone_from(&new_view.child);
        self.handle
            .inner
            .rect_factory
            .lock()
            .clone_from(&new_view.rect_factory);
        self.handle
            .inner
            .shuttle_builder
            .lock()
            .clone_from(&new_view.shuttle_builder);
        self.handle.inner.curve.lock().clone_from(&new_view.curve);
        self.handle
            .inner
            .reverse_curve
            .lock()
            .clone_from(&new_view.reverse_curve);
        self.handle
            .inner
            .transition_on_user_gestures
            .store(new_view.transition_on_user_gestures, Ordering::Relaxed);
    }

    /// Everything a hero needs from outside itself is acquired **here**, in the one
    /// lifecycle hook that has a `LifecycleContext` and is not a frame phase: the route's
    /// registry, the render tree, and the rebuild capability.
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let _prev = std::mem::replace(&mut *self.handle.inner.owner.lock(), ctx.pipeline_owner());
        let _prev = self
            .handle
            .inner
            .rebuild
            .lock()
            .replace(ctx.rebuild_handle());

        let registry = ctx.get::<HeroScope, _>(|scope| scope.registry.clone());
        if let Some(registry) = registry {
            // A rejected duplicate keeps its handle but is not stored, and `dispose`'s
            // identity check means it will not evict the winner.
            registry.register(self.handle.tag().clone(), self.handle.clone());
            self.registry = Some(registry);
        }
    }

    /// The mirror. A registry entry that outlived its hero would hand a controller a
    /// handle whose render node is gone and whose rebuild is inert.
    fn dispose(&mut self) {
        if let Some(registry) = &self.registry {
            registry.deregister(self.handle.tag(), &self.handle);
        }
    }

    /// Builds the anchored box, with the state-preserving custom placeholder. An
    /// offstage hero's animations stop (via `TickerMode`) while its copy flies.
    ///
    /// | Case | Structure |
    /// |---|---|
    /// | custom `placeholder` | child kept **offstage** at a constant path, placeholder shown as a sibling — state preserved |
    /// | in flight | `SizedBox` of the frozen size only while in flight |
    /// | default path | `Offstage`, no key: nothing reparents |
    /// | `showPlaceholder && !include_child` | bare `SizedBox`, in the default (no-placeholder) path |
    ///
    /// The [`AnchoredBox`] is always present, in flight or not: it is what publishes
    /// the `RenderId` a controller measures, and a node that came and went would make
    /// `render_id()` flicker.
    fn build(&self, view: &Hero, ctx: &dyn BuildContext) -> impl IntoView {
        // The ambient `HeroMode`, re-sampled every build with a real dependency: a
        // scope that flips `enabled` rebuilds this hero, so the measurement pass
        // always reads the current value. `true` with no scope above.
        let hero_mode_enabled = ctx
            .depend_on::<HeroModeScope, _>(|scope| scope.enabled)
            .unwrap_or(true);
        self.handle
            .inner
            .hero_mode_enabled
            .store(hero_mode_enabled, Ordering::Relaxed);

        let anchor = self.handle.inner.anchor.clone();
        let placeholder = self.handle.placeholder_size();
        let show_placeholder = placeholder.is_some();

        // Custom placeholder: a hero configured with one uses **one constant
        // structure** in and out of flight — `SizedBox → Stack[ Offstage→child, … ]` —
        // so the child's element (slot 0) is never reparented and its state survives with
        // no `GlobalKey`. The placeholder visual is appended at slot 1 only while in
        // flight; the closure never sees the child, so it cannot drop it. This preserves
        // state uniformly (both flight directions). Default heroes (below) keep the
        // exact fixed chain, no `Stack`.
        if let Some(build_placeholder) = &view.placeholder {
            let mut layers: Vec<BoxedView> = vec![
                Offstage::new()
                    .offstage(show_placeholder)
                    .child(TickerMode::new(view.child.clone()).enabled(!show_placeholder))
                    .into_view()
                    .boxed(),
            ];
            if let Some(size) = placeholder {
                layers.push(build_placeholder(size));
            }
            let sized = match placeholder {
                Some(size) => SizedBox::new(size.width, size.height),
                None => SizedBox::default(),
            };
            return AnchoredBox::new(anchor, sized.child(Stack::new(layers)));
        }

        // The destination hero drops its child — the shuttle carries it — so this
        // branch legitimately changes shape, and the child's state is not preserved.
        if show_placeholder && !self.handle.includes_child() {
            let size = placeholder.expect("show_placeholder implies a size");
            return AnchoredBox::new(anchor, SizedBox::new(size.width, size.height));
        }

        // The **fixed chain**:
        //
        //   SizedBox(size?) → Offstage(showPlaceholder) → TickerMode(!showPlaceholder) → child
        //
        // The structure is constant across "not in flight" (`SizedBox::default()`,
        // unconstrained, `Offstage(false)`) and "in flight, keep child"
        // (`SizedBox(size)`, `Offstage(true)`). Because the child sits at the same
        // depth under the same two view types either way, reconciliation preserves its
        // element — and therefore its state — with **no `GlobalKey`**.
        let sized = match placeholder {
            Some(size) => SizedBox::new(size.width, size.height),
            None => SizedBox::default(),
        };
        AnchoredBox::new(
            anchor,
            sized.child(Offstage::new().offstage(show_placeholder).child(
                // The hero left behind offstage keeps its subtree — and its
                // state — but its animations must not keep running while the
                // shuttle carries a copy of it across the screen.
                TickerMode::new(view.child.clone()).enabled(!show_placeholder),
            )),
        )
    }
}

// ============================================================================
// HeroMode
// ============================================================================

/// Enables or disables [`Hero`] flights for a subtree.
///
/// While [`enabled`](Self::enabled) is `false`, no hero in the subtree participates
/// in a flight — it stays put during route transitions, whichever route carries its
/// matching tag. A nested *enabled* scope cannot re-enable a disabled subtree: the
/// effective value is the AND of every enclosing scope.
#[derive(Clone)]
pub struct HeroMode {
    enabled: bool,
    child: BoxedView,
}

impl HeroMode {
    /// A scope that allows hero flights — `enabled` defaults to `true`,
    /// so a bare `HeroMode` changes nothing.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            enabled: true,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Whether [`Hero`]es in this subtree may fly.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl fmt::Debug for HeroMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroMode")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl View for HeroMode {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for HeroMode {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // AND with the enclosing scope, so `HeroMode(false) > HeroMode(true) > Hero`
        // stays disabled.
        // `depend_on`, not `get`: an outer scope flipping re-derives this one.
        let ancestor_enabled = ctx
            .depend_on::<HeroModeScope, _>(|scope| scope.enabled)
            .unwrap_or(true);
        HeroModeScope {
            enabled: ancestor_enabled && self.enabled,
            child: self.child.clone(),
        }
    }
}

/// The inherited carrier of [`HeroMode`]'s effective flag: what a [`Hero`] actually
/// reads. Private — the public surface is `HeroMode`, which composes the AND.
#[derive(Clone)]
struct HeroModeScope {
    /// The AND of this scope's `enabled` and every scope's above it.
    enabled: bool,
    child: BoxedView,
}

impl fmt::Debug for HeroModeScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroModeScope")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl InheritedView for HeroModeScope {
    type Data = bool;

    fn data(&self) -> &Self::Data {
        &self.enabled
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.enabled != old.enabled
    }
}

impl_inherited_view!(HeroModeScope);
