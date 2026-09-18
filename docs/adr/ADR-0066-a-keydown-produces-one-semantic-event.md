# ADR-0066 — A `keyDown:` produces one semantic event, and the input method is a route not a second producer

- **Status:** Accepted
- **Date:** 2026-09-17
- **Issue:** App.5 (`docs/ROADMAP-TRACKER.md`) — "IME working" on a native platform
- **Supersedes:** nothing. Records the contract the macOS backend's
  `NSTextInputClient` conformance implements.
- **Depends on:** ADR-0030 (`text_input` capability), ADR-0032 (the cursor area is
  a single rect), ADR-0033 (the caret is hidden while `Preedit.cursor` is `None`).

## Context

A key press on the macOS native backend reaches `FLUIContentView`'s `keyDown:`
and is converted there, by `extract_key`, into exactly one `Key::Character`.
That is the whole input path: one physical key, one semantic event.

Composition cannot be built on that path. AppKit's input method is a *protocol
the view must implement*, and it is entered from only one place —
`-[NSResponder interpretKeyEvents:]` — which forwards the event to the view's
input context and calls *back* into the same view:
`setMarkedText:selectedRange:replacementRange:` while composing,
`insertText:replacementRange:` on commit, `doCommandBySelector:` for keys the
input method declines.

So the naive wiring is: keep `keyDown:`'s existing conversion, and also call
`interpretKeyEvents:`. **That makes one physical key press produce two events** —
the `Key::Character` from `keyDown:` and the `ImeEvent::Commit` from
`insertText:`. It is the same defect class ADR-0044 §3 records for
`record_compositor_tick`: a per-pump call that reaches into a second producer and
double-counts one input.

The tempting refinement — "call `interpretKeyEvents:` only for keys that look
like composition, and convert directly otherwise" — is not available, because
whether a key composes is a property of the active input method, not of the key.
On a U.S. layout a plain letter *does* reach `insertText:`, so any predicate
evaluated before the input method runs is wrong for some layout, and the two
producers coexist exactly where it guesses wrong.

## Decision

**The direct-conversion route and the input-method route are mutually exclusive,
and the selector is attachment state.**

`keyDown:` consults the view's text-input state:

- **`ime_allowed == false`** — the default, and the state of every window in this
  tree today — the existing conversion runs unchanged. Behaviour is byte-identical
  to the pre-ADR path; the conformance adds no regression surface.
- **`ime_allowed == true`** — the event is handed to `interpretKeyEvents:` and
  `keyDown:` emits nothing itself. Characters arrive as `Preedit` / `Commit`;
  keys the input method declines arrive through `doCommandBySelector:`, which
  re-dispatches the event through the same conversion the direct route uses, so
  arrows, Escape and friends keep working while attached.

The invariant this buys, at protocol level:

> **Per `keyDown:`, exactly one of a `Key`-family event or an `ImeEvent` is
> produced — and which one depends only on whether a text input is attached, not
> on which key was pressed.**

It is stated at protocol level rather than only in a crate's mapping decisions
because it is an input contract every backend owes, not a macOS implementation
detail: the Windows backend's eventual `WM_IME_*` / `WM_CHAR` split faces the
identical question and must record its own gate.

**The contract is not invented here — the reference implementation agrees, and
states it in these terms.** `winit`'s `Window::set_ime_allowed` documentation
(0.30.13, `src/window.rs`, read 2026-09-17):

> "When IME is allowed, the window will receive `Ime` events, and during the
> preedit phase the window will NOT get `KeyboardInput` events." … "When IME is
> not allowed, the window won't receive `Ime` events, and will receive
> `KeyboardInput` events for every keypress instead." … "IME is **not** allowed
> by default."

That is the mutual exclusion, gated on attachment, with the same default — the
market's shape. FLUI's winit backend already inherits it by delegation
(`WinitTextInput`); this ADR is what makes the *native* AppKit backend, which
owns the pipeline instead of delegating it, obey the same rule rather than
inventing a second one.

## Why attachment is the gate, and not the input method's answer

`-[NSTextInputContext handleEvent:]` also exists and returns a `BOOL` — "did the
input method consume this". Consulting it looks like a tighter rule, and it was
considered and set aside: the routing decision is already correct *before* the
call, because attachment is the thing that should decide whether the keyboard
path is live. Asking the context first would make the invariant a property of
AppKit's answer rather than of this code, and would then take the keyboard path
in exactly the states where the two producers are both live. `interpretKeyEvents:`
is also NSResponder's own entry point — reachable without first obtaining a
context object — and pairs with `doCommandBySelector:` as an explicit decline
path.

## What was rejected

- **`interpretKeyEvents:` on every `keyDown:`.** The two-producer defect above.
- **A pre-call predicate on the key** ("is this a composing key"). Undecidable
  before the input method runs; wrong per layout.
- **Synthesizing the commit as a `Key::Character`** so only one event type
  exists. It preserves the count but destroys the marked-text lifecycle: the
  app can no longer tell composition from commit, which is precisely the
  distinction ADR-0030/0032/0033 are built on.
- **Gating on "a text input widget is focused"** rather than on the capability.
  The backend cannot see focus; `set_ime_allowed` is the interface it does have,
  and ADR-0030 already made attachment the framework-side concept.

## Consequences

- **`ime_allowed` becomes load-bearing.** Enabling it changes what a character
  key produces. A widget that attaches a text input and still expects raw
  `Key::Character` events for the same press gets one or the other, never both —
  which is the intended contract, and the reason attachment is owned by the
  framework-side capability rather than by a widget-local flag.
- **Disabling mid-composition drops the composition; it does not commit it.**
  `set_ime_allowed(false)` while marked text is live clears the view's marked
  state and emits `ImeEvent::Disabled`. This follows winit's own macOS
  implementation, which does exactly that — `set_ime_allowed`
  (`winit-0.30.13/src/platform_impl/macos/view.rs:880`) clears `marked_text`
  before setting `ImeState::Disabled` and queueing `WindowEvent::Ime(Ime::Disabled)`
  — although winit's *public* documentation for the call (`src/window.rs:1265`)
  states only the attachment rule and says nothing about an open composition, so
  the implementation rather than the doc is the citation. The deliberate
  divergence is from Flutter, which closes a connection *without discarding the
  composed characters*: `EditableTextState.connectionClosed`
  (`editable_text.dart:4138`, checked at the pinned 3.44.0) nulls the connection
  and unfocuses, the unfocus reaches `controller.clearComposing()`, and
  `clearComposing` (`editable_text.dart:378`) empties only the composing
  *range* — the controller's `text` keeps the characters, now as ordinary text.
  FLUI drops them instead: the marked text is input-method state the framework
  does not own, and keeping it would present a character the user never confirmed
  as text they had typed. The grounding is that implementation plus the
  client-side bug class `flui-types/src/ime.rs` records — a client never told the
  composition ended stays wedged for the rest of the focus session — with the
  Flutter contrast recorded only to make the difference explicit.
- **Every backend that gains composition owes this decision.** The invariant is
  per-backend because the pipeline is; a backend that adds an input method
  without an explicit gate reintroduces the double-producer silently.
- **No new event type is required.** `PlatformInput::Ime` and `PlatformTextInput`
  already exist; this ADR constrains how a backend may *use* them, and adds
  nothing to the surface.
