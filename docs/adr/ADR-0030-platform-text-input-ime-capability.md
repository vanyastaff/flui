# ADR-0030: Platform text input (IME)

- **Status:** Accepted
- **Date:** 2026-07-17
- **Absorbs:** ADR-0032, ADR-0033
- **Superseded in part by:** ADR-0078 (capability acquisition)
- **Amended by:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (§2: `PlatformTextInput`
  now lives in `flui-platform-api`, re-exported at its old `flui-platform` path; its contract is
  unchanged)

## Context

Flutter bundles IME, system chrome and haptics behind one `services` layer
(`TextInputConnection` over a method channel). FLUI does not port that layer:
each becomes a capability trait on `flui-platform` (`docs/FOUNDATIONS.md`, "No
`flui-services`"). This record fixes the IME vocabulary, the platform
capability, the client contract a text field implements, how the composing
region renders, and how the candidate window follows it.

Who *owns* a text-input session — the presentation, through
`flui_interaction::TextInputOwner` — is ADR-0037 §5. This record states the
behavior that ownership must deliver. The macOS `NSTextInputClient` contract is
ADR-0069.

## Decision

### 1. `flui_types::ImeEvent` is winit-shaped

`ImeEvent` is `Enabled` / `Preedit { text, cursor }` / `Commit(String)` /
`Disabled`, mirroring winit 0.30's `Ime` enum. `Preedit.cursor` is a byte range
into the preedit string, not into the document; `cursor: None` means "hide the
caret". The type lives in `flui-types` so widgets can name it without
depending on `flui-platform`.

This is winit's push model, not the W3C
`compositionstart`/`compositionupdate`/`compositionend` triad and not
Android's pull-model `InputConnection`. The divergence is documented on the
type itself, where a future web or Android backend author has to bridge it.

### 2. `PlatformTextInput` is a fallible per-window capability

`PlatformTextInput { set_ime_allowed(bool), set_ime_cursor_area(Bounds<Pixels>) }`
is reached through `PlatformWindow::text_input() -> Option<Arc<dyn PlatformTextInput>>`,
default `None` — the same discovery shape as `PlatformWindow::display()`. A
backend that cannot do IME returns `None` instead of inheriting methods it
cannot honor. `PlatformInput` carries IME events as `PlatformInput::Ime`.

The winit backend converts `WindowEvent::Ime` in a pure, unit-tested function
and returns a `WinitTextInput` wrapping its `Arc<winit::window::Window>`. The
headless backend's `FakeTextInput` records every call, and `MockWindow`
returns the same instance on every `text_input()` call so tests can observe
the history independently of whoever attached.

### 3. Session contract

- One active client per presentation. `attach` replaces the previous client
  with no coordination from the caller (Flutter parity: one current
  `TextInputConnection`).
- `attach` returns a token; `detach(token)` is a no-op unless the token is
  still the active one. A replaced field's delayed blur or dispose cannot
  disable IME under the field that replaced it.
- IME is enabled on attach and disabled only by the active detach or by the
  owner closing.
- Detach-on-dispose is part of the client contract.

### 4. Suppression and composition end

- A client suppresses `Key::Character` insertion **only while a composition is
  non-empty**. winit already withholds `KeyboardInput` during composition and
  right after a commit; suppressing all typing after `Enabled` would kill plain
  keyboard input for the rest of the session.
- `Disabled` mid-composition strips the uncommitted slice. This is winit's
  semantics and a deliberate divergence from Flutter's
  `connectionClosed`, which keeps it.
- `Preedit("")` while composing strips the slice and **ends** the composition
  (`composing = None`). Leaving an empty-but-active region made `is_composing()`
  stay true forever and suppressed typing for the rest of the focus session.
  With no active composition, an empty preedit is a no-op: winit on X11 sends
  one on IME start before any slice exists (#1054).

### 5. Composing state is one value

`TextEditingController` stores `Option<ComposingState>` where
`ComposingState { range, caret_hidden }`, not a range plus a sibling
`caret_hidden: bool`. Every site that ends composition (`commit_text`,
`clear_composing`, non-IME edits, the empty-preedit path) drops `caret_hidden`
with it, so the flag cannot outlive the composition. `caret_hidden_by_ime()`
returns `false` when nothing is composing.

Direct caret movement while composing clears `caret_hidden` and leaves the
range and its underline in place: the user took the caret back, the
composition continues.

### 6. Rendering the composing region

- **Underline — a declared approximation.** `RenderEditable` paints one flat
  1-logical-pixel rect per selection box, 1px below the alphabetic baseline,
  clamped inside each box. Flutter merges `TextDecoration.underline` into the
  composing span and lets the text engine draw it from font metrics; this is
  not that, and must not be described as parity. The color is recomputed from
  the editable's own `TextStyle` with the engine's rule
  (`foreground.or(color).unwrap_or(BLACK)`), so it always matches the glyphs.
  When span-level decoration reaches `TextStyle` and the editable's span
  builder, the underline moves onto it and this paint path is deleted; the two
  must not coexist.
- **Hidden caret at the widget layer.** `RenderEditable` keeps its single
  `show_caret` flag; the widget sets it to
  `focused && !controller.caret_hidden_by_ime()`.
- **Focus gates the composing range.** The range is passed to the render
  object only while the field is enabled and has primary focus (Flutter's
  `withComposing: !readOnly && _hasFocus`). Blur detaches the IME client but
  does not end the composition, so an unfocused field must not keep painting a
  stale underline.
- `rect_for_composing_range()` returns `None`, never `Rect::ZERO`, when there
  is no active range, no layout or no boxes: a zero rect would tell the
  platform the composition sits at the origin.
- Single line only. The byte-range-to-box lookup is correct because
  `RenderEditable` is constrained to `max_lines(1)`; multiline must revisit it.

### 7. The candidate window follows the composition

**Single-rect reduction (divergence).** Flutter sends the field's
transform-to-root and a local caret rect as two channels and lets the embedder
compose them. winit's `set_ime_cursor_area` takes one rect in window-root
logical pixels, so FLUI composes `transform × local_rect` on the framework side
and sends one rect. The observable behavior — the candidate window tracks the
caret through scrolling, transforms and ancestor offsets — is preserved; an
embedder that wants two channels back can re-decompose.

**Rect source.** The composing region's rect, falling back to the caret rect
when there is none — Flutter's `_updateComposingRectIfNeeded` order.
`RenderEditable::caret_local_rect()` is visibility-independent (`show_caret`
gates only paint), because composition is exactly when the caret may be hidden
and the window must still track it.

**A per-attach post-frame loop.** `CursorAreaLoop` starts on attach, fires once
per completed frame, sends the rect only when it changed, and reschedules
itself. It stops only when its attach ends:

- **Every attach is a fresh session.** The alive flag and the last-sent cache
  are created per attach. A shared alive flag lets a blur+refocus inside one
  frame run two loops; a shared cache suppresses the first send of a refocus
  at the same caret position. `ImeEvent::Enabled` also clears the current
  attach's cache, since a backend may restart the session without a focus
  change.
- **A transient `None` is a skip, never a stop.** An unreachable anchor mid-
  rebuild, a missing pipeline owner or root: nothing is sent this frame, and
  the loop reschedules.
- **Every scheduling failure is a `tracing::warn!`**, including the first. A
  loop that silently never starts leaves the candidate window at `(0, 0)` with
  no signal.
- The transform is read from a second, inner `SubtreeAnchor` wrapped directly
  around the editable, so it starts at the editable structurally rather than
  relying on the intervening subtree applying no offset.

**Coordinate space.** `Bounds<Pixels>` at every seam is window-root logical
pixels, matching `PlatformWindow::bounds`. DPI conversion is the backend's job.

The loop's post-frame and text-input handles are lifecycle capabilities; where
they may be acquired is ADR-0078.

## Consequences

- IME works end to end on winit and macOS (ADR-0069); composing text is
  visibly distinct and the caret hides while the IME owns it.
- The underline is an approximation until text decoration lands, and the
  composing geometry is single-line until multiline editing lands.
- The internal-scroll case is open: `caret_local_rect` is viewport-relative by
  construction and stays so; content-space geometry, if ever needed, is a
  separate accessor.
- Behavior is verified headlessly against recording fakes; the platform
  candidate window itself is checked by hand on each native backend.

## Alternatives rejected

- **W3C- or Android-shaped event vocabulary.** Both map imperfectly onto the
  lead desktop backend's four-variant push model; bridging happens in the
  backend that needs it.
- **Methods directly on `PlatformWindow` with no-op defaults.** Every window
  would inherit IME methods it cannot honor, with no way for callers to tell.
- **A sibling `caret_hidden: bool`.** Correct only by discipline at every
  composition-ending site; the combined value makes the leak unrepresentable.
- **Two-channel cursor area (transform + local rect).** Not the surface winit
  exposes; the single rect preserves the observable contract.
- **Bespoke hidden-caret render state.** The existing `show_caret` flag
  already expresses it.
