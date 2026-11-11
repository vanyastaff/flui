//! [`Intent`], [`Action`], [`CallbackAction`] and the [`Actions`] widget — the
//! typed command layer `Shortcuts` dispatches into.
//!
//! ADR-0023.
//!
//! # Resolution (ADR-0023)
//!
//! Resolution **stops at the first scope that declares the intent's type at
//! all**, enabled or not. A disabled mapping does **not** fall through to an
//! enclosing scope's mapping for the same type. Actions are keyed by
//! [`TypeId`] and **chained at provide time**: each `Actions` widget publishes
//! a link that shares its bindings and names the chain above it, and a lookup
//! walks the links outward from the nearest provider.
//!
//! The erasure (`TypeId` key + `dyn Any` downcast inside the typed wrapper)
//! is the same shape as the one sanctioned `Navigator` pop-result boundary
//! (ADR-0019): the downcast can only be reached through the matching
//! `TypeId`, so it cannot fail.
//!
//! # Deferred, and named
//!
//! An `ActionDispatcher` as a replaceable object, an
//! `Actions` handler hook and a do-nothing action (write `CallbackAction::new(|_cx, _| ())`
//! until the propagation-control use case arrives). A `Shortcuts` resolves
//! from the primary focus's position, the chain each `Focus` records on its
//! node (ADR-0079).

use std::any::{Any, TypeId};
use std::rc::Rc;

use flui_foundation::ChangeNotifier;
use flui_interaction::focus::{
    FocusManager, FocusNode, FocusNodeRegistration, KeyEventResult, NodeContext,
};
use flui_platform::TargetPlatform;
use flui_view::element::ElementKind;

use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome, Provided, ProvidedValue};

use crate::support::{RefCallback, ref_callback};

/// A marker for "something the user wants to happen".
/// Carries the operation's parameters; an [`Action`] bound to its type
/// performs it.
///
/// ```rust
/// # use flui_widgets::Intent;
/// struct SaveIntent;
/// impl Intent for SaveIntent {}
/// ```
///
/// A behavioural widget's gesture performs an `Intent`, never a bare
/// callback, so its keyboard, assistive-technology and single-pointer paths
/// can perform the same one ([`GestureCommand`](crate::GestureCommand),
/// ADR-0216): passing a closure where an intent is expected is a compile
/// error that says so.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an `Intent`",
    label = "a gesture performs an `Intent`, not a callback",
    note = "declare the operation as a type (`struct Archive; impl Intent for Archive {{}}`) and bind what it does with `Actions`, so its key, assistive-technology and pointer paths perform the same intent"
)]
pub trait Intent: Any {}

/// What an [`Action::invoke`] did, threaded through to
/// [`Action::to_key_event_result`], where `NextFocusAction` maps "focus
/// actually moved" onto the key result.
///
/// What a key-dispatched action can say is *whether it did anything*. (ADR-0023 dropped the
/// return value entirely — "until a non-key caller needs one". The Tab
/// intents are that caller, and ADR-0026's review chose the breaking
/// signature over a second parallel method: two methods that must agree is a
/// permanent hazard, a break today is one afternoon. Prime Directive #2.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ActionOutcome {
    /// The action ran and did its thing.
    #[default]
    Performed,
    /// The action ran but changed nothing — a `Tab` at a `Stop` edge with
    /// nowhere to go. On the key path this reports the event **unconsumed**,
    /// so an outer handler may still see it.
    NotPerformed,
}

/// Performs the operation an intent of type `T` describes.
pub trait Action<T: Intent> {
    /// Whether this action can run for `intent` right now. A
    /// disabled action stops resolution **at this scope** — it does not fall
    /// through to an enclosing [`Actions`] scope's mapping for the same
    /// intent type: the
    /// search stops the moment a scope declares the type, enabled or not.
    fn is_enabled(&self, intent: &T) -> bool {
        let _ = intent;
        true
    }

    /// Notifies after [`is_enabled`](Self::is_enabled) may have started
    /// answering differently, with no rebuild in between: an action whose
    /// enablement follows state outside the widget tree (a page view's
    /// position) returns the notifier that state changes through.
    ///
    /// What listens is the semantics layer: it offers a gesture's custom
    /// accessibility action only while its action is enabled, and describes the
    /// node again on this signal, so the action appears or goes without
    /// the view rebuilding.
    ///
    /// `None`, the default, means enablement never changes after the action is
    /// bound: it is a fact about the action, not a forgotten override. It is
    /// read once, when the action is bound with [`Actions::action`].
    fn enablement_changes(&self) -> Option<ChangeNotifier> {
        None
    }

    /// Perform the operation inside the dispatch that resolved it:
    /// `cx` is the key event's [`EventCx`], so an action writes signals the
    /// same way every other event callback does (ADR-0086). The
    /// [`ActionOutcome`] reaches the key path through
    /// [`to_key_event_result`](Self::to_key_event_result).
    fn invoke(&self, cx: &mut EventCx<'_>, intent: &T) -> ActionOutcome;

    /// What a key event that invoked this action reports.
    ///
    /// **The only method that answers this**, and it reads the outcome, so it
    /// cannot contradict what `invoke` actually did. (Two methods that had to
    /// agree would disagree the moment an action's key result depends on its
    /// work, as the focus-traversal actions' does.)
    ///
    /// The default consumes the key when the action did something, and reports
    /// it unconsumed when the action declined — so an action that changed
    /// nothing lets the event keep bubbling.
    fn to_key_event_result(&self, intent: &T, outcome: ActionOutcome) -> KeyEventResult {
        let _ = intent;
        match outcome {
            ActionOutcome::Performed => KeyEventResult::Handled,
            ActionOutcome::NotPerformed => KeyEventResult::SkipRemainingHandlers,
        }
    }
}

/// An [`Action`] from a closure.
///
/// The closure receives the key event's [`EventCx`] and the intent, and may
/// return `()` or a `Result` whose refusal is reported at the dispatch
/// boundary (ADR-0086):
///
/// ```rust
/// # use flui_view::{Signal, SignalWriteExt};
/// # use flui_widgets::{CallbackAction, Intent};
/// struct SaveIntent;
/// impl Intent for SaveIntent {}
///
/// fn save_action(saves: Signal<u32>) -> CallbackAction<SaveIntent> {
///     CallbackAction::new(move |cx, _: &SaveIntent| saves.update(cx, |n| *n += 1))
/// }
/// ```
pub struct CallbackAction<T: Intent> {
    on_invoke: RefCallback<T>,
}

impl<T: Intent> CallbackAction<T> {
    /// An always-enabled, key-consuming action calling `on_invoke`.
    pub fn new<F, R>(on_invoke: F) -> Self
    where
        F: for<'a> Fn(&mut EventCx<'_>, &'a T) -> R + 'static,
        R: EventOutcome,
    {
        Self {
            on_invoke: ref_callback(on_invoke),
        }
    }
}

impl<T: Intent> std::fmt::Debug for CallbackAction<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackAction")
            .field("intent", &std::any::type_name::<T>())
            .finish_non_exhaustive()
    }
}

impl<T: Intent> Action<T> for CallbackAction<T> {
    fn invoke(&self, cx: &mut EventCx<'_>, intent: &T) -> ActionOutcome {
        (self.on_invoke)(cx, intent);
        ActionOutcome::Performed
    }
}

// ============================================================================
// Erasure
// ============================================================================

/// How one action type answers one intent type, with both erased: the
/// functions an [`ErasedAction`] reaches its action through. One table
/// exists per pair of types, so binding an action allocates nothing beyond
/// the action itself, and one action bound to several intent types is held
/// once.
struct BindingTable {
    is_enabled: fn(&dyn Any, &dyn Any) -> bool,
    invoke: fn(&dyn Any, &mut EventCx<'_>, &dyn Any) -> ActionOutcome,
    to_key_event_result: fn(&dyn Any, &dyn Any, ActionOutcome) -> KeyEventResult,
}

/// Names the [`BindingTable`] of action type `A` for intent type `T`.
struct Binding<T, A>(std::marker::PhantomData<fn(&T, &A)>);

impl<T: Intent, A: Action<T> + 'static> Binding<T, A> {
    const TABLE: BindingTable = BindingTable {
        is_enabled: |action, intent| bound::<A>(action).is_enabled(typed::<T>(intent)),
        invoke: |action, cx, intent| bound::<A>(action).invoke(cx, typed::<T>(intent)),
        to_key_event_result: |action, intent, outcome| {
            bound::<A>(action).to_key_event_result(typed::<T>(intent), outcome)
        },
    };
}

/// One `Action<T>` behind `dyn Any` intents, so a heterogeneous map can hold
/// it. Reached only through the matching `TypeId`, so the inner downcast
/// cannot fail.
#[derive(Clone)]
pub(crate) struct ErasedAction {
    action: Rc<dyn Any>,
    table: &'static BindingTable,
    enablement_changes: Option<ChangeNotifier>,
}

/// The typed view of an erased intent. Reached only through the matching
/// `TypeId`, so the downcast cannot fail.
fn typed<T: Intent>(intent: &dyn Any) -> &T {
    let typed = intent.downcast_ref::<T>(); // ADR-0023 — keyed by this intent's TypeId, so only a `T` arrives; same shape as the sanctioned Navigator pop-result boundary.
    typed.expect(
        "BUG: an ErasedAction received an intent of a foreign type; \
         the Actions map must be keyed by the intent's TypeId",
    )
}

/// The typed view of an erased action. A table is paired only with the
/// action type it was made for, so the downcast cannot fail.
fn bound<A: 'static>(action: &dyn Any) -> &A {
    action
        .downcast_ref::<A>()
        .expect("BUG: a binding table was paired with an action of another type")
}

impl ErasedAction {
    fn new<T: Intent, A: Action<T> + 'static>(action: A) -> Self {
        Self::shared::<T, A>(Rc::new(action))
    }

    /// `action` bound to `T` without a copy of its own: the bindings of one
    /// action to several intent types share it.
    fn shared<T: Intent, A: Action<T> + 'static>(action: Rc<A>) -> Self {
        let enablement_changes = action.enablement_changes();
        Self {
            action,
            table: &Binding::<T, A>::TABLE,
            enablement_changes,
        }
    }

    pub(crate) fn is_enabled(&self, intent: &dyn Any) -> bool {
        (self.table.is_enabled)(&*self.action, intent)
    }

    /// The signal [`Action::enablement_changes`] gave when this was bound.
    pub(crate) fn enablement_changes(&self) -> Option<&ChangeNotifier> {
        self.enablement_changes.as_ref()
    }

    /// Invoke outside a key dispatch: a gesture, a control's press, an
    /// assistive-technology action.
    pub(crate) fn invoke(&self, cx: &mut EventCx<'_>, intent: &dyn Any) -> ActionOutcome {
        (self.table.invoke)(&*self.action, cx, intent)
    }

    /// Invoke inside the key event's dispatch, then report what the key
    /// dispatch should do — **one** call, so the key result cannot disagree
    /// with what actually ran.
    pub(crate) fn invoke_for_key(&self, cx: &mut EventCx<'_>, intent: &dyn Any) -> KeyEventResult {
        let outcome = (self.table.invoke)(&*self.action, cx, intent);
        (self.table.to_key_event_result)(&*self.action, intent, outcome)
    }
}

/// The bindings in force at one position of the tree: the nearest scope's
/// own, then each enclosing scope's.
///
/// A scope adds one link that shares its binding list and names the chain
/// above it, so layering copies nothing however many bindings the enclosing
/// scopes hold. A lookup walks the links outward and stops at the first that
/// declares the type: a nearer scope's mapping entirely replaces an
/// enclosing scope's mapping for the same type; there is no fallback list to
/// search past it.
#[derive(Clone)]
pub(crate) struct ActionChain(Rc<ChainLink>);

struct ChainLink {
    own: Rc<Vec<(TypeId, ErasedAction)>>,
    enclosing: Option<ActionChain>,
}

impl ActionChain {
    /// The action the nearest declaring scope binds to `intent_type`,
    /// enabled or not. A scope that binds the type twice answers with the
    /// later binding.
    pub(crate) fn get(&self, intent_type: TypeId) -> Option<&ErasedAction> {
        let mut link: &ChainLink = &self.0;
        loop {
            let bound = link
                .own
                .iter()
                .rev()
                .find_map(|(bound, action)| (*bound == intent_type).then_some(action));
            if bound.is_some() {
                return bound;
            }
            link = &link.enclosing.as_ref()?.0;
        }
    }

    /// Whether any scope of the chain declares `intent_type`.
    pub(crate) fn declares(&self, intent_type: TypeId) -> bool {
        self.get(intent_type).is_some()
    }

    /// Whether `self` and `other` are one chain: the same link, not merely
    /// equal bindings.
    pub(crate) fn is(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ActionChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut links = f.debug_list();
        let mut link: Option<&ChainLink> = Some(&self.0);
        while let Some(current) = link {
            links.entry(&current.own.len());
            link = current.enclosing.as_ref().map(|chain| &*chain.0);
        }
        links.finish()
    }
}

/// The action bound to `intent`'s type, if its nearest declaring scope's
/// mapping is enabled. The walk **stops** the moment a scope declares the
/// type, whether or not that mapping is enabled. A
/// disabled nearest mapping therefore returns `None` here rather than
/// falling through to an enclosing scope's mapping for the same type.
pub(crate) fn resolve<'c>(chain: &'c ActionChain, intent: &dyn Any) -> Option<&'c ErasedAction> {
    let action = chain.get(intent.type_id())?;
    action.is_enabled(intent).then_some(action)
}

// ============================================================================
// The widget
// ============================================================================

/// Binds intent types to [`Action`]s for a subtree.
///
/// Resolution stops at the **nearest** scope that declares a mapping for the
/// intent's type — enabled or not: a disabled
/// nearer mapping is not skipped in favor of an enclosing scope's mapping for
/// the same type. A [`Shortcuts`](crate::Shortcuts) dispatches into it from
/// the keyboard, handing the action the key event's [`EventCx`]. There is no
/// invoke-from-build entry point: the only place it could be called from is
/// `build`, which has no event context (ADR-0086).
#[derive(Clone)]
pub struct Actions {
    /// Shared, so a clone of the view and the chain link it publishes copy
    /// no list.
    own: Rc<Vec<(TypeId, ErasedAction)>>,
    child: BoxedView,
}

impl Actions {
    /// An action scope around `child` with no bindings yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            own: Rc::default(),
            child: BoxedView::new(child.into_view()),
        }
    }

    /// Bind intent type `T` to `action`. A binding nearer the invoker
    /// entirely shadows an enclosing one for the same type — even when
    /// `action` turns out disabled, since resolution stops at the
    /// nearest declaring scope regardless of its enabled state.
    #[must_use]
    pub fn action<T: Intent>(mut self, action: impl Action<T> + 'static) -> Self {
        Rc::make_mut(&mut self.own).push(erased_action(action));
        self
    }
}

impl std::fmt::Debug for Actions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actions")
            .field("bindings", &self.own.len())
            .finish_non_exhaustive()
    }
}

impl View for Actions {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Actions {
    /// Layer this widget's bindings over the enclosing chain: a lookup stops
    /// at the nearest scope that declares the type at all, so a type this
    /// widget declares is answered here and any other by the chain above.
    /// If `own` binds the same type twice, the later call wins, same as a
    /// duplicate key in a map literal.
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }

    fn provide(&self, ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        values.value(EnclosingActions(layered_chain(
            ambient_action_chain(ctx),
            Rc::clone(&self.own),
        )));
    }
}

/// `own` layered over `enclosing`: each of `own`'s types replaces the
/// enclosing entry for that type, and every other type keeps it. Later
/// entries in `own` win over earlier ones for the same type.
pub(crate) fn layered_chain(
    enclosing: Option<ActionChain>,
    own: Rc<Vec<(TypeId, ErasedAction)>>,
) -> ActionChain {
    ActionChain(Rc::new(ChainLink { own, enclosing }))
}

/// `action` as one entry of a chain, keyed by `T`.
pub(crate) fn erased_action<T: Intent>(action: impl Action<T> + 'static) -> (TypeId, ErasedAction) {
    (TypeId::of::<T>(), ErasedAction::new(action))
}

/// `action` as one entry of a chain, keyed by `T`, sharing the action with
/// the entries that bind it to other intent types.
pub(crate) fn shared_action<T: Intent, A: Action<T> + 'static>(
    action: &Rc<A>,
) -> (TypeId, ErasedAction) {
    (
        TypeId::of::<T>(),
        ErasedAction::shared::<T, A>(Rc::clone(action)),
    )
}

/// What a focused widget records on its node for the `Shortcuts` above it:
/// the action chain at its position, where an intent is resolved, and the
/// platform convention it was built under, which picks the chords of the
/// platform-dependent bindings.
#[derive(Clone)]
pub(crate) struct FocusRecord {
    chain: ActionChain,
    platform: TargetPlatform,
}

impl FocusRecord {
    /// The record of a widget built under `platform` with `chain` above it.
    pub(crate) fn new(chain: ActionChain, platform: TargetPlatform) -> Self {
        Self { chain, platform }
    }

    /// The record of a widget at `ctx` with `chain` above it.
    pub(crate) fn at(ctx: &dyn BuildContext, chain: ActionChain) -> Self {
        Self::new(chain, flui_view::environment::PlatformConvention::of(ctx))
    }

    /// The platform convention the recording widget was built under.
    pub(crate) fn platform(&self) -> TargetPlatform {
        self.platform
    }

    /// Whether `self` and `other` record the same chain and convention, so
    /// recording one over the other changes nothing.
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        self.chain.is(&other.chain) && self.platform == other.platform
    }

    /// Record `self` on `node`, until the returned registration drops.
    pub(crate) fn register_on(&self, node: &Rc<FocusNode>) -> FocusNodeRegistration {
        node.register_context(Rc::new(self.clone()) as NodeContext)
    }
}

/// The record a focused widget left on `node`. `None` for a node no `Focus`
/// or text field hosts, or one with no `Actions` above it.
fn record_at(node: &FocusNode) -> Option<Rc<FocusRecord>> {
    node.context()?.downcast::<FocusRecord>().ok()
}

/// The chain recorded on `node`: the bindings visible at the focused widget's
/// position, which is where a shortcut's intent is resolved.
pub(crate) fn chain_at(node: &FocusNode) -> Option<ActionChain> {
    record_at(node).map(|record| record.chain.clone())
}

/// The platform convention the widget that recorded on `node` was built
/// under.
pub(crate) fn platform_at(node: &FocusNode) -> Option<TargetPlatform> {
    record_at(node).map(|record| record.platform)
}

/// The nearest provider's chain, if any. Resolved at call time, so late reads
/// (a key handler built earlier) still see the tree's current bindings only if
/// they re-read — which is why `Focus` records the chain with a dependency and
/// `Shortcuts` reads the primary focus's record at key time (ADR-0079).
pub(crate) fn ambient_action_chain(ctx: &dyn BuildContext) -> Option<ActionChain> {
    ctx.get::<EnclosingActions, _>(|enclosing| enclosing.0.clone())
}

/// The layered [`ActionChain`] as an element publishes it to its subtree
/// (ADR-0322 §5). Private: `Actions` is the public surface.
#[derive(Clone)]
pub(crate) struct EnclosingActions(ActionChain);

impl EnclosingActions {
    /// `chain`, to publish from an element that layers its own bindings.
    pub(crate) fn new(chain: ActionChain) -> Self {
        Self(chain)
    }

    /// The chain in force at `ctx`, registering a dependency: the reader
    /// rebuilds when an enclosing scope layers another chain.
    pub(crate) fn depend_on(ctx: &dyn BuildContext) -> Option<ActionChain> {
        ctx.depend_on::<Self, _>(|enclosing| enclosing.0.clone())
    }
}

impl std::fmt::Debug for EnclosingActions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnclosingActions")
            .field("chain", &self.0)
            .finish()
    }
}

impl ProvidedValue for EnclosingActions {
    /// Rebuilding an `Actions` mints a fresh chain, so dependents (a
    /// `Shortcuts` that captured the chain into its key handler) re-capture.
    fn update_should_notify(&self, old: &Self) -> bool {
        !self.0.is(&old.0)
    }
}

/// Publishes `chain` to `child`: a widget that keeps its own layered chain
/// (a selection area, whose menu items perform through it) provides it as an
/// `Actions` would.
#[derive(Clone)]
pub(crate) struct ActionScope {
    chain: ActionChain,
    child: BoxedView,
}

impl ActionScope {
    pub(crate) fn new(chain: ActionChain, child: impl IntoView) -> Self {
        Self {
            chain,
            child: BoxedView::new(child.into_view()),
        }
    }
}

impl std::fmt::Debug for ActionScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionScope")
            .field("chain", &self.chain)
            .finish_non_exhaustive()
    }
}

impl View for ActionScope {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for ActionScope {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }

    fn provide(&self, _ctx: &dyn BuildContext, values: &mut Provided<'_>) {
        values.value(EnclosingActions(self.chain.clone()));
    }
}
// ============================================================================
// Activation intents (ADR-0079)
// ============================================================================

/// "Activate the focused control" — what the default bindings send for Enter
/// and Select on key-down and for Space on its release. A control answers it with an
/// [`Actions`] binding around its `Focus`; nothing answers it at the root, so
/// an unclaimed activation key keeps bubbling.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActivateIntent;
impl Intent for ActivateIntent {}

/// "Activate this button" — the variant a
/// button answers the same way as [`ActivateIntent`], so an app can bind a
/// key to buttons alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonActivateIntent;
impl Intent for ButtonActivateIntent {}

/// "The activation key went down" — Space's key-down, whose release is the
/// [`ActivateIntent`].
///
/// A control answers it by consuming the key-down (and may show itself
/// pressed until the activation or a focus change), so the press and the
/// release that activates belong to the same control. A control that does
/// not answer it lets the key-down bubble; its activation still comes with
/// the release.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActivationPressIntent;
impl Intent for ActivationPressIntent {}

/// "Open the focused link" — what a paragraph with links binds Enter and
/// Select to on key-down, around its links (ADR-0079, amendment for links).
///
/// A link answers it with its paragraph's link handler
/// ([`RichText::on_link`](crate::RichText::on_link)). Space is not bound: its
/// release is [`ActivateIntent`], which a link declines, because a link is
/// not a button.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActivateLinkIntent;
impl Intent for ActivateLinkIntent {}

/// "Go back one page" — Alt+Left and `BrowserBack` (Cmd+[ and `BrowserBack`
/// on Apple platforms), and the mouse's back button.
///
/// A navigator answers it by popping its current page; with nothing to pop it
/// declines, and the key reaches the host.
#[derive(Debug, Clone, Copy, Default)]
pub struct BackIntent;
impl Intent for BackIntent {}

/// "Dismiss what is on top" — Escape.
///
/// Only a dismissible surface (a modal route, a menu) answers it; a text field
/// consumes it without effect, so Escape typed into a field closes nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct DismissIntent;
impl Intent for DismissIntent {}

/// "Show the focused item's context menu" — the Menu key and Shift+F10
/// outside Apple platforms, which have no menu key.
///
/// A [`GestureAlternatives`](crate::GestureAlternatives) whose single-pointer
/// alternative is a context menu answers it by opening that menu; with
/// nothing answering it, the key reaches the host.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShowContextMenuIntent;
impl Intent for ShowContextMenuIntent {}

// ============================================================================
// Focus traversal intents (ADR-0026)
// ============================================================================

/// "Move focus to the next widget" — what `WidgetsApp` binds Tab to.
#[derive(Debug, Clone, Copy, Default)]
pub struct NextFocusIntent;
impl Intent for NextFocusIntent {}

/// "Move focus to the previous widget", bound to Shift+Tab.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreviousFocusIntent;
impl Intent for PreviousFocusIntent {}

/// Advances the focus through the active scope's traversal order.
///
/// The key result is **what the traversal did**: `Handled` when focus moved,
/// `SkipRemainingHandlers` when it did not (a `Stop` edge with nowhere to go),
/// so an unmoved Tab keeps bubbling instead of being swallowed (`:2340-2348`).
#[derive(Debug, Clone)]
pub struct NextFocusAction {
    focus_manager: Rc<FocusManager>,
}

impl NextFocusAction {
    /// Bind traversal to one presentation's focus owner.
    #[must_use]
    pub fn new(focus_manager: Rc<FocusManager>) -> Self {
        Self { focus_manager }
    }
}

impl Action<NextFocusIntent> for NextFocusAction {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &NextFocusIntent) -> ActionOutcome {
        if self.focus_manager.focus_next() {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Steps the focus backwards. Same result contract as
/// [`NextFocusAction`].
#[derive(Debug, Clone)]
pub struct PreviousFocusAction {
    focus_manager: Rc<FocusManager>,
}

impl PreviousFocusAction {
    /// Bind reverse traversal to one presentation's focus owner.
    #[must_use]
    pub fn new(focus_manager: Rc<FocusManager>) -> Self {
        Self { focus_manager }
    }
}

impl Action<PreviousFocusIntent> for PreviousFocusAction {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &PreviousFocusIntent) -> ActionOutcome {
        if self.focus_manager.focus_previous() {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

// ============================================================================
// Text editing intents (ADR-0023)
// ============================================================================

/// Move focus along one geometric direction after the focused control declines the key.
#[derive(Debug, Clone, Copy)]
pub struct DirectionalFocusIntent(pub flui_interaction::FocusDirection);
impl Intent for DirectionalFocusIntent {}

/// Directional navigation bound to one presentation's focus owner.
#[derive(Debug, Clone)]
pub struct DirectionalFocusAction {
    focus_manager: Rc<FocusManager>,
}
impl DirectionalFocusAction {
    /// Bind directional traversal to this presentation.
    #[must_use]
    pub fn new(focus_manager: Rc<FocusManager>) -> Self {
        Self { focus_manager }
    }
}
impl Action<DirectionalFocusIntent> for DirectionalFocusAction {
    fn invoke(&self, _cx: &mut EventCx<'_>, intent: &DirectionalFocusIntent) -> ActionOutcome {
        if self.focus_manager.focus_in_direction(intent.0) {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Copy or cut the focused field's selection, or the focused draggable item.
/// [`DefaultFocusTraversal`](super::shortcuts::DefaultFocusTraversal)
/// binds Ctrl+C/Ctrl+X (Cmd on Apple platforms); an `EditableText` answers it
/// on its own focus node, and a [`Draggable`](crate::Draggable) by holding its
/// items for a paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopySelectionTextIntent {
    /// Write the selection to the clipboard and keep it.
    Copy,
    /// Write the selection to the clipboard and delete it.
    Cut,
}
impl Intent for CopySelectionTextIntent {}

/// Paste the clipboard's text over the focused field's selection, or the
/// held drag into the focused [`DropTarget`](crate::DropTarget); bound to
/// Ctrl+V (Cmd+V on Apple platforms).
#[derive(Debug, Clone, Copy, Default)]
pub struct PasteTextIntent;
impl Intent for PasteTextIntent {}

/// Select all text in the focused field without changing its contents.
/// Bound to Ctrl+A (Cmd+A on Apple platforms) by [`DefaultFocusTraversal`](super::shortcuts::DefaultFocusTraversal).
/// An active IME composition retains ownership of its selection.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectAllTextIntent;
impl Intent for SelectAllTextIntent {}
