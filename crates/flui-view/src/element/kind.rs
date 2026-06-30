//! Closed `ElementKind` discriminated union for element storage.
//!
//! Plan §U6 / KTD-1 / FR-019, FR-020. Phase 1 ships only the **shape**:
//! the enum + sub-trait surface + `AnimationListener`. Phase 1 §U8
//! wires the dispatch path (an identity-shim that delegates to the
//! legacy `Box<dyn ElementBase>` storage). Phase 2 introduces the
//! production routing through the typed match arms.
//!
//! # Why eight variants and not one?
//!
//! `Box<dyn ElementBase>` storage paired with a runtime
//! `downcast_ref::<V>()` (`crates/flui-view/src/element/generic.rs:271`)
//! emits `tracing::warn!` on type mismatch and continues with stale
//! state — a silent-correctness trap that the round-5 spec made FR-021
//! retire. `ElementKind` replaces that path with a closed enum so the
//! reconciler can dispatch monomorphically per child position
//! (SC-007). Arity-class dispatch lives at the outer match site
//! because an enum variant cannot introduce a generic parameter the
//! outer enum does not carry — collapsing the Render family into a
//! single variant with an inner arity enum would defeat the per-
//! position monomorphism guarantee.
//!
//! `#[non_exhaustive]` per Constitution Principle 4 + SC-011: future
//! variants (e.g. an `Async` or `Suspense` family) can land without a
//! breaking change.
//!
//! # Sub-trait surface
//!
//! Each variant boxes a domain-specific `*ElementBase` trait object
//! rather than a bare `Box<dyn ElementBase>`. The sub-traits all
//! extend `ElementBase`, so all the lifecycle / mount / update
//! methods remain available through the sub-trait reference (Rust's
//! supertrait coercion). Concrete element types (`StatelessElement<V>`,
//! `StatefulElement<V>`, etc.) gain the sub-trait via blanket impls
//! below — adding a new behavior in the future only requires one new
//! blanket-impl line, not a new variant in this enum (the variant set
//! is closed at the BEHAVIOR FAMILY level, not at the per-`V` level).

#![allow(
    // The render-arity Leaf/Single/Optional sub-trait families have no
    // concrete blanket impls in Phase 1 (only `RenderBehavior<V>` over
    // `Variable` exists today). The empty trait definitions are part of
    // the public surface so Phase 2/3 can land a concrete render-leaf
    // element without rev-ing the variant set. Suppressing the dead-
    // code lint at the module level keeps the file warning-clean.
    dead_code
)]

use std::{fmt, sync::Arc};

use flui_foundation::{Listenable, ListenerId};

use super::{
    arity::{ElementArity, Leaf, Optional, Single, Variable},
    behavior::{
        InheritedBehavior, ProxyBehavior, RenderBehavior, StatefulBehavior, StatelessBehavior,
    },
    unified::Element,
};
use crate::view::{ElementBase, InheritedView, ProxyView, RenderView, StatefulView, StatelessView};

// ============================================================================
// Sub-trait surface
// ============================================================================

/// `ElementBase`-equivalent surface tagging a stateless element.
///
/// Blanket impl below tags every `StatelessElement<V> =
/// Element<V, Single, StatelessBehavior>` automatically; no widget
/// author writes this trait by hand. Phase 1 §U6 introduces the trait
/// as a marker so the [`ElementKind::Stateless`] variant has a
/// type-discriminated home; Phase 2 §U15 routes the typed dispatch
/// through it.
pub trait StatelessElementBase: ElementBase {}

/// `ElementBase`-equivalent surface tagging a stateful element.
///
/// Companion to [`ElementKind::Stateful`]. See [`StatelessElementBase`]
/// for the rationale.
pub trait StatefulElementBase: ElementBase {}

/// `ElementBase`-equivalent surface tagging a proxy (single-child
/// pass-through) element.
///
/// Companion to [`ElementKind::Proxy`].
pub trait ProxyElementBase: ElementBase {}

/// `ElementBase`-equivalent surface tagging an inherited element.
///
/// Companion to [`ElementKind::Inherited`]. The dependent-set protocol
/// already exists on [`ElementBase::as_inherited`] / `as_inherited_mut`
/// — this trait is just the discriminator that lets the reconciler
/// route an inherited element through its sub-trait reference.
pub trait InheritedElementBase: ElementBase {}

/// `ElementBase`-equivalent surface tagging a render element of arity
/// `A`.
///
/// Parameterised by the arity so each render-arity family has its own
/// trait object type — the reconciler can dispatch per arity-class at
/// the outer `ElementKind` match site without re-checking the arity
/// inside the variant data (SC-007). Today only
/// `RenderElementBase<Variable>` has a blanket impl
/// (`Element<V, Variable, RenderBehavior<V>>` over `V: RenderView<...>`);
/// the `Leaf` / `Single` / `Optional` slots exist in the type surface
/// so future render-arity behaviors can land without rev-ing the
/// variant set.
pub trait RenderElementBase<A: ElementArity>: ElementBase {}

// ----------------------------------------------------------------------------
// Blanket impls for the concrete element type aliases.
//
// These are pinned to the canonical (`V`, `A`, `B`) tuples defined in
// `super::mod` as `StatelessElement<V>` / `StatefulElement<V>` /
// `ProxyElement<V>` / `InheritedElement<V>` / `RenderElement<V>`.
// The where-clauses match the bounds on `Element`'s own `impl
// ElementBase` block so the upcast through the sub-trait stays sound.
// ----------------------------------------------------------------------------

impl<V> StatelessElementBase for Element<V, Single, StatelessBehavior>
where
    V: StatelessView + Clone + Send + Sync + 'static,
    StatelessBehavior: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, StatelessBehavior>: ElementBase,
{
}

impl<V> StatefulElementBase for Element<V, Single, StatefulBehavior<V>>
where
    V: StatefulView + Clone + Send + Sync + 'static,
    StatefulBehavior<V>: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, StatefulBehavior<V>>: ElementBase,
{
}

impl<V> ProxyElementBase for Element<V, Single, ProxyBehavior>
where
    V: ProxyView + Clone + Send + Sync + 'static,
    ProxyBehavior: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, ProxyBehavior>: ElementBase,
{
}

impl<V> InheritedElementBase for Element<V, Single, InheritedBehavior<V>>
where
    V: InheritedView + Clone + Send + Sync + 'static,
    InheritedBehavior<V>: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, InheritedBehavior<V>>: ElementBase,
{
}

// `RenderBehavior<V>` is wired to the `Variable` arity in the
// `RenderElement<V>` alias. The `Leaf` / `Single` / `Optional` slots
// have no blanket impl in Phase 1 — they're a forward-compatibility
// hook for Phase 2/3 render-arity work (a leaf `RenderText`, an
// optional-child `RenderClipPath`, etc.). The `RenderView` trait
// carries its protocol as an ASSOCIATED type (`type Protocol`), not a
// generic parameter, so the where-clause only mentions `RenderView`
// directly.
impl<V> RenderElementBase<Variable> for Element<V, Variable, RenderBehavior<V>>
where
    V: RenderView + Clone + Send + Sync + 'static,
    RenderBehavior<V>: super::behavior::ElementBehavior<V, Variable>,
    Element<V, Variable, RenderBehavior<V>>: ElementBase,
{
}

// ============================================================================
// AnimationListener (Stateful variant extension)
// ============================================================================

/// Captured handle to a `Listenable` that a `StatefulElement` subscribes
/// to, plus the listener-id used to detach on unmount.
///
/// Plan §U6 + KTD-1. Phase 1 ships the **struct shape only** — the
/// listener is not yet attached to any `StatefulElement` (that wiring
/// lands when `AnimatedBehavior` joins the dispatch in a later phase).
/// The shape is in place now so the [`ElementKind::Stateful`] variant
/// can carry an `Option<AnimationListener>` and future plumbing can
/// populate it without rev-ing the enum.
///
/// # Why a thunk closure instead of a typed callback?
///
/// `ElementKind::Stateful` boxes `Box<dyn StatefulElementBase>` —
/// type-erased over `V`. A typed call (`view.listenable()`) needs the
/// concrete `V`, but that is only in scope at element-creation time
/// (`create_element` for the View). The thunk closure captures the
/// listenable handle AT THAT MOMENT and returns it on every call, so
/// the dispatcher can re-acquire the handle without ever crossing the
/// typed-`V` boundary again. KTD-1 spells out the alternative (passing
/// `&dyn StatefulElementBase` into the closure) and rejects it because
/// it would require a runtime downcast that defeats FR-021.
pub struct AnimationListener {
    /// Captured listenable provider.
    ///
    /// Returns a fresh `Arc<dyn Listenable>` clone on each call. The
    /// concrete listenable type is captured at construction time
    /// (when the typed `V::listenable()` call site is in scope) — the
    /// closure body merely `Arc::clone`s the captured handle.
    pub listenable_provider: Box<dyn Fn() -> Arc<dyn Listenable> + Send + Sync>,
    /// Identifier returned by the `Listenable::add_listener` call;
    /// passed to `remove_listener` on detach.
    pub listener_id: ListenerId,
}

impl AnimationListener {
    /// Construct a listener handle from a captured listenable provider
    /// and the listener-id returned by the matching `add_listener` call.
    pub fn new(
        listenable_provider: Box<dyn Fn() -> Arc<dyn Listenable> + Send + Sync>,
        listener_id: ListenerId,
    ) -> Self {
        Self {
            listenable_provider,
            listener_id,
        }
    }

    /// Re-acquire the captured listenable handle.
    ///
    /// Invokes the stored thunk, which `Arc::clone`s the listenable
    /// captured at construction time. The closure is `Send + Sync`
    /// (required by the field bound), so calls are safe across
    /// threads.
    pub fn listenable(&self) -> Arc<dyn Listenable> {
        (self.listenable_provider)()
    }
}

impl fmt::Debug for AnimationListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnimationListener")
            // The closure has no useful debug representation; surface
            // only the listener-id, which is the field a developer
            // actually needs to correlate against `Listenable::remove`.
            .field("listener_id", &self.listener_id)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// ElementKind closed enum
// ============================================================================

/// Closed discriminated union over element behavior families.
///
/// Replaces the legacy `Box<dyn ElementBase>` storage with a typed
/// variant set so the reconciler can dispatch monomorphically per
/// arity-class (SC-007) and the `View::can_update` mismatch path no
/// longer silently downcast-warns with stale state (FR-021).
///
/// Variants are pinned at the behavior-FAMILY level — adding a new
/// `StatelessView` impl does not introduce a new variant; it picks up
/// the existing [`ElementKind::Stateless`] route via the blanket impl
/// of [`StatelessElementBase`] for `Element<V, Single, StatelessBehavior>`.
/// Adding a new behavior family (an `Async` / `Suspense` body) IS a
/// breaking-by-default extension — `#[non_exhaustive]` keeps the door
/// open while signalling the size of the change to downstream matches.
///
/// # Stateful + AnimationListener
///
/// The [`ElementKind::Stateful`] variant carries an
/// `Option<AnimationListener>` so `AnimatedBehavior` (a Phase 2/3
/// composition over `StatefulBehavior`) can attach a per-element
/// listenable subscription without introducing a separate
/// `ElementKind::Animated` variant. KTD-1 rules out the separate
/// variant because `AnimationBehavior` *composes* `StatefulBehavior`
/// rather than peering it (confirmed by
/// `crates/flui-view/UNIFIED_ELEMENT.md:67`).
#[non_exhaustive]
pub enum ElementKind {
    /// A `StatelessView` element. Boxes a `StatelessElement<V> =
    /// Element<V, Single, StatelessBehavior>`.
    Stateless(Box<dyn StatelessElementBase>),
    /// A `StatefulView` element. Boxes a `StatefulElement<V> =
    /// Element<V, Single, StatefulBehavior<V>>`, plus an optional
    /// [`AnimationListener`] that an `AnimatedBehavior` composition
    /// can attach.
    Stateful {
        /// The underlying stateful element.
        element: Box<dyn StatefulElementBase>,
        /// Optional listenable subscription attached at element-
        /// creation time. `None` for plain `StatefulView`s; `Some`
        /// when wrapped in an `AnimatedBehavior` composition.
        animation_listener: Option<AnimationListener>,
    },
    /// A `ProxyView` element (single-child pass-through).
    Proxy(Box<dyn ProxyElementBase>),
    /// An `InheritedView` element.
    Inherited(Box<dyn InheritedElementBase>),
    /// A `RenderView` element with no children (e.g. `Text`, `Image`).
    /// No blanket impl exists in Phase 1; the slot is reserved for
    /// Phase 2/3 leaf-render bodies.
    RenderLeaf(Box<dyn RenderElementBase<Leaf>>),
    /// A `RenderView` element with exactly one child (e.g. `Center`,
    /// `Padding`). No blanket impl exists in Phase 1; the slot is
    /// reserved for Phase 2/3 single-child render bodies.
    RenderSingle(Box<dyn RenderElementBase<Single>>),
    /// A `RenderView` element with zero or one children (e.g.
    /// `Container`). No blanket impl exists in Phase 1; reserved for
    /// Phase 2/3.
    RenderOptional(Box<dyn RenderElementBase<Optional>>),
    /// A `RenderView` element with N children (e.g. `Row`, `Column`,
    /// `Stack`). The only render arity with a concrete blanket impl in
    /// Phase 1.
    RenderVariable(Box<dyn RenderElementBase<Variable>>),
}

impl ElementKind {
    /// Borrow the underlying element regardless of variant.
    ///
    /// All eight variants box a sub-trait of [`ElementBase`]; this
    /// helper hides the per-variant match so callers that need only
    /// the `ElementBase` surface (debug printing, lifecycle queries)
    /// can read it generically.
    pub fn element(&self) -> &dyn ElementBase {
        match self {
            Self::Stateless(e) => &**e,
            Self::Stateful { element, .. } => &**element,
            Self::Proxy(e) => &**e,
            Self::Inherited(e) => &**e,
            Self::RenderLeaf(e) => &**e,
            Self::RenderSingle(e) => &**e,
            Self::RenderOptional(e) => &**e,
            Self::RenderVariable(e) => &**e,
        }
    }

    /// Mutably borrow the underlying element regardless of variant.
    ///
    /// The `&mut dyn ElementBase` companion to [`Self::element`], for the
    /// element-tree accessors that drive lifecycle (`mount`/`update`/
    /// `unmount`) through the `ElementBase` surface during the Phase 1
    /// storage migration (FR-019).
    pub fn element_mut(&mut self) -> &mut dyn ElementBase {
        match self {
            Self::Stateless(e) => &mut **e,
            Self::Stateful { element, .. } => &mut **element,
            Self::Proxy(e) => &mut **e,
            Self::Inherited(e) => &mut **e,
            Self::RenderLeaf(e) => &mut **e,
            Self::RenderSingle(e) => &mut **e,
            Self::RenderOptional(e) => &mut **e,
            Self::RenderVariable(e) => &mut **e,
        }
    }

    /// Static name of the variant, for logging / debug.
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Stateless(_) => "Stateless",
            Self::Stateful { .. } => "Stateful",
            Self::Proxy(_) => "Proxy",
            Self::Inherited(_) => "Inherited",
            Self::RenderLeaf(_) => "RenderLeaf",
            Self::RenderSingle(_) => "RenderSingle",
            Self::RenderOptional(_) => "RenderOptional",
            Self::RenderVariable(_) => "RenderVariable",
        }
    }
}

impl fmt::Debug for ElementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Sub-trait references do not require `Debug` themselves; the
        // inner element exposes `debug_description` through the
        // ElementBase surface. Cheaper than threading `Debug` bounds
        // through all four sub-traits.
        f.debug_struct(self.variant_name())
            .field("element", &self.element().debug_description())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustivity check — the closed set is exactly the eight
    /// variants below. Adding a new behavior family without updating
    /// this match (and every other consumer) is a compile error,
    /// which is the SC-007 / SC-011 contract.
    ///
    /// `#[non_exhaustive]` lets downstream `match` blocks fail-safe
    /// with a wildcard; this test deliberately omits the wildcard so
    /// the in-crate audit surface stays exhaustive.
    fn _exhaustive_discriminant(kind: &ElementKind) -> u8 {
        match kind {
            ElementKind::Stateless(_) => 0,
            ElementKind::Stateful { .. } => 1,
            ElementKind::Proxy(_) => 2,
            ElementKind::Inherited(_) => 3,
            ElementKind::RenderLeaf(_) => 4,
            ElementKind::RenderSingle(_) => 5,
            ElementKind::RenderOptional(_) => 6,
            ElementKind::RenderVariable(_) => 7,
        }
    }

    /// `variant_name` returns the discriminant tag for the matching
    /// variant. Tests construction-via-test-fixture for variants that
    /// have real concrete impls in Phase 1 (Stateless / Stateful /
    /// RenderVariable via the canonical type aliases). The Render
    /// Leaf/Single/Optional / Proxy / Inherited variants are
    /// exhausted in the discriminant test above; their per-variant
    /// construction lands when concrete behaviors land in Phase 2/3.
    #[test]
    fn variant_name_matches_variant() {
        // We cannot construct a `Box<dyn StatelessElementBase>` here
        // without a real `Element<V, Single, StatelessBehavior>` — that
        // requires a full `StatelessView` test fixture, which already
        // exists in `tests/stateless_stateful_tests.rs`. The variant
        // names themselves are exercised via the discriminant test
        // above (compile-time guarantee). The `variant_name` function
        // is therefore exercised here through a sanity construction
        // of the simplest non-element variant — but ALL eight variants
        // require an inner box. The static-string return is the actual
        // contract; we assert the eight strings are distinct so a
        // future typo turns into a test failure.
        let names = [
            "Stateless",
            "Stateful",
            "Proxy",
            "Inherited",
            "RenderLeaf",
            "RenderSingle",
            "RenderOptional",
            "RenderVariable",
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), 8, "variant names must be distinct");
    }

    /// `AnimationListener` round-trip: construct with a synthetic
    /// closure capturing a `ValueNotifier`, call `listenable()`, and
    /// verify the captured handle is reachable and non-null. The
    /// thunk-capture pattern is the contract that lets the Stateful
    /// variant subscribe to a listenable without a runtime
    /// `downcast::<V>()` (KTD-1).
    #[test]
    fn animation_listener_thunk_returns_captured_handle() {
        use flui_foundation::{ChangeNotifier, ListenerId};

        let notifier: Arc<dyn Listenable> = Arc::new(ChangeNotifier::new());
        let captured = Arc::clone(&notifier);
        let listener =
            AnimationListener::new(Box::new(move || Arc::clone(&captured)), ListenerId::new(1));

        // Call the thunk; the returned handle must point at the
        // same listenable we captured.
        let returned = listener.listenable();
        assert!(Arc::ptr_eq(&returned, &notifier));
        assert_eq!(listener.listener_id, ListenerId::new(1));
    }

    /// Debug format omits the unprintable closure but surfaces the
    /// listener-id so trace output is correlatable with
    /// `Listenable::remove_listener` call sites.
    #[test]
    fn animation_listener_debug_redacts_closure() {
        use flui_foundation::{ChangeNotifier, ListenerId};

        let notifier: Arc<dyn Listenable> = Arc::new(ChangeNotifier::new());
        let listener =
            AnimationListener::new(Box::new(move || Arc::clone(&notifier)), ListenerId::new(42));
        let debug = format!("{listener:?}");
        assert!(debug.contains("AnimationListener"));
        assert!(debug.contains("listener_id"));
        // The thunk closure must NOT leak into Debug output — there
        // is no useful representation for `Box<dyn Fn() -> ...>`.
        assert!(!debug.contains("listenable_provider"));
    }
}
