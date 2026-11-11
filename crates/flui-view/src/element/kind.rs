//! Closed `ElementKind` discriminated union for element storage.
//!
//! The enum gives each behavior family a type-discriminated home. It is a
//! storage and match surface: `element` and `element_mut` erase every variant
//! to `&dyn ElementBase`, and the lifecycle path runs through that trait object.
//!
//! # Why closed variants and not one?
//!
//! `Box<dyn ElementBase>` storage paired with a runtime
//! `downcast_ref::<V>()` emits `tracing::warn!` on type mismatch and
//! continues with stale state. `ElementKind` makes the behavior family a
//! closed, matchable fact instead of something recovered by downcasting.
//! Arity-class dispatch lives at the outer match site because an enum variant
//! cannot introduce a generic parameter the outer enum does not carry.
//!
//! `#[non_exhaustive]`: future variants (e.g. an `Async` or
//! `Suspense` family) can land without a breaking change.
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

use flui_rendering::parent_data::SliverSlot;
use std::fmt;

use super::{
    arity::{Single, Variable},
    behavior::{
        AnimatedBehavior, ElementBehavior, InheritedBehavior, ParentDataBehavior, ProxyBehavior,
        RenderBehavior, StatefulBehavior, StatelessBehavior,
    },
    stateful_render::StatefulRenderBehavior,
    unified::Element,
};
use crate::view::{
    AnimatedView, ElementBase, InheritedView, ParentDataView, ProxyView, RenderView,
    StatefulRenderView, StatefulView, StatelessView,
};

// ============================================================================
// Sub-trait surface
// ============================================================================

/// `ElementBase`-equivalent surface tagging a stateless element.
///
/// Blanket impl below tags every `StatelessElement<V> =
/// Element<V, Single, StatelessBehavior>` automatically; no widget
/// author writes this trait by hand. The trait is the marker that gives
/// the [`ElementKind::Stateless`] variant a type-discriminated home.
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

/// `ElementBase`-equivalent surface tagging a render element.
///
/// Render elements are all `Variable`-arity (`Element<V, Variable, …>`),
/// so the trait is not parameterised by arity; the discriminator the
/// reconciler routes on is the variant itself.
pub trait RenderElementBase: ElementBase {}

/// `ElementBase`-equivalent surface tagging the render-tree ROOT element.
///
/// Companion to [`ElementKind::Root`]. The root (`RootRenderElement`)
/// is a first-class
/// element *kind*, not a behavior-family element: it owns the `PipelineOwner`
/// and bootstraps the render tree, so it neither composes a `View`-behavior nor
/// fits the `Element<V, A, Behavior>` shape. A dedicated sealed sub-trait keeps
/// it out of the behavior taxonomy without an unsealed `Box<dyn ElementBase>`.
pub trait RootElementBase: ElementBase {}

/// `ElementBase`-equivalent surface tagging an element that handles bubbling
/// notifications.
///
/// Companion to [`ElementKind::Notification`]. Notification listener elements
/// override [`ElementBase::on_notification`] to translate the object-safe
/// `(TypeId, &dyn Any)` dispatch shape into their typed callback. They are a
/// distinct family because neither stateless nor render behavior owns that
/// interception hook.
pub trait NotificationElementBase: ElementBase {}

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
    V: StatelessView + Clone + 'static,
    StatelessBehavior: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, StatelessBehavior>: ElementBase,
{
}

impl<V> StatefulElementBase for Element<V, Single, StatefulBehavior<V>>
where
    V: StatefulView + Clone + 'static,
    StatefulBehavior<V>: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, StatefulBehavior<V>>: ElementBase,
{
}

impl<V> ProxyElementBase for Element<V, Single, ProxyBehavior>
where
    V: ProxyView + Clone + 'static,
    ProxyBehavior: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, ProxyBehavior>: ElementBase,
{
}

impl<V> InheritedElementBase for Element<V, Single, InheritedBehavior<V>>
where
    V: InheritedView + Clone + 'static,
    InheritedBehavior<V>: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, InheritedBehavior<V>>: ElementBase,
{
}

// `RenderBehavior<V>` is wired to the `Variable` arity in the
// `RenderElement<V>` alias. The `RenderView` trait carries its protocol
// as an ASSOCIATED type (`type Protocol`), not a generic parameter, so
// the where-clause only mentions `RenderView` directly.
impl<V> RenderElementBase for Element<V, Variable, RenderBehavior<V>>
where
    V: RenderView + Clone + 'static,
    RenderBehavior<V>: super::behavior::ElementBehavior<V, Variable>,
    Element<V, Variable, RenderBehavior<V>>: ElementBase,
{
}

// A `StatefulRenderView` element owns a render object, so the reconciler
// routes it as a render element.
impl<V> RenderElementBase for Element<V, Variable, StatefulRenderBehavior<V>>
where
    V: StatefulRenderView + Clone + 'static,
    StatefulRenderBehavior<V>: super::behavior::ElementBehavior<V, Variable>,
    Element<V, Variable, StatefulRenderBehavior<V>>: ElementBase,
{
}

// Animated + ParentData wiring: these two families map onto the existing
// `ElementKind` variants. `AnimatedView: StatefulView` and
// `AnimatedBehavior` composes the stateful body, so an `AnimatedElement` routes
// to the `Stateful` variant. `ParentDataBehavior` is proxy-shaped, so a
// `ParentDataElement` routes to the `Proxy` variant.
impl<V> StatefulElementBase for Element<V, Single, AnimatedBehavior<V>>
where
    V: AnimatedView + Clone + 'static,
    AnimatedBehavior<V>: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, AnimatedBehavior<V>>: ElementBase,
{
}

impl<V> ProxyElementBase for Element<V, Single, ParentDataBehavior>
where
    V: ParentDataView + Clone + 'static,
    ParentDataBehavior: super::behavior::ElementBehavior<V, Single>,
    Element<V, Single, ParentDataBehavior>: ElementBase,
{
}

// Special, non-behavior-family elements get their own kind (see the
// `Root`/`Error` variants + their marker traits above). Both are standalone
// `ElementBase` impls (not `Element<V, A, Behavior>`), so a dedicated sealed
// sub-trait is the type-safe home — no unsealed `Box<dyn ElementBase>`.
impl<V> RootElementBase for crate::view::RootRenderElement<V>
where
    V: crate::view::View + Clone + 'static,
    crate::view::RootRenderElement<V>: ElementBase,
{
}

// ============================================================================
// ElementKind closed enum
// ============================================================================

/// Closed discriminated union over element behavior families.
///
/// The variant set is a typed record of the behavior family, so the
/// `View::can_update` mismatch path matches on it instead of downcasting.
/// `element` and `element_mut` erase every variant to `&dyn ElementBase`.
///
/// Variants are pinned at the behavior-FAMILY level — adding a new
/// `StatelessView` impl does not introduce a new variant; it picks up
/// the existing [`ElementKind::Stateless`] route via the blanket impl
/// of [`StatelessElementBase`] for `Element<V, Single, StatelessBehavior>`.
/// Adding a new behavior family (an `Async` / `Suspense` body) IS a
/// breaking-by-default extension — `#[non_exhaustive]` keeps the door
/// open while signalling the size of the change to downstream matches.
#[non_exhaustive]
pub enum ElementKind {
    /// A `StatelessView` element. Boxes a `StatelessElement<V> =
    /// Element<V, Single, StatelessBehavior>`.
    Stateless(Box<dyn StatelessElementBase>),
    /// A `StatefulView` element. Boxes a `StatefulElement<V> =
    /// Element<V, Single, StatefulBehavior<V>>`.
    Stateful(Box<dyn StatefulElementBase>),
    /// A `ProxyView` element (single-child pass-through).
    Proxy(Box<dyn ProxyElementBase>),
    /// An `InheritedView` element.
    Inherited(Box<dyn InheritedElementBase>),
    /// A notification-listener element.
    Notification(Box<dyn NotificationElementBase>), // closed ElementKind storage variant
    /// A `RenderView` element with N children (e.g. `Row`, `Column`,
    /// `Stack`).
    RenderVariable(Box<dyn RenderElementBase>),
    /// The render-tree ROOT element (`RootRenderElement`). Owns the `PipelineOwner` + render-tree
    /// bootstrap; a first-class kind distinct from the behavior families.
    /// See [`RootElementBase`].
    Root(Box<dyn RootElementBase>),
}

impl ElementKind {
    /// Get the `TypeId` of the view configuration that created this element.
    pub fn view_type_id(&self) -> std::any::TypeId {
        self.element().view_type_id()
    }

    /// Get the current lifecycle state of the inner element.
    pub fn lifecycle(&self) -> crate::element::Lifecycle {
        self.element().lifecycle()
    }

    /// Create a stateless-family element kind.
    pub fn stateless<V>(view: &V) -> Self
    where
        V: StatelessView + crate::view::View + Clone + 'static,
        StatelessBehavior: ElementBehavior<V, Single>,
        Element<V, Single, StatelessBehavior>: ElementBase,
    {
        Self::Stateless(Box::new(Element::<V, Single, StatelessBehavior>::new(
            view,
            StatelessBehavior::new(),
        )))
    }

    /// Create a stateful-family element kind.
    pub fn stateful<V>(view: &V) -> Self
    where
        V: StatefulView + crate::view::View + Clone + 'static,
        StatefulBehavior<V>: ElementBehavior<V, Single>,
        Element<V, Single, StatefulBehavior<V>>: ElementBase,
    {
        Self::Stateful(Box::new(Element::<V, Single, StatefulBehavior<V>>::new(
            view,
            StatefulBehavior::new(),
        )))
    }

    /// Create a proxy-family element kind.
    pub fn proxy<V>(view: &V) -> Self
    where
        V: ProxyView + crate::view::View + Clone + 'static,
        ProxyBehavior: ElementBehavior<V, Single>,
        Element<V, Single, ProxyBehavior>: ElementBase,
    {
        Self::Proxy(Box::new(Element::<V, Single, ProxyBehavior>::new(
            view,
            ProxyBehavior,
        )))
    }

    /// Create an inherited-family element kind.
    pub fn inherited<V>(view: &V) -> Self
    where
        V: InheritedView + crate::view::View + Clone + 'static,
        InheritedBehavior<V>: ElementBehavior<V, Single>,
        Element<V, Single, InheritedBehavior<V>>: ElementBase,
    {
        Self::Inherited(Box::new(Element::<V, Single, InheritedBehavior<V>>::new(
            view,
            InheritedBehavior::new(view),
        )))
    }

    /// Create a render-object element kind for the currently wired variable arity.
    pub fn render_variable<V>(view: &V) -> Self
    where
        V: RenderView + crate::view::View + Clone + 'static,
        RenderBehavior<V>: ElementBehavior<V, Variable>,
        Element<V, Variable, RenderBehavior<V>>: ElementBase,
    {
        Self::RenderVariable(Box::new(Element::<V, Variable, RenderBehavior<V>>::new(
            view,
            RenderBehavior::new(),
        )))
    }

    /// Create the element kind of a render view with state.
    pub fn stateful_render<V>(view: &V) -> Self
    where
        V: StatefulRenderView + crate::view::View + Clone + 'static,
        StatefulRenderBehavior<V>: ElementBehavior<V, Variable>,
        Element<V, Variable, StatefulRenderBehavior<V>>: ElementBase,
    {
        Self::RenderVariable(Box::new(
            Element::<V, Variable, StatefulRenderBehavior<V>>::new(
                view,
                StatefulRenderBehavior::new(),
            ),
        ))
    }

    /// Create an animated stateful-family element kind.
    pub fn animated<V>(view: &V) -> Self
    where
        V: AnimatedView + crate::view::View + Clone + 'static,
        AnimatedBehavior<V>: ElementBehavior<V, Single>,
        Element<V, Single, AnimatedBehavior<V>>: ElementBase,
    {
        Self::Stateful(Box::new(Element::<V, Single, AnimatedBehavior<V>>::new(
            view,
            AnimatedBehavior::new(),
        )))
    }

    /// Create a parent-data proxy-family element kind.
    pub fn parent_data<V>(view: &V) -> Self
    where
        V: ParentDataView + crate::view::View + Clone + 'static,
        ParentDataBehavior: ElementBehavior<V, Single>,
        Element<V, Single, ParentDataBehavior>: ElementBase,
    {
        Self::Proxy(Box::new(Element::<V, Single, ParentDataBehavior>::new(
            view,
            ParentDataBehavior,
        )))
    }

    /// Borrow the underlying element regardless of variant.
    ///
    /// Every variant boxes a sub-trait of [`ElementBase`]; this
    /// helper hides the per-variant match so callers that need only
    /// the `ElementBase` surface (debug printing, lifecycle queries)
    /// can read it generically.
    pub fn element(&self) -> &dyn ElementBase {
        match self {
            Self::Stateless(e) => &**e,
            Self::Stateful(e) => &**e,
            Self::Proxy(e) => &**e,
            Self::Inherited(e) => &**e,
            Self::Notification(e) => &**e,
            Self::RenderVariable(e) => &**e,
            Self::Root(e) => &**e,
        }
    }

    /// Mutably borrow the underlying element regardless of variant.
    ///
    /// The `&mut dyn ElementBase` companion to [`Self::element`], for the
    /// element-tree accessors that drive lifecycle (`mount`/`update`/
    /// `unmount`) through the `ElementBase` surface.
    pub fn element_mut(&mut self) -> &mut dyn ElementBase {
        match self {
            Self::Stateless(e) => &mut **e,
            Self::Stateful(e) => &mut **e,
            Self::Proxy(e) => &mut **e,
            Self::Inherited(e) => &mut **e,
            Self::Notification(e) => &mut **e,
            Self::RenderVariable(e) => &mut **e,
            Self::Root(e) => &mut **e,
        }
    }

    /// Consume into a type-erased `Box<dyn ElementBase>` (trait upcast).
    ///
    /// Bridges to the `IntoElement` / `BoxedElement` type-erasure utility,
    /// which is a separate surface from the element-tree's `ElementKind`
    /// storage — it intentionally re-erases to the base trait for callers that
    /// only want an opaque element handle.
    #[must_use]
    pub fn into_boxed(self) -> Box<dyn ElementBase> {
        match self {
            Self::Stateless(e) => e,
            Self::Stateful(e) => e,
            Self::Proxy(e) => e,
            Self::Inherited(e) => e,
            Self::Notification(e) => e,
            Self::RenderVariable(e) => e,
            Self::Root(e) => e,
        }
    }

    /// Static name of the variant, for logging / debug.
    pub fn variant_name(&self) -> &'static str {
        match self {
            Self::Stateless(_) => "Stateless",
            Self::Stateful(_) => "Stateful",
            Self::Proxy(_) => "Proxy",
            Self::Inherited(_) => "Inherited",
            Self::Notification(_) => "Notification",
            Self::RenderVariable(_) => "RenderVariable",
            Self::Root(_) => "Root",
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

impl ElementBase for ElementKind {
    fn view_type_id(&self) -> std::any::TypeId {
        self.element().view_type_id()
    }

    fn view_type_name(&self) -> &'static str {
        self.element().view_type_name()
    }

    fn debug_kind(&self) -> &'static str {
        self.element().debug_kind()
    }

    fn inspect_view(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        self.element().inspect_view(out);
    }

    fn current_key_hash(&self) -> Option<u64> {
        self.element().current_key_hash()
    }

    fn current_key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        self.element().current_key()
    }

    fn set_self_id(&mut self, id: flui_foundation::ElementId) {
        self.element_mut().set_self_id(id);
    }

    fn slot(&self) -> usize {
        self.element().slot()
    }

    fn depth(&self) -> usize {
        self.element().depth()
    }

    fn set_depth(&mut self, depth: crate::view::ElementDepth) {
        self.element_mut().set_depth(depth);
    }

    fn lifecycle(&self) -> crate::element::Lifecycle {
        self.element().lifecycle()
    }

    fn mount(
        &mut self,
        parent: Option<flui_foundation::ElementId>,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        self.element_mut().mount(parent, slot, owner);
    }

    fn unmount(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.element_mut().unmount(owner);
    }

    fn activate(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.element_mut().activate(owner);
    }

    fn deactivate(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.element_mut().deactivate(owner);
    }

    fn update(&mut self, new_view: &dyn crate::view::View, owner: &mut crate::ElementOwner<'_>) {
        self.element_mut().update(new_view, owner);
    }

    fn mark_needs_build(&mut self) {
        self.element_mut().mark_needs_build();
    }

    fn is_dirty(&self) -> bool {
        self.element().is_dirty()
    }

    fn build_into_views(&mut self, owner: &mut crate::ElementOwner<'_>) -> crate::view::ChildViews {
        self.element_mut().build_into_views(owner)
    }

    fn notify_dependency_change(&mut self, owner: &mut crate::ElementOwner<'_>) {
        self.element_mut().notify_dependency_change(owner);
    }

    fn update_slot(&mut self, new_slot: usize) {
        self.element_mut().update_slot(new_slot);
    }

    fn deactivate_child(&mut self, child: flui_foundation::ElementId) {
        self.element_mut().deactivate_child(child);
    }

    fn debug_description(&self) -> String {
        self.element().debug_description()
    }

    fn set_pipeline_owner(&mut self, owner: flui_rendering::pipeline::PipelineCell) {
        self.element_mut().set_pipeline_owner(owner);
    }

    fn pipeline_owner(&self) -> Option<flui_rendering::pipeline::PipelineCell> {
        self.element().pipeline_owner()
    }

    fn child_render_id(&self) -> Option<flui_foundation::RenderId> {
        self.element().child_render_id()
    }

    fn set_parent_render_id(&mut self, parent_id: Option<flui_foundation::RenderId>) {
        self.element_mut().set_parent_render_id(parent_id);
    }
    fn child_sliver_slot(&self, slot: usize) -> Option<SliverSlot> {
        self.element().child_sliver_slot(slot)
    }
    fn set_sliver_slot(&mut self, slot: Option<SliverSlot>) {
        self.element_mut().set_sliver_slot(slot);
    }
    fn sliver_slot(&self) -> Option<SliverSlot> {
        self.element().sliver_slot()
    }
    fn sliver_slot_for_child(&self, logical: usize) -> SliverSlot {
        self.element().sliver_slot_for_child(logical)
    }
    fn hosts_sparse_children(&self) -> bool {
        self.element().hosts_sparse_children()
    }

    fn as_inherited(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        self.element().as_inherited()
    }

    fn as_inherited_mut(&mut self) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        self.element_mut().as_inherited_mut()
    }

    fn take_provided_scope_stale(&mut self) -> bool {
        self.element_mut().take_provided_scope_stale()
    }

    fn view_as_any(&self) -> Option<&dyn std::any::Any> {
        self.element().view_as_any()
    }

    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        self.element().state_as_any()
    }

    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        self.element().render_id()
    }

    fn parent_data_config(&self) -> Option<Box<dyn flui_rendering::parent_data::ParentData>> {
        self.element().parent_data_config()
    }

    fn apply_parent_data_config(
        &self,
        parent_data: &mut dyn flui_rendering::parent_data::ParentData,
    ) -> flui_rendering::RenderUpdateImpact {
        self.element().apply_parent_data_config(parent_data)
    }

    fn parent_data_type_id(&self) -> Option<std::any::TypeId> {
        self.element().parent_data_type_id()
    }

    fn parent_data_debug_type_name(&self) -> Option<&'static str> {
        self.element().parent_data_debug_type_name()
    }

    fn parent_data_typical_ancestor_description(&self) -> Option<&'static str> {
        self.element().parent_data_typical_ancestor_description()
    }

    fn parent_data_storage_type_name(&self) -> Option<&'static str> {
        self.element().parent_data_storage_type_name()
    }

    fn on_notification(&self, type_id: std::any::TypeId, notification: &dyn std::any::Any) -> bool {
        self.element().on_notification(type_id, notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustivity check — the closed set is exactly the seven
    /// variants below. Adding a new behavior family without updating
    /// this match (and every other consumer) is a compile error —
    /// that's the point of keeping this test's match exhaustive.
    ///
    /// `#[non_exhaustive]` lets downstream `match` blocks fail-safe
    /// with a wildcard; this test deliberately omits the wildcard so
    /// the in-crate audit surface stays exhaustive.
    fn _exhaustive_discriminant(kind: &ElementKind) -> u8 {
        match kind {
            ElementKind::Stateless(_) => 0,
            ElementKind::Stateful(_) => 1,
            ElementKind::Proxy(_) => 2,
            ElementKind::Inherited(_) => 3,
            ElementKind::Notification(_) => 4,
            ElementKind::RenderVariable(_) => 5,
            ElementKind::Root(_) => 6,
        }
    }
}
