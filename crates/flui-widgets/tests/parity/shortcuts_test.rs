//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/shortcuts_test.dart` and
//! `.../actions_test.dart` (tag `3.44.0`).
//!
//! FLUI's `Shortcuts`/`CallbackShortcuts`/`SingleActivator`
//! (`crates/flui-widgets/src/interaction/shortcuts.rs`) and
//! `Actions`/`Intent`/`Action`/`CallbackAction`
//! (`crates/flui-widgets/src/interaction/actions.rs`) already carry a
//! substantial self-authored unit-test suite pinning ADR-0023's bubbling
//! key-dispatch design end to end. This file's job is different, matching
//! this crate's `navigator_test.rs`/`focus_test.rs` precedent: every case
//! below is anchored to a **named upstream Flutter test**. Where a case is
//! already fully exercised by an existing self-authored test, the citation
//! was added there instead of duplicating it here (see each function's doc
//! comment in `interaction/shortcuts.rs`/`interaction/actions.rs`).
//!
//! `FocusManager::global()` is an owner-thread (thread-local) singleton;
//! nextest's libtest runner reuses OS threads across many `#[test]`
//! functions in this binary, so every key-dispatching case below takes
//! [`SHORTCUTS_TEST_LOCK`] and explicitly `unfocus()`s whatever it attached
//! — the same convention `focus_test.rs` documents and uses.
//!
//! ## Ported cases
//! - `'trigger on key events'` (`CallbackShortcuts` group) — two independent
//!   bindings on one `CallbackShortcuts`, each firing only for its own key —
//!   [`callback_shortcuts_trigger_on_matching_key_events_only`].
//! - `'nested CallbackShortcuts stop propagation'` — an inner
//!   `CallbackShortcuts` binding the same key as an outer one consumes the
//!   event first; the outer never sees it —
//!   [`nested_callback_shortcuts_the_inner_consuming_binding_stops_the_outer`].
//! - `'non-overlapping nested CallbackShortcuts fire appropriately'` — each
//!   nested `CallbackShortcuts` reacts only to its own key, and a key the
//!   inner ignores still bubbles to the outer —
//!   [`non_overlapping_nested_callback_shortcuts_each_fire_for_their_own_key`].
//!   **Adapted**: the oracle uses `CharacterActivator` (not ported, no
//!   consumer yet — ADR-0023); `SingleActivator::character` serves the same
//!   "bind by produced character" role for this case.
//! - `'Works correctly with Shortcuts too'` (`CallbackShortcuts` group) — a
//!   `CallbackShortcuts` nested with an `Intent`-mapped `Shortcuts`/`Actions`
//!   pair: the innermost `CallbackShortcuts` consumes its key before
//!   `Shortcuts` ever sees it, and `Shortcuts` resolving its intent consumes
//!   the key before it can bubble to the outermost `CallbackShortcuts` —
//!   [`callback_shortcuts_interoperate_with_a_nested_shortcuts_actions_pair`].
//!   **Adapted**: `CharacterActivator` → `SingleActivator::character`, same
//!   substitution as above.
//! - `"Shortcuts passes to the next Shortcuts widget if it doesn't map the
//!   key"` — an inner `Shortcuts` with no matching binding lets the key
//!   bubble past it to an outer `Shortcuts` that does map it —
//!   [`an_unmatched_key_bubbles_past_an_inner_shortcuts_to_an_outer_one`].
//!   **Adapted**: the oracle roots the outer `Shortcuts` on a
//!   `Shortcuts.manager` (a shared `ShortcutManager`, not ported — ADR-0023)
//!   with `Actions` sitting *between* the two `Shortcuts` widgets. FLUI's
//!   `Shortcuts` resolves its `Actions` chain from its own position, not the
//!   focused leaf's (ADR-0023's documented resolve-at-own-position
//!   divergence — see `tab_tests` in `interaction/shortcuts.rs`), so `Actions`
//!   must wrap *both* `Shortcuts` widgets here instead of sitting between
//!   them, or the outer `Shortcuts` would resolve against no chain at all.
//!   The bubbling assertion this test exists to pin — an unmapped key
//!   passing an inner `Shortcuts` to reach an outer one — is unaffected by
//!   where `Actions` sits.
//! - `'Disabled actions stop propagation to an ancestor'` — **the divergence
//!   this port found**: a nearer `Actions` scope's mapping for an intent
//!   type, if disabled, stops resolution outright rather than falling
//!   through to an enclosing scope's mapping for the same type. FLUI's
//!   `Actions::resolve` shipped with the opposite behavior (a Vec-of-
//!   candidates walk that skipped disabled entries down to an enabled outer
//!   one) — traced to a misreading of `_visitActionsAncestors`/`_castAction`
//!   recorded in ADR-0023; `maybeInvoke`'s own doc is explicit that the
//!   search stops the moment a scope declares the type, enabled or not
//!   (`actions.dart:993-995`). Fixed alongside this port:
//!   `ActionChain` is now one candidate per type (a nearer scope's mapping
//!   *replaces* an enclosing one), not a fallback list — see the ADR-0023
//!   correction note and `interaction/actions.rs`'s
//!   `a_disabled_nearer_action_stops_resolution_at_its_own_scope` for the
//!   internal-unit-test half of this fix. This file's version is an
//!   independent, oracle-named regression pin against the same behavior —
//!   [`a_disabled_action_stops_propagation_to_an_ancestor_scope`].
//!   **Adapted**: no `ActionDispatcher`/`Actions.invoke` static API (ADR-0023
//!   deferred); ported through `Actions::maybe_invoke` from a probe widget's
//!   `build`, which is the same "does resolution stop here" question the
//!   oracle's `Actions.invoke` asks.
//! - `'Actions can invoke actions in ancestor dispatcher'` — the fix's other
//!   branch: a nearer scope that declares **no** mapping for the intent type
//!   at all still lets resolution continue to an enclosing scope's mapping
//!   (only a *declared* mapping stops the walk) —
//!   [`an_intent_undeclared_at_the_nearer_scope_resolves_from_the_ancestor`].
//!   Paired with the disabled-action case above per this port's mutation
//!   discipline: one pins each side of `resolve`'s "found but disabled" vs.
//!   "not found here" branch. **Adapted**: same `ActionDispatcher` substitution
//!   as above.
//!
//! ## Not ported
//! - `LogicalKeySet` group (`test('LogicalKeySet passes parameters
//!   correctly.')`, `'... works as a map key.'`, `'.hashCode is stable'`,
//!   `'.hashCode is order-independent'`, `'.diagnostics work.'`, `'handles
//!   two keys'`, the three `numLock works as expected...` cases) —
//!   `LogicalKeySet`'s exact-pressed-set semantics need a
//!   `HardwareKeyboard`-style pressed-key tracker FLUI does not have;
//!   documented not-ported in `interaction/shortcuts.rs`'s module doc
//!   (ADR-0023). `SingleActivator` covers the catalog's real uses.
//! - `CharacterActivator` group (`'is triggered on events with correct
//!   character'`, `'handles repeated events'`, `'rejects repeated events if
//!   requested'`, `'handles Alt, Ctrl and Meta'`, `'isActivatedBy works as
//!   expected'`, its diagnostics case) — `CharacterActivator` itself waits
//!   for a consumer (ADR-0023); the two `CallbackShortcuts` ports above
//!   substitute `SingleActivator::character` where the oracle's own case
//!   only needs "bind by produced character", not `CharacterActivator`'s
//!   shift-insensitivity.
//! - `ShortcutManager`/`Shortcuts.manager` group (`'Default constructed
//!   Shortcuts has empty shortcuts'`, `'Default constructed Shortcuts.manager
//!   has empty shortcuts'`, `'Shortcuts.manager passes on shortcuts'`,
//!   `'ShortcutManager handles shortcuts'`, `'Shortcuts.manager lets manager
//!   handle shortcuts'`, `'ShortcutManager ignores key presses with no
//!   primary focus'`, its object-creation leak-tracker case) — a shared,
//!   externally-constructed `ShortcutManager` is not ported; FLUI's
//!   `Shortcuts` is one manager per widget (ADR-0023, named not-ported).
//! - `'Shortcuts can disable a shortcut with Intent.doNothing'` — depends on
//!   `Shortcuts.manager` plus a `WidgetsApp` host; not ported (see above).
//! - `"Shortcuts that aren't bound to an action don't absorb keys meant for
//!   text fields"`, `'Shortcuts that are bound to an action do override text
//!   fields'`, `'Shortcuts can override intents that apply to text fields'`
//!   (plus its `DoNothingAndStopPropagationIntent` variant) — these pin
//!   `WidgetsApp`'s default text-editing shortcut wiring and a
//!   `TestTextField` fixture; FLUI has no default-text-shortcuts layer yet.
//! - `'Shortcuts pass debug label to focus node.'` — FLUI's `Shortcuts`
//!   hardcodes its own `debug_label("Shortcuts")` (see `shortcuts.rs`); there
//!   is no per-instance override constructor parameter to port a distinct
//!   assertion against, and no `Focus.of` ambient lookup (not ported) to
//!   reach the node from a `BuildContext` the way the oracle does.
//! - `'Shortcuts support multiple intents'` — exercises `PrioritizedIntents`,
//!   `ScrollIntent`, a full `WidgetsApp`/`ListView`/`PrimaryScrollController`
//!   stack; none of that machinery exists in FLUI yet.
//! - `'Shortcuts support activators that returns null in triggers'` — a
//!   custom `ShortcutActivator` implementation with nullable `triggers`;
//!   FLUI's activator surface is the sealed `SingleActivator`, not a trait
//!   third-party activators implement.
//! - `'Shortcuts does not insert a semantics node when includeSemantics is
//!   false'` — no semantics-layer integration for `Shortcuts` yet
//!   (`includeSemantics`, ADR-0023 named not-ported).
//! - `'Updating shortcuts triggers dependency rebuild'` — exercises
//!   `ShortcutRegistrar`'s dependency-notification callback; not ported (see
//!   `ShortcutRegistrar` group below).
//! - `ShortcutRegistrar` group in full (`'trigger ShortcutRegistrar on key
//!   events'`, `'WidgetsApp has a ShortcutRegistrar listening'`, `"doesn't
//!   override text field shortcuts"`, `'nested ShortcutRegistrars stop
//!   propagation'`, `'non-overlapping nested ShortcutRegistrars fire
//!   appropriately'`, `'Works correctly with Shortcuts too'` (the
//!   `ShortcutRegistrar` one), its object-creation case, `'using a disposed
//!   token asserts'`, `'setting duplicate bindings asserts'`, `'sets debug
//!   label on focus node'`) — `ShortcutRegistrar`/`ShortcutRegistryEntry` has
//!   no FLUI equivalent (ADR-0023, named not-ported); no consumers yet.
//! - `actions_test.dart`'s `ActionDispatcher`/`Actions.of`/`Actions.find`
//!   group (`'ActionDispatcher invokes actions when asked.'`, the custom-
//!   dispatcher cases, `'invoke throws when no action is found'`, `'Actions
//!   widget can be found with of'`, `'Action can be found with find'`) — no
//!   replaceable `ActionDispatcher` object, and no `Actions.of`/`Actions.find`
//!   ambient lookups; FLUI's only entry points are `Actions::maybe_invoke`
//!   and `Shortcuts`' own key dispatch (ADR-0023, named not-ported).
//! - `FocusableActionDetector` group in full (both copies of `'keeps track of
//!   focus and hover even when disabled.'`, `'changes mouse cursor when
//!   hovered'`, `'can be used without callbacks'`, `'can prevent its
//!   descendants from being focusable'`, `'... from being traversable'`,
//!   `'can exclude Focus semantics'`) — `FocusableActionDetector` does not
//!   exist in FLUI.
//! - `'Actions.invoke returns the value of Action.invoke'`, `'ContextAction
//!   can return null'` — FLUI's `Action::invoke` returns the fixed
//!   [`ActionOutcome`] enum (ADR-0023/ADR-0026), not an arbitrary `Object?`;
//!   there is no `ContextAction` variant.
//! - `'can listen to enabled state of Actions'` — `Action.addActionListener`
//!   is not ported (ADR-0023, named not-ported).
//! - `'VoidCallbackAction'` — not implemented; the deferral note in
//!   `interaction/actions.rs`'s module doc already names the workaround
//!   (`CallbackAction::new(|_| ())`).
//! - `'default Intent debugFillProperties'`, `'default Actions
//!   debugFillProperties'`, `'Actions implements debugFillProperties'` — no
//!   `Diagnosticable` tree-dump equivalent for `Intent`/`Actions`.
//! - The overridable-action group in full (`'Basic usage'`, `'Does not break
//!   after use'`, `'Does not override if not overridable'`, `'The final
//!   override controls isEnabled'`, `'The override can choose to defer
//!   isActionEnabled to the overridable'`, `'Throws on infinite recursions'`,
//!   `'Throws on invoking invalid override'`, `'Make an overridable action
//!   overridable'`, `'Overriding Actions can change the intent'`, `'Override
//!   non-context overridable Actions with a ContextAction'`, `'Override a
//!   ContextAction with a regular Action'`) — `Action.overridable`/
//!   `OverridableAction`'s override-chain mechanism does not exist in FLUI;
//!   adding it is a new public-API decision, out of this test-port's bounds.
//!
//! Widget mapping: `Shortcuts`/`CallbackShortcuts` → a
//! `Focus(can_request_focus: false)` wrapper; `Actions` → an `InheritedView`
//! scope layering an `ActionChain` (`interaction/actions.rs`).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_interaction::events::{Key, KeyEvent, KeyState, Modifiers};
use flui_interaction::routing::{FocusManager, FocusNode};
use flui_view::element::ElementKind;
use flui_widgets::prelude::*;
use flui_widgets::{
    Action, ActionOutcome, Actions, CallbackAction, CallbackShortcuts, Focus, Intent, Shortcuts,
    SingleActivator, SizedBox,
};
use parking_lot::Mutex;

use crate::common::{lay_out, loose};

/// Conservatively serializes this file's key-dispatch fixtures on top of
/// `FocusManager::global()`'s owner-thread singleton — see the module doc.
static SHORTCUTS_TEST_LOCK: Mutex<()> = Mutex::new(());

/// A leaf big enough to mount without tripping a zero-size edge case, and
/// otherwise inert — geometry is not what these tests assert about.
fn leaf() -> SizedBox {
    SizedBox::new(10.0, 10.0)
}

fn key_down(character: &str, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        state: KeyState::Down,
        key: Key::Character(character.into()),
        modifiers,
        ..KeyEvent::default()
    }
}

fn key_up(character: &str, modifiers: Modifiers) -> KeyEvent {
    KeyEvent {
        state: KeyState::Up,
        ..key_down(character, modifiers)
    }
}

// ============================================================================
// CallbackShortcuts
// ============================================================================

/// `'trigger on key events'` (`CallbackShortcuts` group, `shortcuts_test.dart`).
#[test]
fn callback_shortcuts_trigger_on_matching_key_events_only() {
    let _guard = SHORTCUTS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let invoked_a = Rc::new(Cell::new(0));
    let invoked_b = Rc::new(Cell::new(0));
    let field = FocusNode::with_debug_label("two-binding-field");

    let a_for_binding = Rc::clone(&invoked_a);
    let b_for_binding = Rc::clone(&invoked_b);
    let _laid = lay_out(
        CallbackShortcuts::new(Focus::new(leaf()).focus_node(Arc::clone(&field)))
            .binding(SingleActivator::character("a"), move || {
                a_for_binding.set(a_for_binding.get() + 1);
            })
            .binding(SingleActivator::character("b"), move || {
                b_for_binding.set(b_for_binding.get() + 1);
            }),
        loose(200.0),
    );
    field.request_focus();

    assert!(manager.dispatch_key_event(&key_down("a", Modifiers::empty())));
    assert_eq!(invoked_a.get(), 1);
    assert_eq!(invoked_b.get(), 0);
    assert!(!manager.dispatch_key_event(&key_up("a", Modifiers::empty())));
    assert_eq!(invoked_a.get(), 1);
    assert_eq!(invoked_b.get(), 0);

    invoked_a.set(0);
    invoked_b.set(0);
    assert!(manager.dispatch_key_event(&key_down("b", Modifiers::empty())));
    assert_eq!(invoked_a.get(), 0);
    assert_eq!(invoked_b.get(), 1);
    assert!(!manager.dispatch_key_event(&key_up("b", Modifiers::empty())));
    assert_eq!(invoked_a.get(), 0);
    assert_eq!(invoked_b.get(), 1);

    manager.unfocus();
}

/// `'nested CallbackShortcuts stop propagation'` (`CallbackShortcuts` group,
/// `shortcuts_test.dart`).
#[test]
fn nested_callback_shortcuts_the_inner_consuming_binding_stops_the_outer() {
    let _guard = SHORTCUTS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let invoked_outer = Rc::new(Cell::new(0));
    let invoked_inner = Rc::new(Cell::new(0));
    let field = FocusNode::with_debug_label("nested-cs-stop-field");

    let outer_for_binding = Rc::clone(&invoked_outer);
    let inner_for_binding = Rc::clone(&invoked_inner);
    let inner = CallbackShortcuts::new(Focus::new(leaf()).focus_node(Arc::clone(&field))).binding(
        SingleActivator::character("a"),
        move || {
            inner_for_binding.set(inner_for_binding.get() + 1);
        },
    );
    let _laid = lay_out(
        CallbackShortcuts::new(inner).binding(SingleActivator::character("a"), move || {
            outer_for_binding.set(outer_for_binding.get() + 1);
        }),
        loose(200.0),
    );
    field.request_focus();

    assert!(manager.dispatch_key_event(&key_down("a", Modifiers::empty())));
    assert_eq!(
        invoked_outer.get(),
        0,
        "the inner binding consumed the key first"
    );
    assert_eq!(invoked_inner.get(), 1);

    manager.unfocus();
}

/// `'non-overlapping nested CallbackShortcuts fire appropriately'`
/// (`CallbackShortcuts` group, `shortcuts_test.dart`). **Adapted**:
/// `CharacterActivator` → `SingleActivator::character` (see module doc).
#[test]
fn non_overlapping_nested_callback_shortcuts_each_fire_for_their_own_key() {
    let _guard = SHORTCUTS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let invoked_outer = Rc::new(Cell::new(0));
    let invoked_inner = Rc::new(Cell::new(0));
    let field = FocusNode::with_debug_label("non-overlapping-cs-field");

    let outer_for_binding = Rc::clone(&invoked_outer);
    let inner_for_binding = Rc::clone(&invoked_inner);
    let inner = CallbackShortcuts::new(Focus::new(leaf()).focus_node(Arc::clone(&field))).binding(
        SingleActivator::character("a"),
        move || {
            inner_for_binding.set(inner_for_binding.get() + 1);
        },
    );
    let _laid = lay_out(
        CallbackShortcuts::new(inner).binding(SingleActivator::character("b"), move || {
            outer_for_binding.set(outer_for_binding.get() + 1);
        }),
        loose(200.0),
    );
    field.request_focus();

    assert!(manager.dispatch_key_event(&key_down("a", Modifiers::empty())));
    assert_eq!(invoked_outer.get(), 0);
    assert_eq!(invoked_inner.get(), 1);

    assert!(manager.dispatch_key_event(&key_down("b", Modifiers::empty())));
    assert_eq!(
        invoked_outer.get(),
        1,
        "the inner CallbackShortcuts ignored 'b', so it bubbled to the outer"
    );
    assert_eq!(invoked_inner.get(), 1);

    manager.unfocus();
}

/// `'Works correctly with Shortcuts too'` (`CallbackShortcuts` group,
/// `shortcuts_test.dart`). **Adapted**: `CharacterActivator` →
/// `SingleActivator::character` (see module doc).
#[test]
fn callback_shortcuts_interoperate_with_a_nested_shortcuts_actions_pair() {
    struct TestIntentA;
    impl Intent for TestIntentA {}
    struct TestIntentB;
    impl Intent for TestIntentB {}

    let _guard = SHORTCUTS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let invoked_callback_a = Rc::new(Cell::new(0));
    let invoked_callback_b = Rc::new(Cell::new(0));
    let invoked_action_a = Arc::new(AtomicUsize::new(0));
    let invoked_action_b = Arc::new(AtomicUsize::new(0));
    let field = FocusNode::with_debug_label("cs-shortcuts-interop-field");

    let cb_a = Rc::clone(&invoked_callback_a);
    let cb_b = Rc::clone(&invoked_callback_b);
    let act_a = Arc::clone(&invoked_action_a);
    let act_b = Arc::clone(&invoked_action_b);

    let innermost = CallbackShortcuts::new(Focus::new(leaf()).focus_node(Arc::clone(&field)))
        .binding(SingleActivator::character("a"), move || {
            cb_a.set(cb_a.get() + 1);
        });
    let shortcuts = Shortcuts::new(innermost)
        .shortcut(SingleActivator::character("a"), TestIntentA)
        .shortcut(SingleActivator::character("b"), TestIntentB);
    let outer_callback =
        CallbackShortcuts::new(shortcuts).binding(SingleActivator::character("b"), move || {
            cb_b.set(cb_b.get() + 1);
        });

    let _laid = lay_out(
        Actions::new(outer_callback)
            .action(CallbackAction::new(move |_intent: &TestIntentA| {
                act_a.fetch_add(1, Ordering::SeqCst);
            }))
            .action(CallbackAction::new(move |_intent: &TestIntentB| {
                act_b.fetch_add(1, Ordering::SeqCst);
            })),
        loose(200.0),
    );
    field.request_focus();

    assert!(manager.dispatch_key_event(&key_down("a", Modifiers::empty())));
    assert_eq!(
        invoked_callback_a.get(),
        1,
        "the innermost CallbackShortcuts consumed 'a' before Shortcuts ever saw it"
    );
    assert_eq!(invoked_callback_b.get(), 0);
    assert_eq!(invoked_action_a.load(Ordering::SeqCst), 0);
    assert_eq!(invoked_action_b.load(Ordering::SeqCst), 0);

    invoked_callback_a.set(0);
    invoked_callback_b.set(0);
    invoked_action_a.store(0, Ordering::SeqCst);
    invoked_action_b.store(0, Ordering::SeqCst);

    assert!(manager.dispatch_key_event(&key_down("b", Modifiers::empty())));
    assert_eq!(invoked_callback_a.get(), 0);
    assert_eq!(
        invoked_callback_b.get(),
        0,
        "Shortcuts consumed 'b' via TestIntentB before it could bubble to the outer CallbackShortcuts"
    );
    assert_eq!(invoked_action_a.load(Ordering::SeqCst), 0);
    assert_eq!(invoked_action_b.load(Ordering::SeqCst), 1);

    manager.unfocus();
}

// ============================================================================
// Shortcuts
// ============================================================================

/// `"Shortcuts passes to the next Shortcuts widget if it doesn't map the
/// key"` (`shortcuts_test.dart`). **Adapted**: see module doc — `Actions`
/// wraps both `Shortcuts` widgets instead of sitting between them.
#[test]
fn an_unmatched_key_bubbles_past_an_inner_shortcuts_to_an_outer_one() {
    struct OuterIntent;
    impl Intent for OuterIntent {}
    struct InnerIntent;
    impl Intent for InnerIntent {}

    let _guard = SHORTCUTS_TEST_LOCK.lock();
    let manager = FocusManager::global();
    manager.unfocus();

    let outer_invoked = Rc::new(Cell::new(0));
    let inner_invoked = Rc::new(Cell::new(0));
    let field = FocusNode::with_debug_label("nested-shortcuts-bubble-field");

    let outer_for_action = Rc::clone(&outer_invoked);
    let inner_for_action = Rc::clone(&inner_invoked);

    let inner_shortcuts = Shortcuts::new(Focus::new(leaf()).focus_node(Arc::clone(&field)))
        .shortcut(SingleActivator::character("z"), InnerIntent);
    let outer_shortcuts = Shortcuts::new(inner_shortcuts)
        .shortcut(SingleActivator::character("s").shift(), OuterIntent);

    let _laid = lay_out(
        Actions::new(outer_shortcuts)
            .action(CallbackAction::new(move |_intent: &OuterIntent| {
                outer_for_action.set(outer_for_action.get() + 1);
            }))
            .action(CallbackAction::new(move |_intent: &InnerIntent| {
                inner_for_action.set(inner_for_action.get() + 1);
            })),
        loose(200.0),
    );
    field.request_focus();

    assert!(
        manager.dispatch_key_event(&key_down("s", Modifiers::SHIFT)),
        "consumed by the outer Shortcuts"
    );
    assert_eq!(
        outer_invoked.get(),
        1,
        "the key bubbled past the inner Shortcuts, which does not map it, to the outer one"
    );
    assert_eq!(
        inner_invoked.get(),
        0,
        "the inner Shortcuts' own binding never matched"
    );

    manager.unfocus();
}

// ============================================================================
// Actions — the resolution-stops-at-the-nearest-declaring-scope divergence
// ============================================================================

/// A leaf whose build invokes `TestIntent` through the ambient `Actions`
/// chain — the only place a `BuildContext` exists to call
/// [`Actions::maybe_invoke`]. Mirrors `interaction/actions.rs`'s own
/// `InvokeProbe` test fixture, reimplemented here because that one is
/// crate-private and this file is a separate, external test crate.
#[derive(Clone)]
struct InvokeProbe<T: Intent + Clone + 'static> {
    intent: T,
    ran: Rc<Cell<bool>>,
}

impl<T: Intent + Clone + 'static> View for InvokeProbe<T> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl<T: Intent + Clone + 'static> StatelessView for InvokeProbe<T> {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        if Actions::maybe_invoke(ctx, &self.intent) {
            self.ran.set(true);
        }
        SizedBox::new(1.0, 1.0)
    }
}

/// `'Disabled actions stop propagation to an ancestor'` (`actions_test.dart`)
/// — the divergence this port found and fixed; see the module doc's "Ported
/// cases" entry and the ADR-0023 correction note.
///
/// Red-check: this exact assertion pair failed against the pre-fix
/// `ActionChain` (a per-type `Vec<ErasedAction>` searched for the first
/// *enabled* candidate across every declaring scope) — `ran` was `true` and
/// `outer_ran` was `true`, because the disabled inner mapping was skipped in
/// favor of the outer's enabled one. See `interaction/actions.rs`'s
/// `a_disabled_nearer_action_stops_resolution_at_its_own_scope` for the
/// mutation-verified internal half of this fix.
#[test]
fn a_disabled_action_stops_propagation_to_an_ancestor_scope() {
    #[derive(Clone)]
    struct TestIntent;
    impl Intent for TestIntent {}

    struct DisabledAction;
    impl Action<TestIntent> for DisabledAction {
        fn is_enabled(&self, _intent: &TestIntent) -> bool {
            false
        }
        fn invoke(&self, _intent: &TestIntent) -> ActionOutcome {
            unreachable!("BUG: a disabled action must never be invoked");
        }
    }

    let ran = Rc::new(Cell::new(false));
    let outer_ran = Rc::new(Cell::new(false));
    let outer_for_action = Rc::clone(&outer_ran);

    let _laid = lay_out(
        Actions::new(
            Actions::new(InvokeProbe {
                intent: TestIntent,
                ran: Rc::clone(&ran),
            })
            .action(DisabledAction),
        )
        .action(CallbackAction::new(move |_intent: &TestIntent| {
            outer_for_action.set(true);
        })),
        loose(50.0),
    );

    assert!(
        !ran.get(),
        "maybe_invoke reported false: the disabled nearer mapping stopped the search"
    );
    assert!(
        !outer_ran.get(),
        "the outer action was never reached — maybeInvoke's own doc says the search \
         stops once a scope declares the type, enabled or not (actions.dart:993-995)"
    );
}

/// `'Actions can invoke actions in ancestor dispatcher'` (`actions_test.dart`)
/// — the fix's other branch: a nearer scope declaring **no** mapping for the
/// type (as opposed to a disabled one) still lets resolution reach the
/// ancestor. Paired with
/// [`a_disabled_action_stops_propagation_to_an_ancestor_scope`] per this
/// port's mutation discipline — together they pin both branches of
/// `resolve`'s "declared but disabled" vs. "not declared here" distinction.
///
/// Red-check: makes the merge in `Actions::build` skip inheriting the
/// enclosing chain whenever `own` is non-empty (instead of only overwriting
/// the types `own` actually declares) — `ran` flips to `false`, since the
/// inner scope's now-empty-looking chain would shadow the outer's mapping
/// for `TestIntent` even though `own` never declared it.
#[test]
fn an_intent_undeclared_at_the_nearer_scope_resolves_from_the_ancestor() {
    #[derive(Clone)]
    struct TestIntent;
    impl Intent for TestIntent {}

    let ran = Rc::new(Cell::new(false));

    let _laid = lay_out(
        Actions::new(Actions::new(InvokeProbe {
            intent: TestIntent,
            ran: Rc::clone(&ran),
        }))
        .action(CallbackAction::new(move |_intent: &TestIntent| {})),
        loose(50.0),
    );

    assert!(
        ran.get(),
        "the nearer scope declared nothing for this type, so resolution continued to the ancestor"
    );
}
