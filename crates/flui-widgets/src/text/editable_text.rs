//! [`EditableText`] — single-line editable text backed by a
//! [`TextEditingController`].

#[cfg(doc)]
use crate::text::selection_style::DefaultSelectionStyle;
#[cfg(doc)]
use crate::{DefaultSelectionControls, DotHandles};
#[cfg(doc)]
use flui_interaction::text_input::TextInputClient;
#[cfg(doc)]
use flui_objects::RenderEditable;
#[cfg(doc)]
use flui_painting::typography::TextSpan;
#[cfg(doc)]
use flui_platform::text::store::EnterAction;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_foundation::geometry::{Bounds, Point, Rect};
use flui_foundation::panic::FirstPanic;
use flui_foundation::{CreationSite, TextDirection};
use flui_interaction::events::PointerId;
use flui_interaction::focus::{
    FocusAttachment, FocusManager, FocusNode, FocusNodeRegistration, KeyEventHandler, RectProvider,
};
use flui_interaction::pointer::PressProbe;
use flui_interaction::text_input::{ClientToken, ClipboardHandle, TextInputHandle};
use flui_painting::{styling::Color, typography::TextStyle};
use flui_platform::TargetPlatform;
use flui_platform::input::keyboard::{Key, KeyState, NamedKey};
use flui_platform::text::store::{OwnerCalls, TextInputOptions};
use flui_rendering::pipeline::PipelineCell;
use flui_view::prelude::*;

use crate::SelectionHandles;
use crate::interaction::actions::{
    ActionChain, ActivateIntent, ActivationPressIntent, CopySelectionTextIntent, DismissIntent,
    EnclosingActions, FocusRecord, PasteTextIntent, SelectAllTextIntent, ShowContextMenuIntent,
    erased_action, layered_chain,
};
use crate::support::ref_callback;
use crate::text::controller::TextEditingController;
use crate::text::primary_selection::PrimarySelectionOwner;
use crate::text::selection_style::SelectionStyle;
use crate::text::text_store::EditableTextStore;
mod clipboard;
mod cursor_area;
mod edit_commands;
mod edit_observer;
mod field_semantics;
mod field_view;
mod handles;
mod input_requests;
mod key_handler;
mod menu;
mod obscuring;
mod platform_selection;
mod pointer;
mod view_state;
use crate::text::{MagnifierChoice, MenuSettings, ShowSelectionMenu};
use cursor_area::CursorAreaLoop;
pub(super) use edit_commands::FieldCommands;
use edit_commands::SelectAllTextAction;
use input_requests::{CompositionCommit, KeyboardRequest};
use platform_selection::PlatformSelection;

pub use handles::{CaretHandle, LongPressSelection};
use handles::{FieldHandles, FieldLongPress, HandleParts};

use clipboard::{ClipboardTextAction, claim_primary};
pub(super) use edit_observer::EditObserver;
use field_semantics::FieldSemantics;
pub(crate) use key_handler::is_word_jump_modifier;
use key_handler::{KeptByTheField, build_key_handler};
use obscuring::DEFAULT_OBSCURING_CHARACTER;
pub(super) use obscuring::{obscure, source_offset_for_masked_offset};

type ImeFocusTransition = Rc<dyn Fn(bool)>;

#[derive(Clone, Copy, PartialEq, Eq)]
struct SelectionDrag {
    contact: PointerId,
    source_anchor: usize,
    /// Whether the contact is a touch, whose release shows the handles.
    touch: bool,
}

/// Callback for [`EditableText::on_submitted`] — see that method's doc.
/// Exported (not crate-private) so [`RawTextField`](super::text_field::RawTextField)'s
/// own `on_submitted` passthrough and `flui_material::TextField`'s can share
/// one canonical alias instead of each declaring their own. It receives the
/// dispatch's [`EventContext`] first (ADR-0086), and is stored already adapted to
/// report its outcome.
pub type SubmitCallback = Rc<dyn Fn(&mut EventContext<'_>, &str)>;

/// A single-line text field that accepts keyboard input when focused.
///
/// The low-level editable primitive.
/// [`RawTextField`](super::text_field::RawTextField) wraps this with decoration
/// and tap-to-focus.
///
/// # Key routing
///
/// `EditableText` installs its key handler directly on its explicit
/// [`FocusNode`] in `init_state`. Platform key events arrive via
/// `FocusManager::dispatch_key_event` (wired in `flui-app`), which routes them
/// to the focused node's handler.  Only `KeyState::Down` events (including
/// key-repeat) are processed; `KeyState::Up` events are ignored.
///
/// # IME composition
///
/// The field is a text store the platform's input method pulls from
/// ([`flui_platform::text::store::TextStore`], ADR-0090). On focus gain,
/// `EditableTextState` attaches it as a [`TextInputClient`] through
/// [`LifecycleContext::text_input_handle`] (acquired in `init_state`, per the
/// frame-capability rule that method's doc states). The input method reads
/// the text, selection, composition and geometry in UTF-16 offsets and edits
/// them under a lock; a push-model [`flui_platform::ImeEvent`] is
/// projected onto the same store, so there is one editing path. On blur and
/// on dispose the client is detached (the ADR-0030 detach-on-dispose
/// contract — a field unmounted while still focused must not leave a stale
/// IME client attached), and a disposed field refuses every lock.
///
/// A field that loses its input commits its composition first, keeping the
/// text (ADR-0142 item 4): on blur before it detaches, on a pointer-down on
/// it before the caret moves, and on a paste before the clipboard's text
/// lands. The platform ends its composition; one that cannot (a pull host
/// answering `Abandoned`, or a push platform) has the composing range
/// cleared in place. Each step of the transition still runs when the
/// commit's owner code (`on_changed`) panics, and that first failure is
/// resumed after them.
///
/// A platform session is one change to the field: its edits are written to
/// the controller once, and after the lock is released the listeners hear
/// of it once and [`EditableText::on_changed`] runs at most once, only when
/// the committed text (the text without the composition) changed. A
/// platform selection is kept exactly, even inside a grapheme cluster; a tap
/// or an arrow key still snaps to one (Mapping decisions #33 and #34 in
/// `flui-widgets/ARCHITECTURE.md`). An obscured field reports itself
/// protected: the input method may edit it and ask for geometry, but not
/// read its text.
///
/// Inside a frame transaction the store cannot commit: a lock asked for
/// then runs after the frame. A key press first runs any such queued grant,
/// so a key typed after an IME commit lands after it; an edit made through
/// the controller directly while a grant is queued (`set_text`) goes ahead
/// of it.
///
/// **Suppression contract**: the key handler skips `Key::Character`
/// insertion only while [`TextEditingController::is_composing`] is `true` —
/// suppressing unconditionally after focus gain would silently kill plain
/// (non-IME) typing for the rest of the session, since a host sends
/// `Key::Character` for keys outside composition and routes the keys
/// composition consumes as `Preedit` and `Commit`. See [`flui_platform::ImeEvent`]'s doc for the full contract.
///
/// # IME cursor-area tracking
///
/// While an IME client is attached (focus gain to blur/dispose),
/// `EditableTextState` also runs a self-rescheduling post-frame loop (ADR-0030)
/// that reads the composing region's current global rect when one is
/// active, falling back to the collapsed caret's rect otherwise — through
/// the second, inner [`SubtreeAnchor`](flui_objects::SubtreeAnchor) wrapping
/// the render view directly (`build_field_view`) and
/// [`RenderEditable::rect_for_composing_range`]/
/// [`RenderEditable::caret_local_rect`] — and forwards it to
/// [`TextInputHandle::set_cursor_area`] whenever it changes, so the platform
/// IME candidate window follows the composing text (or the caret, once
/// composition ends). This is a winit-style single-rect protocol rather than a
/// transform plus local rect — see ADR-0030 for
/// the loop mechanics (why it is per-attach: a fresh alive-flag and a fresh
/// last-sent cache each attach, rather than shared across the field's
/// lifetime) and ADR-0030 for the composing-rect-over-caret-rect fallback
/// order this loop now applies.
///
/// # Select all
///
/// Ctrl+A (Cmd+A on Apple platforms) selects the entire document through
/// [`SelectAllTextIntent`], including in an obscured field. It needs no
/// clipboard and does not call `on_changed`: selecting leaves the text
/// unchanged. An active IME composition keeps its own selection, so the
/// action is disabled until composition ends.
///
/// # Clipboard
///
/// The field answers [`CopySelectionTextIntent`] and [`PasteTextIntent`] on
/// its own focus node — the chain a `Shortcuts` resolves an intent against
/// when this field holds the primary focus — so the Ctrl/Cmd+C, X and V
/// bindings [`DefaultFocusTraversal`](crate::DefaultFocusTraversal) installs
/// under every application root ([`app_root`](crate::app_root)) reach it.
/// Its actions are layered over the chain visible at its position, and are
/// the nearest declaration of those two
/// intent types, so they win over an ancestor `Actions` binding for them.
///
/// | Intent | Enabled when | Does |
/// |---|---|---|
/// | Copy | a clipboard is installed, the field is not obscured, and the selection is not empty | writes the selection; the selection stays |
/// | Cut | as Copy, and the field takes an edit: enabled, mounted, holding the focus | writes the selection, then deletes it at the owner turn the clipboard answers it took the text, or that only this application can read it back, if the field still takes an edit and its text is as the cut left it; a write the clipboard refused deletes nothing |
/// | Paste | a clipboard is installed and the field takes an edit, as for Cut | commits an active IME composition, keeping its text, then replaces the selection with the clipboard's text, line breaks removed |
///
/// A disabled action leaves the key unconsumed, so an obscured field's
/// Ctrl+C keeps bubbling. Paste consumes the key even when the clipboard is
/// empty.
///
/// The clipboard is acquired in `init_state` through
/// `ctx.capability::<ClipboardCapability>()` (ADR-0152); a refusal leaves
/// copy, cut and paste disabled.
///
/// # Primary selection
///
/// Where the platform has one (X11 and Wayland), a non-empty selection in a
/// field that is not obscured is offered as the primary selection, read only
/// when another application pastes it, and let go when the selection empties,
/// the field is obscured or it unmounts. A middle press focuses the field and
/// pastes the primary selection where it lands, as Paste does, once the read
/// settles; it starts no selection. Elsewhere the capability is refused and a
/// middle press is left to whatever else uses it.
///
/// # DEFERRED (v1)
///
/// The following are absent in v1; do not use these features and expect them
/// to work:
/// - **Multi-tap and shift-click selection** — a tap places the caret, a drag
///   extends the selection, and Shift with an arrow or Home/End extends it
///   from the keyboard. What is absent is anything needing a click COUNT, or
///   a modifier on the POINTER: shift-click extension, double-tap word
///   selection and triple-tap line selection. `Listener` delivers raw pointer
///   events, and the arbitration that produces those lives in
///   `flui-interaction`'s recognisers;
///   [`RenderEditable::word_range_at_local_offset`] is already there for the
///   double-tap case when one is wired above this.
/// - **Selection handles and the selection toolbar** — the draggable
///   endpoints and the copy/paste menu shown on touch platforms.
/// - **Multi-line** — newlines are inserted as literal characters but line
///   wrapping, multi-line layout, and vertical scrolling are not implemented.
/// - **Input formatters** — no validation or transformation pipeline.
/// - **Scroll when text overflows** — the rendered text clips without scrolling.
#[derive(Clone, StatefulView)]
pub struct EditableText {
    /// Controller that owns the text buffer and caret.
    pub(super) controller: TextEditingController,
    /// Focus ownership is explicit and presentation-local. The caller owns
    /// the node.
    pub(super) focus_node: Rc<FocusNode>,
    /// Explicit logical height, or the laid-out line height when absent.
    pub(super) caret_height: Option<f64>,
    /// Color of the caret bar when the field is focused.
    pub(super) caret_color: Color,
    /// How the selection is highlighted; `None` takes the nearest
    /// [`DefaultSelectionStyle`]'s.
    pub(super) selection_style: Option<SelectionStyle>,
    /// Whether this field accepts focus and input. `true` by default.
    ///
    /// **Named hoist**: `enabled` conceptually belongs to the decorated field,
    /// but [`RawTextField`](super::text_field::RawTextField) has no
    /// decoration/enabled plumbing yet, so this substrate carries it on
    /// `EditableText` itself, one layer lower — see
    /// [`enabled`](Self::enabled)'s doc comment for exactly what it
    /// withholds.
    pub(super) enabled: bool,
    /// Take the focus on mount; see [`autofocus`](Self::autofocus).
    pub(super) autofocus: bool,
    /// Style applied to the field's [`TextSpan`], flowing through
    /// [`TextSpan::with_style`]. `None` renders with the span's own default.
    pub(super) text_style: Option<TextStyle>,
    /// Replace every character with [`obscuring_character`](Self::obscuring_character)
    /// before the render view is built — a password field.
    ///
    /// "Before the render view", not "when painting": the substitution is
    /// upstream of the render object, so it governs semantics and diagnostics
    /// as much as pixels. That scope IS the feature — see below.
    ///
    /// The substitution happens where the controller's text becomes the
    /// render view's, so nothing below this widget ever receives the real
    /// text: `RenderEditable`'s `plain_text`, its `TextPainter`, the layer
    /// tree and every diagnostic downstream all carry the mask. That is
    /// stronger than redacting at each of those points, because it cannot be
    /// forgotten at a new one.
    pub(super) obscure_text: bool,
    /// What the field asks of the input method and the on-screen keyboard
    /// (default [`TextInputOptions::TEXT`]). It never filters what the field
    /// accepts.
    pub(super) text_input: TextInputOptions,
    /// The character painted in place of each source character.
    pub(super) obscuring_character: char,
    /// Called with the current text when Enter is pressed while this field
    /// has focus — see [`Self::on_submitted`]'s doc.
    pub(super) on_submitted: Option<SubmitCallback>,
    /// Called with the new text after each edit the user makes — see
    /// [`Self::on_changed`]'s doc.
    pub(super) on_changed: Option<TextChanged>,
    /// What assistive technology is told about the field beyond its value.
    pub(super) field_semantics: FieldSemantics,
    pub(super) handles: Option<Rc<dyn SelectionHandles>>,
    pub(super) caret_handle: Option<CaretHandle>,
    pub(super) menu: MenuSettings,
    pub(super) magnifier: Option<MagnifierChoice>,
    pub(super) long_press: Option<LongPressSelection>,
    #[view(site)]
    site: CreationSite,
}

/// Callback for [`EditableText::on_changed`], with the dispatch's
/// [`EventContext`].
pub(crate) type TextChanged = Rc<dyn Fn(&mut EventContext<'_>, &str)>;

impl EditableText {
    /// Create an `EditableText` driven by `controller` and `focus_node`.
    #[must_use]
    #[track_caller]
    pub fn new(controller: TextEditingController, focus_node: Rc<FocusNode>) -> Self {
        Self {
            controller,
            focus_node,
            caret_height: None,
            caret_color: Color::BLACK,
            // Transparent by default, so the primitive paints no highlight
            // until a caller (a decorated `TextField`, a theme) chooses one.
            selection_style: None,
            enabled: true,
            autofocus: false,
            text_style: None,
            obscure_text: false,
            text_input: TextInputOptions::TEXT,
            obscuring_character: DEFAULT_OBSCURING_CHARACTER,
            on_submitted: None,
            on_changed: None,
            field_semantics: FieldSemantics::default(),
            handles: None,
            caret_handle: None,
            menu: MenuSettings::default(),
            magnifier: None,
            long_press: None,
            site: CreationSite::here(),
        }
    }

    /// The field's accessible name, which a screen reader announces on
    /// reaching it: what its visible label says.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.field_semantics.label = Some(label.into());
        self
    }

    /// Supplementary text a screen reader reads after the field's name and
    /// value: what its helper or error line says.
    #[must_use]
    pub fn semantic_hint(mut self, hint: impl Into<String>) -> Self {
        self.field_semantics.hint = Some(hint.into());
        self
    }

    /// What the field shows while empty, which a screen reader reads in
    /// place of a value.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.field_semantics.placeholder = Some(placeholder.into());
        self
    }

    /// Whether the field's value failed validation, which a screen reader
    /// announces with the field.
    #[must_use]
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.field_semantics.invalid = invalid;
        self
    }

    /// Whether the form needs this field filled in, which a screen reader
    /// announces with the field.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.field_semantics.required = required;
        self
    }

    /// Names the field's node for tests and automation (ADR-0240): exported
    /// as UI Automation's `AutomationId`, never announced.
    #[must_use]
    pub fn identifier(mut self, identifier: impl Into<std::sync::Arc<str>>) -> Self {
        self.field_semantics.identifier = Some(identifier.into());
        self
    }

    /// [`identifier`](Self::identifier) when `identifier` is `Some`: how a
    /// decorated field forwards its own.
    #[must_use]
    pub(crate) fn identifier_if(mut self, identifier: Option<&std::sync::Arc<str>>) -> Self {
        self.field_semantics.identifier = identifier.cloned();
        self
    }

    /// Override the caret bar height in logical pixels. By default it follows
    /// the laid-out text line; an override is independent of text scaling.
    #[must_use]
    pub fn caret_height(mut self, height: f64) -> Self {
        self.caret_height = Some(height);
        self
    }

    /// Highlight the selection with `style`, in place of the nearest
    /// [`DefaultSelectionStyle`]'s or [`SelectionStyle::default`]: its
    /// active colour while the field holds the focus, its inactive one after
    /// a blur, which keeps the selection.
    #[must_use]
    pub fn selection_style(mut self, style: SelectionStyle) -> Self {
        self.selection_style = Some(style);
        self
    }

    /// Build the touch handles with `handles`, in place of the nearest
    /// [`DefaultSelectionControls`]' or [`DotHandles`] in the opaque active
    /// selection colour, as a [`SelectionArea`](crate::SelectionArea) does.
    ///
    #[must_use]
    pub fn caret_color(mut self, color: Color) -> Self {
        self.caret_color = color;
        self
    }

    /// Replace every character with
    /// [`obscuring_character`](Self::obscuring_character) before the render
    /// view is built — a password field (default `false`).
    ///
    /// Not only paint: the render object never receives the real characters,
    /// so its diagnostics and anything derived from its text carry the mask
    /// too. See the [`obscure_text`](Self::obscure_text) field's doc.
    #[must_use]
    pub fn obscure_text(mut self, obscure: bool) -> Self {
        self.obscure_text = obscure;
        self
    }

    /// What the field asks of the input method and the on-screen keyboard
    /// (default [`TextInputOptions::TEXT`]): the layout its purpose names, the
    /// label of the Enter key, capitalization, correction and what the system
    /// may fill it with. An obscured field is a password field whatever it
    /// asks, never suggested or corrected.
    ///
    /// Enter acts by the label: after [`Self::on_submitted`]'s callback, a
    /// field whose Enter says [`EnterAction::Next`] moves the focus to the
    /// next field and one that says [`EnterAction::Previous`] to the one
    /// before, on every host and from a hardware keyboard too.
    #[must_use]
    pub fn text_input(mut self, text_input: TextInputOptions) -> Self {
        self.text_input = text_input;
        self
    }

    /// Override the character painted in place of each source character
    /// (default `'\u{2022}'`).
    ///
    /// Takes a `char`, so "exactly one character" is a type rather than a
    /// runtime check on a string.
    #[must_use]
    pub fn obscuring_character(mut self, character: char) -> Self {
        self.obscuring_character = character;
        self
    }

    /// Set whether the field accepts focus and keyboard input (default
    /// `true`) — see the [`enabled`](Self::enabled) field's doc comment for
    /// why this lives on `EditableText` rather than on the decorated field.
    ///
    /// A disabled field withholds focus acquisition by marking its explicit
    /// node
    /// [`FocusNode::set_can_request_focus`]`(false)`, which keyboard-traversal
    /// (`focus_next`/`focus_previous`) already honors and which releases
    /// primary focus itself if the field is focused when it becomes disabled; no separate
    /// `did_update_view` unfocus step is needed. Its key handler also stops
    /// mutating the controller while disabled, so even a stray dispatch
    /// reaching an already-focused-then-disabled node is a no-op.
    ///
    /// Tap suppression is a decoration-level concern (an enclosing
    /// `TextField`'s `GestureDetector`), not this primitive's — out of scope
    /// here, see `TextField`'s own docs.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Take the focus when the field mounts, or when this turns `true`
    /// later, as [`Focus::autofocus`](crate::Focus::autofocus) does: once,
    /// and only while its scope has nothing focused yet or holds only the
    /// route's provisional first focus. A focus restored on pop is not
    /// replaced.
    #[must_use]
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    /// Apply `style` to the field's rendered [`TextSpan`] via
    /// [`TextSpan::with_style`].
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// Call `callback` with the field's current text when Enter is pressed
    /// while it has focus.
    ///
    /// Fires on an Enter press, which is what every host delivers the
    /// on-screen keyboard's action key as, whatever its label
    /// ([`Self::text_input`]). The key is consumed
    /// ([`KeyEventResult::Handled`](flui_interaction::focus::KeyEventResult))
    /// when a callback is set or the label moved the focus; otherwise Enter
    /// is left unconsumed (`Ignored`) so an ancestor can still act on it.
    ///
    /// No multiline support exists in this substrate (there is no
    /// newline-insertion behavior to conflict with), so Enter has exactly
    /// one meaning here: submit, then the label's focus move.
    ///
    /// The callback receives the dispatch's `&mut EventContext<'_>` first, so it
    /// writes a signal directly (ADR-0086); the field opens that write from
    /// the writer source its state is created with.
    #[must_use]
    pub fn on_submitted<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, &str) -> R + 'static,
        R: EventOutcome,
    {
        self.on_submitted = Some(ref_callback(callback));
        self
    }

    /// Call `callback` with the new text after each edit the user makes —
    /// typing, deleting, an IME commit, a cut or a paste.
    ///
    /// Only user edits: a caller changing the controller itself
    /// (`set_text`, `clear`) does not call it, which is what keeps a form
    /// field's reset from counting as the user's input. Runs after the edit,
    /// with no borrow of the field held, inside a write the field opens: the
    /// callback receives that `&mut EventContext<'_>` first (ADR-0086). An IME
    /// commit reaches it once the frame that deferred the commit has ended.
    ///
    /// The text is the committed text
    /// ([`TextEditingController::committed_text`]): an input method's
    /// composition is left out, and a session that only composes or
    /// cancels a composition does not call it. For an input-method edit it
    /// runs after the method's lock is released, so it may edit the field.
    #[must_use]
    pub fn on_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, &str) -> R + 'static,
        R: EventOutcome,
    {
        self.on_changed = Some(ref_callback(callback));
        self
    }
}

// Hand-written rather than derived: `on_submitted`'s `Rc<dyn Fn(&str)>` has
// no `Debug` impl (a trait object over a closure has no useful
// representation beyond its presence), so a derive would reject every field
// once this one exists. Mirrors `EditableTextState`'s own manual impl just
// below for the same reason.
impl std::fmt::Debug for EditableText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditableText")
            .field("controller", &self.controller)
            .field("focus_node", &self.focus_node)
            .field("caret_height", &self.caret_height)
            .field("caret_color", &self.caret_color)
            .field("selection_style", &self.selection_style)
            .field("enabled", &self.enabled)
            .field("autofocus", &self.autofocus)
            .field("text_style", &self.text_style)
            .field("obscure_text", &self.obscure_text)
            .field("text_input", &self.text_input)
            .field("obscuring_character", &self.obscuring_character)
            .field("on_submitted", &self.on_submitted.is_some())
            .field("on_changed", &self.on_changed.is_some())
            .field("field_semantics", &self.field_semantics)
            .field("handles", &self.handles.is_some())
            .field("caret_handle", &self.caret_handle)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// EditableTextState
// ============================================================================

/// Persistent state for [`EditableText`].
///
/// Attaches the caller-owned [`FocusNode`] for this field and wires it to the
/// presentation-local [`FocusManager`] on mount.
pub struct EditableTextState {
    /// Focus node representing this field in its presentation focus tree.
    focus_node: Rc<FocusNode>,
    /// Shared identity read by manager listeners so a live widget can replace
    /// its explicit node without reinstalling presentation subscriptions.
    observed_focus_node: Rc<RefCell<Rc<FocusNode>>>,
    /// Refuses a resumed accessibility edit after disablement or disposal,
    /// even if an external node is subsequently attached to another field.
    accepts_semantics_actions: Rc<Cell<bool>>,
    /// Exact manager acquired from the mounting build owner.
    focus_manager: Rc<FocusManager>,
    /// The one generation-checked attachment owned by this mounted state.
    /// Shared privately with accessibility delivery; cloning the `Rc` does
    /// not mint a second attachment or expose reparenting to a consumer.
    focus_attachment: Rc<RefCell<Option<Rc<FocusAttachment>>>>,
    /// Geometry provider retained so a replacement external node receives the
    /// same live render-anchor measurement.
    rect_provider: Option<RectProvider>,
    /// Generation-checked ownership of the geometry source installed by this
    /// mounted field.
    rect_provider_registration: Option<FocusNodeRegistration>,
    /// Generation-checked ownership of this field's key handler.
    key_handler_registration: Option<FocusNodeRegistration>,
    /// Publishes the field's `RenderId` while mounted, so the node's rect
    /// provider can measure it for reading-order traversal.
    anchor: flui_objects::SubtreeAnchor,
    /// Publishes the `RenderId` of exactly the `EditableTextRenderView` —
    /// the inner anchor (ADR-0030), wrapped directly around it in
    /// `build_field_view`, so the IME cursor-area loop's `transform_to`
    /// starts right at the editable instead of walking through `anchor`'s
    /// wider subtree (which also covers the `AnimatedBuilder` in between).
    inner_anchor: flui_objects::SubtreeAnchor,
    /// The probe the field's store asks the window's pointer targets whether
    /// they would take a press with, set by the `EditableTextRenderView`
    /// from its render-object context each time it is built or updated.
    press_probe: Rc<Cell<Option<PressProbe>>>,
    /// Acquired in `init_state`, never in `build` — the pointer handlers need
    /// it to reach the anchored `RenderEditable` and to map a global point
    /// into that object's local space, and a frame phase is not where a
    /// presentation capability may be taken.
    pipeline_owner: Option<PipelineCell>,
    /// The node this field's node hangs under — the nearest enclosing focus
    /// parent at mount, or the root scope's backing node. Detached from in
    /// `dispose`.
    parent: Option<Rc<FocusNode>>,
    /// The controller this mounted field drives, behind a shared cell so it
    /// can be RETARGETED.
    ///
    /// A plain clone would be captured by value into the key handler and the
    /// IME attach callback at mount, and a parent rebuilding with a different
    /// controller would then leave those closures driving the original — the
    /// swap silently ignored. The cell is what lets `did_update_view` point
    /// every one of them at the replacement by writing once.
    ///
    /// `Rc<RefCell<_>>` rather than a lock: this is presentation-local state
    /// touched only on the owner thread, like every other `Rc` field here.
    controller: Rc<RefCell<TextEditingController>>,
    /// ID for the listener we added to `controller` so we can remove it on
    /// dispose — avoids a `remove_all_listeners` that would disrupt other
    /// subscribers.
    controller_listener_id: Option<flui_foundation::Subscription>,
    /// The single notifier that drives the inner `AnimatedBuilder`.  Fires on
    /// text changes (forwarded from the controller listener) **and** on focus
    /// changes (forwarded from the FocusManager listener).
    rebuild_notifier: flui_foundation::ChangeNotifier,
    /// ID for the focus-change listener we added to the [`FocusManager`], so
    /// dispose removes exactly ours.
    focus_listening: Option<flui_foundation::Subscription>,
    /// ID for the second `FocusManager` listener — attaches/detaches the IME
    /// client on this field's own focus transitions. Kept separate from
    /// `focus_listener_id` so the (already-tested) rebuild-on-focus-change
    /// listener is untouched by the IME wiring.
    ime_focus_listening: Option<flui_foundation::Subscription>,
    /// Reusable focus-edge operation shared by the manager listener and live
    /// focus-node replacement reconciliation.
    ime_focus_transition: Option<ImeFocusTransition>,
    /// The IME attach/detach capability, acquired once in `init_state` (the
    /// frame-capability rule `post_frame_handle` follows —
    /// `LifecycleContext::text_input_handle`'s doc). `None` when no binding
    /// installed one (a bare `ElementTree` in a unit test): the field then
    /// simply never attaches, rather than panicking or silently no-opping
    /// through a stub.
    ime_handle: Option<TextInputHandle>,
    /// The active IME client token, if this field currently has one
    /// attached. Shared with the IME focus-listener closure (`Rc<RefCell<_>>`
    /// because that closure is `'static` and cannot borrow `&mut self`) so
    /// both the closure (attach on focus gain, detach on blur) and `dispose`
    /// (detach-on-unmount, independent of any focus-loss notification) can
    /// clear it.
    ime_token: Rc<RefCell<Option<ClientToken>>>,
    /// The post-frame scheduling capability the IME cursor-area loop uses,
    /// acquired once in `init_state` beside `ime_handle` (trigger-22: a
    /// lifecycle-only frame capability is acquired in `init_state`, never in
    /// `build`). `None` under a binding that installs no post-frame handle —
    /// the loop then simply never starts (warned, not panicked; see
    /// `init_state`'s IME focus listener).
    post_frame_handle: Option<flui_scheduler::PostFrameHandle>,
    /// The current IME attach's cursor-area loop alive-flag, if a loop is
    /// currently running. `None` when no loop is running (never attached,
    /// or already blurred/disposed).
    ///
    /// A *fresh* `Rc<Cell<bool>>` is minted per attach (ADR-0030): sharing
    /// one flag across attaches would let a stale queued firing from a
    /// PREVIOUS attach flip it back to `true` behavior on a blur→refocus,
    /// resurrecting a loop that should have died, or running two loops at
    /// once. Detach/dispose flips THIS slot's flag `false` and takes it out
    /// of the slot; a loop closure that already holds its own clone still
    /// sees the flip (shared `Cell`) and dies on its next firing.
    cursor_area_alive: Rc<RefCell<Option<Rc<Cell<bool>>>>>,
    /// The current [`EditableText::on_submitted`] callback, behind a shared
    /// cell for the same reason `controller` is: the key handler closure
    /// installed in `init_state` reads through it at DISPATCH time, so a
    /// parent rebuilding with a different callback (or none) takes effect
    /// without re-registering the handler. Unlike `controller`/`focus_node`,
    /// no identity comparison gates the update in `did_update_view` — a
    /// closure has no meaningful identity to compare, so it is simply
    /// overwritten every rebuild, which is cheap and always correct.
    on_submitted: Rc<RefCell<Option<SubmitCallback>>>,
    /// The current [`EditableText::on_changed`] callback, read at edit time
    /// through a shared cell for the reason `on_submitted` is.
    on_changed: Rc<RefCell<Option<TextChanged>>>,
    /// Whether an edit is reported through `on_changed`: cleared when
    /// dispose begins, so an edit the input method makes while the field
    /// lets go of it reaches no owner (ADR-0142 item 4). The callback itself
    /// stays in its cell, retired by whichever holder outlives the field.
    reports_edits: Rc<Cell<bool>>,
    /// The presentation's clipboard, acquired in `init_state`. `None` only
    /// on a bare owner; the clipboard actions are then disabled.
    clipboard: Option<ClipboardHandle>,
    /// The field's hold on the primary selection, acquired in `init_state`:
    /// claimed while the field shows a selection and is not obscured, read
    /// by a middle press.
    primary: Rc<PrimarySelectionOwner>,
    selection_drag: Rc<Cell<Option<SelectionDrag>>>,
    /// Whether the field is obscured, read by the copy action at key time
    /// and kept current by `did_update_view`.
    obscure: Rc<Cell<bool>>,
    /// What the field asks of the input method, read by the text store's
    /// status and the Enter key's handler, and kept current by
    /// `did_update_view`.
    text_input: Rc<Cell<TextInputOptions>>,
    /// The mask character, read by the text store's geometry and kept
    /// current by `did_update_view`.
    obscuring_character: Rc<Cell<char>>,
    /// This field as the input method's text store (ADR-0090), built in
    /// `init_state` and attached while the field has focus.
    text_store: Option<Rc<EditableTextStore>>,
    /// The enclosing `Actions` chain the recorded one was layered over, so a
    /// changed ancestor chain is recorded again.
    enclosing_action_chain: Option<ActionChain>,
    /// This field's clipboard actions layered over the enclosing chain: the
    /// record on its focus node that a `Shortcuts` resolves intents against.
    action_chain: Option<ActionChain>,
    /// The record `action_chain` and the convention make on the focus node.
    focus_record: Option<FocusRecord>,
    /// Generation-checked ownership of that record on the node.
    action_chain_registration: Option<FocusNodeRegistration>,
    /// The latest view's `autofocus`.
    autofocus: bool,
    /// Whether the one autofocus request was made; it is never repeated.
    did_autofocus: bool,
    /// `on_changed` and `on_submitted` run inside a write it opens
    /// (ADR-0086).
    writer: WriterSource,
    /// The touch handles: shown by a touch, taken by a key, a mouse, the
    /// focus leaving or a change they did not make.
    touch_handles: Rc<FieldHandles>,
    /// The platform whose conventions the field followed as of its last
    /// build, which the key handler reads at dispatch: the word-jump
    /// modifier and the chords left to the route.
    platform: Rc<Cell<TargetPlatform>>,
    /// The direction the field laid its line out in as of its last build,
    /// which the key handler reads at dispatch: Left and Right step toward
    /// the side of the line they name.
    direction: Rc<Cell<TextDirection>>,
    /// The touch long press that selects a word and shows its handles.
    long_press: FieldLongPress,
    /// Whether the host serving the field draws its selection UI, which the
    /// field then draws and recognizes none of.
    platform_selection: Rc<PlatformSelection>,
}

impl std::fmt::Debug for EditableTextState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditableTextState")
            .field("focus_node_id", &self.focus_node.id().get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for EditableText {
    type State = EditableTextState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> EditableTextState {
        let focus_node = Rc::clone(&self.focus_node);
        focus_node.set_can_request_focus(self.enabled);
        EditableTextState {
            focus_node: Rc::clone(&focus_node),
            observed_focus_node: Rc::new(RefCell::new(focus_node)),
            accepts_semantics_actions: Rc::new(Cell::new(self.enabled)),
            focus_manager: ctx.focus_manager(),
            focus_attachment: Rc::new(RefCell::new(None)),
            rect_provider: None,
            rect_provider_registration: None,
            key_handler_registration: None,
            anchor: flui_objects::SubtreeAnchor::new(),
            inner_anchor: flui_objects::SubtreeAnchor::new(),
            press_probe: Rc::new(Cell::new(None)),
            pipeline_owner: None,
            parent: None,
            controller: Rc::new(RefCell::new(self.controller.clone())),
            controller_listener_id: None,
            rebuild_notifier: flui_foundation::ChangeNotifier::new(),
            focus_listening: None,
            ime_focus_listening: None,
            ime_focus_transition: None,
            ime_handle: None,
            ime_token: Rc::new(RefCell::new(None)),
            post_frame_handle: None,
            cursor_area_alive: Rc::new(RefCell::new(None)),
            writer: ctx.writer_source(),
            touch_handles: FieldHandles::new(),
            platform: Rc::new(Cell::new(TargetPlatform::Unknown)),
            direction: Rc::new(Cell::new(TextDirection::Ltr)),
            long_press: FieldLongPress::new(),
            platform_selection: Rc::new(PlatformSelection::new(ctx.rebuild_handle())),
            on_submitted: Rc::new(RefCell::new(self.on_submitted.clone())),
            on_changed: Rc::new(RefCell::new(self.on_changed.clone())),
            reports_edits: Rc::new(Cell::new(true)),
            clipboard: None,
            primary: Rc::default(),
            selection_drag: Rc::new(Cell::new(None)),
            obscure: Rc::new(Cell::new(self.obscure_text)),
            text_input: Rc::new(Cell::new(self.text_input)),
            obscuring_character: Rc::new(Cell::new(self.obscuring_character)),
            text_store: None,
            enclosing_action_chain: None,
            action_chain: None,
            focus_record: None,
            action_chain_registration: None,
            autofocus: self.autofocus,
            did_autofocus: false,
        }
    }
}

impl EditableTextState {
    /// Follows the overlay the touch handles go in, the vsync that times
    /// the caret's, and the gesture arena the long press competes in.
    fn follow_handle_scope(&mut self, ctx: &dyn LifecycleContext) {
        self.touch_handles.set_scope(
            crate::Overlay::maybe_of(ctx),
            flui_view::environment::VsyncScope::maybe_of(ctx),
            ctx.haptics(),
        );
        let drag = Rc::downgrade(&self.selection_drag);
        self.long_press.build(
            flui_view::environment::GesturePolicy::of(ctx),
            &self.touch_handles,
            // A long press takes its contact from the field's own drag.
            Rc::new(move || {
                if let Some(drag) = drag.upgrade() {
                    drag.set(None);
                }
            }),
        );
    }

    /// The one autofocus request, made the first time `autofocus` is (or
    /// becomes) `true` once the node is attached; the latch is set before
    /// the request runs, so a panicking focus listener does not repeat it.
    fn try_autofocus(&mut self) {
        if self.did_autofocus || !self.autofocus {
            return;
        }
        self.did_autofocus = true;
        crate::interaction::focus::request_autofocus(
            self.parent.as_ref(),
            &self.focus_manager,
            &self.focus_node,
        );
    }

    /// The observer every user-edit path reports through.
    fn edit_observer(&self) -> EditObserver {
        EditObserver {
            controller: Rc::clone(&self.controller),
            on_changed: Rc::clone(&self.on_changed),
            reports: Rc::clone(&self.reports_edits),
            writer: self.writer.clone(),
        }
    }

    /// What commits this field's composition before it loses its input.
    fn composition_commit(&self) -> CompositionCommit {
        CompositionCommit {
            handle: self.ime_handle.clone(),
            token: Rc::clone(&self.ime_token),
        }
    }

    /// The key handler for `node`: [`build_key_handler`], reporting each
    /// edit it makes through `on_changed`.
    fn key_handler(&self, node: &Rc<FocusNode>) -> KeyEventHandler {
        let handler = build_key_handler(
            Rc::clone(&self.controller),
            Rc::clone(node),
            Rc::clone(&self.on_submitted),
            Rc::clone(&self.text_input),
            self.writer.clone(),
            Rc::clone(&self.platform),
            Rc::clone(&self.direction),
        );
        let edits = self.edit_observer();
        let store = self.text_store.clone();
        let handles = Rc::downgrade(&self.touch_handles);
        Rc::new(move |event| {
            // Each step runs though an earlier one failed (a queued grant,
            // `on_changed`, the observer): the platform hears of the key's
            // edit before the first failure is resumed.
            let mut calls = OwnerCalls::new();
            // A key ends a touch's handles.
            if matches!(event.state, KeyState::Down(_))
                && let Some(handles) = handles.upgrade()
            {
                calls.run(|| {
                    let mut failure = FirstPanic::new();
                    handles.hide(&mut failure);
                    failure.resume();
                });
            }
            // An IME grant still queued from the last frame lands first, so
            // this key's edit follows it rather than overtaking it.
            if let Some(store) = &store {
                calls.run(|| store.run_deferred_before_app_edit());
            }
            // Enter only submits; a controller change there is the submit
            // callback's own programmatic edit, not the user's.
            let result = calls.run(|| {
                if matches!(event.key, Key::Named(NamedKey::Enter)) {
                    handler(event)
                } else {
                    edits.around(|| handler(event))
                }
            });
            if let Some(store) = &store {
                calls.run(|| store.controller_changed());
            }
            calls.resume();
            result.expect("BUG: a key edit that failed resumed its failure above")
        })
    }

    /// This field's copy, cut and paste, the middle press's paste included.
    fn clipboard_action(&self) -> ClipboardTextAction {
        ClipboardTextAction {
            attachment: Rc::clone(&self.focus_attachment),
            enabled: Rc::clone(&self.accepts_semantics_actions),
            controller: Rc::clone(&self.controller),
            focus_node: Rc::clone(&self.observed_focus_node),
            obscure: Rc::clone(&self.obscure),
            clipboard: self.clipboard.clone(),
            primary: Rc::clone(&self.primary),
            edits: self.edit_observer(),
            commit: self.composition_commit(),
        }
    }

    /// This field's select-all.
    fn select_all_action(&self) -> SelectAllTextAction {
        SelectAllTextAction {
            controller: Rc::clone(&self.controller),
            focus_node: Rc::clone(&self.observed_focus_node),
        }
    }

    /// The listener this field adds to its controller: a change rebuilds
    /// the field, and the selection it leaves is claimed as the primary
    /// selection, or let go when empty.
    fn controller_listener(&self) -> Rc<dyn Fn()> {
        let rebuild_notifier = self.rebuild_notifier.clone();
        let primary = Rc::clone(&self.primary);
        let controller = Rc::downgrade(&self.controller);
        let obscure = Rc::clone(&self.obscure);
        let handles = Rc::downgrade(&self.touch_handles);
        Rc::new(move || {
            // Each runs though the other's owner code fails; the first
            // failure is resumed after both.
            let mut failure = FirstPanic::new();
            failure.contain(|| rebuild_notifier.notify_listeners());
            failure.contain(|| claim_primary(&primary, &controller, &obscure));
            if let Some(handles) = handles.upgrade() {
                handles.controller_changed(&mut failure);
            }
            failure.resume();
        })
    }

    /// This field's clipboard actions layered over the `Actions` chain at
    /// its position, recorded on its focus node — where a `Shortcuts` looks
    /// an intent up while this field holds the primary focus. Depends on the
    /// enclosing chain, so a changed one is recorded again; the same record
    /// `Focus` keeps (ADR-0079).
    fn record_action_chain(&mut self, ctx: &dyn BuildContext) {
        let enclosing = EnclosingActions::depend_on(ctx);
        let platform = flui_view::environment::PlatformConvention::of(ctx);
        let unchanged = match (&enclosing, &self.enclosing_action_chain) {
            (Some(new), Some(held)) => new.is(held),
            (None, None) => true,
            _ => false,
        };
        let same_platform = self
            .focus_record
            .as_ref()
            .is_some_and(|record| record.platform() == platform);
        if unchanged
            && same_platform
            && self
                .action_chain_registration
                .as_ref()
                .is_some_and(FocusNodeRegistration::is_current)
        {
            return;
        }
        let action = self.clipboard_action();
        let chain = layered_chain(
            enclosing.clone(),
            Rc::new(vec![
                erased_action::<CopySelectionTextIntent>(action.clone()),
                erased_action::<PasteTextIntent>(action),
                erased_action::<SelectAllTextIntent>(self.select_all_action()),
                erased_action::<ActivateIntent>(KeptByTheField),
                erased_action::<ActivationPressIntent>(KeptByTheField),
                erased_action::<DismissIntent>(KeptByTheField),
                erased_action::<ShowContextMenuIntent>(ShowSelectionMenu::new(
                    self.touch_handles.menu(),
                    enclosing.as_ref(),
                )),
            ]),
        );
        // Register the new record before the old token drops: the old one is
        // no longer current then, so dropping it leaves the new one in place.
        let record = FocusRecord::new(chain.clone(), platform);
        self.action_chain_registration = Some(record.register_on(&self.focus_node));
        self.focus_record = Some(record);
        self.action_chain = Some(chain);
        self.enclosing_action_chain = enclosing;
    }
}

/// `Rect` (min/max corners) to `Bounds` (origin/size) — `PlatformTextInput::
/// set_ime_cursor_area`'s parameter convention, matching `PlatformWindow::
/// bounds`.
pub(super) fn bounds_from_rect(rect: Rect) -> Bounds<f64> {
    Bounds::new(
        Point::new(rect.min.x, rect.min.y),
        flui_foundation::geometry::Size::new(rect.width(), rect.height()),
    )
}
