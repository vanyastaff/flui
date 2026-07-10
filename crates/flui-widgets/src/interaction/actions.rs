//! [`Intent`], [`Action`], [`CallbackAction`] and the [`Actions`] widget — the
//! typed command layer `Shortcuts` dispatches into.
//!
//! ADR-0023 U3.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/actions.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `Intent` (`:64`), `Action<T>` (`:135`),
//! `CallbackAction<T>` (`:606`), `Actions` (`:729`), the ancestor resolution
//! (`_visitActionsAncestors`, `:759-790`; `maybeInvoke`, `:1032-1044`).
//!
//! # The Rust shape (ADR-0023 U3)
//!
//! Flutter keys its map by the intent's runtime `Type` and walks
//! `_ActionsScope` ancestors at invoke time. FLUI keys by [`TypeId`] and
//! **chains at provide time**: each `Actions` widget layers its own map over
//! the enclosing chain, so one nearest-provider lookup sees, per intent type,
//! the same ordered candidates Flutter's walk would visit — nearest first,
//! first *enabled* one wins, a disabled nearer action falls through to an
//! enabled outer one exactly as `maybeInvoke` continues its walk.
//!
//! The erasure (`TypeId` key + `dyn Any` downcast inside the typed wrapper)
//! is the same shape as the one sanctioned `Navigator` pop-result boundary
//! (FR-033/widgets): the downcast can only be reached through the matching
//! `TypeId`, so it cannot fail.
//!
//! # Deferred, and named
//!
//! `ActionDispatcher` as a replaceable object, `Action.addActionListener`,
//! `Actions.handler`, `DoNothingAction` (write `CallbackAction::new(|_| ())`
//! until the propagation-control use case arrives), and invoke-at-primary-focus
//! context resolution (ADR-0023 O-1 — `Shortcuts` resolves from its own
//! position until `FocusNode` records an element).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

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
pub trait Intent: Any + Send + Sync {}

/// Performs the operation an intent of type `T` describes — Flutter's
/// `Action<T>` (`actions.dart:135`).
pub trait Action<T: Intent>: Send + Sync {
    /// Whether this action can run for `intent` right now (`:267`). A
    /// disabled action lets resolution fall through to an enclosing
    /// [`Actions`] scope, as Flutter's walk does.
    fn is_enabled(&self, intent: &T) -> bool {
        let _ = intent;
        true
    }

    /// Whether a key event that invoked this action counts as consumed
    /// (`:297`): `false` maps to
    /// [`SkipRemainingHandlers`](flui_interaction::KeyEventResult::SkipRemainingHandlers)
    /// in `Shortcuts` (`:312-314`).
    fn consumes_key(&self, intent: &T) -> bool {
        let _ = intent;
        true
    }

    /// Perform the operation (`:354`). The return value Flutter threads
    /// through is dropped on the key-dispatch path anyway (`:348-349`); FLUI
    /// omits it until a non-key caller needs one.
    fn invoke(&self, intent: &T);
}

/// An [`Action`] from a closure — Flutter's `CallbackAction<T>`
/// (`actions.dart:606`).
pub struct CallbackAction<T: Intent> {
    on_invoke: Arc<dyn Fn(&T) + Send + Sync>,
}

impl<T: Intent> CallbackAction<T> {
    /// An always-enabled, key-consuming action calling `on_invoke`.
    pub fn new(on_invoke: impl Fn(&T) + Send + Sync + 'static) -> Self {
        Self {
            on_invoke: Arc::new(on_invoke),
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
    fn invoke(&self, intent: &T) {
        (self.on_invoke)(intent);
    }
}

// ============================================================================
// Erasure
// ============================================================================

/// One `Action<T>` behind `dyn Any` intents, so a heterogeneous map can hold
/// it. Reached only through the matching `TypeId`, so the inner downcast
/// cannot fail.
/// A predicate over an erased intent (`is_enabled` / `consumes_key`).
type ErasedPredicate = Arc<dyn Fn(&dyn Any) -> bool + Send + Sync>;
/// The erased `invoke`.
type ErasedInvoke = Arc<dyn Fn(&dyn Any) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct ErasedAction {
    is_enabled: ErasedPredicate,
    consumes_key: ErasedPredicate,
    invoke: ErasedInvoke,
}

/// The typed view of an erased intent. Reached only through the matching
/// `TypeId`, so the downcast cannot fail.
fn typed<T: Intent>(intent: &dyn Any) -> &T {
    let typed = intent.downcast_ref::<T>(); // PORT-CHECK-OK-DOWNCAST: ADR-0023 U3 — keyed by this intent's TypeId, so only a `T` arrives; same shape as the sanctioned Navigator pop-result boundary.
    typed.expect(
        "BUG: an ErasedAction received an intent of a foreign type; \
         the Actions map must be keyed by the intent's TypeId",
    )
}

impl ErasedAction {
    fn new<T: Intent, A: Action<T> + 'static>(action: A) -> Self {
        let action = Arc::new(action);
        let enabled_action = Arc::clone(&action);
        let consumes_action = Arc::clone(&action);
        Self {
            is_enabled: Arc::new(move |intent| enabled_action.is_enabled(typed::<T>(intent))),
            consumes_key: Arc::new(move |intent| consumes_action.consumes_key(typed::<T>(intent))),
            invoke: Arc::new(move |intent| action.invoke(typed::<T>(intent))),
        }
    }

    pub(crate) fn is_enabled(&self, intent: &dyn Any) -> bool {
        (self.is_enabled)(intent)
    }

    pub(crate) fn consumes_key(&self, intent: &dyn Any) -> bool {
        (self.consumes_key)(intent)
    }

    pub(crate) fn invoke(&self, intent: &dyn Any) {
        (self.invoke)(intent);
    }
}

/// Per intent type, the candidate actions in resolution order — nearest
/// `Actions` scope first. What Flutter's ancestor walk visits, precomputed at
/// provide time.
pub(crate) type ActionChain = Arc<HashMap<TypeId, Vec<ErasedAction>>>;

/// The first **enabled** action for `intent` in `chain` — `Actions.maybeInvoke`'s
/// walk-until-enabled (`actions.dart:1032-1044`).
pub(crate) fn resolve<'c>(chain: &'c ActionChain, intent: &dyn Any) -> Option<&'c ErasedAction> {
    chain
        .get(&intent.type_id())?
        .iter()
        .find(|action| action.is_enabled(intent))
}

// ============================================================================
// The widget
// ============================================================================

/// Binds intent types to [`Action`]s for a subtree — Flutter's `Actions`
/// (`actions.dart:729`).
///
/// Resolution is nearest-scope-first with fall-through past disabled actions,
/// as Flutter's ancestor walk resolves (`:1032-1044`). Invoke with
/// [`Actions::maybe_invoke`] from build-time code, or let a
/// [`Shortcuts`](crate::Shortcuts) dispatch into it from the keyboard.
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

    /// Bind intent type `T` to `action`. A binding nearer the invoker shadows
    /// an enclosing one — unless disabled, which falls through.
    #[must_use]
    pub fn action<T: Intent>(mut self, action: impl Action<T> + 'static) -> Self {
        self.own
            .push((TypeId::of::<T>(), ErasedAction::new(action)));
        self
    }

    /// Find the first enabled action for `intent` — nearest scope first — and
    /// invoke it. Returns whether one ran. Flutter's `Actions.maybeInvoke`
    /// (`actions.dart:1032-1044`), minus the result value (dropped on the key
    /// path anyway, `:348-349`).
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
    /// **prepend** per type, so the nearest scope resolves first.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let mut chain: HashMap<TypeId, Vec<ErasedAction>> = ambient_action_chain(ctx)
            .map(|enclosing| (*enclosing).clone())
            .unwrap_or_default();
        for (type_id, action) in self.own.iter().rev() {
            chain.entry(*type_id).or_default().insert(0, action.clone());
        }
        ActionChainProvider {
            chain: Arc::new(chain),
            child: self.child.clone(),
        }
    }
}

/// The nearest provider's chain, if any. Resolved at call time, so late reads
/// (a key handler built earlier) still see the tree's current bindings only if
/// they re-read — `Shortcuts` captures at build and re-captures when this
/// provider's subtree rebuilds (ADR-0023 O-1).
pub(crate) fn ambient_action_chain(ctx: &dyn BuildContext) -> Option<ActionChain> {
    ctx.get::<ActionChainProvider, _>(|provider| Arc::clone(&provider.chain))
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
        !Arc::ptr_eq(&self.chain, &old.chain)
    }
}

impl_inherited_view!(ActionChainProvider);

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::SizedBox;
    use crate::test_harness::mount;

    struct AddToCounter(usize);
    impl Intent for AddToCounter {}

    /// A leaf whose build invokes `intent` through the ambient chain — the
    /// only place a `BuildContext` exists.
    #[derive(Clone)]
    struct InvokeProbe {
        amount: usize,
        ran: Arc<AtomicUsize>,
    }

    impl View for InvokeProbe {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateless(self)
        }
    }

    impl StatelessView for InvokeProbe {
        fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
            if Actions::maybe_invoke(ctx, &AddToCounter(self.amount)) {
                self.ran.fetch_add(1, Ordering::SeqCst);
            }
            SizedBox::new(1.0, 1.0)
        }
    }

    /// An action that reports disabled, to prove fall-through.
    struct Disabled;
    impl Action<AddToCounter> for Disabled {
        fn is_enabled(&self, _intent: &AddToCounter) -> bool {
            false
        }
        fn invoke(&self, _intent: &AddToCounter) {
            unreachable!("BUG: a disabled action must never be invoked (actions.dart:1032-1044)");
        }
    }

    /// Nearest-scope-first with the typed payload delivered: the inner
    /// binding shadows the outer, and the intent's field reaches the closure.
    ///
    /// Red-check: resolve from the raw own-map instead of the layered chain —
    /// the outer counter moves and the inner assertion flips.
    #[test]
    fn the_nearest_enabled_action_wins_and_receives_the_payload() {
        let ran = Arc::new(AtomicUsize::new(0));
        let outer_sum = Arc::new(AtomicUsize::new(0));
        let inner_sum = Arc::new(AtomicUsize::new(0));

        let outer_counter = Arc::clone(&outer_sum);
        let inner_counter = Arc::clone(&inner_sum);
        let _harness = mount(
            Actions::new(
                Actions::new(InvokeProbe {
                    amount: 5,
                    ran: Arc::clone(&ran),
                })
                .action(CallbackAction::new(move |intent: &AddToCounter| {
                    inner_counter.fetch_add(intent.0, Ordering::SeqCst);
                })),
            )
            .action(CallbackAction::new(move |intent: &AddToCounter| {
                outer_counter.fetch_add(intent.0, Ordering::SeqCst);
            })),
        );

        assert_eq!(ran.load(Ordering::SeqCst), 1, "maybe_invoke reported true");
        assert_eq!(
            inner_sum.load(Ordering::SeqCst),
            5,
            "the nearest action ran, payload intact"
        );
        assert_eq!(
            outer_sum.load(Ordering::SeqCst),
            0,
            "the outer action was shadowed"
        );
    }

    /// A **disabled** nearer action falls through to an enabled outer one —
    /// Flutter's walk continues past scopes whose action is disabled
    /// (`actions.dart:1032-1044`). A plain nearest-map merge would shadow the
    /// outer action and invoke nothing.
    #[test]
    fn a_disabled_nearer_action_falls_through_to_the_outer_scope() {
        let ran = Arc::new(AtomicUsize::new(0));
        let outer_sum = Arc::new(AtomicUsize::new(0));

        let outer_counter = Arc::clone(&outer_sum);
        let _harness = mount(
            Actions::new(
                Actions::new(InvokeProbe {
                    amount: 7,
                    ran: Arc::clone(&ran),
                })
                .action(Disabled),
            )
            .action(CallbackAction::new(move |intent: &AddToCounter| {
                outer_counter.fetch_add(intent.0, Ordering::SeqCst);
            })),
        );

        assert_eq!(ran.load(Ordering::SeqCst), 1);
        assert_eq!(
            outer_sum.load(Ordering::SeqCst),
            7,
            "resolution fell through the disabled nearer action"
        );
    }

    /// No binding anywhere: `maybe_invoke` reports `false` and nothing runs.
    #[test]
    fn maybe_invoke_without_a_binding_reports_false() {
        let ran = Arc::new(AtomicUsize::new(0));
        let _harness = mount(InvokeProbe {
            amount: 1,
            ran: Arc::clone(&ran),
        });
        assert_eq!(ran.load(Ordering::SeqCst), 0, "nothing to invoke");
    }
}
