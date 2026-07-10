//! The `Hero` view, its per-route registry, and the handle a `HeroController` drives.
//!
//! ADR-0021 U3.5. **Private.** No public `Hero`, no prelude export. A `Hero` is a
//! pass-through that can be *told* to show a placeholder, and a registry entry that
//! lets the controller find it by tag. U4's private `_HeroFlight` now drives this
//! placeholder machinery, but the public/customizable Hero API is still absent.
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
//!   (`:330-333`). Nested navigators remain out of scope either way — no
//!   `HeroControllerScope` exists — so the outer controller simply never sees them.
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

// U3.5 is the registry + view + handle. `HeroController` reads the registry, but the
// `Hero` view has no production constructor until the public API lands (U6), so
// `dead_code` cascades from the view through the handle it owns. Deleting it and
// re-deriving it later is how a seam stops matching the ADR that specified it.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

use super::subtree::AnchoredBox;
use crate::{Offstage, SizedBox};

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
    fn new(tag: HeroTag, shuttle_child: BoxedView) -> Self {
        Self {
            inner: Arc::new(HeroInner {
                tag,
                anchor: SubtreeAnchor::new(),
                placeholder: Mutex::new(None),
                include_child: AtomicBool::new(true),
                owner: Mutex::new(None),
                rebuild: Mutex::new(None),
                shuttle_child: Mutex::new(shuttle_child),
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
/// **Private.** This slice gives it `tag` and `child` and nothing else — no
/// `createRectTween`, no `flightShuttleBuilder`, no `placeholderBuilder`, no
/// `transitionOnUserGestures`, no `HeroMode`. Its `build` is a pass-through until a
/// controller tells it otherwise.
#[derive(Clone)]
pub(crate) struct Hero {
    tag: HeroTag,
    child: BoxedView,
}

impl Hero {
    pub(crate) fn new(tag: HeroTag, child: impl IntoView) -> Self {
        Self {
            tag,
            child: BoxedView(Box::new(child.into_view())),
        }
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
            handle: HeroHandle::new(self.tag.clone(), self.child.clone()),
            registry: None,
        }
    }
}

/// `_HeroState` (`heroes.dart:362`).
pub(crate) struct HeroState {
    handle: HeroHandle,
    /// The route's registry, resolved once from the ambient [`HeroScope`]. `None` for
    /// a `Hero` mounted outside any route, which is inert rather than an error —
    /// Flutter's `_allHeroesFor` simply never visits it.
    registry: Option<HeroRegistry>,
}

impl HeroState {
    /// Test-facing: the handle a controller would drive.
    #[cfg(test)]
    pub(crate) fn handle(&self) -> HeroHandle {
        self.handle.clone()
    }
}

impl ViewState<Hero> for HeroState {
    /// Keep the shuttle's source current: `_defaultHeroFlightShuttleBuilder` reads
    /// `toHero.widget.child` at flight start, i.e. the *latest* configuration.
    fn did_update_view(&mut self, _old: &Hero, new_view: &Hero) {
        *self.handle.inner.shuttle_child.lock() = new_view.child.clone();
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

    /// `_HeroState.build` (`heroes.dart:410-438`), minus `placeholderBuilder` and
    /// `TickerMode`, plus the anchor.
    ///
    /// | Flutter | here |
    /// |---|---|
    /// | `SizedBox(width: _placeholderSize?.width, …)` | `SizedBox` only while in flight |
    /// | `Offstage(offstage: showPlaceholder, child: KeyedSubtree(key: _key, …))` | `Offstage`, no key: nothing reparents (ADR-0021 D1) |
    /// | `showPlaceholder && !_shouldIncludeChild` ⇒ bare `SizedBox` | same |
    ///
    /// The [`AnchoredBox`] is always present, in flight or not: it is what publishes
    /// the `RenderId` a controller measures, and a node that came and went would make
    /// `render_id()` flicker.
    fn build(&self, view: &Hero, _ctx: &dyn BuildContext) -> impl IntoView {
        let anchor = self.handle.inner.anchor.clone();
        let placeholder = self.handle.placeholder_size();
        let show_placeholder = placeholder.is_some();

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
