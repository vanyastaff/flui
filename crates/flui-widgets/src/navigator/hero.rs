//! The `Hero` view, its per-route registry, and the handle a `HeroController` drives.
//!
//! ADR-0021 U3.5 through §7n. `Hero` is public; its registry, handle and tag
//! storage stay private. A `Hero` registers with its route, can be *told* to show a
//! placeholder, and exposes the signed-off customization hooks:
//! `create_rect_tween`, `flight_shuttle_builder`, and FLUI's state-preserving
//! `placeholder`.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/heroes.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `Hero` (`:180`), `_HeroState` (`:362-439`),
//! `Hero._allHeroesFor` (`:279-345`).
//!
//! # Registration replaces the element walk
//!
//! `_allHeroesFor` walks a route's element subtree, tests `widget is Hero`, and reads
//! `hero.state as _HeroState` (`:317-321`). FLUI cannot: a downcast from `&dyn View`
//! is exactly what FR-033 forbids, and an element walk from an observer callback is
//! what ADR-0021 §7f spent a commit removing.
//!
//! So the direction is inverted. Each `Hero` **registers itself** with the nearest
//! enclosing [`HeroScope`] in `init_state` and deregisters in `dispose`. The registry
//! is owned by the route (`ModalInner`), reachable by `RouteId` through the
//! navigator's modal registry, and the controller reads it as pure data. No
//! `GlobalKey`, no tree re-entry, no downcast.
//!
//! Two consequences, both recorded:
//!
//! * **Registration order, not tree order.** `_allHeroesFor`'s map is filled in
//!   depth-first visit order. Ours is filled in mount order, which for a static
//!   subtree is the same, and for a dynamic one is not. Nothing in the flight
//!   algorithm depends on the order — it looks tags up, never iterates positionally.
//! * **A `Hero` under a nested `Navigator` registers with its own route**, because it
//!   finds *its* nearest `HeroScope`. Flutter reaches the same answer through
//!   `Navigator.of(hero) == navigator` (`:322`) plus a `ModalRoute.of` fallback
//!   (`:330-333`). A nested navigator is isolated by `HeroControllerScope::none`;
//!   full cross-navigator flights remain out of scope.
//!
//! # Duplicate tags
//!
//! Flutter throws inside an `assert` (`:287-305`): a debug-only error, and in release
//! `result[tag] = heroState` silently keeps the **last** hero registered. ADR-0021 D8
//! asked whether that deserves an `expect("BUG: …")`. It does not: a duplicate tag is
//! a *caller* mistake, and [`PANIC-POLICY`](../../../../docs/PANIC-POLICY.md) reserves
//! panics for framework invariants. FLUI logs and keeps the **first**, which is the
//! divergence a stable registry needs — "last wins" would make the surviving hero
//! depend on mount order.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_animation::{Animatable, Animation};
use flui_foundation::{RenderId, ViewKey};
use flui_geometry::Rect;
use flui_objects::SubtreeAnchor;
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Size;
use flui_types::geometry::px;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{RebuildHandle, impl_inherited_view};
use parking_lot::{Mutex, RwLock};

use super::hero_controller::FlightDirection;
use super::subtree::AnchoredBox;
use crate::{Offstage, SizedBox, Stack};

/// Builds the [`RectTween`](flui_animation::RectTween)-like path a hero's shuttle
/// follows. Flutter's `CreateRectTween` (`heroes.dart:27`); the default is a linear
/// `RectTween`. Erased and `Arc`-shared so it is `Clone + Send + Sync + 'static`.
pub(crate) type RectTweenFactory =
    Arc<dyn Fn(Rect, Rect) -> Box<dyn Animatable<Rect> + Send + Sync> + Send + Sync>;

/// Builds the widget shown in flight instead of the default (a fresh copy of the
/// destination hero's child). Flutter's `HeroFlightShuttleBuilder` (`heroes.dart:45`),
/// minus the two foreign `BuildContext`s FLUI cannot hand out (ADR-0021 §7n D-N.2): the
/// builder receives the flight animation, the direction, and the source and destination
/// hero child views directly.
pub(crate) type ShuttleBuilder = Arc<
    dyn Fn(&Arc<dyn Animation<f32>>, FlightDirection, &BoxedView, &BoxedView) -> BoxedView
        + Send
        + Sync,
>;

/// Builds the widget left in the hero's place while it is in flight. FLUI's
/// state-preserving alternative to Flutter's lossy `placeholderBuilder` (ADR-0021 §7n
/// D-N.3): it takes only the frozen [`Size`], never the child, so it *cannot* drop the
/// child — the real child stays offstage and its state survives.
pub(crate) type PlaceholderBuilder = Arc<dyn Fn(Size) -> BoxedView + Send + Sync>;

/// What identifies a hero across two routes. Flutter's `Hero.tag`, an `Object`
/// compared with `==` (`heroes.dart:286-309`).
///
/// Backed by [`ViewKey`], the framework's existing reconciliation-key trait: it
/// already provides value equality (`key_eq`) and hashing (`key_hash`) across erased
/// key types, which is precisely what a tag is. **No `dyn Any`, no downcast** — this
/// type never calls `ViewKey::as_any`, so FR-033 is untouched. The `Arc<dyn ViewKey>`
/// boundary is registered with port-check trigger #9 (FR-036).
#[derive(Clone)]
pub(crate) struct HeroTag(Arc<dyn ViewKey>);

impl HeroTag {
    /// Tag a hero with any [`ViewKey`] — `ValueKey<&str>`, `ValueKey<u64>`, a domain
    /// newtype. Flutter accepts any `Object`; this accepts anything the framework
    /// already knows how to compare.
    pub(crate) fn new(key: impl ViewKey) -> Self {
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

/// Every [`Hero`] mounted inside one route, by tag. Flutter's `_allHeroesFor` result
/// map (`heroes.dart:284`), built by registration rather than by an element walk.
///
/// Cloneable and `'static`: the route owns one, the [`HeroScope`] hands clones to its
/// descendants, and the controller reads it through [`ModalHandle`]. The lock is
/// private and never escapes — every accessor copies or clones out.
///
/// [`ModalHandle`]: super::modal_route::ModalHandle
#[derive(Clone, Default)]
pub(crate) struct HeroRegistry {
    heroes: Arc<Mutex<HashMap<HeroTag, HeroHandle>>>,
}

impl HeroRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register `handle` under `tag`, keeping the **first** registration.
    ///
    /// Returns whether it was accepted. Flutter's duplicate-tag `assert` throws in
    /// debug and silently last-wins in release (`heroes.dart:287-309`); see the module
    /// docs for why this logs and first-wins instead.
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

    /// The handle registered under `tag`, cloned out.
    pub(crate) fn get(&self, tag: &HeroTag) -> Option<HeroHandle> {
        self.heroes.lock().get(tag).cloned()
    }

    /// Every registered tag, cloned out. The caller matches these against another
    /// route's registry; nothing here depends on the order.
    pub(crate) fn tags(&self) -> Vec<HeroTag> {
        self.heroes.lock().keys().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.heroes.lock().len()
    }
}

impl fmt::Debug for HeroRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeroRegistry")
            .field("tags", &self.tags())
            .finish()
    }
}

/// Provides a route's [`HeroRegistry`] to the heroes inside it.
///
/// The ambient-lookup pattern `VsyncScope` already uses (ADR-0020 U5.2): an
/// `InheritedView` a descendant reads **once**, in `init_state`. It never notifies —
/// the registry handle is fixed for the scope's lifetime — so a `Hero` never rebuilds
/// because of it.
///
/// This is what replaces `_allHeroesFor`'s element walk *and* Flutter's
/// `Navigator.of(hero) == navigator` check: a hero registers with the route it is
/// lexically inside, and can reach no other.
#[derive(Clone)]
pub(crate) struct HeroScope {
    registry: HeroRegistry,
    child: BoxedView,
}

impl HeroScope {
    pub(crate) fn new(registry: HeroRegistry, child: impl IntoView) -> Self {
        Self {
            registry,
            child: BoxedView(Box::new(child.into_view())),
        }
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
        &self.child
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
    tag: HeroTag,
    /// The hero's own render node, published on `attach` and cleared on `detach`
    /// (ADR-0021 U2's `RenderSubtreeAnchor`). This is FLUI's
    /// `context.findRenderObject()` for a hero — `BuildContext::find_render_object`
    /// walks strict *ancestors* and cannot answer it.
    anchor: SubtreeAnchor,
    /// `_HeroState._placeholderSize` (`heroes.dart:364`). `Some` iff in flight.
    placeholder: Mutex<Option<Size>>,
    /// `_HeroState._shouldIncludeChild` (`:368`).
    include_child: AtomicBool,
    /// The render tree, so `start_flight` can read its own committed size the way
    /// `_HeroState.startFlight` reads `box.size` (`:384-387`).
    owner: Mutex<Option<Arc<RwLock<PipelineOwner>>>>,
    /// `setState`. Acquired in `init_state`, fired from a post-frame callback —
    /// never from `build`/layout/paint (port-check trigger #22).
    rebuild: Mutex<Option<RebuildHandle>>,
    /// The hero's current child, for the flight shuttle to inflate afresh.
    ///
    /// `_defaultHeroFlightShuttleBuilder` returns `toHero.widget.child`
    /// (`heroes.dart:1083-1090`) — the *destination* hero's child, built anew in the
    /// overlay. Nothing is reparented (ADR-0021 D1), so this is a `BoxedView` clone,
    /// kept current through `did_update_view`.
    shuttle_child: Mutex<BoxedView>,
    /// `Hero.createRectTween` (`heroes.dart:202`), or `None` for the linear default.
    /// Read by the controller when it builds a flight (ADR-0021 §7n D-N.1).
    rect_factory: Mutex<Option<RectTweenFactory>>,
    /// `Hero.flightShuttleBuilder` (`heroes.dart:240`), or `None` for the default
    /// shuttle. Read by the controller when it builds a flight (§7n D-N.2).
    shuttle_builder: Mutex<Option<ShuttleBuilder>>,
}

/// An owned, `'static` capability to drive one mounted [`Hero`].
///
/// The ADR-0019 §3.2 pattern again: a `HeroController` can never hold `&mut HeroState`
/// — nothing can — so the state that a flight mutates lives behind this handle.
#[derive(Clone)]
pub(crate) struct HeroHandle {
    inner: Arc<HeroInner>,
}

impl HeroHandle {
    fn new(
        tag: HeroTag,
        shuttle_child: BoxedView,
        rect_factory: Option<RectTweenFactory>,
        shuttle_builder: Option<ShuttleBuilder>,
    ) -> Self {
        Self {
            inner: Arc::new(HeroInner {
                tag,
                anchor: SubtreeAnchor::new(),
                placeholder: Mutex::new(None),
                include_child: AtomicBool::new(true),
                owner: Mutex::new(None),
                rebuild: Mutex::new(None),
                shuttle_child: Mutex::new(shuttle_child),
                rect_factory: Mutex::new(rect_factory),
                shuttle_builder: Mutex::new(shuttle_builder),
            }),
        }
    }

    /// Whether both handles name the same mounted hero.
    fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Whether both handles name the same mounted hero — the "same tag" vs "same
    /// hero" distinction the duplicate-tag contract and the flight-divert logic both
    /// turn on (`heroes.dart:744-745`, `:766`).
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        self.is(other)
    }

    pub(crate) fn tag(&self) -> &HeroTag {
        &self.inner.tag
    }

    /// The hero's render node, or `None` before it attaches and after it detaches.
    ///
    /// Resolving to `Some` says nothing about layout — `attach` runs during build.
    /// Ask [`PipelineOwner::box_size`] for geometry (ADR-0021 U1/U2).
    pub(crate) fn render_id(&self) -> Option<RenderId> {
        self.inner.anchor.get()
    }

    /// `_HeroState._placeholderSize` — `Some` exactly while in flight.
    pub(crate) fn placeholder_size(&self) -> Option<Size> {
        *self.inner.placeholder.lock()
    }

    /// What the flight's shuttle should show: a fresh inflation of this hero's child.
    ///
    /// `_defaultHeroFlightShuttleBuilder` (`heroes.dart:1076-1090`) returns
    /// `toHero.widget.child`. Flutter's version also compensates for a `MediaQuery`
    /// padding difference between the two heroes; FLUI has no `MediaQuery`, so the
    /// `toMediaQueryData == null` early return (`:1089`) is the whole function.
    pub(crate) fn shuttle_child(&self) -> BoxedView {
        self.inner.shuttle_child.lock().clone()
    }

    /// This hero's `create_rect_tween` factory, if it set one (`heroes.dart:202`).
    pub(crate) fn rect_factory(&self) -> Option<RectTweenFactory> {
        self.inner.rect_factory.lock().clone()
    }

    /// This hero's `flight_shuttle_builder`, if it set one (`heroes.dart:240`).
    pub(crate) fn shuttle_builder(&self) -> Option<ShuttleBuilder> {
        self.inner.shuttle_builder.lock().clone()
    }

    /// Whether an in-flight hero keeps its child offstage inside the placeholder.
    pub(crate) fn includes_child(&self) -> bool {
        self.inner.include_child.load(Ordering::Relaxed)
    }

    /// The hero's bounding box in `ancestor`'s coordinate space, or `None` when it is
    /// unmounted, not laid out, or not a descendant of `ancestor`.
    ///
    /// Flutter's `_HeroFlightManifest._boundingBoxFor` (`heroes.dart:501-509`):
    /// `MatrixUtils.transformRect(box.getTransformTo(ancestor), Offset.zero & box.size)`.
    /// Its `assert(box.hasSize && box.size.isFinite)` becomes an `Option` here — a
    /// hero on an unbuilt route is a routine `None`, not a broken invariant.
    pub(crate) fn bounding_box_in(&self, ancestor: RenderId) -> Option<Rect> {
        let render_id = self.render_id()?;
        let owner = self.inner.owner.lock().clone()?;
        let owner = owner.read();
        let size = owner.box_size(render_id)?;
        let transform = owner.transform_to(render_id, ancestor)?;
        Some(transform.transform_rect(&Rect::from_ltwh(px(0.0), px(0.0), size.width, size.height)))
    }

    /// `_HeroState.startFlight` (`heroes.dart:381-389`): freeze the hero at its
    /// committed size and rebuild it as a placeholder.
    ///
    /// Returns the captured size, or `None` when the hero has no committed layout to
    /// freeze — Flutter asserts `box.hasSize` here and would crash; a `None` route
    /// simply does not fly.
    ///
    /// `include_child_in_placeholder` is `true` for the *from* hero of a push and
    /// `false` otherwise (`:379-380`): the source subtree is preserved offstage so its
    /// state survives the flight, while the destination's is not yet needed.
    pub(crate) fn start_flight(&self, include_child_in_placeholder: bool) -> Option<Size> {
        let render_id = self.render_id()?;
        let size = {
            let owner = self.inner.owner.lock().clone()?;
            let owner = owner.read();
            owner.box_size(render_id)?
        };

        self.inner
            .include_child
            .store(include_child_in_placeholder, Ordering::Relaxed);
        *self.inner.placeholder.lock() = Some(size);
        self.request_rebuild();
        Some(size)
    }

    /// `_HeroState.endFlight` (`heroes.dart:397-408`): drop the placeholder and show
    /// the child again. Safe to call on a hero that is not in flight.
    ///
    /// `keep_placeholder` leaves it frozen — Flutter uses it when a flight ends by
    /// being diverted into another.
    pub(crate) fn end_flight(&self, keep_placeholder: bool) {
        {
            let mut placeholder = self.inner.placeholder.lock();
            if keep_placeholder || placeholder.is_none() {
                return;
            }
            *placeholder = None;
        }
        self.request_rebuild();
    }

    /// `setState`. Inert on an unmounted hero, as `_HeroState.endFlight`'s
    /// `if (mounted)` guard is (`heroes.dart:403`).
    fn request_rebuild(&self) {
        if let Some(rebuild) = self.inner.rebuild.lock().as_ref() {
            rebuild.schedule();
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
/// tag — Flutter's `Hero` (`heroes.dart:180`).
///
/// A `HeroController` must observe the `Navigator` for flights to run; a bare
/// `Navigator` now installs a default one, and `HeroControllerScope` customizes or
/// disables that. The public surface includes the baseline `tag`/`child` plus the
/// ADR-0021 §7n hooks: [`create_rect_tween`](Self::create_rect_tween),
/// [`flight_shuttle_builder`](Self::flight_shuttle_builder), and FLUI's
/// state-preserving [`placeholder`](Self::placeholder). `transitionOnUserGestures`,
/// `HeroMode`, and full cross-navigator flight parity remain deferred.
#[derive(Clone)]
pub struct Hero {
    tag: HeroTag,
    child: BoxedView,
    rect_factory: Option<RectTweenFactory>,
    shuttle_builder: Option<ShuttleBuilder>,
    placeholder: Option<PlaceholderBuilder>,
}

impl Hero {
    /// A hero identified by `tag`. Any [`ViewKey`] works — `ValueKey::new("photo")`,
    /// a domain newtype — and two heroes fly together iff their tags compare equal.
    /// Flutter takes any `Object`; this takes anything the framework can already
    /// compare and hash.
    pub fn new(tag: impl ViewKey, child: impl IntoView) -> Self {
        Self {
            tag: HeroTag::new(tag),
            child: BoxedView(Box::new(child.into_view())),
            rect_factory: None,
            shuttle_builder: None,
            placeholder: None,
        }
    }

    /// Shape the path the hero's shuttle flies along. Flutter's `Hero.createRectTween`
    /// (`heroes.dart:202`): `factory(begin, end)` returns the tween the flight
    /// interpolates as its animation runs 0→1. The default is a linear
    /// [`RectTween`](flui_animation::RectTween). When both this and the
    /// [`HeroController`](super::hero_controller::HeroController)'s default are set, this
    /// one wins (`heroes.dart:495`).
    #[must_use]
    pub fn create_rect_tween<F, A>(mut self, factory: F) -> Self
    where
        F: Fn(Rect, Rect) -> A + Send + Sync + 'static,
        A: Animatable<Rect> + Send + Sync + 'static,
    {
        self.rect_factory = Some(Arc::new(move |begin, end| {
            Box::new(factory(begin, end)) as Box<dyn Animatable<Rect> + Send + Sync>
        }));
        self
    }

    /// Replace the default in-flight widget. Flutter's `Hero.flightShuttleBuilder`
    /// (`heroes.dart:240`), with FLUI's divergence (ADR-0021 §7n D-N.2): the builder
    /// receives the flight `animation`, the `direction`, and the source and destination
    /// hero child views — not Flutter's two foreign `BuildContext`s, which FLUI has no
    /// way to hand out. When both heroes of a pair supply one, the destination's wins
    /// (`heroes.dart:1040`).
    #[must_use]
    pub fn flight_shuttle_builder<F, V>(mut self, builder: F) -> Self
    where
        F: Fn(&Arc<dyn Animation<f32>>, FlightDirection, &BoxedView, &BoxedView) -> V
            + Send
            + Sync
            + 'static,
        V: IntoView,
    {
        self.shuttle_builder = Some(Arc::new(move |animation, direction, from, to| {
            BoxedView(Box::new(
                builder(animation, direction, from, to).into_view(),
            ))
        }));
        self
    }

    /// Show a custom widget in the hero's place while it is in flight, **without**
    /// losing the child's state.
    ///
    /// FLUI's state-preserving alternative to Flutter's lossy `placeholderBuilder`
    /// (ADR-0021 §7n D-N.3). The closure takes only the frozen [`Size`] the space must
    /// hold; it never receives the child, so it cannot drop it. FLUI keeps the real
    /// child offstage at a constant tree position, so its state survives the flight with
    /// no `GlobalKey`. The default (no placeholder) leaves an empty box of that size, as
    /// Flutter does.
    #[must_use]
    pub fn placeholder<F, V>(mut self, builder: F) -> Self
    where
        F: Fn(Size) -> V + Send + Sync + 'static,
        V: IntoView,
    {
        self.placeholder = Some(Arc::new(move |size| {
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
            handle: HeroHandle::new(
                self.tag.clone(),
                self.child.clone(),
                self.rect_factory.clone(),
                self.shuttle_builder.clone(),
            ),
            registry: None,
        }
    }
}

/// `_HeroState` (`heroes.dart:362`). `pub` only because `StatefulView::State` requires
/// it (as `NavigatorState` is); **not** re-exported, so it is reachable only as
/// `<Hero as StatefulView>::State` and carries no public API of its own.
pub struct HeroState {
    handle: HeroHandle,
    /// The route's registry, resolved once from the ambient [`HeroScope`]. `None` for
    /// a `Hero` mounted outside any route, which is inert rather than an error —
    /// Flutter's `_allHeroesFor` simply never visits it.
    registry: Option<HeroRegistry>,
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
    /// factory, and the shuttle builder are all read at flight start, i.e. from the
    /// *latest* `Hero` configuration.
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
    }

    /// Everything a hero needs from outside itself is acquired **here**, in the one
    /// lifecycle hook that has a `BuildContext` and is not a frame phase: the route's
    /// registry, the render tree, and the rebuild capability (port-check trigger #22).
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        *self.handle.inner.owner.lock() = ctx.pipeline_owner();
        *self.handle.inner.rebuild.lock() = Some(ctx.rebuild_handle());

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

    /// `_HeroState.build` (`heroes.dart:410-438`), minus `TickerMode`, plus the anchor
    /// and FLUI's state-preserving custom placeholder (§7n D-N.3).
    ///
    /// | Flutter | here |
    /// |---|---|
    /// | `placeholderBuilder != null` ⇒ builder output, child dropped, no `_key` | custom `placeholder`: child kept **offstage** at a constant path, placeholder shown as a sibling — state preserved |
    /// | `SizedBox(width: _placeholderSize?.width, …)` | `SizedBox` only while in flight |
    /// | `Offstage(offstage: showPlaceholder, child: KeyedSubtree(key: _key, …))` | `Offstage`, no key: nothing reparents (ADR-0021 D1) |
    /// | `showPlaceholder && !_shouldIncludeChild` ⇒ bare `SizedBox` | same, in the default (no-placeholder) path |
    ///
    /// The [`AnchoredBox`] is always present, in flight or not: it is what publishes
    /// the `RenderId` a controller measures, and a node that came and went would make
    /// `render_id()` flicker.
    fn build(&self, view: &Hero, _ctx: &dyn BuildContext) -> impl IntoView {
        let anchor = self.handle.inner.anchor.clone();
        let placeholder = self.handle.placeholder_size();
        let show_placeholder = placeholder.is_some();

        // Custom placeholder (§7n D-N.3): a hero configured with one uses **one constant
        // structure** in and out of flight — `SizedBox → Stack[ Offstage→child, … ]` —
        // so the child's element (slot 0) is never reparented and its state survives with
        // no `GlobalKey`. The placeholder visual is appended at slot 1 only while in
        // flight; the closure never sees the child, so it cannot drop it. This preserves
        // state uniformly (both flight directions), where Flutter's `placeholderBuilder`
        // drops it. Default heroes (below) keep the exact fixed chain, no `Stack`.
        if let Some(build_placeholder) = &view.placeholder {
            let mut layers: Vec<BoxedView> = vec![
                Offstage::new()
                    .offstage(show_placeholder)
                    .child(view.child.clone())
                    .into_view()
                    .boxed(),
            ];
            if let Some(size) = placeholder {
                layers.push(build_placeholder(size));
            }
            let sized = match placeholder {
                Some(size) => SizedBox::new(size.width.0, size.height.0),
                None => SizedBox::default(),
            };
            return AnchoredBox::new(anchor, sized.child(Stack::new(layers)));
        }

        // `if (showPlaceholder && !_shouldIncludeChild) return SizedBox(w, h);`
        // (`heroes.dart:423-425`): the destination hero drops its child — the shuttle
        // carries it — so this branch legitimately changes shape, and the child's
        // state is not preserved (as in Flutter).
        if show_placeholder && !self.handle.includes_child() {
            let size = placeholder.expect("show_placeholder implies a size");
            return AnchoredBox::new(anchor, SizedBox::new(size.width.0, size.height.0));
        }

        // The **fixed chain** — Flutter's `:427-437`, minus `TickerMode` (FLUI has no
        // ticker gating) and minus the `KeyedSubtree(_key)`:
        //
        //   SizedBox(size?) → Offstage(showPlaceholder) → child
        //
        // The structure is constant across "not in flight" (`SizedBox::default()`,
        // unconstrained, `Offstage(false)`) and "in flight, keep child"
        // (`SizedBox(size)`, `Offstage(true)`). Because the child sits at the same
        // depth under the same two view types either way, reconciliation preserves its
        // element — and therefore its state — with **no `GlobalKey`** (ADR-0021 D2).
        // Flutter's `_key` guards the *caller-supplied `placeholderBuilder`* shape,
        // which this slice does not support (deferred to the public API, §7k).
        let sized = match placeholder {
            Some(size) => SizedBox::new(size.width.0, size.height.0),
            None => SizedBox::default(),
        };
        AnchoredBox::new(
            anchor,
            sized.child(
                Offstage::new()
                    .offstage(show_placeholder)
                    .child(view.child.clone()),
            ),
        )
    }
}
