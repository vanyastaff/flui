//! [`SingleActivator`] and [`CallbackShortcuts`] — keyboard shortcuts riding
//! the leaf→root key dispatch.
//!
//! ADR-0023 and ADR-0156. A shortcut widget is, mechanically, a
//! `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper: it sees a key only
//! when every `Focus` below it — most importantly the focused field —
//! *ignored* the event and the ADR-0023 walk bubbled it up.
//!
//! # Deferred, and named (ADR-0023)
//!
//! `LogicalKeySet` (needs a `HardwareKeyboard`-style pressed-set tracker),
//! `CharacterActivator` (no consumer), a shared `ShortcutManager`, and
//! `includeSemantics`. The Intent-mapped [`Shortcuts`] resolves its intent at
//! the **primary focus**: each `Focus` records the
//! [`Actions`] chain visible at its position on its node, so an `Actions`
//! between the focused widget and the `Shortcuts` — a button's activation —
//! is found (ADR-0079).
//!
//! Of the standard text-editing shortcuts, [`DefaultFocusTraversal`] binds only
//! copy, cut, paste and select-all; the Insert-key clipboard chords
//! (Ctrl/Shift+Insert, Shift+Delete) and the caret-movement intents are not
//! bound (`EditableText`'s own key handler moves the caret).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use flui_interaction::focus::{FocusNode, KeyEventResult};
use flui_platform::TargetPlatform;
use flui_platform::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey,
};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome};

use super::actions::{
    Actions, ActivateIntent, ActivationPressIntent, BackIntent, CopySelectionTextIntent,
    DirectionalFocusAction, DirectionalFocusIntent, DismissIntent, EnclosingActions, Intent,
    NextFocusAction, NextFocusIntent, PasteTextIntent, PreviousFocusAction, PreviousFocusIntent,
    SelectAllTextIntent, ShowContextMenuIntent, chain_at, platform_at, resolve,
};
use super::focus::Focus;
use crate::support::{command_modifiers, event_callback};

/// A callback bound to a [`SingleActivator`] in [`CallbackShortcuts`]: it
/// receives the key event's [`EventCx`] (ADR-0086).
pub type ShortcutCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

// ============================================================================
// SingleActivator
// ============================================================================

/// A shortcut trigger: one logical key plus a modifier policy.
///
/// `SingleActivator::character("c").control()` matches Ctrl+C and *only*
/// Ctrl+C: an event with an extra Shift held does not match, because each
/// modifier must be equal. Key-repeat events match by default; opt out with
/// [`allow_repeats(false)`](Self::allow_repeats). An activator matches
/// key-down events, or only the key's release once built with
/// [`on_release`](Self::on_release).
///
/// Matching uses ordinary Control/Alt/Meta/Shift dimensions. Explicit AltGraph
/// excludes Control/Alt aliases while preserving Meta/Shift; incidental locks,
/// Fn and Symbol do not affect matching. Produced-character triggers remain
/// usable with AltGraph. A source-composing key never matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingleActivator {
    trigger: Key,
    control: bool,
    shift: Option<bool>,
    alt: bool,
    meta: bool,
    include_repeats: bool,
    on_release: bool,
}

impl SingleActivator {
    /// An activator for the logical key `trigger`, no modifiers.
    #[must_use]
    pub fn new(trigger: Key) -> Self {
        Self {
            trigger,
            control: false,
            shift: Some(false),
            alt: false,
            meta: false,
            include_repeats: true,
            on_release: false,
        }
    }

    /// An activator for the character `character` produces — `"c"`, `"+"`.
    #[must_use]
    pub fn character(character: impl Into<String>) -> Self {
        Self::new(Key::character(character))
    }

    /// A character trigger independent of the Shift key needed to produce it.
    /// Control, Alt and Meta still match exactly. Named keys always use exact Shift.
    #[must_use]
    pub fn character_ignoring_shift(character: impl Into<String>) -> Self {
        Self {
            shift: None,
            ..Self::character(character)
        }
    }

    /// An activator for a named (non-character) key — `NamedKey::Escape`.
    #[must_use]
    pub fn named(key: NamedKey) -> Self {
        Self::new(Key::Named(key))
    }

    /// Require the Control modifier.
    #[must_use]
    pub fn control(mut self) -> Self {
        self.control = true;
        self
    }

    /// Require the Shift modifier (`:497`).
    #[must_use]
    pub fn shift(mut self) -> Self {
        self.shift = Some(true);
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

    /// Match the key's release instead of its key-down — Space activates a
    /// control when it comes up, so holding it shows the control pressed
    /// first. The modifiers are those held at the release. A release
    /// arrives once per press, so the repeat policy does not apply to it.
    #[must_use]
    pub fn on_release(mut self) -> Self {
        self.on_release = true;
        self
    }

    /// The logical key that triggers the activator.
    #[must_use]
    pub fn trigger(&self) -> &Key {
        &self.trigger
    }

    /// The modifiers a press must hold, as a menu's shortcut hint shows
    /// them. A character trigger that ignores Shift
    /// ([`character_ignoring_shift`](Self::character_ignoring_shift)) reports
    /// it without Shift.
    #[must_use]
    pub fn modifiers(&self) -> Modifiers {
        [
            (Modifiers::CONTROL, self.control),
            (Modifiers::SHIFT, self.shift.unwrap_or(false)),
            (Modifiers::ALT, self.alt),
            (Modifiers::META, self.meta),
        ]
        .into_iter()
        .filter(|(_, held)| *held)
        .fold(Modifiers::NONE, |modifiers, (modifier, _)| {
            modifiers | modifier
        })
    }

    /// Whether `event` triggers this activator: a key-down (or allowed repeat)
    /// of exactly the trigger key under exactly the required modifiers — or,
    /// for an [`on_release`](Self::on_release) activator, that key's release.
    ///
    /// An ASCII letter trigger matches either case: the trigger names the
    /// key, not the character, while a
    /// FLUI `Key::Character` carries what the key produced — `"C"` under Caps
    /// Lock or Shift. The exact Shift check still tells Ctrl+Shift+C apart.
    #[must_use]
    pub fn matches(&self, event: &KeyEvent) -> bool {
        let modifiers = command_modifiers(event.modifiers);
        let phase = if self.on_release {
            event.state() == KeyState::Up
        } else {
            event.state() == KeyState::Down
                && (self.include_repeats || event.repeat() == KeyRepeat::First)
        };
        phase
            && event.composition != ImeComposition::Active
            && (trigger_matches(&self.trigger, &event.key) || self.matches_latin_position(event))
            && modifiers.contains(Modifiers::CONTROL) == self.control
            && self
                .shift
                .is_none_or(|required| modifiers.contains(Modifiers::SHIFT) == required)
            && modifiers.contains(Modifiers::ALT) == self.alt
            && modifiers.contains(Modifiers::META) == self.meta
    }

    /// Whether a command chord typed on a layout without Latin letters is the
    /// chord's Latin letter or digit by its physical key: Ctrl+С on a Russian
    /// layout is Ctrl+C. Only a Control or Meta activator qualifies, and only
    /// when the layout produced a non-ASCII character, so a layout that has
    /// the letter (AZERTY, Dvorak) keeps matching by what the key produced.
    ///
    /// [`overlaps`](Self::overlaps) cannot see this branch: which character a
    /// layout puts on a physical key is not known from the activators alone,
    /// so Ctrl+C beside Ctrl+с is not reported as a duplicate, and the first
    /// match wins. The Latin chord already covers that layout.
    fn matches_latin_position(&self, event: &KeyEvent) -> bool {
        let Key::Character(produced) = &event.key else {
            return false;
        };
        let Key::Character(trigger) = &self.trigger else {
            return false;
        };
        (self.control || self.meta)
            && !produced.as_str().is_ascii()
            && latin_of_code(event.code).is_some_and(|latin| {
                let shifted = event.modifiers.contains(Modifiers::SHIFT);
                let wanted = trigger.as_str();
                wanted.eq_ignore_ascii_case(latin)
                    || (shifted && shifted_latin(latin).is_some_and(|symbol| wanted == symbol))
            })
    }

    /// Whether one key event can trigger both `self` and `other`, by the
    /// rules of [`matches`](Self::matches): letters without case, an ignored
    /// Shift beside either Shift state, and a first key-down whatever the
    /// repeat policy.
    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        let shift = match (self.shift, other.shift) {
            (Some(ours), Some(theirs)) => ours == theirs,
            _ => true,
        };
        self.on_release == other.on_release
            && self.control == other.control
            && self.alt == other.alt
            && self.meta == other.meta
            && shift
            && (trigger_matches(&self.trigger, &other.trigger)
                || trigger_matches(&other.trigger, &self.trigger))
    }
}

/// The letter, digit or punctuation mark a US layout puts on the physical key
/// `code`.
fn latin_of_code(code: Code) -> Option<&'static str> {
    let name = code.as_str();
    if let Some(tail) = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
    {
        return (tail.len() == 1).then_some(tail);
    }
    Some(match name {
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Minus" => "-",
        "Equal" => "=",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backquote" => "`",
        "Backslash" => "\\",
        _ => return None,
    })
}

/// The symbol a US layout puts on the same key as `plain` under Shift.
fn shifted_latin(plain: &str) -> Option<&'static str> {
    Some(match plain {
        "1" => "!",
        "2" => "@",
        "3" => "#",
        "4" => "$",
        "5" => "%",
        "6" => "^",
        "7" => "&",
        "8" => "*",
        "9" => "(",
        "0" => ")",
        "[" => "{",
        "]" => "}",
        "-" => "_",
        "=" => "+",
        "," => "<",
        "." => ">",
        "/" => "?",
        ";" => ":",
        "'" => "\"",
        "`" => "~",
        "\\" => "|",
        _ => return None,
    })
}

/// `pressed` is `trigger`, an ASCII letter compared without case.
fn trigger_matches(trigger: &Key, pressed: &Key) -> bool {
    match (trigger, pressed) {
        (Key::Character(trigger), Key::Character(pressed)) => {
            let trigger = trigger.as_str();
            let pressed = pressed.as_str();
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

/// Binds key combinations to callbacks for its subtree — the `Intent`-free
/// shortcut widget.
///
/// While the primary focus sits inside `child`, a key event that every inner
/// `Focus` ignored bubbles here; **every** matching binding fires
/// and the event counts as handled iff at least one did. The
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
            child: BoxedView::new(child.into_view()),
        }
    }

    /// Fire `callback` whenever `activator` matches a key the focused subtree
    /// ignored. It runs inside the key event's dispatch and receives its
    /// `ctx`, so it writes signals like any other event callback (ADR-0086).
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
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for CallbackShortcuts {
    /// `Focus(canRequestFocus: false, onKeyEvent: …)` around the child,
    /// so it sees only the keys the focused subtree ignored.
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let bindings = self.bindings.clone();
        Focus::new(self.child.clone())
            .can_request_focus(false)
            .debug_label("CallbackShortcuts")
            .on_key_event(move |ctx, event| {
                let mut handled = false;
                for (activator, callback) in &bindings {
                    if activator.matches(event) {
                        callback(ctx);
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
/// [`Actions`] chain.
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
/// stopping the bubbling *without* consuming — when the action declined;
/// an action may override it to decide otherwise. No
/// match, or no enabled action: the key keeps bubbling.
#[derive(Clone, StatefulView)]
pub struct Shortcuts {
    bindings: Vec<Binding>, // ADR-0023 — activator-to-intent pairs; read back only through its own TypeId.
    child: BoxedView,
    /// The node its key handler lives on, when the owner needs to name it.
    focus_node: Option<Rc<FocusNode>>,
    /// Bindings resolved by the focused widget's convention, and the
    /// platform they follow where the focused node records none.
    by_convention: Option<(Rc<ConventionBindings>, TargetPlatform)>,
}

impl Shortcuts {
    /// A shortcut boundary around `child` with no bindings yet.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            bindings: Vec::new(),
            child: BoxedView::new(child.into_view()),
            focus_node: None,
            by_convention: None,
        }
    }

    /// Host the key handler on `node`, which the caller owns.
    fn focus_node(mut self, node: Rc<FocusNode>) -> Self {
        self.focus_node = Some(node);
        self
    }

    /// Also match `bindings` for the platform convention the focused widget
    /// recorded, or `fallback` where it recorded none. Checked after the
    /// fixed bindings.
    fn by_convention(mut self, bindings: Rc<ConventionBindings>, fallback: TargetPlatform) -> Self {
        self.by_convention = Some((bindings, fallback));
        self
    }

    /// Bind `activator` to `intent`. Earlier bindings match first.
    #[must_use]
    pub fn shortcut(mut self, activator: SingleActivator, intent: impl Intent) -> Self {
        self.bindings.push((activator, Rc::new(intent)));
        self
    }

    /// Bind `activator` to an intent another owner shares.
    pub(crate) fn shared_shortcut(
        mut self,
        activator: SingleActivator,
        intent: Rc<dyn Intent>,
    ) -> Self {
        self.bindings.push((activator, intent));
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
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

    /// The `Focus(canRequestFocus: false, onKeyEvent: …)` wrapper.
    /// This widget's own chain is captured with
    /// a real dependency, as the fallback for a focused node that records
    /// none; the primary focus's chain is read at key time.
    fn build(&self, view: &Shortcuts, ctx: &dyn BuildContext) -> impl IntoView {
        let own_chain = EnclosingActions::depend_on(ctx);
        let focus_manager = Rc::clone(
            self.focus_manager
                .as_ref()
                .expect("BUG: Shortcuts built before init_state"),
        );
        let shortcuts = view.bindings.clone();
        let by_convention = view.by_convention.clone();
        let mut focus = Focus::new(view.child.clone());
        if let Some(node) = &view.focus_node {
            focus = focus.focus_node(Rc::clone(node));
        }
        focus
            .can_request_focus(false)
            .debug_label("Shortcuts")
            .on_key_event(move |ctx, event| {
                // The FIRST matching activator decides; an unresolvable
                // intent falls through as ignored, it does not try later
                // activators.
                let focused = focus_manager.primary_focus();
                let matching = |bindings: &[Binding]| {
                    bindings
                        .iter()
                        .find(|(activator, _)| activator.matches(event))
                        .map(|(_, intent)| Rc::clone(intent))
                };
                let found = matching(&shortcuts).or_else(|| {
                    let (bindings, fallback) = by_convention.as_ref()?;
                    let platform = focused
                        .as_deref()
                        .and_then(platform_at)
                        .unwrap_or(*fallback);
                    matching(&bindings.for_platform(platform))
                });
                let Some(intent) = found else {
                    return KeyEventResult::Ignored;
                };
                let chain = focused
                    .as_deref()
                    .and_then(chain_at)
                    .or_else(|| own_chain.clone());
                let Some(chain) = chain else {
                    return KeyEventResult::Ignored;
                };
                let intent: &dyn Any = &*intent;
                match resolve(&chain, intent) {
                    // One call: invoke and read `to_key_event_result` off what
                    // it actually did, so the key result cannot disagree with
                    // the invocation.
                    Some(action) => action.invoke_for_key(ctx, intent),
                    None => KeyEventResult::Ignored,
                }
            })
    }
}

// ============================================================================
// Default focus traversal
// ============================================================================

/// Installs the standard keyboard bindings for a subtree:
///
/// | Key | Intent | Repeats |
/// |---|---|---|
/// | Tab, Shift+Tab | [`NextFocusIntent`], [`PreviousFocusIntent`] | yes |
/// | Enter, Select (key-down) | [`ActivateIntent`] | no |
/// | Space key-down / Space release | [`ActivationPressIntent`] / [`ActivateIntent`] | no |
/// | Escape | [`DismissIntent`] | no |
/// | Alt+Left, `BrowserBack` (Cmd+[, `BrowserBack` on macOS and iOS) | [`BackIntent`] | no |
/// | unmodified arrows | [`DirectionalFocusIntent`] | yes |
/// | Menu, Shift+F10 (neither on macOS and iOS; Menu alone on Android) | [`ShowContextMenuIntent`] | no |
///
/// Ctrl+C, Ctrl+X and Ctrl+V (Cmd on macOS and iOS) copy, cut and paste in
/// the focused text field ([`CopySelectionTextIntent`], [`PasteTextIntent`]),
/// and Ctrl+A (Cmd+A on Apple platforms) selects all its text
/// ([`SelectAllTextIntent`]).
///
/// The chords that differ by platform (select all, the clipboard, back and
/// the context menu) follow the
/// [`PlatformConvention`](flui_view::environment::PlatformConvention) the
/// focused widget is built under, so a subtree that names another platform
/// answers its own chords; with nothing focused they follow this widget's.
///
/// Holding Enter activates once: a control is activated once per physical
/// press. Space activates on its release, as WinUI and the web do, so a
/// control can show itself pressed while Space is held. Apple platforms go
/// back with Cmd+[ because Option+Left moves the caret by word there.
///
/// The application root is where these bindings are meant to be installed.
/// Numpad Enter reaches FLUI as the
/// same logical `Enter`, so one binding covers both; `GameButtonA` has no
/// logical key in FLUI's key model and is not bound. An intent nothing in the
/// focused chain answers — Back on the first page, Escape with no modal
/// surface — leaves the key unhandled for the host.
///
/// A clipboard chord resolves at the primary focus like every other binding
/// here: an `EditableText` answers it on its own node, and with no text field
/// focused nothing does, so the chord keeps bubbling to whatever else binds
/// it.
///
/// [`app_root`](crate::app_root) installs this widget above every
/// presentation root `flui-app` and `HeadlessHost` attach; the UI runtime
/// itself installs no key bindings. It remains public for embedders that
/// compose their own root and for deliberately isolated subtrees. Each
/// instance binds actions to its own [`LifecycleContext::focus_manager`],
/// with no ambient process singleton.
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
    /// The platform-dependent bindings, built once per platform a focused
    /// widget follows.
    convention: Rc<ConventionBindings>,
}

/// Which clipboard intent a default binding carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardBinding {
    Copy,
    Cut,
    Paste,
}

/// The clipboard chords `platform` uses: Cmd on macOS and iOS, Control
/// everywhere else.
fn clipboard_activators(platform: TargetPlatform) -> [(SingleActivator, ClipboardBinding); 3] {
    let chord = |key: &str| command_activator(platform, key);
    [
        (chord("c"), ClipboardBinding::Copy),
        (chord("x"), ClipboardBinding::Cut),
        (chord("v"), ClipboardBinding::Paste),
    ]
}

/// The chords that go back one page on `platform`: Cmd+[ on macOS and iOS,
/// where Option+Left moves by word, Alt+Left elsewhere; `BrowserBack`
/// everywhere (the mouse's back button arrives as one).
fn back_activators(platform: TargetPlatform) -> [SingleActivator; 2] {
    let chord = match platform {
        TargetPlatform::MacOS | TargetPlatform::iOS => SingleActivator::character("[").meta(),
        _ => SingleActivator::named(NamedKey::ArrowLeft).alt(),
    };
    [chord, SingleActivator::named(NamedKey::BrowserBack)]
}

impl ShowContextMenuIntent {
    /// The keys [`DefaultFocusTraversal`] binds to this intent on
    /// `platform`: the Menu key everywhere but Apple platforms, which have
    /// none, and Shift+F10 on the desktop platforms that use it (Windows,
    /// Linux, Fuchsia). Where the list is empty, a menu's keyboard path is its
    /// focus-revealed control (ADR-0216 §2). A menu shows these as its
    /// shortcut hint.
    #[must_use]
    pub fn default_keys(platform: TargetPlatform) -> Vec<SingleActivator> {
        let once = |activator: SingleActivator| activator.allow_repeats(false);
        match platform {
            TargetPlatform::MacOS | TargetPlatform::iOS => Vec::new(),
            TargetPlatform::Android => vec![once(SingleActivator::named(NamedKey::ContextMenu))],
            _ => vec![
                once(SingleActivator::named(NamedKey::ContextMenu)),
                once(SingleActivator::named(NamedKey::F10).shift()),
            ],
        }
    }
}

/// What a key a gesture command must not take already does on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReservedKey {
    /// Tab and Shift+Tab move focus; Enter, Space and Select activate.
    Traversal,
    /// Escape cancels, closes or returns to rest: never a model effect.
    Escape,
    /// Goes back one page.
    Back,
    /// Closes the window or the tab.
    Close,
    /// Types, moves the caret or edits in a focused text field.
    TextEditing,
}

/// The reserved meaning `activator` already has on `platform`, if any: a
/// gesture command bound to it would steal traversal, Escape, Back, closing
/// or text editing (ADR-0216 §1).
pub(crate) fn reserved_key(
    platform: TargetPlatform,
    activator: &SingleActivator,
) -> Option<ReservedKey> {
    let apple = matches!(platform, TargetPlatform::MacOS | TargetPlatform::iOS);
    let command = if apple {
        activator.meta && !activator.control
    } else {
        activator.control && !activator.meta
    };
    let shift = activator.shift.unwrap_or(false);
    if back_activators(platform)
        .iter()
        .any(|back| back.trigger == activator.trigger && same_modifiers(back, activator))
    {
        return Some(ReservedKey::Back);
    }
    match &activator.trigger {
        Key::Named(NamedKey::Escape) => Some(ReservedKey::Escape),
        Key::Named(NamedKey::Tab | NamedKey::Enter | NamedKey::Select) => {
            Some(ReservedKey::Traversal)
        }
        Key::Named(
            NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::ArrowUp
            | NamedKey::ArrowDown
            | NamedKey::Home
            | NamedKey::End
            | NamedKey::PageUp
            | NamedKey::PageDown
            | NamedKey::Backspace
            | NamedKey::Delete
            | NamedKey::Insert,
        ) => Some(ReservedKey::TextEditing),
        Key::Named(NamedKey::F4) if activator.alt || activator.control => {
            (!apple).then_some(ReservedKey::Close)
        }
        Key::Character(text) => {
            let letter = text.as_str().to_ascii_lowercase();
            if text.as_str() == " " && !activator.control && !activator.alt && !activator.meta {
                return Some(ReservedKey::Traversal);
            }
            if !activator.control && !activator.alt && !activator.meta {
                // Unmodified or Shift-only: what the key types.
                return Some(ReservedKey::TextEditing);
            }
            if command && !activator.alt && letter == "w" {
                return Some(ReservedKey::Close);
            }
            if apple && command && letter == "q" {
                return Some(ReservedKey::Close);
            }
            if command
                && !activator.alt
                && matches!(letter.as_str(), "a" | "c" | "x" | "v" | "z" | "y")
            {
                return Some(ReservedKey::TextEditing);
            }
            // Control+letter moves and deletes in Apple text fields, the
            // Emacs bindings of the Cocoa text system.
            if apple
                && activator.control
                && !activator.meta
                && !activator.alt
                && matches!(
                    letter.as_str(),
                    "a" | "b" | "d" | "e" | "f" | "h" | "k" | "n" | "o" | "p" | "t" | "v" | "y"
                )
            {
                return Some(ReservedKey::TextEditing);
            }
            // Option+letter types a character on Apple keyboards.
            (apple && activator.alt && !activator.meta && !activator.control && !shift)
                .then_some(ReservedKey::TextEditing)
        }
        _ => None,
    }
}

fn same_modifiers(a: &SingleActivator, b: &SingleActivator) -> bool {
    a.control == b.control
        && a.alt == b.alt
        && a.meta == b.meta
        && a.shift.unwrap_or(false) == b.shift.unwrap_or(false)
}

/// One activator and the intent it raises.
type Binding = (SingleActivator, Rc<dyn Intent>);

/// Bindings whose chords follow the platform convention of the focused
/// widget rather than of the `Shortcuts` that holds them, so a
/// [`PlatformConvention`] subtree raises its intents on its own chords.
/// Each platform's table is built once, when a key first needs it.
///
/// [`PlatformConvention`]: flui_view::environment::PlatformConvention
struct ConventionBindings {
    bind: fn(TargetPlatform) -> Vec<Binding>,
    built: RefCell<Vec<(TargetPlatform, Rc<[Binding]>)>>,
}

impl ConventionBindings {
    fn new(bind: fn(TargetPlatform) -> Vec<Binding>) -> Self {
        Self {
            bind,
            built: RefCell::new(Vec::new()),
        }
    }

    /// The table for `platform`. `bind` is this crate's own table, so no
    /// user code runs while the cache is borrowed.
    fn for_platform(&self, platform: TargetPlatform) -> Rc<[Binding]> {
        let built = self
            .built
            .borrow()
            .iter()
            .find(|(of, _)| *of == platform)
            .map(|(_, bindings)| Rc::clone(bindings));
        built.unwrap_or_else(|| {
            let bindings: Rc<[Binding]> = Rc::from((self.bind)(platform));
            self.built
                .borrow_mut()
                .push((platform, Rc::clone(&bindings)));
            bindings
        })
    }
}

/// [`DefaultFocusTraversal`]'s bindings whose chords differ by platform:
/// select all and the clipboard on the command modifier, back, and the
/// context menu.
fn convention_bindings(platform: TargetPlatform) -> Vec<Binding> {
    let once = |activator: SingleActivator| activator.allow_repeats(false);
    let mut bindings: Vec<Binding> = vec![(
        command_activator(platform, "a"),
        Rc::new(SelectAllTextIntent),
    )];
    for activator in back_activators(platform) {
        bindings.push((once(activator), Rc::new(BackIntent)));
    }
    for activator in ShowContextMenuIntent::default_keys(platform) {
        bindings.push((activator, Rc::new(ShowContextMenuIntent)));
    }
    for (activator, binding) in clipboard_activators(platform) {
        let intent: Rc<dyn Intent> = match binding {
            ClipboardBinding::Copy => Rc::new(CopySelectionTextIntent::Copy),
            ClipboardBinding::Cut => Rc::new(CopySelectionTextIntent::Cut),
            ClipboardBinding::Paste => Rc::new(PasteTextIntent),
        };
        bindings.push((activator, intent));
    }
    bindings
}

fn command_activator(platform: TargetPlatform, key: &str) -> SingleActivator {
    with_command_modifier(platform, SingleActivator::character(key))
}

/// `activator` with `platform`'s command modifier: Cmd on macOS and iOS,
/// Control elsewhere.
pub(crate) fn with_command_modifier(
    platform: TargetPlatform,
    activator: SingleActivator,
) -> SingleActivator {
    match platform {
        TargetPlatform::MacOS | TargetPlatform::iOS => activator.meta(),
        _ => activator.control(),
    }
}

impl std::fmt::Debug for DefaultFocusTraversalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultFocusTraversalState")
            .field("initialized", &self.focus_owner.is_some())
            .field("keys", &self.keys.id())
            .finish_non_exhaustive()
    }
}

impl StatefulView for DefaultFocusTraversal {
    type State = DefaultFocusTraversalState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        DefaultFocusTraversalState {
            focus_owner: None,
            keys: FocusNode::with_debug_label("Shortcuts"),
            convention: Rc::new(ConventionBindings::new(convention_bindings)),
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

    fn build(&self, view: &DefaultFocusTraversal, ctx: &dyn BuildContext) -> impl IntoView {
        let platform = flui_view::environment::PlatformConvention::of(ctx);
        let focus_owner = self
            .focus_owner
            .as_ref()
            .expect("BUG: DefaultFocusTraversal built before init_state")
            .clone();
        let once = |activator: SingleActivator| activator.allow_repeats(false);
        let mut shortcuts = Shortcuts::new(view.child.clone())
            .focus_node(Rc::clone(&self.keys))
            .shortcut(SingleActivator::named(NamedKey::Tab), NextFocusIntent)
            .shortcut(
                SingleActivator::named(NamedKey::Tab).shift(),
                PreviousFocusIntent,
            )
            .shortcut(
                once(SingleActivator::named(NamedKey::Enter)),
                ActivateIntent,
            )
            .shortcut(once(SingleActivator::character(" ")), ActivationPressIntent)
            .shortcut(SingleActivator::character(" ").on_release(), ActivateIntent)
            .shortcut(
                once(SingleActivator::named(NamedKey::Select)),
                ActivateIntent,
            )
            .shortcut(
                once(SingleActivator::named(NamedKey::Escape)),
                DismissIntent,
            )
            .by_convention(Rc::clone(&self.convention), platform);
        for (key, direction) in [
            (NamedKey::ArrowUp, flui_interaction::FocusDirection::Up),
            (NamedKey::ArrowDown, flui_interaction::FocusDirection::Down),
            (NamedKey::ArrowLeft, flui_interaction::FocusDirection::Left),
            (
                NamedKey::ArrowRight,
                flui_interaction::FocusDirection::Right,
            ),
        ] {
            shortcuts = shortcuts.shortcut(
                SingleActivator::named(key),
                DirectionalFocusIntent(direction),
            );
        }
        Actions::new(shortcuts)
            .action(NextFocusAction::new(Rc::clone(&focus_owner)))
            .action(PreviousFocusAction::new(Rc::clone(&focus_owner)))
            .action(DirectionalFocusAction::new(focus_owner))
    }
}

// The mounted `Shortcuts` suites live in `crates/flui-widgets/tests/shortcuts.rs`.

/// The gesture key collision table (ADR-0216 §1): every platform's default
/// gesture keys and the reserved keys they must avoid. Only the running
/// platform's bindings are reachable through a mounted tree, so the other
/// platforms' rows are checked here, on the tables themselves.
#[cfg(test)]
mod collision_tests {
    use super::{ReservedKey, ShowContextMenuIntent, SingleActivator, reserved_key};
    use flui_foundation::geometry::Axis;
    use flui_platform::TargetPlatform;
    use flui_platform::keyboard::NamedKey;

    const PLATFORMS: [TargetPlatform; 7] = [
        TargetPlatform::Android,
        TargetPlatform::iOS,
        TargetPlatform::Linux,
        TargetPlatform::MacOS,
        TargetPlatform::Windows,
        TargetPlatform::Fuchsia,
        TargetPlatform::Unknown,
    ];

    #[test]
    fn gesture_key_collision_table() {
        for platform in PLATFORMS {
            let menu_keys = ShowContextMenuIntent::default_keys(platform);
            for key in &menu_keys {
                assert_eq!(
                    reserved_key(platform, key),
                    None,
                    "{platform:?}: the menu key {key:?} takes nothing reserved"
                );
            }
            let apple = matches!(platform, TargetPlatform::MacOS | TargetPlatform::iOS);
            assert_eq!(
                menu_keys.is_empty(),
                apple,
                "{platform:?}: only Apple platforms have no menu key"
            );

            // A widget's command keys bind through its command, which drops
            // a reserved one: each must survive on every platform.
            for key in crate::scroll::refresh_keys(platform) {
                assert_eq!(
                    reserved_key(platform, &key),
                    None,
                    "{platform:?}: the refresh key {key:?} takes nothing reserved"
                );
            }
            // A widget's own keys answer only what bubbles past a focused
            // field, which keeps its typing and caret keys
            // (`keys_typed_in_a_field_inside_the_viewer_stay_in_the_field`);
            // traversal, Escape, Back and closing they never take.
            let own_keys = crate::scroll::zoom_keys(platform)
                .into_iter()
                .map(|(key, _)| key)
                .chain(
                    [
                        (Axis::Vertical, false),
                        (Axis::Horizontal, false),
                        (Axis::Horizontal, true),
                    ]
                    .into_iter()
                    .flat_map(|(axis, rtl)| {
                        let (next, previous) = crate::scroll::anchor_keys(axis, rtl);
                        [
                            SingleActivator::named(next),
                            SingleActivator::named(previous),
                        ]
                    }),
                );
            for key in own_keys {
                let reserved = reserved_key(platform, &key);
                assert!(
                    matches!(reserved, None | Some(ReservedKey::TextEditing)),
                    "{platform:?}: the widget key {key:?} takes {reserved:?}"
                );
            }
        }

        let command = |key: &str| SingleActivator::character(key).control();
        let apple_command = |key: &str| SingleActivator::character(key).meta();
        let rows: [(TargetPlatform, SingleActivator, Option<ReservedKey>); 21] = [
            (
                TargetPlatform::Windows,
                SingleActivator::named(NamedKey::Escape),
                Some(ReservedKey::Escape),
            ),
            (
                TargetPlatform::MacOS,
                SingleActivator::named(NamedKey::Escape).shift(),
                Some(ReservedKey::Escape),
            ),
            (
                TargetPlatform::Linux,
                SingleActivator::named(NamedKey::Tab),
                Some(ReservedKey::Traversal),
            ),
            (
                TargetPlatform::Linux,
                SingleActivator::named(NamedKey::Enter),
                Some(ReservedKey::Traversal),
            ),
            (
                TargetPlatform::Android,
                SingleActivator::character(" "),
                Some(ReservedKey::Traversal),
            ),
            (
                TargetPlatform::Windows,
                SingleActivator::named(NamedKey::ArrowLeft).alt(),
                Some(ReservedKey::Back),
            ),
            (
                TargetPlatform::MacOS,
                SingleActivator::character("[").meta(),
                Some(ReservedKey::Back),
            ),
            (
                TargetPlatform::iOS,
                SingleActivator::named(NamedKey::BrowserBack),
                Some(ReservedKey::Back),
            ),
            (
                TargetPlatform::Windows,
                command("w"),
                Some(ReservedKey::Close),
            ),
            (
                TargetPlatform::Windows,
                SingleActivator::named(NamedKey::F4).alt(),
                Some(ReservedKey::Close),
            ),
            (
                TargetPlatform::MacOS,
                apple_command("w"),
                Some(ReservedKey::Close),
            ),
            (
                TargetPlatform::MacOS,
                apple_command("q"),
                Some(ReservedKey::Close),
            ),
            (
                TargetPlatform::Linux,
                SingleActivator::character("x"),
                Some(ReservedKey::TextEditing),
            ),
            (
                TargetPlatform::Linux,
                command("z"),
                Some(ReservedKey::TextEditing),
            ),
            (
                TargetPlatform::MacOS,
                apple_command("v"),
                Some(ReservedKey::TextEditing),
            ),
            (
                TargetPlatform::Windows,
                SingleActivator::named(NamedKey::Delete),
                Some(ReservedKey::TextEditing),
            ),
            (
                TargetPlatform::MacOS,
                SingleActivator::character("e").alt(),
                Some(ReservedKey::TextEditing),
            ),
            (
                TargetPlatform::MacOS,
                command("d"),
                Some(ReservedKey::TextEditing),
            ),
            // What a command may take.
            (TargetPlatform::Windows, command("d"), None),
            (TargetPlatform::MacOS, command("w"), None),
            (
                TargetPlatform::Linux,
                SingleActivator::named(NamedKey::F2),
                None,
            ),
        ];
        for (platform, activator, expected) in rows {
            assert_eq!(
                reserved_key(platform, &activator),
                expected,
                "{platform:?}: {activator:?}"
            );
        }
    }
}
