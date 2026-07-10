//! [`SingleActivator`] and [`CallbackShortcuts`] — keyboard shortcuts riding
//! the leaf→root key dispatch.
//!
//! ADR-0023 U2. A shortcut widget is, mechanically, a
//! `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper
//! (`shortcuts.dart:1134-1143`, `:1225-1231`): it sees a key only when every
//! `Focus` below it — most importantly the focused field — *ignored* the
//! event and the ADR-0023 U1 walk bubbled it up.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/shortcuts.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `SingleActivator` (`:433-581`),
//! `CallbackShortcuts` (`:1181-1231`).
//!
//! # Deferred, and named (ADR-0023 §4)
//!
//! `LogicalKeySet` (needs a `HardwareKeyboard`-style pressed-set tracker),
//! `CharacterActivator` (no consumer), a shared `ShortcutManager`, and
//! `includeSemantics`. The Intent-mapped [`Shortcuts`] resolves its
//! [`Actions`](super::actions::Actions) chain from **its own position**, not
//! the focused leaf's context (ADR-0023 O-1) — visible only when an `Actions`
//! sits between the focused widget and the `Shortcuts`.

use std::any::Any;
use std::sync::Arc;

use flui_interaction::events::{Key, KeyEvent, NamedKey};
use flui_interaction::routing::KeyEventResult;
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::actions::{ActionChainProvider, Intent, resolve};
use super::focus::Focus;

/// A callback bound to a [`SingleActivator`] in [`CallbackShortcuts`].
pub type ShortcutCallback = Arc<dyn Fn() + Send + Sync>;

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
/// `Intent`-mapped `Shortcuts`/`Actions` pair is ADR-0023 U3/U4.
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
    pub fn binding(
        mut self,
        activator: SingleActivator,
        callback: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        self.bindings.push((activator, Arc::new(callback)));
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
            .on_key_event(Arc::new(move |event| {
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
/// (`ShortcutManager.handleKeypress`, `:922-938`); the event is consumed
/// unless that action's [`consumes_key`](super::actions::Action::consumes_key)
/// declines, which stops the bubbling *without* consuming
/// (`actions.dart:312-314`). No match, or no enabled action: the key keeps
/// bubbling.
#[derive(Clone)]
pub struct Shortcuts {
    shortcuts: Vec<(SingleActivator, Arc<dyn Intent>)>, // PORT-CHECK-OK-DYN: ADR-0023 U4 — Flutter's `Map<ShortcutActivator, Intent>`; read back only through its own TypeId.
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
        self.shortcuts.push((activator, Arc::new(intent)));
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
    /// widget's own position (ADR-0023 O-1) with a real dependency, so a
    /// chain that changes rebuilds this widget and the handler re-captures.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let chain = ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone());
        let shortcuts = self.shortcuts.clone();
        Focus::new(self.child.clone())
            .can_request_focus(false)
            .debug_label("Shortcuts")
            .on_key_event(Arc::new(move |event| {
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
                    Some(action) => {
                        let consumes = action.consumes_key(intent);
                        action.invoke(intent);
                        // `Action.toKeyEventResult` (`actions.dart:312-314`).
                        if consumes {
                            KeyEventResult::Handled
                        } else {
                            KeyEventResult::SkipRemainingHandlers
                        }
                    }
                    None => KeyEventResult::Ignored,
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{KeyState, Modifiers};
    use flui_interaction::routing::{FocusManager, FocusNode};

    use super::*;
    use crate::SizedBox;
    use crate::test_harness::{FOCUS_TEST_LOCK, mount};

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

    /// ADR-0023 U1+U2 end to end: a shortcut above a focused `Focus` fires
    /// only for keys that subtree **ignored** — a key the focused handler
    /// consumed never reaches the binding, and a matching ignored key fires
    /// every binding while counting as handled.
    ///
    /// Red-check: revert `dispatch_key_event` to the flat pre-U1 dispatch —
    /// the binding never fires and the second assertion fails.
    #[test]
    fn a_shortcut_fires_only_for_keys_the_focused_subtree_ignored() {
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let fired = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("shortcut-field");

        // The inner "field" consumes the character "x" and ignores all else.
        let inner = Focus::new(SizedBox::new(10.0, 10.0))
            .focus_node(Arc::clone(&field))
            .on_key_event(Arc::new(|event| match &event.key {
                Key::Character(c) if c == "x" => KeyEventResult::Handled,
                _ => KeyEventResult::Ignored,
            }));

        let fired_for_binding = Arc::clone(&fired);
        let _harness = mount(CallbackShortcuts::new(inner).binding(
            SingleActivator::character("d").control(),
            move || {
                fired_for_binding.fetch_add(1, Ordering::SeqCst);
            },
        ));
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

        manager.unfocus();
    }
}

#[cfg(test)]
mod intent_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::events::{KeyState, Modifiers};
    use flui_interaction::routing::{FocusManager, FocusNode};

    use super::super::actions::{Action, Actions, CallbackAction, Intent};
    use super::*;
    use crate::SizedBox;
    use crate::test_harness::{FOCUS_TEST_LOCK, mount};

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

    /// ADR-0023 U3+U4 end to end: Ctrl+S bubbles from the focused field,
    /// `Shortcuts` maps it to `SaveIntent`, and the enclosing `Actions` chain
    /// invokes the bound action; the event is consumed.
    ///
    /// Red-check: drop the `resolve` call from `Shortcuts::build`'s handler —
    /// nothing runs and dispatch reports unhandled.
    #[test]
    fn a_shortcut_dispatches_its_intent_through_the_actions_chain() {
        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let saves = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("intent-field");

        let saves_for_action = Arc::clone(&saves);
        let _harness = mount(
            Actions::new(
                Shortcuts::new(
                    Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Arc::clone(&field)),
                )
                .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(CallbackAction::new(move |_intent: &SaveIntent| {
                saves_for_action.fetch_add(1, Ordering::SeqCst);
            })),
        );
        field.request_focus();

        assert!(manager.dispatch_key_event(&ctrl_s()), "consumed");
        assert_eq!(saves.load(Ordering::SeqCst), 1, "the action ran");

        // Bare "s" does not match the activator: unhandled, nothing runs.
        assert!(!manager.dispatch_key_event(&KeyEvent {
            modifiers: Modifiers::empty(),
            ..ctrl_s()
        }));
        assert_eq!(saves.load(Ordering::SeqCst), 1);

        manager.unfocus();
    }

    /// `Action::consumes_key` = false maps to `SkipRemainingHandlers`
    /// (`actions.dart:312-314`): the action runs, but the event reports
    /// unconsumed and stops bubbling.
    #[test]
    fn a_non_consuming_action_runs_but_leaves_the_event_unconsumed() {
        struct NonConsuming(Arc<AtomicUsize>);
        impl Action<SaveIntent> for NonConsuming {
            fn consumes_key(&self, _intent: &SaveIntent) -> bool {
                false
            }
            fn invoke(&self, _intent: &SaveIntent) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let _guard = FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let runs = Arc::new(AtomicUsize::new(0));
        let field = FocusNode::with_debug_label("nonconsuming-field");
        let _harness = mount(
            Actions::new(
                Shortcuts::new(
                    Focus::new(SizedBox::new(10.0, 10.0)).focus_node(Arc::clone(&field)),
                )
                .shortcut(SingleActivator::character("s").control(), SaveIntent),
            )
            .action(NonConsuming(Arc::clone(&runs))),
        );
        field.request_focus();

        assert!(
            !manager.dispatch_key_event(&ctrl_s()),
            "SkipRemainingHandlers reports the event unconsumed"
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1, "the action still ran");

        manager.unfocus();
    }
}
