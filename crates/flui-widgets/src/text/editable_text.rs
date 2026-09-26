//! [`EditableText`] — single-line editable text backed by a
//! [`TextEditingController`].

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
    sync::Arc,
};
use unicode_segmentation::UnicodeSegmentation;

use flui_foundation::ListenerId;
use flui_foundation::notifier::Listenable;
use flui_interaction::PointerDispatch;
use flui_interaction::events::PointerEventExt;
use flui_interaction::events::{Key, KeyState, Modifiers, NamedKey};
use flui_interaction::routing::{
    FocusAttachment, FocusManager, FocusNode, FocusNodeRegistration, KeyEventHandler,
    KeyEventResult, RectProvider,
};
use flui_interaction::{ClientToken, ClipboardHandle, TextInputHandle};
use flui_objects::RenderEditable;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{
    Color, ImeEvent, Offset, Point, Rect,
    geometry::{Bounds, Pixels},
    platform::TargetPlatform,
    typography::{TextDirection, TextSpan, TextStyle},
};
use flui_view::prelude::*;
use flui_view::{BoxedView, RenderView, impl_render_view};

use crate::AnimatedBuilder;
use crate::interaction::actions::{
    Action, ActionChain, ActionChainProvider, ActionOutcome, CopySelectionTextIntent,
    PasteTextIntent, as_node_context, erased_action, layered_chain,
};
use crate::semantics::Semantics;
use crate::text::controller::TextEditingController;

type ImeFocusTransition = Rc<dyn Fn(bool)>;
/// Callback for [`EditableText::on_submitted`] — see that method's doc.
/// Exported (not crate-private) so [`RawTextField`](super::text_field::RawTextField)'s
/// own `on_submitted` passthrough and `flui_material::TextField`'s can share
/// one canonical alias instead of each declaring their own.
pub type SubmitCallback = Rc<dyn Fn(&str)>;

// ============================================================================
// EditableText
// ============================================================================

/// Flutter's own `EditableText.obscuringCharacter` default, U+2022 BULLET.
const DEFAULT_OBSCURING_CHARACTER: char = '\u{2022}';

/// The masked text, and each of `offsets` mapped into it.
///
/// `offsets` is rewritten in place: every entry arrives as a byte offset into
/// `text` and leaves as the corresponding byte offset into the returned mask.
/// A slice rather than one value because the caret is never the only offset
/// that has to make the trip — the selection's two ends do too, and mapping
/// them separately meant three walks over the text where the mask itself only
/// needs one. It also keeps the correspondence in a single place: a selection
/// mapped by a different rule than the caret would paint a highlight that does
/// not line up with the caret inside it.
///
/// One mask character per SOURCE **extended grapheme cluster** — the
/// user-perceived character, not the Unicode scalar and not the UTF-16 code
/// unit.
///
/// **Why not the reference's unit.** Flutter builds the mask as
/// `obscuringCharacter * text.length`, and Dart's `String.length` counts
/// UTF-16 code units, so a single emoji becomes TWO bullets and a
/// family-emoji ZWJ sequence becomes eleven. That is an artifact of Dart's
/// string representation rather than a designed contract, and it leaks: the
/// bullet count tells an onlooker which keystrokes were astral. Flutter's own
/// docs warn against `String.length` for user-visible character counts
/// (`editable_text.dart:1440-1444`) — the obscuring path just predates or
/// ignores that.
///
/// **Why the grapheme.** `TextEditingController` moves and deletes by
/// grapheme cluster (see its "Character unit" doc), so the mask counts the
/// same unit: one Backspace removes one cluster and one bullet. Masking per
/// scalar while the caret stepped per cluster would put the two out of step —
/// a Backspace on a family emoji would remove five bullets at once — and the
/// caret would land between bullets that correspond to no boundary the
/// controller can produce.
///
/// Every offset in `offsets` sits at a grapheme boundary (the controller only
/// ever produces those), so "clusters strictly before it" is the count that
/// maps. Average and worst case O(clusters × offsets); `offsets` is three
/// entries at its largest, so this is the single walk the mask needs either
/// way.
fn obscure(text: &str, offsets: &mut [usize], mask: char) -> String {
    let mask_len = mask.len_utf8();
    let mut masked = String::with_capacity(text.len());
    let mut mapped = vec![0_usize; offsets.len()];
    for (byte_offset, _) in text.grapheme_indices(true) {
        for (slot, source) in mapped.iter_mut().zip(offsets.iter()) {
            if byte_offset < *source {
                *slot += mask_len;
            }
        }
        masked.push(mask);
    }
    offsets.copy_from_slice(&mapped);
    masked
}

/// The SOURCE byte offset a point in the root's coordinate space falls on,
/// or `None` when the field is not mounted, not laid out, or the point misses
/// it.
///
/// # Why the global point and not the listener's local one
///
/// `Listener` hands its callbacks the position in its own box as well as the
/// root's. The listener's box is not the editable's: `AnchoredBox` sits
/// between them, and relying on proxies being zero-offset is an unstated
/// coupling that a later wrapper would break silently. Mapping the root's
/// point through `transform_to` is what `EditableTextState::global_caret_rect`
/// already does in the other direction, and it stays correct whatever the
/// subtree grows.
///
/// # Why the answer is in SOURCE space
///
/// The render object holds masked text on an obscured field, so the offset it
/// returns is a masked one while [`TextEditingController`] holds source bytes.
/// The conversion happens here, at the same seam
/// [`build_field_view`] masks at, so the two directions cannot drift apart.
/// `source_text` must be the CONTROLLER's own source string (the caller's
/// job to supply — `RenderEditable::plain_text()` is the MASKED string on
/// an obscured field, the wrong input for
/// [`source_offset_for_masked_offset`], whose own doc spells out why: it
/// walks `source`'s grapheme clusters to answer a SOURCE byte offset, and
/// walking the masked string instead just answers back the masked offset
/// it was given, silently corrupting every obscured-field tap).
fn source_offset_at_global(
    owner: &PipelineCell,
    inner_anchor: &flui_objects::SubtreeAnchor,
    global: Offset<Pixels>,
    obscuring: Option<char>,
    source_text: &str,
) -> Option<usize> {
    let anchor_id = inner_anchor.get()?;
    // `try_with`, not `with`: a pointer event can land in a callback a frame
    // phase happens to drive, and `with` panics on a live `with_mut` checkout.
    // "The tree is busy" is a real answer here — the gesture does nothing for
    // that event — and `try_with`'s own doc names this exact case.
    owner
        .try_with(|owner| {
            let root_id = owner.root_id()?;
            let tree = owner.render_tree();
            let editable_id = *tree.children(anchor_id).first()?;
            let editable = tree
                .get(editable_id)?
                .as_box()?
                .render_object()
                .downcast_ref::<RenderEditable>()?; // the pointer handlers reach the one concrete render object type this widget mounts under `inner_anchor`, through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure — the same sanctioned boundary as `CursorAreaLoop::global_caret_rect`.
            let to_root = owner.transform_to(editable_id, root_id)?;
            let (x, y) = to_root.try_inverse()?.transform_point(global.dx, global.dy);
            let masked = editable.byte_offset_for_local_offset(Offset::new(x, y))?;
            Some(match obscuring {
                Some(mask) => source_offset_for_masked_offset(source_text, masked, mask),
                None => masked,
            })
        })
        .flatten()
}

/// The SOURCE byte range of the word under a point in the root's
/// coordinate space — the double-tap counterpart of
/// [`source_offset_at_global`]; see its doc for the coordinate mapping
/// and masked/source distinction, both shared verbatim here.
///
/// Delegates to [`RenderEditable::word_range_at_local_offset`]
/// (`flui-objects`), which itself delegates to `flui-painting`'s
/// `TextLayout::get_word_boundary` (not a doc link: `flui-painting` is a
/// dev-dependency of this crate, not a regular one, so the path would not
/// resolve in a normal `cargo doc` build)
/// — UAX #29 segmentation via the same `unicode-segmentation` crate this
/// widget's own Ctrl/Alt+Arrow word-jump uses (`controller.rs`'s private
/// `next_word_boundary`/`prev_word_boundary`), but NOT the same function:
/// that pair answers a directional "next/
/// previous stop" query with its own asymmetric tie-break (see the
/// controller's `# Word unit` doc), while this one answers "which segment
/// is under this exact position", with a DIFFERENT tie-break suited to
/// that question. A double-tap and a keyboard word-jump can therefore
/// land on different boundaries for the same buffer in edge cases (a
/// caret sitting exactly on a segment boundary, say) — a known
/// consequence of not sharing one primitive across a crate boundary that
/// only `flui-painting` can see (`TextLayout`) and only
/// `flui-widgets::controller` can see (the raw buffer, no shaping), not
/// an oversight.
///
/// `source_text` must be the CONTROLLER's own source string — see
/// [`source_offset_at_global`]'s doc for why `RenderEditable::plain_text()`
/// (the masked string on an obscured field) is the wrong input here too.
fn source_word_range_at_global(
    owner: &PipelineCell,
    inner_anchor: &flui_objects::SubtreeAnchor,
    global: Offset<Pixels>,
    obscuring: Option<char>,
    source_text: &str,
) -> Option<Range<usize>> {
    let anchor_id = inner_anchor.get()?;
    owner
        .try_with(|owner| {
            let root_id = owner.root_id()?;
            let tree = owner.render_tree();
            let editable_id = *tree.children(anchor_id).first()?;
            let editable = tree
                .get(editable_id)?
                .as_box()?
                .render_object()
                .downcast_ref::<RenderEditable>()?; // same sanctioned boundary as `source_offset_at_global` above.
            let to_root = owner.transform_to(editable_id, root_id)?;
            let (x, y) = to_root.try_inverse()?.transform_point(global.dx, global.dy);
            let masked = editable.word_range_at_local_offset(Offset::new(x, y))?;
            Some(match obscuring {
                Some(mask) => {
                    source_offset_for_masked_offset(source_text, masked.start, mask)
                        ..source_offset_for_masked_offset(source_text, masked.end, mask)
                }
                None => masked,
            })
        })
        .flatten()
}

/// The source byte offset a masked byte offset corresponds to — the inverse
/// of [`obscure`]'s mapping.
///
/// Needed because [`build_field_view`] masks the text *before* it reaches the
/// render object, so every offset a pointer query returns is in MASKED byte
/// space while [`TextEditingController`] holds SOURCE bytes. Writing one into
/// the other is a silent corruption on any obscured field:
/// [`DEFAULT_OBSCURING_CHARACTER`] is `U+2022`, three bytes, so the two spaces
/// diverge at the very first character.
///
/// The correspondence is the one [`obscure`] establishes — exactly one mask
/// character per source grapheme cluster — read backwards: the masked offset
/// divided by the mask's width is a cluster index, and that cluster's byte
/// offset is the answer.
///
/// A masked offset past the end clamps to the source's end, and one that is
/// not a multiple of the mask width rounds down to the mask character it falls
/// inside. Neither should occur — the render object clamps to its own char
/// boundaries, which for masked text are multiples of the mask width — but
/// clamping rather than asserting keeps a wrong offset from panicking a field.
fn source_offset_for_masked_offset(source: &str, masked_offset: usize, mask: char) -> usize {
    let cluster_index = masked_offset / mask.len_utf8();
    source
        .grapheme_indices(true)
        .nth(cluster_index)
        .map_or(source.len(), |(byte_offset, _)| byte_offset)
}

/// A single-line text field that accepts keyboard input when focused.
///
/// Flutter parity: `widgets/editable_text.dart` `EditableText` — the low-level
/// editable primitive.  [`RawTextField`](super::text_field::RawTextField) wraps
/// this with decoration and tap-to-focus.
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
/// On focus gain, `EditableTextState` attaches an IME client through
/// [`LifecycleContext::text_input_handle`] (acquired in `init_state`, per the
/// frame-capability rule that method's doc states) — its callback routes
/// each [`ImeEvent`] to the matching [`TextEditingController`] composing
/// operation (`Preedit` → `set_composing_text`, `Commit` → `commit_text`,
/// `Disabled` → `clear_composing`). On blur and on dispose the client is
/// detached (the ADR-0030 detach-on-dispose contract — a field unmounted
/// while still focused must not leave a stale IME client attached).
///
/// **Suppression contract**: the key handler skips `Key::Character`
/// insertion only while [`TextEditingController::is_composing`] is `true` —
/// suppressing unconditionally after focus gain would silently kill plain
/// (non-IME) typing for the rest of the session, since winit only sends
/// `Key::Character` for keys it did **not** already route through
/// composition. See [`ImeEvent`]'s doc for the full contract.
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
/// composition ends). This is a winit single-rect reduction of Flutter's
/// transform+local-rect protocol (`editable_text.dart`'s
/// `_updateSizeAndTransform`/`_updateComposingRectIfNeeded`/
/// `_schedulePeriodicPostFrameCallbacks`, tag `3.44.0`) — see ADR-0030 for
/// the loop mechanics (why it is per-attach: a fresh alive-flag and a fresh
/// last-sent cache each attach, rather than shared across the field's
/// lifetime) and ADR-0030 for the composing-rect-over-caret-rect fallback
/// order this loop now applies.
///
/// # Clipboard
///
/// The field answers [`CopySelectionTextIntent`] and [`PasteTextIntent`] on
/// its own focus node — the chain a `Shortcuts` resolves an intent against
/// when this field holds the primary focus — so the Ctrl/Cmd+C, X and V
/// bindings [`DefaultFocusTraversal`](crate::DefaultFocusTraversal) installs
/// under every `FocusRoot` reach it. Its actions are layered over the chain
/// visible at its position, and are the nearest declaration of those two
/// intent types, so they win over an ancestor `Actions` binding for them.
///
/// | Intent | Enabled when | Does |
/// |---|---|---|
/// | Copy | a clipboard is installed, the field is not obscured, and the selection is not empty | writes the selection; the selection stays |
/// | Cut | as Copy, and the field is enabled | writes the selection, then deletes it |
/// | Paste | a clipboard is installed, the field is enabled, and no IME composition is active | replaces the selection with the clipboard's text, line breaks removed |
///
/// A disabled action leaves the key unconsumed, so an obscured field's
/// Ctrl+C keeps bubbling. Paste consumes the key even when the clipboard is
/// empty, as Flutter's does.
///
/// The clipboard is acquired in `init_state` through
/// [`LifecycleContext::clipboard_handle`]. ADR-0084: acquired as
/// `cx.capability::<Clipboard>()` once the capability registry lands.
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
///   endpoints and the copy/paste menu Flutter shows on touch platforms.
/// - **Multi-line** — newlines are inserted as literal characters but line
///   wrapping, multi-line layout, and vertical scrolling are not implemented.
/// - **Input formatters** — no validation or transformation pipeline.
/// - **Scroll when text overflows** — the rendered text clips without scrolling.
#[derive(Clone, StatefulView)]
pub struct EditableText {
    /// Controller that owns the text buffer and caret.
    pub(super) controller: TextEditingController,
    /// Focus ownership is explicit and presentation-local. The caller owns
    /// the node, matching Flutter's required `EditableText.focusNode`.
    pub(super) focus_node: Rc<FocusNode>,
    /// Height of the rendered caret bar in logical pixels.
    pub(super) caret_height: f32,
    /// Color of the caret bar when the field is focused.
    pub(super) caret_color: Color,
    pub(super) selection_color: Color,
    /// Whether this field accepts focus and input. `true` by default.
    ///
    /// **Named hoist, not a direct port**: the oracle has no
    /// `EditableText.enabled` property at this tag — `enabled` lives on
    /// `TextField` and flows down as `_isEnabled` into
    /// `_effectiveFocusNode.canRequestFocus`
    /// (`text_field.dart:1183,1282-1299`, tag `3.44.0`). FLUI's
    /// [`RawTextField`](super::text_field::RawTextField) has no
    /// decoration/enabled plumbing yet, so this substrate hoists the behavior
    /// onto `EditableText` itself, one layer lower than the oracle — see
    /// [`enabled`](Self::enabled)'s doc comment for exactly what it
    /// withholds.
    pub(super) enabled: bool,
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
    /// The character painted in place of each source character. Flutter's
    /// default, and Flutter asserts it is exactly one character.
    pub(super) obscuring_character: char,
    /// Called with the current text when Enter is pressed while this field
    /// has focus — see [`Self::on_submitted`]'s doc.
    pub(super) on_submitted: Option<SubmitCallback>,
    /// Called with the new text after each edit the user makes — see
    /// [`Self::on_changed`]'s doc.
    pub(super) on_changed: Option<TextChanged>,
}

/// Callback for [`EditableText::on_changed`].
type TextChanged = Rc<dyn Fn(&str)>;

impl EditableText {
    /// Create an `EditableText` driven by `controller` and `focus_node`.
    #[must_use]
    pub fn new(controller: TextEditingController, focus_node: Rc<FocusNode>) -> Self {
        Self {
            controller,
            focus_node,
            caret_height: 18.0,
            caret_color: Color::BLACK,
            // Transparent by default, so the primitive paints no highlight
            // until a caller (a decorated `TextField`, a theme) chooses one —
            // the same arm Flutter reaches with a null `selectionColor`.
            selection_color: Color::TRANSPARENT,
            enabled: true,
            text_style: None,
            obscure_text: false,
            obscuring_character: DEFAULT_OBSCURING_CHARACTER,
            on_submitted: None,
            on_changed: None,
        }
    }

    /// Override the caret bar height (default 18 logical pixels).
    #[must_use]
    pub fn caret_height(mut self, height: f32) -> Self {
        self.caret_height = height;
        self
    }

    /// Fill for the selection highlight.
    ///
    /// Defaults to [`Color::TRANSPARENT`]: the primitive tracks a selection
    /// whether or not it paints one, and a field with no chosen colour should
    /// not invent a highlight. `TextField` and the Material/Cupertino themes
    /// are where a real colour comes from.
    #[must_use]
    pub fn selection_color(mut self, color: Color) -> Self {
        self.selection_color = color;
        self
    }

    /// Override the caret color (default black).
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

    /// Override the character painted in place of each source character
    /// (default `'\u{2022}'`, Flutter's own).
    ///
    /// Takes a `char`, so "exactly one character" is a type rather than the
    /// `assert(obscuringCharacter.length == 1)` the reference performs at
    /// runtime on a `String`.
    #[must_use]
    pub fn obscuring_character(mut self, character: char) -> Self {
        self.obscuring_character = character;
        self
    }

    /// Set whether the field accepts focus and keyboard input (default
    /// `true`) — see the [`enabled`](Self::enabled) field's doc comment for
    /// why this is a named hoist of `TextField.enabled`, not a direct
    /// `EditableText` parity port.
    ///
    /// A disabled field withholds focus acquisition by marking its explicit
    /// node
    /// [`FocusNode::set_can_request_focus`]`(false)`, which keyboard-traversal
    /// (`focus_next`/`focus_previous`) already honors and which — matching
    /// Flutter's `FocusNode.canRequestFocus` setter — releases primary focus
    /// itself if the field is focused when it becomes disabled; no separate
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
    /// Flutter parity: `EditableText.onSubmitted` (`editable_text.dart`) —
    /// fires on a raw Enter keypress here rather than an IME action-button
    /// commit, since this substrate has no platform IME-action-button
    /// integration yet (see the type doc's `# DEFERRED (v1)` list). The key
    /// is consumed ([`KeyEventResult::Handled`](flui_interaction::routing::KeyEventResult))
    /// only when a callback is set — with none, Enter is left unconsumed
    /// (`Ignored`) so an ancestor can still act on it, unchanged from this
    /// field's behavior before this method existed.
    ///
    /// No multiline support exists in this substrate (there is no
    /// newline-insertion behavior to conflict with), so Enter has exactly
    /// one meaning here: submit.
    #[must_use]
    pub fn on_submitted(mut self, callback: impl Fn(&str) + 'static) -> Self {
        self.on_submitted = Some(Rc::new(callback));
        self
    }

    /// Call `callback` with the new text after each edit the user makes —
    /// typing, deleting, an IME commit, a cut or a paste — Flutter's
    /// `EditableText.onChanged`.
    ///
    /// Only user edits: a caller changing the controller itself
    /// (`set_text`, `clear`) does not call it, which is what keeps a form
    /// field's reset from counting as the user's input. Runs after the edit,
    /// with no borrow of the field held.
    #[must_use]
    pub fn on_changed(mut self, callback: impl Fn(&str) + 'static) -> Self {
        self.on_changed = Some(Rc::new(callback));
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
            .field("selection_color", &self.selection_color)
            .field("enabled", &self.enabled)
            .field("text_style", &self.text_style)
            .field("obscure_text", &self.obscure_text)
            .field("obscuring_character", &self.obscuring_character)
            .field("on_submitted", &self.on_submitted.is_some())
            .field("on_changed", &self.on_changed.is_some())
            .finish()
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
    /// Exact manager acquired from the mounting build owner.
    focus_manager: Option<Rc<FocusManager>>,
    /// Generation-checked attachment owned by this mounted state.
    focus_attachment: Option<FocusAttachment>,
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
    controller_listener_id: Option<ListenerId>,
    /// The single notifier that drives the inner `AnimatedBuilder`.  Fires on
    /// text changes (forwarded from the controller listener) **and** on focus
    /// changes (forwarded from the FocusManager listener).
    rebuild_notifier: flui_foundation::notifier::ChangeNotifier,
    /// ID for the focus-change listener we added to the [`FocusManager`], so
    /// dispose removes exactly ours.
    focus_listener_id: Option<ListenerId>,
    /// ID for the second `FocusManager` listener — attaches/detaches the IME
    /// client on this field's own focus transitions. Kept separate from
    /// `focus_listener_id` so the (already-tested) rebuild-on-focus-change
    /// listener is untouched by the IME wiring.
    ime_focus_listener_id: Option<ListenerId>,
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
    local_post_frame_handle: Option<flui_scheduler::LocalPostFrameHandle>,
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
    /// The presentation's clipboard, acquired in `init_state`. `None` only
    /// on a bare owner; the clipboard actions are then disabled.
    clipboard: Option<ClipboardHandle>,
    /// Whether the field is obscured, read by the copy action at key time
    /// and kept current by `did_update_view`.
    obscure: Rc<Cell<bool>>,
    /// The enclosing `Actions` chain the recorded one was layered over, so a
    /// changed ancestor chain is recorded again.
    enclosing_action_chain: Option<ActionChain>,
    /// This field's clipboard actions layered over the enclosing chain: the
    /// record on its focus node that a `Shortcuts` resolves intents against.
    action_chain: Option<ActionChain>,
    /// Generation-checked ownership of that record on the node.
    action_chain_registration: Option<FocusNodeRegistration>,
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

    fn create_state(&self) -> EditableTextState {
        let focus_node = Rc::clone(&self.focus_node);
        focus_node.set_can_request_focus(self.enabled);
        EditableTextState {
            focus_node: Rc::clone(&focus_node),
            observed_focus_node: Rc::new(RefCell::new(focus_node)),
            focus_manager: None,
            focus_attachment: None,
            rect_provider: None,
            rect_provider_registration: None,
            key_handler_registration: None,
            anchor: flui_objects::SubtreeAnchor::new(),
            inner_anchor: flui_objects::SubtreeAnchor::new(),
            pipeline_owner: None,
            parent: None,
            controller: Rc::new(RefCell::new(self.controller.clone())),
            controller_listener_id: None,
            rebuild_notifier: flui_foundation::notifier::ChangeNotifier::new(),
            focus_listener_id: None,
            ime_focus_listener_id: None,
            ime_focus_transition: None,
            ime_handle: None,
            ime_token: Rc::new(RefCell::new(None)),
            local_post_frame_handle: None,
            cursor_area_alive: Rc::new(RefCell::new(None)),
            on_submitted: Rc::new(RefCell::new(self.on_submitted.clone())),
            on_changed: Rc::new(RefCell::new(self.on_changed.clone())),
            clipboard: None,
            obscure: Rc::new(Cell::new(self.obscure_text)),
            enclosing_action_chain: None,
            action_chain: None,
            action_chain_registration: None,
        }
    }
}

impl EditableTextState {
    /// Attach the pointer handlers that turn a tap into a caret and a drag
    /// into a selection.
    ///
    /// Split out of `build` only for size; it runs on every rebuild and holds
    /// no state of its own beyond `drag_anchor`, shared with
    /// [`Self::wrap_double_tap_word_select`] (see its doc).
    ///
    /// # What is deliberately absent
    ///
    /// Shift-click extension and triple-tap line selection. Each needs a
    /// click-count or a modifier this level does not see — `Listener`
    /// delivers raw pointer events, and the tap/multi-tap arbitration that
    /// produces those lives in `flui-interaction`'s recognisers. Double-tap
    /// word selection lives one level up, in
    /// [`Self::wrap_double_tap_word_select`], composed around this
    /// method's own return value rather than added here.
    fn install_pointer_handlers(
        &self,
        field: crate::interaction::Listener,
        view: &EditableText,
        drag_anchor: Rc<Cell<Option<usize>>>,
    ) -> impl IntoView {
        let enabled = view.enabled;
        let obscuring = view.obscure_text.then_some(view.obscuring_character);
        let owner = self.pipeline_owner.clone();
        let anchor = self.inner_anchor.clone();
        let controller = Rc::clone(&self.controller);
        let focus_node = Rc::clone(&self.focus_node);
        // `drag_anchor` is where the current drag began, in SOURCE byte
        // space, shared with `wrap_double_tap_word_select` (this method's
        // caller passes the SAME cell to both). `None` means no drag of
        // ours is in flight, which is what makes a move that started
        // outside this field — or one that arrived after a cancel, OR one
        // silenced by a double-tap widening this same contact into a word
        // selection (see `wrap_double_tap_word_select`'s doc) — a no-op
        // rather than a selection anchored at whatever was last there.

        let resolve = {
            let owner = owner.clone();
            let anchor = anchor.clone();
            let controller = Rc::clone(&controller);
            move |global: Offset<Pixels>| -> Option<usize> {
                // Owned, not borrowed across the call: `source_text` must be
                // the CONTROLLER's source string, not `RenderEditable::
                // plain_text()` (masked on an obscured field) — see
                // `source_offset_at_global`'s doc for why passing the wrong
                // one silently corrupts every obscured-field tap.
                let source_text = controller.borrow().text();
                source_offset_at_global(owner.as_ref()?, &anchor, global, obscuring, &source_text)
            }
        };

        let down = {
            let resolve = resolve.clone();
            let controller = Rc::clone(&controller);
            let focus_node = Rc::clone(&focus_node);
            let drag_anchor = Rc::clone(&drag_anchor);
            move |dispatch: PointerDispatch<'_>| {
                if !enabled {
                    return;
                }
                // Focus first: a tap on an unfocused field must both focus it
                // and place the caret, and the caret would otherwise be set on
                // a field that then rebuilds without it.
                focus_node.request_focus();
                let Some(offset) = resolve(dispatch.global.position()) else {
                    return;
                };
                controller.borrow().set_caret_byte_offset(offset);
                drag_anchor.set(Some(offset));
            }
        };

        let moved = {
            let resolve = resolve.clone();
            let controller = Rc::clone(&controller);
            let drag_anchor = Rc::clone(&drag_anchor);
            move |dispatch: PointerDispatch<'_>| {
                let Some(from) = drag_anchor.get() else {
                    return;
                };
                let Some(to) = resolve(dispatch.global.position()) else {
                    return;
                };
                // The anchor stays where the drag began; the caret follows the
                // pointer, including backwards. `set_selection` is a no-op
                // when neither moved, which a move stream reports constantly.
                controller.borrow().set_selection(from, to);
            }
        };

        // Up and cancel do the same thing, and cancel is not optional: it is
        // documented as "abandon any in-flight tracking", and a drag anchor
        // left set after one makes the NEXT move — which may belong to another
        // gesture entirely — extend a selection the user abandoned.
        let release = {
            let drag_anchor = Rc::clone(&drag_anchor);
            move |_: PointerDispatch<'_>| drag_anchor.set(None)
        };
        let cancel = {
            let drag_anchor = Rc::clone(&drag_anchor);
            move |_: PointerDispatch<'_>| drag_anchor.set(None)
        };

        field
            .on_pointer_down(down)
            .on_pointer_move(moved)
            .on_pointer_up(release)
            .on_pointer_cancel(cancel)
    }

    /// Wrap `child` (the pointer-handled field this method's caller just
    /// built) in a [`crate::interaction::GestureDetector`] that extends the
    /// selection to the enclosing word on double-tap.
    ///
    /// `GestureDetector` sits OUTSIDE [`crate::interaction::Listener`], not
    /// the other way round: `Listener` never enters the gesture arena
    /// (its own doc explains why), so it keeps placing the caret on every
    /// tap — including the second one of a double-tap — with nothing
    /// competing for that contact. `GestureDetector`'s
    /// `DoubleTapGestureRecognizer` independently watches the SAME pointer
    /// stream and, once it confirms two taps, widens that already-placed
    /// caret into a word selection. Reimplementing double-tap timing/slop
    /// detection by hand inside `Listener`'s handlers instead of composing
    /// the real recognizer would duplicate
    /// `flui_interaction::DoubleTapGestureRecognizer` rather than reuse
    /// it — see this crate's `ARCHITECTURE.md` Mapping decision for this
    /// composition.
    ///
    /// `on_double_tap_down`, not `on_double_tap`: the word selection should
    /// land as soon as the second tap is confirmed, the same instant
    /// `Listener`'s own `down` handler already placed the caret there —
    /// waiting for the second contact to also lift (`on_double_tap`) would
    /// put the caret and the word-select visibly out of sync for the
    /// gesture's duration.
    ///
    /// `drag_anchor` (the SAME cell `install_pointer_handlers` writes) is
    /// cleared here after the word selection lands. `Listener`'s own `down`
    /// handler already ran for this same contact (both layers see every
    /// pointer event — see this method's `GestureDetector`-vs-`Listener`
    /// doc above) and set the anchor to place the caret, and a touch
    /// contact is essentially never perfectly still: the next `move`, still
    /// on the same still-down second tap, would otherwise read that anchor
    /// and call `set_selection(anchor, moved_to)` — collapsing the word
    /// selection this method just made back down to a near-zero-byte range
    /// anchored at the tap point. Clearing it makes that move a no-op
    /// (`install_pointer_handlers`'s `moved` returns early with no anchor),
    /// exactly like a move that arrived after a cancel.
    fn wrap_double_tap_word_select(
        &self,
        child: impl IntoView,
        view: &EditableText,
        drag_anchor: Rc<Cell<Option<usize>>>,
    ) -> impl IntoView {
        let obscuring = view.obscure_text.then_some(view.obscuring_character);
        let owner = self.pipeline_owner.clone();
        let anchor = self.inner_anchor.clone();
        let controller = Rc::clone(&self.controller);

        let mut detector =
            crate::interaction::GestureDetector::new().behavior(HitTestBehavior::Opaque);
        // Attach the callback ONLY while enabled, rather than always
        // attaching it and returning early inside — `on_double_tap_down`
        // being set at all is what makes `RecognizerGroup::double_tap_active`
        // join the arena for a contact (see its own doc). A disabled field
        // is documented to ignore pointer input entirely; leaving the
        // callback attached would still hold the shared arena across the
        // double-tap window for every tap on a disabled field, delaying an
        // ancestor's own tap and letting this no-op recognizer compete to
        // win a contact it does nothing with.
        if view.enabled {
            detector = detector.on_double_tap_down(move |details| {
                let Some(owner) = owner.as_ref() else {
                    return;
                };
                let source_text = controller.borrow().text();
                let Some(range) = source_word_range_at_global(
                    owner,
                    &anchor,
                    details.global_position,
                    obscuring,
                    &source_text,
                ) else {
                    return;
                };
                controller.borrow().set_selection(range.start, range.end);
                drag_anchor.set(None);
            });
        }
        detector.child(child)
    }

    fn manager(&self) -> &Rc<FocusManager> {
        self.focus_manager.as_ref().expect(
            "BUG: EditableText lifecycle used before init_state installed its focus manager",
        )
    }

    /// The key handler for `node`: [`build_key_handler`], reporting each
    /// edit it makes through `on_changed`.
    fn key_handler(&self, node: &Rc<FocusNode>) -> KeyEventHandler {
        let handler = build_key_handler(
            Rc::clone(&self.controller),
            Rc::clone(node),
            Rc::clone(&self.on_submitted),
        );
        let edits = EditObserver {
            controller: Rc::clone(&self.controller),
            on_changed: Rc::clone(&self.on_changed),
        };
        Rc::new(move |event| {
            // Enter only submits; a controller change there is the submit
            // callback's own programmatic edit, not the user's.
            if matches!(event.key, Key::Named(NamedKey::Enter)) {
                return handler(event);
            }
            edits.around(|| handler(event))
        })
    }

    /// This field's clipboard actions layered over the `Actions` chain at
    /// its position, recorded on its focus node — where a `Shortcuts` looks
    /// an intent up while this field holds the primary focus. Depends on the
    /// enclosing chain, so a changed one is recorded again; the same record
    /// `Focus` keeps (ADR-0079).
    fn record_action_chain(&mut self, ctx: &dyn BuildContext) {
        let enclosing = ctx.depend_on::<ActionChainProvider, _>(|provider| provider.data().clone());
        let unchanged = match (&enclosing, &self.enclosing_action_chain) {
            (Some(new), Some(held)) => Rc::ptr_eq(new, held),
            (None, None) => true,
            _ => false,
        };
        if unchanged
            && self
                .action_chain_registration
                .as_ref()
                .is_some_and(FocusNodeRegistration::is_current)
        {
            return;
        }
        let action = ClipboardTextAction {
            controller: Rc::clone(&self.controller),
            focus_node: Rc::clone(&self.observed_focus_node),
            obscure: Rc::clone(&self.obscure),
            clipboard: self.clipboard.clone(),
            edits: EditObserver {
                controller: Rc::clone(&self.controller),
                on_changed: Rc::clone(&self.on_changed),
            },
        };
        let chain = layered_chain(
            enclosing.clone(),
            &[
                erased_action::<CopySelectionTextIntent>(action.clone()),
                erased_action::<PasteTextIntent>(action),
            ],
        );
        // Register the new record before the old token drops: the old one is
        // no longer current then, so dropping it leaves the new one in place.
        self.action_chain_registration =
            Some(self.focus_node.register_context(as_node_context(&chain)));
        self.action_chain = Some(chain);
        self.enclosing_action_chain = enclosing;
    }
}

/// Reports a user edit through [`EditableText::on_changed`]: compares the
/// text before and after the edit, and calls the callback with no borrow
/// held when they differ.
#[derive(Clone)]
struct EditObserver {
    controller: Rc<RefCell<TextEditingController>>,
    on_changed: Rc<RefCell<Option<TextChanged>>>,
}

impl EditObserver {
    fn around<R>(&self, edit: impl FnOnce() -> R) -> R {
        if self.on_changed.borrow().is_none() {
            return edit();
        }
        let before = self.controller.borrow().text();
        let result = edit();
        self.report_if_changed(&before);
        result
    }

    fn report_if_changed(&self, before: &str) {
        let after = self.controller.borrow().text();
        if after == before {
            return;
        }
        let callback = self.on_changed.borrow().clone();
        if let Some(callback) = callback {
            callback(&after);
        }
    }
}

/// Copy, cut and paste for one mounted field — see [`EditableText`]'s
/// `# Clipboard` section for when each is enabled.
///
/// Reads the controller, node and obscuring flag through the state's shared
/// cells at key time, so a swapped controller or node is the one acted on.
#[derive(Clone)]
struct ClipboardTextAction {
    controller: Rc<RefCell<TextEditingController>>,
    /// The field's current node; its `can_request_focus` tracks `enabled`.
    focus_node: Rc<RefCell<Rc<FocusNode>>>,
    obscure: Rc<Cell<bool>>,
    clipboard: Option<ClipboardHandle>,
    /// Cut and paste are user edits.
    edits: EditObserver,
}

impl ClipboardTextAction {
    fn enabled(&self) -> bool {
        self.focus_node.borrow().can_request_focus()
    }

    /// The controller, cloned out of its cell so no borrow is held while the
    /// clipboard or the controller's listeners run.
    fn controller(&self) -> TextEditingController {
        self.controller.borrow().clone()
    }
}

impl Action<CopySelectionTextIntent> for ClipboardTextAction {
    fn is_enabled(&self, intent: &CopySelectionTextIntent) -> bool {
        self.clipboard.is_some()
            && !self.obscure.get()
            && (*intent == CopySelectionTextIntent::Copy || self.enabled())
            && self.controller.borrow().has_selection()
    }

    fn invoke(&self, intent: &CopySelectionTextIntent) -> ActionOutcome {
        let Some(clipboard) = &self.clipboard else {
            return ActionOutcome::NotPerformed;
        };
        let controller = self.controller();
        let selected = controller.selected_text();
        if selected.is_empty() {
            return ActionOutcome::NotPerformed;
        }
        clipboard.write_text(selected);
        if *intent == CopySelectionTextIntent::Cut {
            // Replacing the selection with nothing deletes it and leaves the
            // caret at its start.
            self.edits.around(|| controller.insert_str(""));
        }
        ActionOutcome::Performed
    }
}

impl Action<PasteTextIntent> for ClipboardTextAction {
    fn is_enabled(&self, _intent: &PasteTextIntent) -> bool {
        self.clipboard.is_some() && self.enabled() && !self.controller.borrow().is_composing()
    }

    fn invoke(&self, _intent: &PasteTextIntent) -> ActionOutcome {
        let Some(clipboard) = &self.clipboard else {
            return ActionOutcome::NotPerformed;
        };
        let controller = self.controller();
        let edits = self.edits.clone();
        // May complete before `read_text` returns: nothing is borrowed here.
        clipboard.read_text(move |text| {
            let Some(text) = text else {
                return;
            };
            // A single-line field: line breaks are dropped, `\r` included,
            // so a Windows `\r\n` leaves nothing behind.
            let line: String = text
                .chars()
                .filter(|&character| character != '\n' && character != '\r')
                .collect();
            if !line.is_empty() {
                edits.around(|| controller.insert_str(&line));
            }
        });
        ActionOutcome::Performed
    }
}

impl ViewState<EditableText> for EditableTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.focus_manager = Some(ctx.focus_manager());

        // Resolve the focus parent first, but attach only after every
        // focus/IME listener is installed below. An external node may carry a
        // focus request queued before mount; `attach_node` fulfills it
        // synchronously, and no listener may miss that edge.
        let parent = crate::__private::enclosing_focus_parent(ctx);
        self.parent = Some(Rc::clone(&parent));
        let (rect_provider, rect_provider_registration) =
            crate::__private::install_rect_provider(&self.focus_node, &self.anchor, ctx);
        self.rect_provider = Some(rect_provider);
        self.rect_provider_registration = Some(rect_provider_registration);

        // 2. Install the key handler on the node itself. It only fires when
        //    this node is on the primary-focus dispatch path. Gated on
        //    `can_request_focus` (kept in sync with `enabled` in
        //    `did_update_view`) so a stray dispatch to an already-focused
        //    field that has since been disabled is a no-op.
        self.key_handler_registration = Some(
            self.focus_node
                .register_on_key_event(self.key_handler(&self.focus_node)),
        );

        // 2b. The clipboard actions, recorded on the node beside the key
        //     handler: a clipboard chord the handler leaves unconsumed
        //     reaches the `Shortcuts` above, which resolves it here.
        self.clipboard = ctx.clipboard_handle();
        self.record_action_chain(ctx);

        // 3. Forward controller change events into the rebuild notifier so the
        //    inner AnimatedBuilder rebuilds on every keystroke.
        let rebuild_notifier_for_text = self.rebuild_notifier.clone();
        let controller_listener_id = self.controller.borrow().add_listener(Arc::new(move || {
            rebuild_notifier_for_text.notify_listeners();
        }));
        self.controller_listener_id = Some(controller_listener_id);

        // 4. Forward FocusManager focus-change events into the rebuild notifier
        //    so the caret appears / disappears immediately when this field
        //    gains or loses focus. Removed by id in `dispose`.
        let rebuild_notifier_for_focus = self.rebuild_notifier.clone();
        let focus_node_for_rebuild = Rc::clone(&self.observed_focus_node);
        self.focus_listener_id = Some(self.manager().add_listener(Rc::new(
            move |previous, current| {
                let focus_node = focus_node_for_rebuild.borrow();
                // Only rebuild when this node's focus state actually changed.
                let was_focused = previous
                    .as_ref()
                    .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
                let now_focused = current
                    .as_ref()
                    .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
                if was_focused != now_focused {
                    rebuild_notifier_for_focus.notify_listeners();
                }
            },
        )));

        // 5. Attach/detach the IME client on this field's own focus
        //    transitions. `text_input_handle()` is a frame capability —
        //    acquired here, in `init_state`, never in `build` (see
        //    `LifecycleContext::text_input_handle`'s doc) — and stored so the
        //    focus-listener closure below (which cannot borrow `&mut self`)
        //    and `dispose` can both reach it. `local_post_frame_handle()` and
        //    `pipeline_owner()` are acquired alongside it for the same
        //    reason — the IME cursor-area loop (ADR-0030) they drive is
        //    started/stopped by that same closure.
        self.ime_handle = ctx.text_input_handle();
        self.local_post_frame_handle = ctx.local_post_frame_handle();
        let ime_handle_for_focus = self.ime_handle.clone();
        let post_frame_handle_for_focus = self.local_post_frame_handle.clone();
        let pipeline_owner_for_focus = ctx.pipeline_owner();
        self.pipeline_owner.clone_from(&pipeline_owner_for_focus);
        let inner_anchor_for_focus = self.inner_anchor.clone();
        let controller_for_ime = Rc::clone(&self.controller);
        let edits_for_ime = EditObserver {
            controller: Rc::clone(&self.controller),
            on_changed: Rc::clone(&self.on_changed),
        };
        let ime_token_for_focus = Rc::clone(&self.ime_token);
        let cursor_area_alive_for_focus = Rc::clone(&self.cursor_area_alive);
        let ime_focus_transition: ImeFocusTransition = Rc::new(move |now_focused| {
            let Some(handle) = &ime_handle_for_focus else {
                return;
            };
            if now_focused {
                // Fresh per-attach state (ADR-0030): `last_sent` resets
                // so a brand-new IME session always gets its first rect
                // even at an unchanged caret position, and `alive` is a
                // NEW flag so a stale queued firing from a previous
                // attach (see the field's `cursor_area_alive` doc) can
                // never resurrect this session or run alongside it.
                let last_sent: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
                let alive = Rc::new(Cell::new(true));
                let _prev = cursor_area_alive_for_focus
                    .borrow_mut()
                    .replace(Rc::clone(&alive));

                let controller_for_callback = Rc::clone(&controller_for_ime);
                let edits = edits_for_ime.clone();
                let last_sent_for_ime_event = Rc::clone(&last_sent);
                let token = match handle.attach(Rc::new(move |event: &ImeEvent| {
                    edits.around(|| apply_ime_event(&controller_for_callback.borrow(), event));
                    // The backend may have restarted the IME session
                    // (`Enabled` re-fires on that restart) — clearing
                    // `last_sent` guarantees the new session gets a
                    // fresh rect instead of the dedupe cache silently
                    // suppressing it.
                    if matches!(event, ImeEvent::Enabled) {
                        last_sent_for_ime_event.set(None);
                    }
                })) {
                    Ok(token) => token,
                    Err(error) => {
                        alive.set(false);
                        let _prev = cursor_area_alive_for_focus.borrow_mut().take();
                        tracing::warn!(?error, "IME client could not attach to its presentation");
                        return;
                    }
                };
                *ime_token_for_focus.borrow_mut() = Some(token);

                if let Some(post_frame) = post_frame_handle_for_focus.clone() {
                    CursorAreaLoop {
                        post_frame,
                        pipeline_owner: pipeline_owner_for_focus.clone(),
                        inner_anchor: inner_anchor_for_focus.clone(),
                        text_input: handle.clone(),
                        alive,
                        last_sent,
                    }
                    .schedule();
                } else {
                    tracing::warn!(
                        "IME cursor-area tracking not started: no post-frame handle \
                         installed (the platform candidate window will not follow \
                         the caret)"
                    );
                }
            } else {
                if let Some(token) = ime_token_for_focus.borrow_mut().take()
                    && let Err(error) = handle.detach(token)
                {
                    tracing::trace!(
                        ?error,
                        "IME detach reached a presentation that was already closing"
                    );
                }
                let alive = cursor_area_alive_for_focus.borrow_mut().take();
                if let Some(alive) = alive {
                    alive.set(false);
                }
            }
        });
        self.ime_focus_transition = Some(Rc::clone(&ime_focus_transition));
        let focus_node_for_ime = Rc::clone(&self.observed_focus_node);
        self.ime_focus_listener_id = Some(self.manager().add_listener(Rc::new(
            move |previous, current| {
                let focus_node = focus_node_for_ime.borrow();
                let was_focused = previous
                    .as_ref()
                    .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
                let now_focused = current
                    .as_ref()
                    .is_some_and(|node| Rc::ptr_eq(node, &focus_node));
                if was_focused == now_focused {
                    return;
                }
                ime_focus_transition(now_focused);
            },
        )));

        // Attach last. Besides normal pre-mount `request_focus`, this also
        // covers a route scope's pending first-focus intent: either path may
        // synchronously focus this node during attachment, after both the
        // caret rebuild and IME listeners are ready.
        self.focus_attachment = Some(
            parent
                .attach_node(&self.focus_node)
                .expect("BUG: EditableText could not attach its explicit focus node"),
        );
    }

    fn did_update_view(&mut self, _old_view: &EditableText, new_view: &EditableText) {
        // Cheap and unconditional: a closure has no identity worth comparing,
        // so every rebuild just installs whatever `on_submitted` the latest
        // view carries — read through this cell at dispatch time by the key
        // handler installed once in `init_state`.
        self.on_submitted
            .borrow_mut()
            .clone_from(&new_view.on_submitted);
        self.on_changed
            .borrow_mut()
            .clone_from(&new_view.on_changed);
        self.obscure.set(new_view.obscure_text);

        // A parent rebuilding with a DIFFERENT controller retargets the
        // mounted field onto it, rather than the field silently going on
        // driving the one it was born with. The reference does the same in
        // `didUpdateWidget`: drop the listener from the old, add it to the
        // new, and resynchronise.
        //
        // Everything that reaches the controller — the key handler, the IME
        // attach callback, `build` — reads through `self.controller`'s cell at
        // use time, so writing the cell retargets all of them at once. Only
        // the change LISTENER has to move by hand, because it is registered on
        // the controller rather than read from it.
        if !self
            .controller
            .borrow()
            .is_same_controller(&new_view.controller)
        {
            if let Some(id) = self.controller_listener_id.take() {
                self.controller.borrow().remove_listener(id);
            }
            let _prev = std::mem::replace(
                &mut *self.controller.borrow_mut(),
                new_view.controller.clone(),
            );
            let rebuild_notifier_for_text = self.rebuild_notifier.clone();
            self.controller_listener_id =
                Some(self.controller.borrow().add_listener(Arc::new(move || {
                    rebuild_notifier_for_text.notify_listeners();
                })));
            // The visible text is the replacement's now, and nothing else
            // will say so: the old controller's notifications are gone and the
            // new one has not changed since it was handed over.
            self.rebuild_notifier.notify_listeners();
        }

        if !Rc::ptr_eq(&self.focus_node, &new_view.focus_node) {
            let replacement = Rc::clone(&new_view.focus_node);
            replacement.set_can_request_focus(new_view.enabled);
            let replacement_key_handler_registration =
                replacement.register_on_key_event(self.key_handler(&replacement));
            let replacement_rect_provider_registration = self
                .rect_provider
                .as_ref()
                .map(|provider| replacement.register_rect_provider(Rc::clone(provider)));
            let replacement_action_chain_registration = self
                .action_chain
                .as_ref()
                .map(|chain| replacement.register_context(as_node_context(chain)));

            // Keep observing the old node while the transaction reports an
            // exact-primary loss, so the current IME session is detached.
            // A queued request on the replacement can be fulfilled within
            // the same transaction; reconcile that gain explicitly after
            // switching the observed identity.
            let attachment = self
                .focus_attachment
                .take()
                .expect("BUG: a mounted EditableText must retain its FocusAttachment");
            let replacement_attachment = attachment
                .replace_node(&replacement)
                .expect("BUG: EditableText could not atomically replace its focus node");

            self.key_handler_registration.take();
            self.rect_provider_registration.take();
            self.action_chain_registration.take();
            self.focus_node = replacement;
            self.key_handler_registration = Some(replacement_key_handler_registration);
            self.rect_provider_registration = replacement_rect_provider_registration;
            self.action_chain_registration = replacement_action_chain_registration;
            let _prev = std::mem::replace(
                &mut *self.observed_focus_node.borrow_mut(),
                Rc::clone(&self.focus_node),
            );
            self.focus_attachment = Some(replacement_attachment);

            if self.focus_node.has_primary_focus() {
                self.rebuild_notifier.notify_listeners();
                if let Some(transition) = &self.ime_focus_transition {
                    transition(true);
                }
            }
        }

        // A field disabled while focused must not keep the caret and keyboard
        // input — mirrors Flutter's `TextField`/`EditableText` unfocusing when
        // `enabled` flips false mid-focus. `FocusNode::set_can_request_focus`
        // itself releases primary focus on a true-to-false change (Flutter's
        // `FocusNode.canRequestFocus` setter semantics), so this call alone
        // covers the unfocus — no separate `has_primary_focus` check needed.
        //
        self.focus_node.set_can_request_focus(new_view.enabled);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let parent = crate::__private::enclosing_focus_parent(ctx);
        if self
            .parent
            .as_ref()
            .is_none_or(|held| !Rc::ptr_eq(held, &parent))
        {
            self.focus_attachment
                .as_ref()
                .expect("BUG: a mounted EditableText must retain its FocusAttachment")
                .reparent(&parent)
                .expect("BUG: EditableText could not reparent within its presentation");
            self.parent = Some(parent);
        }
        self.record_action_chain(ctx);
    }

    fn build(&self, view: &EditableText, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = Rc::clone(&self.controller);
        let focus_node = Rc::clone(&self.focus_node);
        let enabled = view.enabled;
        let appearance = FieldAppearance {
            caret_height: view.caret_height,
            caret_color: view.caret_color,
            selection_color: view.selection_color,
            text_style: view.text_style.clone(),
            obscure_text: view.obscure_text,
            obscuring_character: view.obscuring_character,
        };
        let inner_anchor = self.inner_anchor.clone();

        // OUTSIDE the inner `AnchoredBox`, not inside it: the IME cursor-area
        // loop reaches the editable by taking `inner_anchor`'s FIRST child,
        // so anything inserted between the two would break that walk.
        let field = crate::interaction::Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(crate::__private::AnchoredBox::new(
                self.anchor.clone(),
                AnimatedBuilder::new(Arc::new(self.rebuild_notifier.clone()), move || {
                    build_field_view(
                        &controller.borrow(),
                        &focus_node,
                        enabled,
                        &appearance,
                        inner_anchor.clone(),
                    )
                }),
            ));

        // Shared with `wrap_double_tap_word_select` below: a double-tap that
        // lands on this same contact must be able to silence the drag this
        // anchor otherwise starts for it — see that method's doc.
        let drag_anchor: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));
        let field = self.install_pointer_handlers(field, view, Rc::clone(&drag_anchor));
        self.wrap_double_tap_word_select(field, view, drag_anchor)
    }

    fn dispose(&mut self) {
        let owns_attachment = self
            .focus_attachment
            .as_ref()
            .is_some_and(FocusAttachment::is_attached);
        if owns_attachment {
            self.rect_provider_registration.take();
            self.key_handler_registration.take();
            self.action_chain_registration.take();
        } else {
            if let Some(registration) = self.rect_provider_registration.take() {
                registration.relinquish();
            }
            if let Some(registration) = self.key_handler_registration.take() {
                registration.relinquish();
            }
            if let Some(registration) = self.action_chain_registration.take() {
                registration.relinquish();
            }
        }
        self.action_chain = None;
        self.enclosing_action_chain = None;
        // Remove the focus-change listener we registered in init_state.
        if let Some(id) = self.focus_listener_id.take() {
            self.manager().remove_listener(id);
        }
        // Remove the IME focus-change listener we registered in init_state.
        if let Some(id) = self.ime_focus_listener_id.take() {
            self.manager().remove_listener(id);
        }
        self.ime_focus_transition = None;

        // Detach the IME client if this field still has one attached — the
        // ADR-0030 detach-on-dispose contract. A field unmounted while
        // focused is not guaranteed a focus-loss notification when its
        // attachment is detached below, so this is the one path that unconditionally
        // closes the IME session on unmount. Harmless no-op if the field
        // already blurred (and so already detached) before unmounting.
        if let Some(token) = self.ime_token.borrow_mut().take()
            && let Some(handle) = &self.ime_handle
            && let Err(error) = handle.detach(token)
        {
            tracing::trace!(
                ?error,
                "IME dispose detach reached a presentation that was already closing"
            );
        }

        // Stop the IME cursor-area loop (ADR-0030) if one is running — the
        // same unconditional-on-unmount contract as the IME token detach
        // just above, and independent of it: a field unmounted while
        // focused is not guaranteed a blur notification, so this is the one
        // path that always flips the current attach's alive flag false,
        // whether or not the field ever blurred first.
        let alive = self.cursor_area_alive.borrow_mut().take();
        if let Some(alive) = alive {
            alive.set(false);
        }

        // Detach through the generation-checked lifecycle authority.
        if let Some(attachment) = self.focus_attachment.take() {
            let _ = attachment.detach();
        }
        self.parent = None;

        // Remove the controller listener we registered in init_state.
        if let Some(id) = self.controller_listener_id.take() {
            self.controller.borrow().remove_listener(id);
        }
        self.focus_manager = None;

        // Deliberately NOT disposed here: `self.rebuild_notifier` is also
        // held by the `AnimatedBuilder` this state's own `build()` output
        // wraps around (`Arc::new(self.rebuild_notifier.clone())`), and that
        // child element's own `on_unmount` calls `remove_listener` on its
        // clone as part of the SAME unmount sweep. `ViewState::dispose`
        // (this method) runs before that child unmounts.
        //
        // `ChangeNotifier::remove_listener` is a safe no-op after dispose
        // (Flutter parity — see `Listenable::remove_listener`'s doc), so
        // calling `dispose()` here first would no longer panic against the
        // child's later `remove_listener` call. It is still left undone: an
        // explicit `dispose()` would mark the shared notifier disposed while
        // the child's subscription is still live, which is unnecessary
        // churn for no behavioral gain here — letting the notifier's `Arc`
        // refcount reach zero naturally (once every clone — this state's
        // and the child element's — is gone) is sufficient, since nothing
        // reads its disposed-flag on this path.
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Route one delivered [`ImeEvent`] to the matching
/// [`TextEditingController`] composing operation — the IME client callback
/// installed in `EditableTextState::init_state`.
///
/// `Enabled` is purely informational (nothing on the controller models
/// "composition is available but not yet started"); the other three variants
/// map 1:1 onto the controller's composing methods, which carry the
/// replace-vs-insert and clamping rules — see each method's doc.
fn apply_ime_event(controller: &TextEditingController, event: &ImeEvent) {
    match event {
        ImeEvent::Preedit { text, cursor } => controller.set_composing_text(text, *cursor),
        ImeEvent::Commit(text) => controller.commit_text(text),
        ImeEvent::Disabled => controller.clear_composing(),
        // Covers `Enabled` (purely informational, see this fn's doc) and any
        // future variant a winit upgrade adds — `ImeEvent` is
        // `#[non_exhaustive]`, so an unhandled new variant is a no-op here
        // until this match is revisited, never a broken build for an
        // unrelated crate bump.
        _ => {}
    }
}

/// The self-rescheduling IME cursor-area tracking loop (ADR-0030).
///
/// One instance is created per IME attach (focus gain). Each firing reads the
/// caret's current global rect and forwards it through
/// [`TextInputHandle::set_cursor_area`] when it changed, then reschedules
/// itself for the next completed frame — Flutter's own
/// `_schedulePeriodicPostFrameCallbacks` cadence (`editable_text.dart`, tag
/// `3.44.0`), dormant whenever no frame runs. `Clone` because
/// [`flui_scheduler::LocalPostFrameHandle::schedule_local`] takes an `FnOnce`, so the only way to
/// make it self-rescheduling without boxing a trait object is for each
/// firing to consume `self` and, if still alive, construct the next firing's
/// closure from a fresh clone of the same capture.
#[derive(Clone)]
struct CursorAreaLoop {
    post_frame: flui_scheduler::LocalPostFrameHandle,
    pipeline_owner: Option<PipelineCell>,
    /// The `EditableTextRenderView`'s own inner anchor (ADR-0030) — see
    /// `EditableTextState::inner_anchor`'s doc.
    inner_anchor: flui_objects::SubtreeAnchor,
    text_input: TextInputHandle,
    /// Per-attach liveness flag — see `EditableTextState::cursor_area_alive`'s
    /// doc for why it is fresh per attach. Checked at the START of every
    /// firing so a callback already queued when the attach ended dies
    /// silently instead of resurrecting a stale loop or running alongside a
    /// newer one.
    alive: Rc<Cell<bool>>,
    /// The last rect actually sent. Fresh per attach (never shared across
    /// attaches, see `EditableTextState::init_state`'s IME focus listener) —
    /// a brand-new IME session must always get the first rect, even at an
    /// unchanged caret position.
    last_sent: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl CursorAreaLoop {
    /// Register the next firing. Every `schedule_local` failure is warned,
    /// never silent: a loop that stops rescheduling without a diagnostic is
    /// a candidate window stuck at (0, 0) with no signal anything is wrong.
    fn schedule(self) {
        let post_frame = self.post_frame.clone();
        if let Err(error) = post_frame.schedule_local(move |_timing| self.fire()) {
            tracing::warn!(
                ?error,
                "IME cursor-area tick could not be (re)scheduled; the platform \
                 candidate window will stop following the caret"
            );
        }
    }

    fn fire(self) {
        if !self.alive.get() {
            return;
        }
        // A `None` read is a transient miss (the anchored subtree unmounted
        // mid-rebuild, or a transform is momentarily unavailable) — skip
        // this firing's send but keep the loop alive. Only `alive == false`
        // ever stops rescheduling.
        if let Some(rect) = self.global_caret_rect()
            && Some(rect) != self.last_sent.get()
        {
            if let Err(error) = self.text_input.set_cursor_area(rect) {
                self.alive.set(false);
                tracing::warn!(
                    ?error,
                    "IME cursor-area tracking stopped because its presentation is unavailable"
                );
                return;
            }
            self.last_sent.set(Some(rect));
        }
        self.schedule();
    }

    /// The IME candidate window's current target rect in window-root-space
    /// logical pixels: `inner_anchor`'s committed transform to the render
    /// root, applied to the anchored `RenderEditable` child's composing
    /// region rect when one is active, falling back to its collapsed caret
    /// rect otherwise — Flutter's own `_updateComposingRectIfNeeded` order
    /// (`editable_text.dart`, tag `3.44.0`: prefer the composing rect,
    /// fall back to the caret rect when none is available). ADR-0030
    /// upgrades this loop from the caret-rect-only reduction ADR-0030
    /// originally landed.
    fn global_caret_rect(&self) -> Option<Bounds<Pixels>> {
        let anchor_id = self.inner_anchor.get()?;
        let owner = self.pipeline_owner.as_ref()?;
        owner.with(|owner| {
            let root_id = owner.root_id()?;
            let tree = owner.render_tree();
            let editable_id = *tree.children(anchor_id).first()?;
            let editable = tree
                .get(editable_id)?
                .as_box()?
                .render_object()
                .downcast_ref::<RenderEditable>()?; // ADR-0030 IME cursor-area loop reaches the one concrete render object type it knows sits under `inner_anchor` (an `EditableTextRenderView`'s `RenderEditable`) through the storage layer's `&dyn RenderObject<BoxProtocol>` erasure.
            let local_rect = editable
                .rect_for_composing_range()
                .unwrap_or_else(|| editable.caret_local_rect());
            let transform = owner.transform_to(anchor_id, root_id)?;
            Some(bounds_from_rect(transform.transform_rect(&local_rect)))
        })
    }
}

/// `Rect` (min/max corners) to `Bounds` (origin/size) — `PlatformTextInput::
/// set_ime_cursor_area`'s parameter convention, matching `PlatformWindow::
/// bounds`.
fn bounds_from_rect(rect: Rect) -> Bounds<Pixels> {
    Bounds::new(
        Point::new(rect.min.x, rect.min.y),
        flui_types::Size::new(rect.width(), rect.height()),
    )
}

/// Whether these modifiers make a key a *command* rather than *text*.
///
/// Meta (Cmd) always is. Control is, except in combination with Alt: Win32
/// reports AltGr as Control+Alt, and since no FLUI backend sets
/// `Modifiers::ALT_GRAPH`, that combination is the only signal an AltGr layout
/// gives us — treating it as a command would make `@`, `€` and the rest
/// untypeable on most non-US keyboards.
///
/// The cost of that carve-out, stated rather than discovered: a genuine
/// Ctrl+Alt+X shortcut is delivered as text. Browsers and Flutter make the
/// same trade for the same reason.
#[inline]
fn is_command_chord(modifiers: Modifiers) -> bool {
    modifiers.contains(Modifiers::META)
        || (modifiers.contains(Modifiers::CONTROL) && !modifiers.contains(Modifiers::ALT))
}

/// The modifier that requests WORD-granularity caret/selection/delete
/// movement on `platform` — Flutter's `DefaultTextEditingShortcuts` binds
/// a different modifier per platform, not the same one everywhere:
///
/// | Platform | Word-jump modifier | Why not the other one too |
/// |---|---|---|
/// | macOS, iOS | Alt (Option) | Ctrl is unbound for word-jump on macOS |
/// | Windows, Linux, Android, Fuchsia, Unknown | Control | Alt+Left/Right/Backspace are reserved by Flutter for LINE-boundary intents this crate does not implement yet (see [`is_word_jump_modifier`]'s `# DEFERRED`); treating Alt as word-jump here too would silently claim that reservation early |
///
/// A pure function of `platform`, table-tested against every
/// [`TargetPlatform`] variant so the mapping itself is verified
/// regardless of which host actually runs the test suite (this crate's
/// tests run on `ubuntu-latest` in CI, which alone would never exercise
/// the macOS/iOS arm).
#[inline]
fn word_jump_modifier(platform: TargetPlatform) -> Modifiers {
    match platform {
        TargetPlatform::MacOS | TargetPlatform::iOS => Modifiers::ALT,
        _ => Modifiers::CONTROL,
    }
}

/// Whether these modifiers request WORD-granularity movement on
/// `platform` — see [`word_jump_modifier`]'s doc for the table.
///
/// # Platform source is a known limitation
///
/// Every caller in this file resolves `platform` from
/// [`TargetPlatform::current()`] (compile-time `cfg(target_os)`) once,
/// in [`build_key_handler`] — a single injection point rather than each
/// call site re-resolving it, so a future runtime override has one place
/// to change. That source is already known wrong for at least one real
/// target: `wasm32` matches none of `current()`'s `cfg(target_os)` arms
/// and falls to `Unknown` → Control, but a macOS browser tab needs Alt
/// too (native Ctrl+Arrow is the OS's own Spaces-switch shortcut there,
/// so Control would never even reach this handler). Web and embedded
/// targets need `TargetPlatform` resolved at RUNTIME instead — the same
/// gap `GestureSettings::native`'s own doc already flags for the
/// analogous gesture-settings case
/// (`flui-interaction/src/settings.rs`). Tracked, not fixed here — see
/// `flui-widgets/ARCHITECTURE.md`'s Mapping decision #19.
///
/// # DEFERRED
///
/// Alt+Left/Right/Backspace as a line-boundary intent on non-Apple
/// platforms (Flutter: `ExtendSelectionToLineBreakIntent`/
/// `DeleteToLineBreakIntent`). Left unhandled (falls through to a plain
/// per-character move) rather than silently reinterpreted as word-jump,
/// so a later line-boundary implementation is not fighting an existing,
/// wrong meaning for the chord.
///
/// Arrow and Backspace/Delete keys never produce a character, so this has
/// no AltGr carve-out to make (contrast [`is_command_chord`], which does).
///
/// # Exact chord, not just "the modifier is held"
///
/// Requires EXACTLY the platform's word-jump modifier among
/// {Ctrl, Alt, Meta} — Shift composes independently (it selects
/// move-vs-extend, handled by the caller) and is not part of this check,
/// but any OTHER command modifier held at the same time disqualifies the
/// chord. Flutter's `SingleActivator` matches the complete modifier
/// state the same way (apart from Shift), and without this a chord that
/// is not meant to be word-jump at all — Ctrl+Alt+Right on Linux,
/// Option+Command+Right on macOS — would wrongly take the word-jump path
/// just because it happens to also hold the required key. Lock-state
/// flags (`CAPS_LOCK`/`NUM_LOCK`/`SCROLL_LOCK`/`FN_LOCK`) and
/// `ALT_GRAPH`/`FN`/`SYMBOL` are deliberately excluded from the mask —
/// they are not "another command modifier" in the sense this guards
/// against, and treating an incidental Caps Lock as disqualifying would
/// be its own new bug.
#[inline]
fn is_word_jump_modifier(modifiers: Modifiers, platform: TargetPlatform) -> bool {
    let command_mask = Modifiers::CONTROL | Modifiers::ALT | Modifiers::META;
    (modifiers & command_mask) == word_jump_modifier(platform)
}

/// Build the key-event handler closure for `controller`.
///
/// Only `KeyState::Down` events (which cover key-repeat) are acted upon, and
/// only while `focus_node` still allows focus — kept in sync with
/// `EditableText::enabled` by `did_update_view` — so input is ignored on a
/// field disabled after it was focused.
/// The key handler reads its controller through the shared cell at DISPATCH
/// time, not at registration time.
///
/// That is what makes a controller swap take effect on a mounted field: the
/// registration installed at `init_state` outlives the swap, so a handler
/// holding a clone would keep typing into the controller the field was born
/// with.
fn build_key_handler(
    controller: Rc<RefCell<TextEditingController>>,
    focus_node: Rc<FocusNode>,
    on_submitted: Rc<RefCell<Option<SubmitCallback>>>,
) -> KeyEventHandler {
    // Resolved once, at handler-construction time, not per keystroke — the
    // one place a future runtime-resolved platform would be injected
    // instead of `TargetPlatform::current()`. See `is_word_jump_modifier`'s
    // doc for why the compile-time source itself is a known limitation.
    let platform = TargetPlatform::current();
    Rc::new(move |event| {
        let controller = controller.borrow();
        if !focus_node.can_request_focus() {
            return KeyEventResult::Ignored;
        }
        if event.state != KeyState::Down {
            return KeyEventResult::Ignored;
        }
        match &event.key {
            // A command chord is not text. Leave it unconsumed so
            // `FocusManager::dispatch_key_event`'s leaf->root walk reaches the
            // enclosing `Shortcuts`/`CallbackShortcuts` — consuming it here is
            // what makes Ctrl+S type "s" and every app shortcut dead while a
            // field is focused. The clipboard chords take this path too:
            // `DefaultFocusTraversal` binds them, and its `Shortcuts` resolves
            // them against the clipboard actions this field records on its
            // own node.
            //
            // Scoped to character insertion ON PURPOSE. The named-key arms
            // below have no ancestor to fall through to: the default bindings
            // cover no caret-movement intent, so guarding them would not route
            // Ctrl+Home somewhere better — it would make it a no-op. Widen
            // this together with the binding that would answer them.
            Key::Character(_) if is_command_chord(event.modifiers) => KeyEventResult::Ignored,
            Key::Character(character_string) => {
                // Suppression contract (`ImeEvent`'s doc): suppress
                // `Key::Character` insertion ONLY while a composition is
                // active. Winit withholds `KeyboardInput` during
                // composition and immediately after a commit, so this path
                // exists mainly for backends/tests that dispatch a
                // character key mid-preedit anyway — without the guard it
                // would double-insert alongside the IME's own commit.
                // Consumed either way: this field owns the key while
                // focused, composing or not.
                if !controller.is_composing() {
                    controller.insert_str(character_string.as_str());
                }
                KeyEventResult::Handled
            }
            // The platform's word-jump modifier + Backspace deletes the
            // WORD behind the caret rather than one character — see
            // `is_word_jump_modifier`'s doc for which modifier that is on
            // this platform.
            Key::Named(NamedKey::Backspace) => {
                if is_word_jump_modifier(event.modifiers, platform) {
                    controller.delete_word_backward();
                } else {
                    controller.backspace();
                }
                KeyEventResult::Handled
            }
            // Ctrl/Alt+Delete is the forward mirror of the Backspace arm
            // above.
            Key::Named(NamedKey::Delete) => {
                if is_word_jump_modifier(event.modifiers, platform) {
                    controller.delete_word_forward();
                } else {
                    controller.delete_forward();
                }
                KeyEventResult::Handled
            }
            // Shift is the difference between MOVING the caret and EXTENDING
            // the selection, and the two are not the same operation with a
            // flag: unmodified, an arrow collapses a selection to its edge and
            // stops there; modified, it steps the extent from wherever it is
            // and leaves the anchor. Flutter draws the same line between
            // `ExtendSelectionByCharacterIntent`'s two `collapseSelection`
            // values (`widgets/editable_text.dart:685,697`). Ctrl/Alt raises
            // the granularity from character to WORD without changing that
            // axis — the two modifiers compose independently, matching
            // Flutter's separate `ExtendSelectionByCharacterIntent`/
            // `ExtendSelectionToNextWordBoundaryIntent` pair.
            Key::Named(NamedKey::ArrowLeft) => {
                let extend = event.modifiers.contains(Modifiers::SHIFT);
                let by_word = is_word_jump_modifier(event.modifiers, platform);
                match (by_word, extend) {
                    (true, true) => controller.extend_selection_word_left(),
                    (true, false) => controller.move_caret_word_left(),
                    (false, true) => controller.extend_selection_left(),
                    (false, false) => controller.move_caret_left(),
                }
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::ArrowRight) => {
                let extend = event.modifiers.contains(Modifiers::SHIFT);
                let by_word = is_word_jump_modifier(event.modifiers, platform);
                match (by_word, extend) {
                    (true, true) => controller.extend_selection_word_right(),
                    (true, false) => controller.move_caret_word_right(),
                    (false, true) => controller.extend_selection_right(),
                    (false, false) => controller.move_caret_right(),
                }
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::Home) => {
                if event.modifiers.contains(Modifiers::SHIFT) {
                    controller.extend_selection_home();
                } else {
                    controller.move_caret_home();
                }
                KeyEventResult::Handled
            }
            Key::Named(NamedKey::End) => {
                if event.modifiers.contains(Modifiers::SHIFT) {
                    controller.extend_selection_end();
                } else {
                    controller.move_caret_end();
                }
                KeyEventResult::Handled
            }
            // Only claimed when something actually consumes it — with no
            // `on_submitted` set, Enter is left `Ignored` so an ancestor
            // can still act on it, unchanged from this field's behavior
            // before `on_submitted` existed. See `on_submitted`'s doc for
            // why a raw Enter keypress rather than an IME action-button
            // commit.
            Key::Named(NamedKey::Enter) => {
                // IME owns Enter while composing — same suppression
                // contract the `Key::Character` arm above follows: an
                // in-progress composition must not also trigger submit.
                if controller.is_composing() {
                    return KeyEventResult::Ignored;
                }
                // A command chord is not a submit, mirroring the
                // `Key::Character` arm's own command-chord guard — without
                // this, Ctrl+Enter/Cmd+Enter would be swallowed here
                // instead of reaching an ancestor `Shortcuts`.
                if is_command_chord(event.modifiers) {
                    return KeyEventResult::Ignored;
                }
                // Shift+Enter is reserved for a future multiline newline,
                // not submit — this substrate has no multiline support yet
                // (see the type doc's `# DEFERRED (v1)` list), so today
                // this is simply `Ignored`, but the reservation is
                // deliberate: a later multiline field must not find
                // Shift+Enter's meaning already claimed by submit.
                if event.modifiers.contains(Modifiers::SHIFT) {
                    return KeyEventResult::Ignored;
                }
                // Clone the callback and drop the borrow before calling
                // it — `on_submitted.borrow()` (this statement's own
                // temporary) and the outer `controller` `Ref` are both
                // live at this point, and the callback is arbitrary user
                // code that may itself call back into this same
                // `EditableText` (`examples/todo.rs`'s `add_item` calls
                // `TextEditingController::clear()` from inside its
                // `on_submitted` callback). Calling it while either guard
                // is still held is the same drop-under-guard hazard as for a
                // `Mutex`/`RwLock`, applied here to a `RefCell`.
                let Some(callback) = on_submitted.borrow().clone() else {
                    return KeyEventResult::Ignored;
                };
                // Auto-repeat (macOS/Win32 report a held Enter as repeated
                // `Down` events, not one Down followed by held state) must
                // not resubmit on every tick — the key is still consumed
                // (`Handled`), just without calling the callback again.
                if !event.repeat {
                    let text = controller.text();
                    drop(controller);
                    callback(&text);
                }
                KeyEventResult::Handled
            }
            Key::Named(_) => KeyEventResult::Ignored,
        }
    })
}

#[derive(Clone, Debug)]
struct EditableTextRenderView {
    text: String,
    caret_byte_offset: usize,
    show_caret: bool,
    /// The IME composing region to underline, gated on `enabled &&
    /// has_primary_focus()` by [`build_field_view`] — the FLUI analog of
    /// Flutter's `buildTextSpan`'s `withComposing: !widget.readOnly` (plus
    /// its own focus gating), named rather than a direct port since no
    /// `readOnly` field exists (see [`EditableText::enabled`]'s doc).
    composing_range: Option<Range<usize>>,
    /// The selected byte range, in the SAME space as `text` — masked when the
    /// field is obscured, because [`build_field_view`] masks before this point
    /// and the render object never sees the source characters.
    selection: Option<Range<usize>>,
    caret_height: f32,
    caret_color: Color,
    selection_color: Color,
    text_style: Option<TextStyle>,
}

impl EditableTextRenderView {
    fn build_render_object(&self) -> RenderEditable {
        let mut span = TextSpan::new(self.text.clone());
        if let Some(style) = self.text_style.clone() {
            span = span.with_style(style);
        }
        RenderEditable::new(span, TextDirection::Ltr)
            .with_caret_byte_offset(self.caret_byte_offset)
            .with_show_caret(self.show_caret)
            .with_caret_width(2.0)
            .with_caret_height(self.caret_height)
            .with_caret_color(self.caret_color)
            .with_selection(self.selection.clone())
            .with_selection_color(self.selection_color)
            .with_composing_range(self.composing_range.clone())
    }
}

impl RenderView for EditableTextRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderEditable;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut span = TextSpan::new(self.text.clone());
        if let Some(style) = self.text_style.clone() {
            span = span.with_style(style);
        }
        let mut impact = render_object.set_text(span);
        impact |= render_object.set_caret_byte_offset(self.caret_byte_offset);
        impact |= render_object.set_show_caret(self.show_caret);
        impact |= render_object.set_caret_size(2.0, self.caret_height);
        impact |= render_object.set_caret_color(self.caret_color);
        impact |= render_object.set_selection(self.selection.clone());
        impact |= render_object.set_selection_color(self.selection_color);
        impact |= render_object.set_composing_range(self.composing_range.clone());
        impact
    }
}

impl_render_view!(EditableTextRenderView);

/// Assemble the visual render view for the text field interior.
///
/// Wraps `EditableTextRenderView` directly in `inner_anchor` (the inner
/// anchor, ADR-0030): a second, inner `SubtreeAnchor` whose only job is to publish
/// exactly the editable's own `RenderId`, so the IME cursor-area loop's
/// `transform_to` starts right at the editable — not at the outer `anchor`
/// wrapping this whole field (which also spans the `AnimatedBuilder` between
/// the two, zero-offset by convention only).
/// Everything about how the field LOOKS, as one value.
///
/// Introduced when adding obscuring pushed `build_field_view` past clippy's
/// argument limit — which was the right signal rather than a threshold to
/// suppress: caret size, caret colour, text style and the obscuring pair are
/// one concept (the field's appearance) that had been travelling as loose
/// parameters, and a caller could already pass a caret colour where a caret
/// height belonged.
#[derive(Clone, Debug)]
struct FieldAppearance {
    caret_height: f32,
    caret_color: Color,
    selection_color: Color,
    text_style: Option<TextStyle>,
    /// Paint each source character as [`Self::obscuring_character`].
    obscure_text: bool,
    obscuring_character: char,
}

fn build_field_view(
    controller: &TextEditingController,
    focus_node: &Rc<FocusNode>,
    enabled: bool,
    appearance: &FieldAppearance,
    inner_anchor: flui_objects::SubtreeAnchor,
) -> BoxedView {
    // `enabled` is defensive here: `did_update_view` already unfocuses a
    // field that becomes disabled while focused, so `has_primary_focus`
    // should already be `false` by the time this runs.
    let focused = enabled && focus_node.has_primary_focus();
    // The masking happens HERE, at the one point the controller's text becomes
    // the render view's, so nothing below ever receives the real characters.
    // The masking happens HERE, so every offset below is in MASKED space and
    // the render object never receives the real characters — the selection
    // included. Mapping it here, beside the caret it has to stay consistent
    // with, is what keeps the two from being masked in different places.
    let source = controller.text();
    let source_selection = controller.selection();
    let (text, caret_byte_offset, selection) = if appearance.obscure_text {
        // Caret and both selection ends mapped in the one walk the mask needs
        // anyway, and by the one rule — see `obscure`.
        let mut offsets = [
            controller.caret_byte_offset(),
            source_selection.start,
            source_selection.end,
        ];
        let masked = obscure(&source, &mut offsets, appearance.obscuring_character);
        let [caret, start, end] = offsets;
        (masked, caret, start..end)
    } else {
        (source, controller.caret_byte_offset(), source_selection)
    };
    // A collapsed selection is the caret's business, and the render object
    // skips it anyway — `None` says so at the seam rather than relying on it.
    let selection = (!selection.is_empty()).then_some(selection);
    // The field's node for assistive technology: Flutter's
    // `RenderEditable.describeSemanticsConfiguration` (`isTextField`,
    // `isObscured`, `value`, tag `3.44.0`), outside the inner anchor so the
    // IME loop still finds the editable as that anchor's first child. The
    // value is the text the render object shows — the mask when obscured.
    let semantics = Semantics::new()
        .container(true)
        .text_field(true)
        .obscured(appearance.obscure_text)
        .enabled(enabled)
        .focused(focused)
        .value(text.clone());
    let field = crate::__private::AnchoredBox::new(
        inner_anchor,
        EditableTextRenderView {
            text,
            caret_byte_offset,
            show_caret: focused && !controller.caret_hidden_by_ime(),
            // Composing-region underline gated on the same `focused` check
            // as `show_caret` — Flutter's `buildTextSpan`'s
            // `withComposing: !widget.readOnly` plus its focus gating
            // (`_EditableTextState.buildTextSpan`, `editable_text.dart`,
            // tag `3.44.0`): an unfocused field must not keep painting a
            // stale composing underline for text it no longer owns input
            // for.
            // Suppressed entirely while obscured, matching the reference:
            // its obscured branch returns a plain span and never applies the
            // composing decoration. Two reasons, and the second is the one
            // that matters — the underline's extent would report how many
            // characters the in-progress IME composition holds, which is a
            // leak the mask exists to prevent; and the range is in SOURCE
            // byte space, so painting it against masked text would underline
            // the wrong run.
            composing_range: if focused && !appearance.obscure_text {
                controller.composing_range()
            } else {
                None
            },
            selection,
            caret_height: appearance.caret_height,
            caret_color: appearance.caret_color,
            selection_color: appearance.selection_color,
            text_style: appearance.text_style.clone(),
        },
    );
    semantics.child(field).boxed()
}

// The key handler, the obscuring mask and the render-view assembly, tested
// without a mounted tree. The mounted suite lives in
// `crates/flui-widgets/tests/editable_text.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::controller::TextEditingController;

    /// The mask is one character per SOURCE grapheme cluster, and the caret
    /// lands where it should in the masked string.
    ///
    /// "£" is two UTF-8 bytes and "😀" is four, so a byte-count mask would
    /// show 2 and 4 bullets and a byte-copied caret offset would land in the
    /// middle of one. The reference's UTF-16 unit count would show 1 and 2.
    /// One per cluster shows one each — matching what
    /// `TextEditingController` moves and deletes by, which is the granularity
    /// that has to agree.
    #[test]
    fn the_mask_is_one_character_per_source_grapheme_whatever_it_encodes_to() {
        let mut offsets = [0];
        let masked = obscure("a£😀b", &mut offsets, '\u{2022}');
        let [caret] = offsets;
        assert_eq!(
            masked.chars().count(),
            4,
            "four source characters must produce four mask characters, not \
             the eight UTF-8 bytes they occupy nor the five UTF-16 units \
             Flutter would count"
        );
        assert!(
            masked.chars().all(|c| c == '\u{2022}'),
            "every character is replaced, got {masked:?}"
        );
        assert_eq!(caret, 0, "a caret before the first char maps to 0");
    }

    /// The caret offset is mapped into the masked string, not copied.
    ///
    /// Copying it would be correct only for all-ASCII text — the case a test
    /// written with "abc" cannot tell apart from a real mapping.
    #[test]
    fn the_caret_offset_is_mapped_into_the_masked_string() {
        let source = "a£😀b";
        let mask = '\u{2022}';
        let mask_len = mask.len_utf8();

        // After "a£" — byte offset 3 in the source (1 + 2), which is the
        // SECOND character boundary.
        let mut offsets = [3];
        let masked = obscure(source, &mut offsets, mask);
        let [caret] = offsets;
        assert_eq!(
            caret,
            2 * mask_len,
            "two source characters precede the caret, so it maps to two mask \
             characters in; copying the source offset would give 3"
        );
        assert!(
            masked.is_char_boundary(caret),
            "the mapped caret must land on a char boundary of the mask"
        );

        // The end of the text maps to the end of the mask.
        let mut end_offsets = [source.len()];
        let masked_end = obscure(source, &mut end_offsets, mask);
        let [caret_end] = end_offsets;
        assert_eq!(caret_end, masked_end.len(), "end maps to end");
    }

    /// A multi-scalar cluster is ONE bullet, and the caret after it maps to
    /// one mask character in — the granularity the controller's Backspace
    /// removes at, so one keystroke removes one bullet.
    #[test]
    fn a_zwj_sequence_masks_to_a_single_bullet_and_maps_back() {
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}";
        let source = format!("a{family}b");
        let mask = '\u{2022}';
        let mask_len = mask.len_utf8();

        let mut offsets = [1 + family.len()];
        let masked = obscure(&source, &mut offsets, mask);
        let [caret] = offsets;
        assert_eq!(masked.chars().count(), 3, "a, the family, b: three bullets");
        assert_eq!(
            caret,
            2 * mask_len,
            "a caret after the family sits after the second bullet"
        );
        assert_eq!(
            source_offset_for_masked_offset(&source, 2 * mask_len, mask),
            1 + family.len(),
            "the second bullet's end maps back to the cluster boundary, never \
             inside the sequence"
        );
    }

    /// An empty field masks to nothing, with the caret at zero.
    #[test]
    fn an_empty_field_masks_to_an_empty_string() {
        let mut offsets = [0];
        let masked = obscure("", &mut offsets, '\u{2022}');
        let [caret] = offsets;
        assert!(
            masked.is_empty(),
            "no source characters, no mask characters"
        );
        assert_eq!(caret, 0);
    }
    /// The key handler's `can_request_focus` guard, in isolation: invoked
    /// directly (bypassing `FocusManager::dispatch_key_event`'s own
    /// primary-focus routing), a disabled node's handler must still refuse
    /// to mutate the controller.
    ///
    /// Oracle analog: `'Does not accept updates when read-only'`
    /// (`editable_text_test.dart`, tag `3.44.0`) — see
    /// `disabled_field_does_not_publish_its_focus_node`'s doc comment above
    /// for the adapted contrast.
    ///
    /// Red-check: delete the `if !focus_node.can_request_focus() { return
    /// false; }` guard at the top of `build_key_handler`'s closure — the
    /// character is inserted and both assertions fail.
    #[test]
    fn disabled_key_handler_ignores_input_even_when_invoked_directly() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::new();
        let focus_node = FocusNode::with_debug_label("test");
        focus_node.set_can_request_focus(false);
        let handler = build_key_handler(
            Rc::new(RefCell::new(controller.clone())),
            Rc::clone(&focus_node),
            Rc::new(RefCell::new(None)),
        );

        let event = KeyEventBuilder::new(Code::KeyA)
            .with_key(Key::Character("a".to_string()))
            .with_state(KeyState::Down)
            .build();

        let consumed = handler(&event);

        assert_eq!(consumed, KeyEventResult::Ignored);
        assert_eq!(
            controller.text(),
            "",
            "a disabled node's key handler must not mutate the controller"
        );
    }

    /// A key carrying a command modifier is a command, not text.
    ///
    /// The field must neither insert it nor consume it: returning `Handled`
    /// stops `FocusManager::dispatch_key_event`'s leaf->root walk at the
    /// field, so the enclosing `Shortcuts` never sees the chord. Together that
    /// makes Ctrl+S type "s" into the document instead of saving it, and every
    /// application shortcut dead while any field is focused.
    ///
    /// Flutter reaches the same place from the other side: `EditableText`
    /// installs no `onKeyEvent` at all — characters arrive through
    /// `TextInputClient.updateEditingValue`, and chords are routed by
    /// `default_text_editing_shortcuts.dart`.
    ///
    /// If reverted (drop the `is_command_chord` guard): every assertion below
    /// that expects `Ignored`/empty text fails.
    #[test]
    fn a_command_chord_is_neither_inserted_nor_consumed() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let chord = |code, key: &str, modifiers| {
            let controller = TextEditingController::new();
            let focus_node = FocusNode::with_debug_label("test");
            let handler = build_key_handler(
                Rc::new(RefCell::new(controller.clone())),
                Rc::clone(&focus_node),
                Rc::new(RefCell::new(None)),
            );
            let event = KeyEventBuilder::new(code)
                .with_key(Key::Character(key.to_string()))
                .with_state(KeyState::Down)
                .with_modifiers(modifiers)
                .build();
            (handler(&event), controller.text())
        };

        for (code, key, modifiers, label) in [
            (
                Code::KeyS,
                "s",
                Modifiers::CONTROL,
                "Ctrl+S must save, not type",
            ),
            (
                Code::KeyZ,
                "z",
                Modifiers::CONTROL,
                "Ctrl+Z must undo, not type",
            ),
            (
                Code::KeyC,
                "c",
                Modifiers::META,
                "Cmd+C must copy, not type",
            ),
        ] {
            let (result, text) = chord(code, key, modifiers);
            assert_eq!(result, KeyEventResult::Ignored, "{label}: must bubble");
            assert_eq!(text, "", "{label}: must not reach the buffer");
        }

        // AltGr is Control+Alt on Win32 (`get_modifiers` sets both from
        // VK_CONTROL/VK_MENU) and no backend sets `ALT_GRAPH`, so a
        // Ctrl-keyed predicate alone would break every AltGr layout: the
        // character it produces is text, not a command.
        let (result, text) = chord(Code::Digit2, "@", Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(
            result,
            KeyEventResult::Handled,
            "AltGr composes text and must still be inserted"
        );
        assert_eq!(
            text, "@",
            "AltGr's composed character must reach the buffer"
        );

        // An unmodified character is unchanged.
        let (result, text) = chord(Code::KeyA, "a", Modifiers::empty());
        assert_eq!(result, KeyEventResult::Handled);
        assert_eq!(text, "a");
    }

    /// The word-jump modifier for the platform these tests actually run
    /// on — `super::word_jump_modifier`, the same function
    /// `is_word_jump_modifier` itself calls, kept under a plain name here
    /// for test-call-site readability.
    fn platform_word_jump_modifier() -> Modifiers {
        super::word_jump_modifier(TargetPlatform::current())
    }

    /// The modifier that is NOT this platform's word-jump chord — Control
    /// on macOS/iOS (unbound for word-jump there), Alt everywhere else
    /// (reserved, unhandled — see `is_word_jump_modifier`'s `# DEFERRED`).
    fn non_word_jump_modifier() -> Modifiers {
        match TargetPlatform::current() {
            TargetPlatform::MacOS | TargetPlatform::iOS => Modifiers::CONTROL,
            _ => Modifiers::ALT,
        }
    }

    /// `word_jump_modifier` is a pure function of `TargetPlatform` — table
    /// every variant explicitly, since this CI only ever runs on
    /// `ubuntu-latest` and a test keyed to `TargetPlatform::current()`
    /// would never exercise the macOS/iOS arm on any real run.
    #[test]
    fn word_jump_modifier_maps_every_platform() {
        assert_eq!(
            super::word_jump_modifier(TargetPlatform::MacOS),
            Modifiers::ALT
        );
        assert_eq!(
            super::word_jump_modifier(TargetPlatform::iOS),
            Modifiers::ALT
        );
        for platform in [
            TargetPlatform::Windows,
            TargetPlatform::Linux,
            TargetPlatform::Android,
            TargetPlatform::Fuchsia,
            TargetPlatform::Unknown,
        ] {
            assert_eq!(
                super::word_jump_modifier(platform),
                Modifiers::CONTROL,
                "{platform:?}"
            );
        }
    }

    /// `is_word_jump_modifier` requires the EXACT chord, not just "the
    /// required modifier happens to be among the ones held" — a chord
    /// that ALSO holds another command modifier is not word-jump on
    /// either the Linux/Windows/... default (Ctrl+Alt+Right, which could
    /// otherwise be mistaken for the plain Ctrl chord) or the macOS/iOS
    /// arm (Option+Command+Right, which could otherwise be mistaken for
    /// the plain Option chord).
    #[test]
    fn is_word_jump_modifier_rejects_a_chord_with_an_extra_command_modifier() {
        assert!(
            !super::is_word_jump_modifier(
                Modifiers::CONTROL | Modifiers::ALT,
                TargetPlatform::Linux
            ),
            "Ctrl+Alt is not the plain Ctrl chord"
        );
        assert!(
            !super::is_word_jump_modifier(Modifiers::META | Modifiers::ALT, TargetPlatform::MacOS),
            "Cmd+Alt is not the plain Alt (Option) chord"
        );
        // The plain chords themselves, and Shift alongside them, still work —
        // Shift is not part of the command-modifier mask this check guards.
        assert!(super::is_word_jump_modifier(
            Modifiers::CONTROL,
            TargetPlatform::Linux
        ));
        assert!(super::is_word_jump_modifier(
            Modifiers::CONTROL | Modifiers::SHIFT,
            TargetPlatform::Linux
        ));
        assert!(super::is_word_jump_modifier(
            Modifiers::ALT,
            TargetPlatform::MacOS
        ));
    }

    /// The platform's word-jump modifier + Backspace deletes the WHOLE
    /// current word, not one character.
    #[test]
    fn word_jump_modifier_backspace_deletes_the_whole_word_behind_the_caret() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::with_text("hello world");
        let focus_node = FocusNode::with_debug_label("test");
        let handler = build_key_handler(
            Rc::new(RefCell::new(controller.clone())),
            Rc::clone(&focus_node),
            Rc::new(RefCell::new(None)),
        );
        controller.move_caret_end();

        let event = KeyEventBuilder::new(Code::Backspace)
            .with_key(Key::Named(NamedKey::Backspace))
            .with_state(KeyState::Down)
            .with_modifiers(platform_word_jump_modifier())
            .build();

        assert_eq!(handler(&event), KeyEventResult::Handled);
        assert_eq!(
            controller.text(),
            "hello ",
            "the whole word \"world\", not just \"d\""
        );
    }

    /// The OTHER platform's word-jump modifier does nothing special for
    /// Backspace here — plain single-character deletion, per
    /// `is_word_jump_modifier`'s per-platform table
    /// (`# DEFERRED`: reserved for a future line-boundary intent on
    /// non-Apple platforms, not silently claimed as word-delete).
    #[test]
    fn non_word_jump_modifier_backspace_deletes_only_one_character() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::with_text("hello worlds");
        let focus_node = FocusNode::with_debug_label("test");
        let handler = build_key_handler(
            Rc::new(RefCell::new(controller.clone())),
            Rc::clone(&focus_node),
            Rc::new(RefCell::new(None)),
        );
        controller.move_caret_end();

        let event = KeyEventBuilder::new(Code::Backspace)
            .with_key(Key::Named(NamedKey::Backspace))
            .with_state(KeyState::Down)
            .with_modifiers(non_word_jump_modifier())
            .build();

        assert_eq!(handler(&event), KeyEventResult::Handled);
        assert_eq!(
            controller.text(),
            "hello world",
            "only the last character, not the whole word"
        );
    }

    /// The platform's word-jump modifier + Left/Right jump the caret by a
    /// WORD, not a character — the keyboard-facing half of
    /// `TextEditingController`'s `# Word unit` contract.
    #[test]
    fn word_jump_modifier_arrow_jumps_the_caret_by_a_word() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::with_text("hello world");
        let focus_node = FocusNode::with_debug_label("test");
        let handler = build_key_handler(
            Rc::new(RefCell::new(controller.clone())),
            Rc::clone(&focus_node),
            Rc::new(RefCell::new(None)),
        );
        controller.move_caret_home();

        let right = KeyEventBuilder::new(Code::ArrowRight)
            .with_key(Key::Named(NamedKey::ArrowRight))
            .with_state(KeyState::Down)
            .with_modifiers(platform_word_jump_modifier())
            .build();
        assert_eq!(handler(&right), KeyEventResult::Handled);
        assert_eq!(controller.caret_byte_offset(), 6, "start of \"world\"");

        let left = KeyEventBuilder::new(Code::ArrowLeft)
            .with_key(Key::Named(NamedKey::ArrowLeft))
            .with_state(KeyState::Down)
            .with_modifiers(platform_word_jump_modifier())
            .build();
        assert_eq!(handler(&left), KeyEventResult::Handled);
        assert_eq!(controller.caret_byte_offset(), 0);
    }

    /// The OTHER platform's word-jump modifier does nothing special for
    /// Right here — plain single-character movement.
    #[test]
    fn non_word_jump_modifier_arrow_moves_only_one_character() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::with_text("hello world");
        let focus_node = FocusNode::with_debug_label("test");
        let handler = build_key_handler(
            Rc::new(RefCell::new(controller.clone())),
            Rc::clone(&focus_node),
            Rc::new(RefCell::new(None)),
        );
        controller.move_caret_home();

        let right = KeyEventBuilder::new(Code::ArrowRight)
            .with_key(Key::Named(NamedKey::ArrowRight))
            .with_state(KeyState::Down)
            .with_modifiers(non_word_jump_modifier())
            .build();
        assert_eq!(handler(&right), KeyEventResult::Handled);
        assert_eq!(
            controller.caret_byte_offset(),
            1,
            "one character, not a jump to the next word's start"
        );
    }

    /// Shift + the platform's word-jump modifier EXTENDS the selection by a
    /// whole word, leaving the anchor — the `(true, true)` arm of the
    /// `ArrowLeft`/`ArrowRight` key handler, untested until now (the other
    /// three combinations of word/character × move/extend all had
    /// coverage; this was the gap).
    #[test]
    fn shift_word_jump_modifier_extends_the_selection_by_a_word() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::with_text("hello world");
        let focus_node = FocusNode::with_debug_label("test");
        let handler = build_key_handler(
            Rc::new(RefCell::new(controller.clone())),
            Rc::clone(&focus_node),
            Rc::new(RefCell::new(None)),
        );
        controller.move_caret_home();

        let right = KeyEventBuilder::new(Code::ArrowRight)
            .with_key(Key::Named(NamedKey::ArrowRight))
            .with_state(KeyState::Down)
            .with_modifiers(platform_word_jump_modifier() | Modifiers::SHIFT)
            .build();
        assert_eq!(handler(&right), KeyEventResult::Handled);
        assert_eq!(
            controller.selection(),
            0..6,
            "anchor stays at 0, extent jumps to the start of \"world\""
        );

        // Shift + word-jump-modifier + Left shrinks the SAME selection back
        // by a word, exercising the `ArrowLeft` arm's own `(true, true)`
        // branch (the `ArrowRight` case above only proves the `ArrowRight`
        // arm's).
        let left = KeyEventBuilder::new(Code::ArrowLeft)
            .with_key(Key::Named(NamedKey::ArrowLeft))
            .with_state(KeyState::Down)
            .with_modifiers(platform_word_jump_modifier() | Modifiers::SHIFT)
            .build();
        assert_eq!(handler(&left), KeyEventResult::Handled);
        assert_eq!(
            controller.selection(),
            0..0,
            "anchor stays at 0, extent jumps back to the start of \"hello\""
        );
    }

    /// `EditableText::text_style` flows through to the rendered `TextSpan` —
    /// the render-view assembly this builder feeds.
    #[test]
    fn text_style_reaches_the_rendered_span() {
        let style = TextStyle::default().with_color(Color::rgb(10, 20, 30));
        let render_view = EditableTextRenderView {
            text: "hello".to_string(),
            caret_byte_offset: 0,
            show_caret: false,
            composing_range: None,
            caret_height: 18.0,
            selection: None,
            caret_color: Color::BLACK,
            selection_color: Color::TRANSPARENT,
            text_style: Some(style.clone()),
        };

        let render_object = render_view.build_render_object();
        let rendered_style = render_object
            .painter()
            .text()
            .expect("a span was set")
            .style()
            .expect("text_style was set");
        assert_eq!(rendered_style.color, style.color);
    }

    /// Without `text_style`, the span carries no explicit style — no
    /// override was silently invented.
    #[test]
    fn no_text_style_leaves_the_span_unstyled() {
        let render_view = EditableTextRenderView {
            text: "hello".to_string(),
            caret_byte_offset: 0,
            show_caret: false,
            composing_range: None,
            caret_height: 18.0,
            selection: None,
            caret_color: Color::BLACK,
            selection_color: Color::TRANSPARENT,
            text_style: None,
        };

        let render_object = render_view.build_render_object();
        assert!(
            render_object
                .painter()
                .text()
                .expect("a span was set")
                .style()
                .is_none()
        );
    }
}
