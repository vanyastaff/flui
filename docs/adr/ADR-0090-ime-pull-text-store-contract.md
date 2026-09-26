# ADR-0090: IME talks to a pull text-store contract with edits and asynchronous locks

- **Status:** Proposed
- **Date:** 2026-09-25
- **Supersedes (on acceptance):** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §1
  (the winit-shaped push vocabulary as the contract) and the push-only shape of §2
- **Related:** [ADR-0037](ADR-0037-presentation-ownership-domains.md) §5 (who owns a text-input
  session), [ADR-0069](ADR-0069-a-keydown-produces-one-semantic-event.md) (one semantic event per
  key; the input method is a route), [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  (capability acquisition), [ADR-0082](ADR-0082-platform-api-contract-crate.md) (the trait moves
  to `flui-platform-api`), [ADR-0092](ADR-0092-per-realm-text-over-parley.md) (the layout that
  answers geometry queries)
- **Refs:** decision D10 and owner decision 8 in the [decision index](../../design/decisions.md);
  panel record in
  [`report-decisions.ru.md`](../research/2026-09-25-architecture-review/report-decisions.ru.md) §8;
  roadmap exit B1 (Windows with Narrator and a Japanese IME)

## Context

[ADR-0030](ADR-0030-platform-text-input-ime-capability.md) made the IME contract winit's push
model: the platform pushes `ImeEvent::{Enabled, Preedit, Commit, Disabled}`
(`crates/flui-types/src/ime.rs:72`) and the framework pushes back two things,
`set_ime_allowed` and `set_ime_cursor_area` (`crates/flui-platform/src/traits/text_input.rs`,
the whole `PlatformTextInput` trait). The platform never asks the framework anything. Its §1
already noted that Android's `InputConnection` is a pull model and left the bridge to "the backend
that needs it".

Three of the platforms FLUI targets are pull models, and the push shape cannot serve them:

- **Windows.** Text Services Framework talks to an `ITextStoreACP`: the text service asks for the
  text in a range, the selection, the screen rect of a range and the character at a point, and it
  *edits* the document (`SetText`, `InsertTextAtSelection`) under a document lock it requests with
  `RequestLock`. The application may grant that lock later (`TS_S_ASYNC`). Reconversion, dictation
  (Win+H) and the candidate window all go through these queries. FLUI's Win32 backend has no IME
  code at all: no `WM_IME*`, `ITextStore*`, `ITfThreadMgr` or `ImmGetContext` anywhere under
  `crates/`, no `text_input()` override (the implementations are headless, macOS and winit only:
  `crates/flui-platform/src/platforms/headless/platform.rs:1250`,
  `crates/flui-platform/src/platforms/macos/window.rs:944`,
  `crates/flui-platform/src/platforms/winit/window.rs:313`), and no text-services feature of the
  `windows` crate enabled (`crates/flui-platform/Cargo.toml:89-105`).
- **macOS.** `NSTextInputClient` is a pull protocol too, and the backend already implements the
  query half with nothing to answer from: `attributedSubstringForProposedRange:actualRange:` returns
  `nil` (`crates/flui-platform/src/platforms/macos/text_input.rs:435`) and
  `characterIndexForPoint:` returns `NSNotFound` because "this view owns no text"
  (`text_input.rs:505-514`). `selectedRange` answers `{NSNotFound, 0}` outside a composition
  (`text_input.rs:402`). Reconversion and the Services menu cannot work while the view has no
  document to read.
- **Android.** `InputConnection` asks for text before and after the cursor, deletes surrounding
  text and sets composing regions.

The framework side has the document but no store surface: `TextEditingController` holds an
`Arc<Mutex<ControllerInner>>` (`crates/flui-widgets/src/text/controller.rs:263`) whose offsets are
UTF-8 bytes (`controller.rs:34`), while macOS already converts UTF-16 `NSRange`s at the boundary
(`crates/flui-platform/src/platforms/macos/text_input.rs:133-136`). The session owner is
`flui_interaction::TextInputOwner`, which imports the platform trait directly
(`crates/flui-interaction/src/text_input.rs:27`) and routes events through
`ImeEventCallback = Rc<dyn Fn(&ImeEvent)>` (`text_input.rs:39`).

The panel first proposed a read-only store. Verification found that TSF needs edits and a lock
that can be granted later, and that a kit testing only reads would certify a contract Windows
cannot use; the contract below includes both.

## Decision

### 1. The contract is a text store the platform pulls from

A text field that accepts IME implements a synchronous text-store surface, called on the owner
thread, in `flui-platform-api` ([ADR-0082](ADR-0082-platform-api-contract-crate.md)):

- **Reads:** document length; text in a range; the selection (anchor and active end); the
  composing range, if any; the screen rect of a range (window-root logical pixels, the coordinate
  space [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §7 fixed); the index at a point.
- **Edits:** replace a range with text; insert at the selection; set the selection; set or clear
  the composing range. Each edit reports the resulting change (range replaced, new length,
  new selection) so the backend can emit its platform's change notification
  (`ITextStoreACPSink::OnTextChange`, `OnSelectionChange`).
- **Offsets are UTF-16 code units** on this surface, because TSF (ACP), AppKit (`NSRange`) and
  Android all count them. The field's own representation stays whatever it is; the conversion
  lives once, in a shared helper next to the contract, not in each backend.
- **Locks.** Reads and edits happen only inside a lock the platform requested: read-only or
  read-write, synchronous or asynchronous. A request made while the field cannot grant it (it is
  inside its own edit, or inside a frame transaction) is answered "granted later"
  (`TS_S_ASYNC`'s meaning) and the grant runs at the next owner-thread anchor where commands may
  commit ([ADR-0027](ADR-0027-owner-affine-ui-realms.md) §3). A synchronous request that cannot be
  granted is refused with a typed error, never blocked on. Edits made under a platform lock are
  one undo step and one change notification to the widget.
- The implementor is the field's editing state over its text layout (a `TextFieldState` over
  Parley, [ADR-0092](ADR-0092-per-realm-text-over-parley.md)); the controller's
  `Arc<Mutex<…>>` does not survive into this surface.

### 2. The push vocabulary becomes a projection

`ImeEvent` stays as the adapter for push-model sources (winit today). The winit backend
translates `Preedit`/`Commit`/`Disabled` into store edits under a synchronous read-write lock,
so there is one editing path. Kept from [ADR-0030](ADR-0030-platform-text-input-ime-capability.md):
§3 (one active client per presentation, token-guarded detach), §5 (composing state is one value),
§6 (rendering the composing region, including its single-line limit) and §7 (the candidate window
follows the composition, one rect). Kept from
[ADR-0069](ADR-0069-a-keydown-produces-one-semantic-event.md): a key press produces one semantic
event, and the input method is a route, not a second producer.

§4's rule that `Disabled` mid-composition strips the uncommitted text is winit behaviour. Under TSF the
equivalent is the text service ending its composition with a final edit (commit or removal)
through the store; the store never strips on its own.

### 3. Windows uses TSF and UIA text patterns from the first line

The Win32 backend implements `ITextStoreACP` over the store and exposes the same store to
assistive technology through UI Automation `TextPattern` and `ValuePattern`. IMM32 is not a
fallback path. Mock `ITextStoreACP` unit tests (the shape Chromium's TSF bridge tests use) land
with the backend.

### 4. A public, headless conformance kit

`flui-testing` ships a versioned conformance kit that any crate runs against its own text
widget. It behaves like a TSF-style client: it requests locks asynchronously, edits from the IME
side, and checks reads against edits. It covers surrogate pairs, grapheme clusters that span
several UTF-16 units, composing ranges, async grants that arrive after a frame, refused
synchronous locks, and rect/point queries against layout. The built-in text field passes the same
kit. This kit, not a live session, is the H0 gate for IME.

### 5. Live evidence stays in exit B1

The live check — a Japanese IME composing and converting (for example "toukyou" to 東京), with
Narrator reading the label and the field — belongs to roadmap exit B1 and is recorded as platform
evidence, not run as a merge gate: a human session cannot gate a merge. Until it is recorded, the
Windows row in `docs/BETA.md` says "no IME". An automated Windows IME device check
(`cargo xtask device windows-ime`: romaji through virtual keys with the IME open, read back
through UIA `TextPattern`) runs on a maintainer host or VM, because the hosted runner's language
set is unverified.

## Alternatives considered

- **Keep ADR-0030's push model and bridge in each backend.** Rejected: the bridge would have to
  invent the document it is asked about. macOS already shows the result — `nil` substrings and
  `NSNotFound` indices (§Context).
- **A read-only store.** Rejected by verification: TSF edits the document and may take its lock
  asynchronously. A kit that exercised only reads would pass on a contract Windows cannot use.
- **IMM32 on Windows.** Rejected: it has no document model for reconversion or dictation, and
  Flutter's Windows embedder on IMM32 carries open issues it cannot close there (#182876,
  #191196, as cited by the panel).
- **winit's IME on Windows.** Rejected for the same reason; winit's vocabulary is the push model
  this ADR replaces.
- **Make the live Windows session an H0 gate.** Rejected: there is no Win32 IME code to gate
  yet, and AGENTS.md does not allow a human session on the merge path.

## Consequences

- `PlatformTextInput` grows from two setters into the store contract; it moves with
  [ADR-0082](ADR-0082-platform-api-contract-crate.md), which also removes the
  `flui-interaction → flui-platform` import at `crates/flui-interaction/src/text_input.rs:27`.
- `TextEditingController`'s locking buffer is replaced by an owner-thread editing state; widget
  code that reads the controller from another thread breaks, which is the intent.
- UTF-16 conversion becomes one tested helper. The macOS backend's own converter is replaced by
  it, and its `nil`/`NSNotFound` answers become real answers.
- Multiline editing still needs [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §6's
  single-line limit revisited; the store contract is line-agnostic, the rendering is not.
- Android IME may need GameActivity rather than the current `native-activity`
  (`crates/flui-platform/Cargo.toml:162`); that is a hypothesis to test when the Android backend
  implements the store, not a decision here.
- Web has no IME today; a hidden-input bridge would implement this same store.

## Verification

None of these exist yet.

- The conformance kit (§4) in `flui-testing`, run in CI against the built-in text field and a
  test-only minimal field.
- Mock `ITextStoreACP` unit tests in the Win32 backend (§3), clippy-only in CI like the rest of
  Win32 until a Windows test job runs them.
- A winit backend test that a `Preedit`/`Commit` sequence produces the same store edits as the
  kit's synchronous-lock script (§2).
- `cargo xtask device windows-ime` on a host with ja-JP installed, and the recorded B1 evidence
  (§5).
