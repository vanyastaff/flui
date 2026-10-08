//! [`Intent`], [`Action`], [`CallbackAction`] and the [`Actions`] widget — the
//! typed command layer `Shortcuts` dispatches into.
//!
//! ADR-0023.
//!
//! # Resolution (ADR-0023)
//!
//! Resolution **stops at the first scope whose own map declares the intent's
//! type at all** — enabled or not. A disabled mapping does **not** fall
//! through to an enclosing scope's mapping for the same type. Actions are
//! keyed by [`TypeId`] and **chained at provide time**: each `Actions` widget
//! layers its own map over the enclosing chain, so one nearest-provider lookup
//! sees, per intent type, the single entry of the nearest declaring scope,
//! whether enabled or not.
//!
//! The erasure (`TypeId` key + `dyn Any` downcast inside the typed wrapper)
//! is the same shape as the one sanctioned `Navigator` pop-result boundary
//! (ADR-0019): the downcast can only be reached through the matching
//! `TypeId`, so it cannot fail.
//!
//! # Deferred, and named
//!
//! An `ActionDispatcher` as a replaceable object, action listeners, an
//! `Actions` handler hook and a do-nothing action (write `CallbackAction::new(|_cx, _| ())`
//! until the propagation-control use case arrives). A `Shortcuts` resolves
//! from the primary focus's position, the chain each `Focus` records on its
//! node (ADR-0079).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

use flui_interaction::routing::{FocusManager, FocusNode, KeyEventResult, NodeContext};
use flui_view::element::ElementKind;
use flui_view::impl_inherited_view;
use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome};

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

/// One `Action<T>` behind `dyn Any` intents, so a heterogeneous map can hold
/// it. Reached only through the matching `TypeId`, so the inner downcast
/// cannot fail.
/// A predicate over an erased intent (`is_enabled`).
type ErasedPredicate = Rc<dyn Fn(&dyn Any) -> bool>;
/// The erased `invoke`, carrying its outcome back out.
type ErasedInvoke = Rc<dyn Fn(&mut EventCx<'_>, &dyn Any) -> ActionOutcome>;
/// The erased `to_key_event_result`.
type ErasedKeyResult = Rc<dyn Fn(&dyn Any, ActionOutcome) -> KeyEventResult>;

#[derive(Clone)]
pub(crate) struct ErasedAction {
    is_enabled: ErasedPredicate,
    invoke: ErasedInvoke,
    to_key_event_result: ErasedKeyResult,
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

impl ErasedAction {
    fn new<T: Intent, A: Action<T> + 'static>(action: A) -> Self {
        let action = Rc::new(action);
        let enabled_action = Rc::clone(&action);
        let key_result_action = Rc::clone(&action);
        Self {
            is_enabled: Rc::new(move |intent| enabled_action.is_enabled(typed::<T>(intent))),
            invoke: Rc::new(move |cx: &mut EventCx<'_>, intent: &dyn Any| {
                action.invoke(cx, typed::<T>(intent))
            }),
            to_key_event_result: Rc::new(move |intent, outcome| {
                key_result_action.to_key_event_result(typed::<T>(intent), outcome)
            }),
        }
    }

    pub(crate) fn is_enabled(&self, intent: &dyn Any) -> bool {
        (self.is_enabled)(intent)
    }

    /// Invoke inside the key event's dispatch, then report what the key
    /// dispatch should do — **one** call, so the key result cannot disagree
    /// with what actually ran.
    pub(crate) fn invoke_for_key(&self, cx: &mut EventCx<'_>, intent: &dyn Any) -> KeyEventResult {
        let outcome = (self.invoke)(cx, intent);
        (self.to_key_event_result)(intent, outcome)
    }
}

/// Per intent type, the action bound by the **nearest** `Actions` scope that
/// declares it — the single entry an ancestor walk would stop at,
/// precomputed at provide time. A nearer scope's mapping
/// entirely replaces an enclosing scope's mapping for the same type; there is
/// no fallback list to search past it.
pub(crate) type ActionChain = Rc<HashMap<TypeId, ErasedAction>>;

/// The action bound to `intent`'s type, if its nearest declaring scope's
/// mapping is enabled. The walk **stops** the moment a scope declares the
/// type, whether or not that mapping is enabled. A
/// disabled nearest mapping therefore returns `None` here rather than
/// falling through to an enclosing scope's mapping for the same type.
pub(crate) fn resolve<'c>(chain: &'c ActionChain, intent: &dyn Any) -> Option<&'c ErasedAction> {
    let action = chain.get(&intent.type_id())?;
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
    own: Vec<(TypeId, ErasedAction)>,
    child: BoxedView,
}

impl Actions {
    /// An action scope around `child` with no bindings yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            own: Vec::new(),
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Bind intent type `T` to `action`. A binding nearer the invoker
    /// entirely shadows an enclosing one for the same type — even when
    /// `action` turns out disabled, since resolution stops at the
    /// nearest declaring scope regardless of its enabled state.
    #[must_use]
    pub fn action<T: Intent>(mut self, action: impl Action<T> + 'static) -> Self {
        self.own.push(erased_action(action));
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
    /// Layer this widget's bindings over the enclosing chain: own actions
    /// **replace** the enclosing entry per type, so the nearest scope's
    /// mapping is the only one a lookup ever sees, since resolution stops at
    /// the nearest scope that declares the type at all. A type this widget does not declare keeps
    /// falling back to whatever the enclosing chain already had. If `own`
    /// binds the same type twice, the later call wins, same as a duplicate
    /// key in a map literal.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        ActionChainProvider {
            chain: layered_chain(ambient_action_chain(ctx), &self.own),
            child: self.child.clone(),
        }
    }
}

/// `own` layered over `enclosing`: each of `own`'s types replaces the
/// enclosing entry for that type, and every other type keeps it. Later
/// entries in `own` win over earlier ones for the same type.
pub(crate) fn layered_chain(
    enclosing: Option<ActionChain>,
    own: &[(TypeId, ErasedAction)],
) -> ActionChain {
    let mut chain: HashMap<TypeId, ErasedAction> = enclosing
        .map(|enclosing| (*enclosing).clone())
        .unwrap_or_default();
    for (type_id, action) in own {
        chain.insert(*type_id, action.clone());
    }
    Rc::new(chain)
}

/// `action` as one entry of a chain, keyed by `T`.
pub(crate) fn erased_action<T: Intent>(action: impl Action<T> + 'static) -> (TypeId, ErasedAction) {
    (TypeId::of::<T>(), ErasedAction::new(action))
}

/// The chain a focused [`Focus`](super::focus::Focus) recorded on its node:
/// the bindings visible at the focused widget's position, which is where
/// a shortcut's intent is resolved. `None` for a node no
/// `Focus` widget hosts, or one with no `Actions` above it.
pub(crate) fn chain_at(node: &FocusNode) -> Option<ActionChain> {
    node.context()?
        .downcast::<HashMap<TypeId, ErasedAction>>()
        .ok()
}

/// The chain as the opaque context a focus node carries ([`chain_at`] reads
/// it back).
pub(crate) fn as_node_context(chain: &ActionChain) -> NodeContext {
    Rc::clone(chain) as NodeContext
}

/// The nearest provider's chain, if any. Resolved at call time, so late reads
/// (a key handler built earlier) still see the tree's current bindings only if
/// they re-read — which is why `Focus` records the chain with a dependency and
/// `Shortcuts` reads the primary focus's record at key time (ADR-0079).
pub(crate) fn ambient_action_chain(ctx: &dyn BuildContext) -> Option<ActionChain> {
    ctx.get::<ActionChainProvider, _>(|provider| Rc::clone(&provider.chain))
}

/// The inherited carrier of the layered [`ActionChain`]. Private: `Actions`
/// is the public surface.
#[derive(Clone)]
pub(crate) struct ActionChainProvider {
    chain: ActionChain,
    child: BoxedView,
}

impl std::fmt::Debug for ActionChainProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionChainProvider")
            .field("intent_types", &self.chain.len())
            .finish_non_exhaustive()
    }
}

impl InheritedView for ActionChainProvider {
    type Data = ActionChain;

    fn data(&self) -> &Self::Data {
        &self.chain
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    /// Rebuilding an `Actions` mints a fresh chain `Arc`, so dependents (a
    /// `Shortcuts` that captured the chain into its key handler) re-capture.
    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.chain, &old.chain)
    }
}

impl_inherited_view!(ActionChainProvider);
// ============================================================================
// Activation intents (ADR-0079)
// ============================================================================

/// "Activate the focused control" — what `WidgetsApp` binds Enter, Space and
/// Select to. A control answers it with an
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

/// Copy or cut the focused field's selection.
/// [`DefaultFocusTraversal`](super::shortcuts::DefaultFocusTraversal)
/// binds Ctrl+C/Ctrl+X (Cmd on Apple platforms); an `EditableText` answers it
/// on its own focus node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopySelectionTextIntent {
    /// Write the selection to the clipboard and keep it.
    Copy,
    /// Write the selection to the clipboard and delete it.
    Cut,
}
impl Intent for CopySelectionTextIntent {}

/// Paste the clipboard's text over the focused field's selection,
/// bound to Ctrl+V (Cmd+V on Apple platforms).
#[derive(Debug, Clone, Copy, Default)]
pub struct PasteTextIntent;
impl Intent for PasteTextIntent {}

/// Select all text in the focused field without changing its contents.
/// Bound to Ctrl+A (Cmd+A on Apple platforms) by [`DefaultFocusTraversal`](super::shortcuts::DefaultFocusTraversal).
/// An active IME composition retains ownership of its selection.
#[derive(Debug, Clone, Copy, Default)]
pub struct SelectAllTextIntent;
impl Intent for SelectAllTextIntent {}
