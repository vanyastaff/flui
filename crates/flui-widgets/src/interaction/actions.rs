//! [`Intent`], [`Action`], [`CallbackAction`] and the [`Actions`] widget — the
//! typed command layer `Shortcuts` dispatches into.
//!
//! ADR-0023.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/actions.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `Intent` (`:64`), `Action<T>` (`:135`),
//! `CallbackAction<T>` (`:606`), `Actions` (`:729`), the ancestor resolution
//! (`_visitActionsAncestors`, `:759-790`; `maybeInvoke`, `:1032-1044`).
//!
//! # The Rust shape (ADR-0023)
//!
//! Flutter keys its map by the intent's runtime `Type` and walks
//! `_ActionsScope` ancestors at invoke time, **stopping at the first scope
//! whose own map declares the intent's type at all** — enabled or not
//! (`_castAction`/`_visitActionsAncestors`, `:736-753`, `:920-931`).
//! `maybeInvoke`'s own doc is explicit: "If a suitable Action is found but its
//! `isEnabled` returns false, the search will stop" (`:993-995`) — a disabled
//! mapping does **not** fall through to an enclosing scope's mapping for the
//! same type. FLUI keys by [`TypeId`] and **chains at provide time**: each
//! `Actions` widget layers its own map over the enclosing chain, so one
//! nearest-provider lookup sees, per intent type, the single entry Flutter's
//! walk would stop at — the nearest declaring scope's action, whether enabled
//! or not.
//!
//! The erasure (`TypeId` key + `dyn Any` downcast inside the typed wrapper)
//! is the same shape as the one sanctioned `Navigator` pop-result boundary
//! (ADR-0019): the downcast can only be reached through the matching
//! `TypeId`, so it cannot fail.
//!
//! # Deferred, and named
//!
//! `ActionDispatcher` as a replaceable object, `Action.addActionListener`,
//! `Actions.handler` and `DoNothingAction` (write `CallbackAction::new(|_| ())`
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

/// A marker for "something the user wants to happen" — Flutter's `Intent`
/// (`actions.dart:64`). Carries the operation's parameters; an [`Action`]
/// bound to its type performs it.
///
/// ```rust
/// # use flui_widgets::Intent;
/// struct SaveIntent;
/// impl Intent for SaveIntent {}
/// ```
pub trait Intent: Any {}

/// What an [`Action::invoke`] did — Flutter threads `invoke`'s return value
/// through to `Action.toKeyEventResult` (`actions.dart:312-314`), where
/// `NextFocusAction` maps "focus actually moved" onto the key result
/// (`focus_traversal.dart:2340-2348`).
///
/// FLUI keeps the channel but not Dart's `Object?`: what a key-dispatched
/// action can say is *whether it did anything*. (ADR-0023 dropped the
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

/// Performs the operation an intent of type `T` describes — Flutter's
/// `Action<T>` (`actions.dart:135`).
pub trait Action<T: Intent> {
    /// Whether this action can run for `intent` right now (`:267`). A
    /// disabled action stops resolution **at this scope** — it does not fall
    /// through to an enclosing [`Actions`] scope's mapping for the same
    /// intent type, matching `maybeInvoke`'s documented contract that the
    /// search stops the moment a scope declares the type, enabled or not
    /// (`actions.dart:993-995`).
    fn is_enabled(&self, intent: &T) -> bool {
        let _ = intent;
        true
    }

    /// Perform the operation (`:354`). The [`ActionOutcome`] reaches the key
    /// path through [`to_key_event_result`](Self::to_key_event_result); a
    /// direct `Actions::maybe_invoke` caller may ignore it.
    fn invoke(&self, intent: &T) -> ActionOutcome;

    /// What a key event that invoked this action reports —
    /// `Action.toKeyEventResult` (`actions.dart:312-314`).
    ///
    /// **The only method that answers this**, and it reads the outcome, so it
    /// cannot contradict what `invoke` actually did. (Flutter splits the
    /// question across `consumesKey` and `toKeyEventResult`; the two disagree
    /// the moment an action's key result depends on its work — as the
    /// focus-traversal actions' does, `focus_traversal.dart:2340-2348`.)
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

/// An [`Action`] from a closure — Flutter's `CallbackAction<T>`
/// (`actions.dart:606`).
pub struct CallbackAction<T: Intent> {
    on_invoke: Rc<dyn Fn(&T)>,
}

impl<T: Intent> CallbackAction<T> {
    /// An always-enabled, key-consuming action calling `on_invoke`.
    pub fn new(on_invoke: impl Fn(&T) + 'static) -> Self {
        Self {
            on_invoke: Rc::new(on_invoke),
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
    fn invoke(&self, intent: &T) -> ActionOutcome {
        (self.on_invoke)(intent);
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
type ErasedInvoke = Rc<dyn Fn(&dyn Any) -> ActionOutcome>;
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
            invoke: Rc::new(move |intent| action.invoke(typed::<T>(intent))),
            to_key_event_result: Rc::new(move |intent, outcome| {
                key_result_action.to_key_event_result(typed::<T>(intent), outcome)
            }),
        }
    }

    pub(crate) fn is_enabled(&self, intent: &dyn Any) -> bool {
        (self.is_enabled)(intent)
    }

    pub(crate) fn invoke(&self, intent: &dyn Any) -> ActionOutcome {
        (self.invoke)(intent)
    }

    /// Invoke, then report what the key dispatch should do — **one** call, so
    /// the key result cannot disagree with what actually ran.
    pub(crate) fn invoke_for_key(&self, intent: &dyn Any) -> KeyEventResult {
        let outcome = (self.invoke)(intent);
        (self.to_key_event_result)(intent, outcome)
    }
}

/// Per intent type, the action bound by the **nearest** `Actions` scope that
/// declares it — the single entry Flutter's ancestor walk would stop at
/// (`_castAction`/`_visitActionsAncestors`, `actions.dart:736-753`,
/// `:920-931`), precomputed at provide time. A nearer scope's mapping
/// entirely replaces an enclosing scope's mapping for the same type; there is
/// no fallback list to search past it.
pub(crate) type ActionChain = Rc<HashMap<TypeId, ErasedAction>>;

/// The action bound to `intent`'s type, if its nearest declaring scope's
/// mapping is enabled — `Actions.maybeInvoke`'s walk, which **stops** the
/// moment a scope declares the type, whether or not that mapping is enabled
/// (`actions.dart:1032-1044`, doc at `:993-995`: "If a suitable Action is
/// found but its `isEnabled` returns false, the search will stop"). A
/// disabled nearest mapping therefore returns `None` here rather than
/// falling through to an enclosing scope's mapping for the same type.
pub(crate) fn resolve<'c>(chain: &'c ActionChain, intent: &dyn Any) -> Option<&'c ErasedAction> {
    let action = chain.get(&intent.type_id())?;
    action.is_enabled(intent).then_some(action)
}

// ============================================================================
// The widget
// ============================================================================

/// Binds intent types to [`Action`]s for a subtree — Flutter's `Actions`
/// (`actions.dart:729`).
///
/// Resolution stops at the **nearest** scope that declares a mapping for the
/// intent's type — enabled or not — exactly as Flutter's ancestor walk
/// resolves and its own doc states (`:1032-1044`, `:993-995`): a disabled
/// nearer mapping is not skipped in favor of an enclosing scope's mapping for
/// the same type. Invoke with [`Actions::maybe_invoke`] from build-time code,
/// or let a [`Shortcuts`](crate::Shortcuts) dispatch into it from the
/// keyboard.
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
    /// `action` turns out disabled, since Flutter's walk stops at the
    /// nearest declaring scope regardless of its enabled state.
    #[must_use]
    pub fn action<T: Intent>(mut self, action: impl Action<T> + 'static) -> Self {
        self.own
            .push((TypeId::of::<T>(), ErasedAction::new(action)));
        self
    }

    /// Resolve `intent`'s type to its nearest declaring scope's action and, if
    /// enabled, invoke it. Returns whether one ran. Flutter's
    /// `Actions.maybeInvoke` (`actions.dart:1032-1044`), minus the result
    /// value (dropped on the key path anyway, `:348-349`).
    pub fn maybe_invoke<T: Intent>(ctx: &dyn BuildContext, intent: &T) -> bool {
        let Some(chain) = ambient_action_chain(ctx) else {
            return false;
        };
        match resolve(&chain, intent) {
            Some(action) => {
                action.invoke(intent);
                true
            }
            None => false,
        }
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
    /// mapping is the only one a lookup ever sees — matching Flutter's walk,
    /// which stops at the nearest scope that declares the type at all
    /// (`actions.dart:920-931`). A type this widget does not declare keeps
    /// falling back to whatever the enclosing chain already had. If `own`
    /// binds the same type twice, the later call wins, same as a duplicate
    /// key in Flutter's `Map<Type, Action<Intent>>` literal.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let mut chain: HashMap<TypeId, ErasedAction> = ambient_action_chain(ctx)
            .map(|enclosing| (*enclosing).clone())
            .unwrap_or_default();
        for (type_id, action) in &self.own {
            chain.insert(*type_id, action.clone());
        }
        ActionChainProvider {
            chain: Rc::new(chain),
            child: self.child.clone(),
        }
    }
}

/// The chain a focused [`Focus`](super::focus::Focus) recorded on its node:
/// the bindings visible at the focused widget's position, which is where
/// Flutter resolves a shortcut's intent (`primaryFocus.context`,
/// `shortcuts.dart`'s `ShortcutManager.handleKeypress`). `None` for a node no
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

/// "Activate the focused control" — Flutter's `ActivateIntent`
/// (`actions.dart`), what `WidgetsApp` binds Enter, Space and Select to
/// (`app.dart:1265-1269`, tag `3.44.0`). A control answers it with an
/// [`Actions`] binding around its `Focus`; nothing answers it at the root, so
/// an unclaimed activation key keeps bubbling.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActivateIntent;
impl Intent for ActivateIntent {}

/// "Activate this button" — Flutter's `ButtonActivateIntent`, the variant a
/// button answers the same way as [`ActivateIntent`], so an app can bind a
/// key to buttons alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonActivateIntent;
impl Intent for ButtonActivateIntent {}

// ============================================================================
// Focus traversal intents (ADR-0026)
// ============================================================================

/// "Move focus to the next widget" — Flutter's `NextFocusIntent`
/// (`focus_traversal.dart:2320`), what `WidgetsApp` binds Tab to
/// (`app.dart:1275`).
#[derive(Debug, Clone, Copy, Default)]
pub struct NextFocusIntent;
impl Intent for NextFocusIntent {}

/// "Move focus to the previous widget" — `PreviousFocusIntent`
/// (`focus_traversal.dart:2360`), bound to Shift+Tab (`app.dart:1276`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PreviousFocusIntent;
impl Intent for PreviousFocusIntent {}

/// Advances the focus through the active scope's traversal order —
/// `NextFocusAction` (`focus_traversal.dart:2330-2348`).
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
    fn invoke(&self, _intent: &NextFocusIntent) -> ActionOutcome {
        if self.focus_manager.focus_next() {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Steps the focus backwards — `PreviousFocusAction`
/// (`focus_traversal.dart:2350-2368`). Same result contract as
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
    fn invoke(&self, _intent: &PreviousFocusIntent) -> ActionOutcome {
        if self.focus_manager.focus_previous() {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}
