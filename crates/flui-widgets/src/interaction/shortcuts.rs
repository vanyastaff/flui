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
//! `includeSemantics`. The Intent-mapped [`Shortcuts`] resolves its intent at
//! the **primary focus**, as Flutter does: each `Focus` records the
//! [`Actions`] chain visible at its position on its node, so an `Actions`
//! between the focused widget and the `Shortcuts` — a button's activation —
//! is found (ADR-0079).

use std::any::Any;
use std::rc::Rc;

use flui_interaction::events::{Key, KeyEvent, NamedKey};
use flui_interaction::routing::{FocusNode, KeyEventResult};
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::actions::{
    ActionChainProvider, Actions, ActivateIntent, Intent, NextFocusAction, NextFocusIntent,
    PreviousFocusAction, PreviousFocusIntent, chain_at, resolve,
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
/// [`Actions`] chain — Flutter's `Shortcuts`
/// (`shortcuts.dart:1004`).
///
/// On a key the focused subtree ignored, the **first** matching activator's
/// intent resolves to the nearest enabled action enclosing the **primary
/// focus** (`ShortcutManager.handleKeypress`, `:922-938`, which resolves
/// against `primaryFocus.context`), so an `Actions` between the focused
/// widget and this one — a button's activation — takes part. When the focused
/// node records no chain (no `Focus` widget hosts it), this widget's own
/// position is used. That action's
/// [`to_key_event_result`](super::actions::Action::to_key_event_result) decides
/// the final [`KeyEventResult`] — it is an overridable method, so the action has
/// the last word. Its default consumes the
/// key when the action performed its work, and reports the event unconsumed —
/// stopping the bubbling *without* consuming — when the action declined
/// (`actions.dart:312-314`); an action may override it to decide otherwise. No
/// match, or no enabled action: the key keeps bubbling.
#[derive(Clone, StatefulView)]
pub struct Shortcuts {
    bindings: Vec<(SingleActivator, Rc<dyn Intent>)>, // ADR-0023 — Flutter's `Map<ShortcutActivator, Intent>`; read back only through its own TypeId.
    child: BoxedView,
    /// The node its key handler lives on, when the owner needs to name it.
    focus_node: Option<Rc<FocusNode>>,
}

impl Shortcuts {
    /// A shortcut boundary around `child` with no bindings yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            bindings: Vec::new(),
            child: BoxedView(Box::new(child.into_view())),
            focus_node: None,
        }
    }

    /// Host the key handler on `node`, which the caller owns.
    fn focus_node(mut self, node: Rc<FocusNode>) -> Self {
        self.focus_node = Some(node);
        self
    }

    /// Bind `activator` to `intent`. Earlier bindings match first.
    #[must_use]
    pub fn shortcut(mut self, activator: SingleActivator, intent: impl Intent) -> Self {
        self.bindings.push((activator, Rc::new(intent)));
        self
    }
}

impl std::fmt::Debug for Shortcuts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shortcuts")
            .field("bindings", &self.bindings.len())
            .finish_non_exhaustive()
    }
}

/// Presentation-local state behind [`Shortcuts`]: the focus owner whose
/// primary focus an intent resolves at.
pub struct ShortcutsState {
    focus_manager: Option<Rc<flui_interaction::FocusManager>>,
}

impl std::fmt::Debug for ShortcutsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShortcutsState")
            .field("initialized", &self.focus_manager.is_some())
            .finish()
    }
}

impl StatefulView for Shortcuts {
    type State = ShortcutsState;

    fn create_state(&self) -> Self::State {
        ShortcutsState {
            focus_manager: None,
        }
    }
}

impl ViewState<Shortcuts> for ShortcutsState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());
    }

    /// The `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper
    /// (`shortcuts.dart:1134-1143`). This widget's own chain is captured with
    /// a real dependency, as the fallback for a focused node that records
    /// none; the primary focus's chain is read at key time.
    fn build(&self, view: &Shortcuts, ctx: &dyn BuildContext) -> impl IntoView {
        let own_chain = ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone());
        let focus_manager = Rc::clone(
            self.focus_manager
                .as_ref()
                .expect("BUG: Shortcuts built before init_state"),
        );
        let shortcuts = view.bindings.clone();
        let mut focus = Focus::new(view.child.clone());
        if let Some(node) = &view.focus_node {
            focus = focus.focus_node(Rc::clone(node));
        }
        focus
            .can_request_focus(false)
            .debug_label("Shortcuts")
            .on_key_event(Rc::new(move |event| {
                // `_find` (`shortcuts.dart:892-899`): the FIRST matching
                // activator decides; an unresolvable intent falls through as
                // ignored, it does not try later activators (`:922-938`).
                let Some((_, intent)) = shortcuts
                    .iter()
                    .find(|(activator, _)| activator.matches(event))
                else {
                    return KeyEventResult::Ignored;
                };
                let chain = focus_manager
                    .primary_focus()
                    .and_then(|focused| chain_at(&focused))
                    .or_else(|| own_chain.clone());
                let Some(chain) = chain else {
                    return KeyEventResult::Ignored;
                };
                let intent: &dyn Any = &**intent;
                match resolve(&chain, intent) {
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

/// Installs the standard keyboard bindings for a subtree: Tab and Shift+Tab
/// move the focus, and Enter, Space and Select activate the focused control
/// ([`ActivateIntent`]).
///
/// Flutter's `WidgetsApp` supplies these bindings at the application root
/// (`app.dart:1263-1276`, tag `3.44.0`). Numpad Enter reaches FLUI as the
/// same logical `Enter`, so one binding covers both; `GameButtonA` has no
/// logical key in FLUI's key model and is not bound. The arrow-key
/// directional traversal and `Escape` → dismiss bindings are not installed
/// yet.
/// [`FocusRoot`](super::focus::FocusRoot) installs this widget automatically
/// for every standard FLUI presentation. It remains public for custom
/// embedders and deliberately isolated subtrees. Each instance binds actions
/// to its own [`LifecycleContext::focus_manager`], with no ambient process
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
    /// The node the bindings' key handler lives on, which the focus owner
    /// starts a key's walk at while nothing is focused.
    keys: Rc<FocusNode>,
}

impl std::fmt::Debug for DefaultFocusTraversalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultFocusTraversalState")
            .field("initialized", &self.focus_owner.is_some())
            .field("keys", &self.keys.id())
            .finish()
    }
}

impl StatefulView for DefaultFocusTraversal {
    type State = DefaultFocusTraversalState;

    fn create_state(&self) -> Self::State {
        DefaultFocusTraversalState {
            focus_owner: None,
            keys: FocusNode::with_debug_label("Shortcuts"),
        }
    }
}

impl DefaultFocusTraversalState {
    /// Make these bindings where a key starts while nothing is focused, so
    /// the first Tab into a window with no focus reaches them. A nested
    /// instance's claim covers this one only while it is mounted.
    fn claim_unfocused_keys(&mut self, owner: Rc<flui_interaction::FocusManager>) {
        self.release_unfocused_keys();
        owner.claim_unfocused_keys(&self.keys);
        self.focus_owner = Some(owner);
    }

    /// Withdraw this instance's claim.
    fn release_unfocused_keys(&self) {
        if let Some(owner) = &self.focus_owner {
            owner.release_unfocused_keys(&self.keys);
        }
    }
}

impl ViewState<DefaultFocusTraversal> for DefaultFocusTraversalState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.claim_unfocused_keys(ctx.focus_manager());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let owner = ctx.focus_manager();
        if self
            .focus_owner
            .as_ref()
            .is_none_or(|held| !Rc::ptr_eq(held, &owner))
        {
            self.claim_unfocused_keys(owner);
        }
    }

    fn dispose(&mut self) {
        self.release_unfocused_keys();
    }

    fn build(&self, view: &DefaultFocusTraversal, _ctx: &dyn BuildContext) -> impl IntoView {
        let focus_owner = self
            .focus_owner
            .as_ref()
            .expect("BUG: DefaultFocusTraversal built before init_state")
            .clone();
        Actions::new(
            Shortcuts::new(view.child.clone())
                .focus_node(Rc::clone(&self.keys))
                .shortcut(SingleActivator::named(NamedKey::Tab), NextFocusIntent)
                .shortcut(
                    SingleActivator::named(NamedKey::Tab).shift(),
                    PreviousFocusIntent,
                )
                .shortcut(SingleActivator::named(NamedKey::Enter), ActivateIntent)
                .shortcut(SingleActivator::character(" "), ActivateIntent)
                .shortcut(SingleActivator::named(NamedKey::Select), ActivateIntent),
        )
        .action(NextFocusAction::new(Rc::clone(&focus_owner)))
        .action(PreviousFocusAction::new(focus_owner))
    }
}

// The mounted `Shortcuts` suites live in `crates/flui-widgets/tests/shortcuts.rs`.
#[cfg(test)]
mod tests {
    use flui_interaction::events::{KeyState, Modifiers};

    use super::*;

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
}
