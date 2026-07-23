//! [`SingleActivator`] and [`CallbackShortcuts`] — keyboard shortcuts riding
//! the leaf→root key dispatch.
//!
//! ADR-0023. A shortcut widget is, mechanically, a
//! `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper
//! (`shortcuts.dart:1134-1143`, `:1225-1231`): it sees a key only when every
//! `Focus` below it — most importantly the focused field — *ignored* the
//! event and the ADR-0023 walk bubbled it up.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/shortcuts.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `SingleActivator` (`:433-581`),
//! `CallbackShortcuts` (`:1181-1231`).
//!
//! # Deferred, and named (ADR-0023)
//!
//! `LogicalKeySet` (needs a `HardwareKeyboard`-style pressed-set tracker),
//! `CharacterActivator` (no consumer), a shared `ShortcutManager`, and
//! `includeSemantics`. The Intent-mapped [`Shortcuts`] resolves its
//! [`Actions`](super::actions::Actions) chain from **its own position**, not
//! the focused leaf's context (ADR-0023's resolve-at-own-position divergence) — visible only when an `Actions`
//! sits between the focused widget and the `Shortcuts`.

use std::any::Any;
use std::rc::Rc;

use flui_interaction::events::{Key, KeyEvent, NamedKey};
use flui_interaction::routing::KeyEventResult;
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::actions::{
    ActionChainProvider, Actions, Intent, NextFocusAction, NextFocusIntent, PreviousFocusAction,
    PreviousFocusIntent, resolve,
};
use super::focus::Focus;

/// A callback bound to a [`SingleActivator`] in [`CallbackShortcuts`].
pub type ShortcutCallback = Rc<dyn Fn()>;

// ============================================================================
// SingleActivator
// ============================================================================

/// A shortcut trigger: one logical key plus an **exact** set of modifiers —
/// Flutter's `SingleActivator` (`shortcuts.dart:433`).
///
/// `SingleActivator::character("c").control()` matches Ctrl+C and *only*
/// Ctrl+C: an event with an extra Shift held does not match, exactly as
/// Flutter's `_shouldAcceptModifiers` demands equality per modifier
/// (`:560-565`). Key-repeat events match by default; opt out with
/// [`allow_repeats(false)`](Self::allow_repeats) (`:461`). Only key-down
/// events ever match (`:576-581`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingleActivator {
    trigger: Key,
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    include_repeats: bool,
}

impl SingleActivator {
    /// An activator for the logical key `trigger`, no modifiers.
    #[must_use]
    pub fn new(trigger: Key) -> Self {
        Self {
            trigger,
            control: false,
            shift: false,
            alt: false,
            meta: false,
            include_repeats: true,
        }
    }

    /// An activator for the character `character` produces — `"c"`, `"+"`.
    #[must_use]
    pub fn character(character: impl Into<String>) -> Self {
        Self::new(Key::Character(character.into()))
    }

    /// An activator for a named (non-character) key — `NamedKey::Escape`.
    #[must_use]
    pub fn named(key: NamedKey) -> Self {
        Self::new(Key::Named(key))
    }

    /// Require the Control modifier (`shortcuts.dart:487`).
    #[must_use]
    pub fn control(mut self) -> Self {
        self.control = true;
        self
    }

    /// Require the Shift modifier (`:497`).
    #[must_use]
    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    /// Require the Alt modifier (`:507`).
    #[must_use]
    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Require the Meta modifier (`:517`).
    #[must_use]
    pub fn meta(mut self) -> Self {
        self.meta = true;
        self
    }

    /// Whether key-repeat events trigger the shortcut too — `true` by default
    /// (`:461`).
    #[must_use]
    pub fn allow_repeats(mut self, allow: bool) -> Self {
        self.include_repeats = allow;
        self
    }

    /// Whether `event` triggers this activator — Flutter's `accepts`
    /// (`shortcuts.dart:576-581`): a key-down (or allowed repeat) of exactly
    /// the trigger key under exactly the required modifiers.
    #[must_use]
    pub fn matches(&self, event: &KeyEvent) -> bool {
        event.state.is_down()
            && (self.include_repeats || !event.repeat)
            && event.key == self.trigger
            && event.modifiers.ctrl() == self.control
            && event.modifiers.shift() == self.shift
            && event.modifiers.alt() == self.alt
            && event.modifiers.meta() == self.meta
    }
}

// ============================================================================
// CallbackShortcuts
// ============================================================================

/// Binds key combinations to callbacks for its subtree — Flutter's
/// `CallbackShortcuts` (`shortcuts.dart:1181`), the `Intent`-free shortcut
/// widget.
///
/// While the primary focus sits inside `child`, a key event that every inner
/// `Focus` ignored bubbles here; **every** matching binding fires
/// (`:1210-1220`) and the event counts as handled iff at least one did. The
/// `Intent`-mapped `Shortcuts` / `Actions` pair is the general form; this is the
/// direct-callback shortcut for when an `Intent` would be ceremony (ADR-0023).
#[derive(Clone)]
pub struct CallbackShortcuts {
    bindings: Vec<(SingleActivator, ShortcutCallback)>,
    child: BoxedView,
}

impl CallbackShortcuts {
    /// A shortcut boundary around `child` with no bindings yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            bindings: Vec::new(),
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Fire `callback` whenever `activator` matches a key the focused subtree
    /// ignored.
    #[must_use]
    pub fn binding(mut self, activator: SingleActivator, callback: impl Fn() + 'static) -> Self {
        self.bindings.push((activator, Rc::new(callback)));
        self
    }
}

impl std::fmt::Debug for CallbackShortcuts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackShortcuts")
            .field(
                "bindings",
                &self
                    .bindings
                    .iter()
                    .map(|(activator, _)| activator)
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl View for CallbackShortcuts {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for CallbackShortcuts {
    /// `Focus(canRequestFocus: false, onKeyEvent: …)` around the child,
    /// exactly as Flutter builds it (`shortcuts.dart:1225-1231`).
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let bindings = self.bindings.clone();
        Focus::new(self.child.clone())
            .can_request_focus(false)
            .debug_label("CallbackShortcuts")
            .on_key_event(Rc::new(move |event| {
                let mut handled = false;
                for (activator, callback) in &bindings {
                    if activator.matches(event) {
                        callback();
                        handled = true;
                    }
                }
                if handled {
                    KeyEventResult::Handled
                } else {
                    KeyEventResult::Ignored
                }
            }))
    }
}

// ============================================================================
// Shortcuts
// ============================================================================

/// Maps key combinations to [`Intent`]s, dispatched through the enclosing
/// [`Actions`](super::actions::Actions) chain — Flutter's `Shortcuts`
/// (`shortcuts.dart:1004`).
///
/// On a key the focused subtree ignored, the **first** matching activator's
/// intent resolves to the nearest enclosing enabled action
/// (`ShortcutManager.handleKeypress`, `:922-938`). That action's
/// [`to_key_event_result`](super::actions::Action::to_key_event_result) decides
/// the final [`KeyEventResult`] — it is an overridable method, so the action has
/// the last word. Its default consumes the
/// key when the action performed its work, and reports the event unconsumed —
/// stopping the bubbling *without* consuming — when the action declined
/// (`actions.dart:312-314`); an action may override it to decide otherwise. No
/// match, or no enabled action: the key keeps bubbling.
#[derive(Clone)]
pub struct Shortcuts {
    shortcuts: Vec<(SingleActivator, Rc<dyn Intent>)>, // PORT-CHECK-OK-DYN: ADR-0023 — Flutter's `Map<ShortcutActivator, Intent>`; read back only through its own TypeId.
    child: BoxedView,
}

impl Shortcuts {
    /// A shortcut boundary around `child` with no bindings yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            shortcuts: Vec::new(),
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Bind `activator` to `intent`. Earlier bindings match first.
    #[must_use]
    pub fn shortcut(mut self, activator: SingleActivator, intent: impl Intent) -> Self {
        self.shortcuts.push((activator, Rc::new(intent)));
        self
    }
}

impl std::fmt::Debug for Shortcuts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shortcuts")
            .field("bindings", &self.shortcuts.len())
            .finish_non_exhaustive()
    }
}

impl View for Shortcuts {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for Shortcuts {
    /// The `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper
    /// (`shortcuts.dart:1134-1143`). The `Actions` chain is captured from this
    /// widget's own position (ADR-0023's resolve-at-own-position divergence) with a real dependency, so a
    /// chain that changes rebuilds this widget and the handler re-captures.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let chain = ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone());
        let shortcuts = self.shortcuts.clone();
        Focus::new(self.child.clone())
            .can_request_focus(false)
            .debug_label("Shortcuts")
            .on_key_event(Rc::new(move |event| {
                let Some(chain) = &chain else {
                    return KeyEventResult::Ignored;
                };
                // `_find` (`shortcuts.dart:892-899`): the FIRST matching
                // activator decides; an unresolvable intent falls through as
                // ignored, it does not try later activators (`:922-938`).
                let Some((_, intent)) = shortcuts
                    .iter()
                    .find(|(activator, _)| activator.matches(event))
                else {
                    return KeyEventResult::Ignored;
                };
                let intent: &dyn Any = &**intent;
                match resolve(chain, intent) {
                    // One call: invoke and read `to_key_event_result` off what
                    // it actually did (`actions.dart:312-314`), so the key
                    // result cannot disagree with the invocation.
                    Some(action) => action.invoke_for_key(intent),
                    None => KeyEventResult::Ignored,
                }
            }))
    }
}

// ============================================================================
// Default focus traversal
// ============================================================================

/// Installs the standard Tab and Shift+Tab focus traversal bindings for a
/// subtree.
///
/// Flutter's `WidgetsApp` supplies these bindings at the application root.
/// [`FocusRoot`](super::focus::FocusRoot) installs this widget automatically
/// for every standard FLUI presentation. It remains public for custom
/// embedders and deliberately isolated subtrees. Each instance binds actions
/// to its own [`BuildContext::focus_manager`], with no ambient process
/// singleton.
#[derive(Clone, Debug, StatefulView)]
pub struct DefaultFocusTraversal {
    child: BoxedView,
}

impl DefaultFocusTraversal {
    /// Wrap `child` in the standard traversal shortcuts and actions.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }
}

/// Presentation-local state behind [`DefaultFocusTraversal`].
pub struct DefaultFocusTraversalState {
    focus_owner: Option<Rc<flui_interaction::FocusManager>>,
}

impl std::fmt::Debug for DefaultFocusTraversalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultFocusTraversalState")
            .field("initialized", &self.focus_owner.is_some())
            .finish()
    }
}

impl StatefulView for DefaultFocusTraversal {
    type State = DefaultFocusTraversalState;

    fn create_state(&self) -> Self::State {
        DefaultFocusTraversalState { focus_owner: None }
    }
}

impl ViewState<DefaultFocusTraversal> for DefaultFocusTraversalState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.focus_owner = Some(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        self.focus_owner = Some(ctx.focus_manager());
    }

    fn build(&self, view: &DefaultFocusTraversal, _ctx: &dyn BuildContext) -> impl IntoView {
        let focus_owner = self
            .focus_owner
            .as_ref()
            .expect("BUG: DefaultFocusTraversal built before init_state")
            .clone();
        Actions::new(
            Shortcuts::new(view.child.clone())
                .shortcut(SingleActivator::named(NamedKey::Tab), NextFocusIntent)
                .shortcut(
                    SingleActivator::named(NamedKey::Tab).shift(),
                    PreviousFocusIntent,
                ),
        )
        .action(NextFocusAction::new(Rc::clone(&focus_owner)))
        .action(PreviousFocusAction::new(focus_owner))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{KeyState, Modifiers};
    use flui_interaction::routing::FocusNode;

    use super::*;
    use crate::SizedBox;
    use crate::test_harness::mount;

    fn key_down(character: &str, modifiers: Modifiers) -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character(character.into()),
            modifiers,
            ..KeyEvent::default()
        }
    }

    /// `_shouldAcceptModifiers` demands **equality** per modifier
    /// (`shortcuts.dart:560-565`): Ctrl+C matches Ctrl+C only — not bare C,
    /// not Ctrl+Shift+C — and never a key-up. Repeats match unless opted out
    /// (`:461`, `:576-581`).
    ///
    /// Flutter parity (`shortcuts_test.dart`, tag `3.44.0`, `SingleActivator`
    /// group): this single assertion set covers what that oracle spreads
    /// across five separate `testWidgets` cases exercising the same
    /// per-event exact-modifier-match contract through a real
    /// `HardwareKeyboard` pressed-key simulator instead of direct
    /// `KeyEvent` construction — `'isActivatedBy works as expected'`,
    /// `'handles Ctrl-C'`, `'handles repeated events'`, `'rejects repeated
    /// events if requested'`, `'handles Shift-Ctrl-C'`. Not duplicated here:
    /// FLUI's `SingleActivator::matches` reads modifiers straight off the
    /// event it is given, so the oracle's own multi-key press/release
    /// *sequencing* (physical Ctrl held across several key events) has no
    /// separate code path to pin — each event's modifier snapshot is all
    /// `matches` ever sees.
    #[test]
    fn single_activator_matches_exact_modifiers_only() {
        let ctrl_c = SingleActivator::character("c").control();

        assert!(ctrl_c.matches(&key_down("c", Modifiers::CONTROL)));
        assert!(
            !ctrl_c.matches(&key_down("c", Modifiers::empty())),
            "bare c"
        );
        assert!(
            !ctrl_c.matches(&key_down("c", Modifiers::CONTROL | Modifiers::SHIFT)),
            "an extra modifier disqualifies — exact match, not superset"
        );
        assert!(
            !ctrl_c.matches(&key_down("d", Modifiers::CONTROL)),
            "wrong key"
        );
        assert!(
            !ctrl_c.matches(&KeyEvent {
                state: KeyState::Up,
                ..key_down("c", Modifiers::CONTROL)
            }),
            "key-up never triggers"
        );

        let repeat = KeyEvent {
            repeat: true,
            ..key_down("c", Modifiers::CONTROL)
        };
        assert!(ctrl_c.matches(&repeat), "repeats match by default");
        assert!(
            !ctrl_c.clone().allow_repeats(false).matches(&repeat),
            "allow_repeats(false) rejects repeats"
        );
    }

    /// `Shortcuts` end to end (ADR-0023): a shortcut above a focused `Focus` fires
    /// only for keys that subtree **ignored** — a key the focused handler
    /// consumed never reaches the binding, and a matching ignored key fires
    /// every binding while counting as handled.
    ///
    /// Red-check: revert `dispatch_key_event` to the earlier flat dispatch —
    /// the binding never fires and the second assertion fails.
    #[test]
    fn a_shortcut_fires_only_for_keys_the_focused_subtree_ignored() {
        let fired = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("shortcut-field");

        // The inner "field" consumes the character "x" and ignores all else.
        let inner = Focus::new(SizedBox::new(10.0, 10.0))
            .focus_node(Rc::clone(&field))
            .on_key_event(Rc::new(|event| match &event.key {
                Key::Character(c) if c == "x" => KeyEventResult::Handled,
                _ => KeyEventResult::Ignored,
            }));

        let fired_for_binding = Arc::clone(&fired);
        let harness = mount(CallbackShortcuts::new(inner).binding(
            SingleActivator::character("d").control(),
            move || {
                fired_for_binding.fetch_add(1, Ordering::SeqCst);
            },
        ));
        let manager = harness.focus_manager();
        field.request_focus();

        // Consumed below: never bubbles to the shortcut.
        assert!(manager.dispatch_key_event(&key_down("x", Modifiers::empty())));
        assert_eq!(
            fired.load(Ordering::SeqCst),
            0,
            "a consumed key stays below"
        );

        // Ignored below and matching: the binding fires, the event is handled.
        assert!(manager.dispatch_key_event(&key_down("d", Modifiers::CONTROL)));
        assert_eq!(fired.load(Ordering::SeqCst), 1, "the shortcut fired");

        // Ignored below and not matching: unhandled, nothing fires.
        assert!(!manager.dispatch_key_event(&key_down("q", Modifiers::empty())));
        assert_eq!(fired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn callback_shortcuts_accept_owner_local_rc_state() {
        let fired = Rc::new(Cell::new(0));
        let fired_for_binding = Rc::clone(&fired);
        let field = FocusNode::with_debug_label("owner-local-shortcut-field");
        let harness = mount(
            CallbackShortcuts::new(
                Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)),
            )
            .binding(SingleActivator::character("l").control(), move || {
                fired_for_binding.set(fired_for_binding.get() + 1);
            }),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&key_down("l", Modifiers::CONTROL)));
        assert_eq!(fired.get(), 1, "shortcut callback captured Rc<Cell<_>>");
    }
}

#[cfg(test)]
mod intent_tests {
    use std::cell::Cell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{KeyState, Modifiers};
    use flui_interaction::routing::FocusNode;

    use super::super::actions::{Action, ActionOutcome, Actions, CallbackAction, Intent};
    use super::*;
    use crate::SizedBox;
    use crate::test_harness::mount;

    struct SaveIntent;
    impl Intent for SaveIntent {}

    fn ctrl_s() -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Character("s".into()),
            modifiers: Modifiers::CONTROL,
            ..KeyEvent::default()
        }
    }

    /// `Shortcuts` end to end (ADR-0023): Ctrl+S bubbles from the focused field,
    /// `Shortcuts` maps it to `SaveIntent`, and the enclosing `Actions` chain
    /// invokes the bound action; the event is consumed.
    ///
    /// Red-check: drop the `resolve` call from `Shortcuts::build`'s handler —
    /// nothing runs and dispatch reports unhandled.
    #[test]
    fn a_shortcut_dispatches_its_intent_through_the_actions_chain() {
        let saves = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("intent-field");

        let saves_for_action = Arc::clone(&saves);
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |_intent: &SaveIntent| {
                saves_for_action.fetch_add(1, Ordering::SeqCst);
            })),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(saves.load(Ordering::SeqCst), 1, "the action ran");

        // Bare "s" does not match the activator: unhandled, nothing runs.
        assert!(!manager.dispatch_key_event(&KeyEvent {
            modifiers: Modifiers::empty(),
            ..ctrl_s()
        }));
        assert_eq!(saves.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shortcut_actions_accept_owner_local_rc_state() {
        let saves = Rc::new(Cell::new(0));
        let saves_for_action = Rc::clone(&saves);
        let field = FocusNode::with_debug_label("owner-local-intent-field");
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |_intent: &SaveIntent| {
                saves_for_action.set(saves_for_action.get() + 1);
            })),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(saves.get(), 1, "action callback captured Rc<Cell<_>>");
    }

    #[test]
    fn shortcut_intents_accept_owner_local_rc_payloads() {
        struct OwnerLocalIntent {
            marker: Rc<Cell<u32>>,
        }
        impl Intent for OwnerLocalIntent {}

        let marker = Rc::new(Cell::new(7));
        let seen = Rc::new(Cell::new(0));
        let seen_for_action = Rc::clone(&seen);
        let field = FocusNode::with_debug_label("owner-local-intent-payload-field");
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(
                        SingleActivator::character("s").control(),
                        OwnerLocalIntent {
                            marker: Rc::clone(&marker),
                        },
                    ),
            )
            .action(CallbackAction::new(move |intent: &OwnerLocalIntent| {
                seen_for_action.set(intent.marker.get());
            })),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(
            seen.get(),
            7,
            "shortcut intent carried an owner-local Rc<Cell<_>> payload"
        );
    }

    /// The default [`Action::to_key_event_result`](super::actions::Action::to_key_event_result)
    /// maps a `NotPerformed` outcome to `SkipRemainingHandlers`
    /// (`actions.dart:312-314`): the action runs, but the event reports
    /// unconsumed and stops bubbling.
    ///
    /// Flutter parity (`actions_test.dart`, tag `3.44.0`): stands in for
    /// `'Base Action class default toKeyEventResult delegates to
    /// consumesKey'`. **Adapted, documented divergence**: Flutter splits the
    /// question across two independently overridable methods,
    /// `consumesKey`/`toKeyEventResult`, which can disagree; FLUI collapsed
    /// them into the one method this test exercises (ADR-0023/ADR-0026) —
    /// there is no separate `consumes_key` to assert delegates to anything.
    #[test]
    fn a_non_consuming_action_runs_but_leaves_the_event_unconsumed() {
        // An action that runs but changes nothing declines the key, so the
        // event keeps bubbling instead of being swallowed.
        struct NonConsuming(Arc<AtomicUsize>);
        impl Action<SaveIntent> for NonConsuming {
            fn invoke(&self, _intent: &SaveIntent) -> ActionOutcome {
                self.0.fetch_add(1, Ordering::SeqCst);
                ActionOutcome::NotPerformed
            }
        }

        let runs = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("nonconsuming-field");
        let harness = mount(
            Actions::new(
                Shortcuts::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&field)))
                    .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(NonConsuming(Arc::clone(&runs))),
        );
        let manager = harness.focus_manager();
        field.request_focus();

        assert!(
            !manager.dispatch_key_event(&ctrl_s()),
            "SkipRemainingHandlers reports the event unconsumed"
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1, "the action still ran");
    }
}

#[cfg(test)]
mod tab_tests {
    use std::rc::Rc;

    use flui_interaction::events::{KeyState, Modifiers, NamedKey};
    use flui_interaction::routing::{FocusNode, FocusScopeNode};
    use flui_view::ViewExt;

    use super::super::focus::FocusScope;
    use super::*;
    use crate::test_harness::mount;
    use crate::{Positioned, SizedBox, Stack};

    fn tab(shift: bool) -> KeyEvent {
        KeyEvent {
            state: KeyState::Down,
            key: Key::Named(NamedKey::Tab),
            modifiers: if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::empty()
            },
            ..KeyEvent::default()
        }
    }

    /// **Tab works, end to end** (ADR-0026): a real key event enters
    /// `dispatch_key_event`, bubbles from the focused field (ADR-0023),
    /// matches the `Shortcuts` activator, resolves `NextFocusIntent` through
    /// the enclosing `Actions`, and moves the focus in reading order.
    ///
    /// Note the nesting the ADR-0026 review made a binding constraint:
    /// **`Actions` must be OUTSIDE `Shortcuts`** — FLUI's `Shortcuts` resolves
    /// its action chain from its own position (ADR-0023's resolve-at-own-position divergence), so the Flutter
    /// habit of `Shortcuts(child: Actions(...))` silently dead-keys Tab.
    ///
    /// Red-check: swap the nesting to `Shortcuts::new(Actions::new(...))` —
    /// the chain resolves to `None`, the handler returns `Ignored`, and the
    /// focus never moves.
    #[test]
    fn tab_and_shift_tab_move_the_focus_through_the_actions_chain() {
        let scope = FocusScopeNode::with_debug_label("tab-scope");
        let left = FocusNode::with_debug_label("left");
        let right = FocusNode::with_debug_label("right");

        let field = |x: f32, node: &Rc<FocusNode>| {
            Positioned::new(Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(node)))
                .left(x)
                .top(0.0)
                .width(10.0)
                .height(10.0)
                .into_view()
                .boxed()
        };

        let harness = mount(FocusScope::with_external_node(
            Rc::clone(&scope),
            Stack::new(vec![field(0.0, &left), field(20.0, &right)]),
        ));
        let manager = harness.focus_manager();
        left.request_focus();

        assert!(manager.dispatch_key_event(&tab(false)), "Tab is consumed");
        assert!(
            right.has_primary_focus(),
            "Tab moved the focus to the next node in reading order"
        );

        assert!(manager.dispatch_key_event(&tab(true)), "Shift+Tab too");
        assert!(left.has_primary_focus(), "and it stepped back");
    }

    /// `NextFocusAction`'s key result is **what the traversal did**
    /// (`focus_traversal.dart:2340-2348`): with a `Stop` edge and nowhere to
    /// go, the action runs, moves nothing, and reports the event
    /// **unconsumed** — so an outer handler still gets its chance. This is the
    /// channel ADR-0023 dropped and ADR-0026's review chose to reopen with
    /// a breaking `invoke -> ActionOutcome` rather than a second, silently
    /// divergent method.
    ///
    /// Red-check: make `to_key_event_result` ignore the outcome (the trait
    /// default) — the dead Tab reports handled and swallows the key.
    #[test]
    fn a_tab_with_nowhere_to_go_reports_the_key_unconsumed() {
        use flui_interaction::routing::TraversalEdgeBehavior;

        let scope = FocusScopeNode::with_debug_label("dead-end-scope");
        scope.set_traversal_edge_behavior(TraversalEdgeBehavior::Stop);
        let only = FocusNode::with_debug_label("only");

        let harness = mount(FocusScope::with_external_node(
            Rc::clone(&scope),
            Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Rc::clone(&only)),
        ));
        let manager = harness.focus_manager();
        only.request_focus();

        assert!(
            !manager.dispatch_key_event(&tab(false)),
            "a Tab that moved nothing is reported unconsumed"
        );
        assert!(only.has_primary_focus(), "and the focus stayed put");
    }
}
