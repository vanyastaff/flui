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
//!
//! Of `DefaultTextEditingShortcuts`, [`DefaultFocusTraversal`] binds only
//! copy, cut and paste; select-all, the Insert-key clipboard chords
//! (Ctrl/Shift+Insert, Shift+Delete) and the caret-movement intents are not
//! bound (`EditableText`'s own key handler moves the caret).

use std::any::Any;
use std::rc::Rc;

use flui_interaction::events::{Key, KeyEvent, NamedKey};
use flui_interaction::routing::{FocusNode, KeyEventResult};
use flui_platform_api::TargetPlatform;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome};

use super::actions::{
    ActionChainProvider, Actions, ActivateIntent, CopySelectionTextIntent, Intent, NextFocusAction,
    NextFocusIntent, PasteTextIntent, PreviousFocusAction, PreviousFocusIntent, chain_at, resolve,
};
use super::focus::Focus;
use crate::support::event_callback;

/// A callback bound to a [`SingleActivator`] in [`CallbackShortcuts`]: it
/// receives the key event's [`EventCx`] (ADR-0086).
pub type ShortcutCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

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
    ///
    /// An ASCII letter trigger matches either case: Flutter's
    /// `LogicalKeyboardKey.keyC` names the key, not the character, while a
    /// FLUI `Key::Character` carries what the key produced — `"C"` under Caps
    /// Lock or Shift. The exact Shift check still tells Ctrl+Shift+C apart.
    #[must_use]
    pub fn matches(&self, event: &KeyEvent) -> bool {
        event.state.is_down()
            && (self.include_repeats || !event.repeat)
            && trigger_matches(&self.trigger, &event.key)
            && event.modifiers.ctrl() == self.control
            && event.modifiers.shift() == self.shift
            && event.modifiers.alt() == self.alt
            && event.modifiers.meta() == self.meta
    }
}

/// `pressed` is `trigger`, an ASCII letter compared without case.
fn trigger_matches(trigger: &Key, pressed: &Key) -> bool {
    match (trigger, pressed) {
        (Key::Character(trigger), Key::Character(pressed)) => {
            trigger == pressed
                || (trigger.len() == 1
                    && trigger.bytes().all(|byte| byte.is_ascii_alphabetic())
                    && trigger.eq_ignore_ascii_case(pressed))
        }
        _ => trigger == pressed,
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
    /// ignored. It runs inside the key event's dispatch and receives its
    /// `cx`, so it writes signals like any other event callback (ADR-0086).
    #[must_use]
    pub fn binding<F, R>(mut self, activator: SingleActivator, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.bindings.push((activator, event_callback(callback)));
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
            .on_key_event(move |cx, event| {
                let mut handled = false;
                for (activator, callback) in &bindings {
                    if activator.matches(event) {
                        callback(cx);
                        handled = true;
                    }
                }
                if handled {
                    KeyEventResult::Handled
                } else {
                    KeyEventResult::Ignored
                }
            })
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
            .on_key_event(move |cx, event| {
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
                    Some(action) => action.invoke_for_key(cx, intent),
                    None => KeyEventResult::Ignored,
                }
            })
    }
}

// ============================================================================
// Default focus traversal
// ============================================================================

/// Installs the standard keyboard bindings for a subtree: Tab and Shift+Tab
/// move the focus, Enter, Space and Select activate the focused control
/// ([`ActivateIntent`]), and Ctrl+C, Ctrl+X and Ctrl+V (Cmd on macOS and
/// iOS) copy, cut and paste in the focused text field
/// ([`CopySelectionTextIntent`], [`PasteTextIntent`]).
///
/// Flutter's `WidgetsApp` supplies the focus bindings at the application root
/// (`app.dart:1263-1276`, tag `3.44.0`), and its `DefaultTextEditingShortcuts`
/// the clipboard ones. Numpad Enter reaches FLUI as the
/// same logical `Enter`, so one binding covers both; `GameButtonA` has no
/// logical key in FLUI's key model and is not bound. The arrow-key
/// directional traversal and `Escape` → dismiss bindings are not installed
/// yet.
///
/// A clipboard chord resolves at the primary focus like every other binding
/// here: an `EditableText` answers it on its own node, and with no text field
/// focused nothing does, so the chord keeps bubbling to whatever else binds
/// it.
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
    /// The platform whose clipboard chords are bound, resolved once in
    /// `create_state` — the same single injection point `EditableText`'s
    /// word-jump modifier uses.
    platform: TargetPlatform,
}

/// Which clipboard intent a default binding carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardBinding {
    Copy,
    Cut,
    Paste,
}

/// The clipboard chords `platform` uses: Cmd on macOS and iOS, Control
/// everywhere else — Flutter's `DefaultTextEditingShortcuts` split
/// (`default_text_editing_shortcuts.dart`, tag `3.44.0`).
fn clipboard_activators(platform: TargetPlatform) -> [(SingleActivator, ClipboardBinding); 3] {
    let chord = |key: &str| {
        let activator = SingleActivator::character(key);
        match platform {
            TargetPlatform::MacOS | TargetPlatform::iOS => activator.meta(),
            _ => activator.control(),
        }
    };
    [
        (chord("c"), ClipboardBinding::Copy),
        (chord("x"), ClipboardBinding::Cut),
        (chord("v"), ClipboardBinding::Paste),
    ]
}

impl std::fmt::Debug for DefaultFocusTraversalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultFocusTraversalState")
            .field("initialized", &self.focus_owner.is_some())
            .field("keys", &self.keys.id())
            .field("platform", &self.platform)
            .finish()
    }
}

impl StatefulView for DefaultFocusTraversal {
    type State = DefaultFocusTraversalState;

    fn create_state(&self) -> Self::State {
        DefaultFocusTraversalState {
            focus_owner: None,
            keys: FocusNode::with_debug_label("Shortcuts"),
            platform: TargetPlatform::current(),
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
        let mut shortcuts = Shortcuts::new(view.child.clone())
            .focus_node(Rc::clone(&self.keys))
            .shortcut(SingleActivator::named(NamedKey::Tab), NextFocusIntent)
            .shortcut(
                SingleActivator::named(NamedKey::Tab).shift(),
                PreviousFocusIntent,
            )
            .shortcut(SingleActivator::named(NamedKey::Enter), ActivateIntent)
            .shortcut(SingleActivator::character(" "), ActivateIntent)
            .shortcut(SingleActivator::named(NamedKey::Select), ActivateIntent);
        for (activator, binding) in clipboard_activators(self.platform) {
            shortcuts = match binding {
                ClipboardBinding::Copy => {
                    shortcuts.shortcut(activator, CopySelectionTextIntent::Copy)
                }
                ClipboardBinding::Cut => {
                    shortcuts.shortcut(activator, CopySelectionTextIntent::Cut)
                }
                ClipboardBinding::Paste => shortcuts.shortcut(activator, PasteTextIntent),
            };
        }
        Actions::new(shortcuts)
            .action(NextFocusAction::new(Rc::clone(&focus_owner)))
            .action(PreviousFocusAction::new(focus_owner))
    }
}

// The mounted `Shortcuts` suites live in `crates/flui-widgets/tests/shortcuts.rs`.
