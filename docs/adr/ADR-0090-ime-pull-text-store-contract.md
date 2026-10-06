# ADR-0090: IME talks to a pull text-store contract with edits and asynchronous locks

- **Status:** Accepted in part (2026-09-26): §1, §2 and §4 are implemented (the migration
  plan's IME text-store row); §3 waits for the Win32 TSF backend, §5 for exit B1. §2 was amended
  on acceptance: the projection takes an asynchronous lock, not a synchronous one (see §2).
- **Amendment (proposed 2026-10-05):** "Committed text, settling and resolving a composition"
  below amends §1, §2, §3, the interim implementation and the verification. Items 1–3 are
  implemented; items 4–7 land with the Win32 text-services host, the widget's composition
  handling and the runtime's anchor debt, and are not yet in the code.
- **Date:** 2026-09-25
- **Supersedes in part:** [ADR-0030](ADR-0030-platform-text-input-ime-capability.md) §1
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
(`crates/flui-platform-api/src/ime.rs`) and the framework pushes back two things,
`set_ime_allowed` and `set_ime_cursor_area` (`crates/flui-platform-api/src/text_input.rs`,
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
  `Arc<Mutex<…>>` does not survive into this surface. Until then the built-in field implements
  it over today's state ("Interim implementation" below).

The surface is `flui_platform_api::text_store`:

- `TextStore` (owner thread, shared as `Rc<dyn TextStore>`, not `Send`): `status`,
  `request_lock`, `run_deferred_grants`, `set_commit_gate`, `set_observer`. `TextStoreObserver`
  is the platform's sink: `text_changed(TextChange)`, `selection_changed`, `layout_changed`,
  `status_changed`, never called while a lock is held or the store's commit gate is shut (what
  waited is sent by the next `run_deferred_grants` at the latest), so a sink may answer a
  notification with a synchronous lock.
- A lock is a closure: `LockGrant::Read(FnOnce(&dyn TextStoreRead))` or
  `LockGrant::ReadWrite(FnOnce(&mut dyn TextStoreEdit))`, requested with `LockTiming::{Sync,
  Async}`, answered `LockOutcome::{Granted, Deferred}` or `TextStoreError::SyncLockUnavailable`
  (`TS_E_SYNCHRONOUS`) / `DeferredQueueFull`. Editing under a read lock does not compile (a
  `compile_fail` doctest). `LockArbiter` is the one lock state machine every store embeds.
- Reads (`TextStoreRead`): `document_len`, `text`, `selection` (`Selection { anchor, active }`),
  `composition` (`Composition { range, hides_caret }`), `rect_for_range` (`RangeRect`),
  `document_bounds`, `index_at_point` (`PointMode::{Exact, Nearest}`). Edits
  (`TextStoreEdit`): `replace` and `insert_at_selection` (each returning a `TS_TEXTCHANGE`-shaped
  `TextChange`), `set_selection`, `set_composition`.
- Offsets are `Utf16Offset`/`Utf16Range`; `text_store::utf16` is the one converter, and an
  offset past the end or inside a surrogate pair is an `OffsetError`, never a clamp.
- "Commits closed" is the realm's frame transaction, and a type a store cannot skip: each
  presentation's `TextInputOwner` holds a `CommitGate` and installs it into every store it
  attaches (`TextStore::set_commit_gate`), and the store's `LockArbiter` reads it on every
  request, so a store has no transaction flag of its own. `flui-app`'s `UiRealm::drive_frame`
  shuts every presentation's gate for the whole `drive_frame_with_lane` call (begin frame
  through post-frame callbacks) and runs the commit anchor (`run_deferred_grants`) after it
  returns, with the scheduler `Idle`, per ADR-0027 §3. A store replaced or detached inside the
  transaction keeps its queued grants for that anchor.
- One read-write session is one change notification to the widget. There is no undo stack in
  the framework yet, so "one undo step" has nothing to apply to; it binds the first undo
  implementation. As amended (item 1, 2 below), the owner of the field — `on_changed`, a form
  field — hears of a session only when it changed the committed text, and only after the
  session's lock is released.

### 2. The push vocabulary becomes a projection

`ImeEvent` stays as the adapter for push-model sources (winit today).
`flui_platform_api::text_store::project_ime_event` translates `Preedit`/`Commit`/`Disabled` into
store edits under a read-write lock, and `TextInputOwner::dispatch` calls it for the active
client, so there is one editing path. The lock is **asynchronous**, not synchronous as first
written: a push event that arrives while the store cannot grant a lock (inside a frame
transaction, or inside another session) is then applied in order at the next commit anchor
instead of being dropped. A preedit's composition carries `hides_caret` for winit's
`cursor: None`. Kept from [ADR-0030](ADR-0030-platform-text-input-ime-capability.md):
§3 (one active client per presentation, token-guarded detach), §5 (composing state is one value),
§6 (rendering the composing region, including its single-line limit) and §7 (the candidate window
follows the composition, one rect). Kept from
[ADR-0069](ADR-0069-a-keydown-produces-one-semantic-event.md): a key press produces one semantic
event, and the input method is a route, not a second producer.

§4's rule that `Disabled` mid-composition strips the uncommitted text is winit behaviour. Under TSF the
equivalent is the text service ending its composition with a final edit (commit or removal)
through the store; the store never strips on its own. The winit strip therefore lives in the
projection, as an explicit edit.

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

The kit is `flui_testing::text_store_kit`, versioned by `KIT_VERSION` (2 since the amendment
below; version 1's cases are unchanged): a field supplies a `TextStoreFixture` (its store, a
reset, an app-side edit, a commit anchor, its own change count, a hook run inside its owner
notification, and its capabilities) and calls `assert_conforms`. The kit holds frame transactions itself,
through a `CommitGate` it installs with `set_commit_gate`, so no fixture can stand in for a
store that ignores its gate. Each `Case` names the
version that added it, so a pinned version never grows. `flui_platform_api::text_store::
InMemoryTextStore` is the test-only minimal field, with `text_store_kit::InMemoryFixture` as the
worked example; the kit's own tests wrap it with one fault each and assert the kit catches every
one.

## Amendment: committed text, settling and resolving a composition

Proposed 2026-10-05. The store contract above let a store run its owner's code inside a
platform session: `EditableText` wrote a session back and called `on_changed` while the lock
was still held, so owner code that asked for a lock was refused, and it compared the whole
text, so `on_changed`, autovalidation and a saved draft saw text the input method was still
composing. The amendment fixes what the owner sees and when:

1. **The owner works with the committed text**: the document with its composing range replaced
   by the text that occupied that range before the composition began — nothing for a new
   preedit, the original words when an input method reconverts text the user already
   committed. The field's owner is told only when the committed text changed; a session that
   only composed, cancelled a composition, or only marked existing text as a composition tells
   it nothing. A store keeps that origin beside its composing range and accounts for a
   session's edits with `text_store::CompositionLedger`, which knows what every character
   stands for: a committed character stands for itself, a character the session inserted
   stands for nothing, and committed text an edit removed from a composition (or from a
   composition an edit cleared) is kept where it was removed. The committed text is every
   visible character, with the composition replaced by what its characters and removals
   stand for. So text the session inserted and then marked is a new preedit, text it found and
   marked is a reconversion, and a composition an edit cleared keeps its origin until it is
   marked again; the text one edit inserted, with what it removed, is one replacement, and
   replacements that rewrote each other's text are one. A session opens with its composition
   standing for itself when the origin is its visible text, as new preedit when the origin is
   empty, and otherwise as one replacement of the origin by the visible text. **Narrowing:**
   when a mark leaves part of a composition (or of a composition an edit cleared) outside the
   new range, that part commits as the user sees it: its characters stand for themselves and its
   removals are dropped. The rest keeps what its own characters and removals stand for, unless a
   replacement with non-empty removed text lies on both sides of the new range; its removed text
   cannot be divided, so the rest of that region then stands for its own visible text, and the
   committed text over it is what the user sees. A replacement's removed text is therefore never
   counted beside any of its own inserted text (no "abcdefDEF" from narrowing a conversion of
   "abcdef" to "ABC"). Text that never stood for anything (new preedit) commits as shown beside
   the origin the rest keeps. `flui-platform-api`'s `the_ledger_follows_the_reference` checks the
   ledger against a reference model over random edit, mark and session sequences, with the
   cases that model found (`composition_ledger_named_cases`). A text form field validates and
   saves the committed text (`TextEditingController::committed_text`). Kit version 2 pins it
   (`composition_only_sessions_do_not_notify_the_owner`,
   `reconverting_committed_text_notifies_only_on_commit`).
2. **The owner hears after the lock is released, before the next grant.** `LockArbiter::request`
   and `run_deferred` take a second function, `settle`, called after each grant has released its
   lock and before the next queued grant runs. A store commits the session's result inside the
   grant and records the notification it owes; it delivers it in `settle`, then the observer
   notifications that waited for the lock. A synchronous lock requested from the owner's code is
   granted (after the grants queued ahead of it), so an `on_changed` that edits the field lands
   after the session and reaches the platform as an application edit. A panic in `settle` does
   not undo the grant, which already ran: the arbiter catches it, hands the first payload to the
   store's `CommitGate` (`defer_failure`; later ones are retained per ADR-0127), and the queue
   keeps running; the gate's owner takes it (`take_failure`) and reports it at its next turn
   through the realm's panic report: `TextInputOwner::dispatch` and `run_deferred_grants` take it
   once they return and resume it inside their containment (the first failure stays
   authoritative), so it reaches the realm's report from the dispatch or anchor that follows. A
   store also tells its observer before it resumes an owner panic, so a grant queued behind it
   never runs before the platform hears of an edit the owner made. A store whose owner never
   installed a gate resumes the panic once the lock is released. Under TSF the grant's `RequestLock` still returns `S_OK` with
   `*phrSession` from `OnLockGranted`. Kit version 2 pins it
   (`owner_notification_runs_after_release`).
3. **A platform session the application overtook is dropped.** The controller counts its
   changes (a generation); a read-write session records the count when it opens, and if the
   application changed the field before the session writes back (a nested modal loop on the
   owner thread, an async task), the session's result is discarded, the application's edit
   stays, and the platform hears of it through the observer once the lock is released, so the
   text service re-reads the document. There is no merge: nothing gives a base for a three-way
   comparison.
4. **A field losing its input resolves its composition by committing it.** Pointer-down in the
   presentation (inside the composing field too), an accepted close request, the end of the
   session, blur, paste and undo, and unmount commit the visible composition before the event's
   handlers run. A platform that cannot terminate its composition (a refused lock) reports
   `Abandoned`; the field then clears the composing range itself, keeping the text. Unmount
   commits in place and calls no `on_changed`; only an input-method edit not yet applied when the
   store detaches is lost. This supersedes ADR-0030 §6's "blur detaches the IME client but does
   not end the composition". Until this item lands, a form reset during a composition leaves the
   preedit in the field (the reset writes only the committed text it compares against).
5. **A commit anchor skipped by an unwound frame is a debt of the realm**, paid at its next owner
   turn (a drained inbox, a background pump or a pump), not at the next frame, and a failed wake
   does not clear it.
6. **The commit gate is open only while the realm is in its slot and not driving a frame.** A
   platform entry while the realm is checked out (a frame, the end of a session) is refused a
   synchronous lock and queues an asynchronous one; no user code runs.
7. **§3's platform mapping follows a pull host.** The Win32 backend receives the focused store
   from the presentation through an owner-thread host rather than reading
   `TextInputOwner::active_store`, which is removed.
8. **Owner code runs inside one containment.** Every point where the arbiter, a store or the
   presentation runs code it does not control — a grant's body, a settle, `on_changed`, the
   controller's listeners, an owner listener, the observer (the flush after a request's grants
   included, which yields to a failure their settle parked), `on_session_start`, the projection,
   a store installing a gate, diagnostics (a `tracing` subscriber is user code), and the
   destruction of any snapshot, refused or queued grant, replaced value, client or store — goes
   through `text_store::OwnerCalls`, whose module doc lists them; a gate whose last clone goes
   with a failure no owner took retains it. What the code is owed (obligations with their values,
   the gate a failure belongs to) is read before it runs, never after, since it may reenter,
   settle a nested session or move the store to another presentation. Each call is contained and
   the first failure in time is authoritative: a settle parks its failure in the admitting gate
   the moment it is caught, ahead of any session the owner's later code opens; a failure a call
   parked in a gate came before that call's own unwind, so it is taken first, while one the gate
   already held waits for its owner's turn (a dispatch, an anchor, a close); a later one is
   retained (ADR-0127). Work stays deliverable: an asynchronous request behind a failing queued
   grant is queued, and a synchronous one, or one refused because the flush before it failed, is
   retained rather than destroyed during the unwind. A snapshot retires inside the scope: dropped
   while the scope is healthy, retained once it has failed or while the thread unwinds. Nested
   work runs in the caller's scope, so it sees the caller's failure. A session the field was
   unmounted under is not written back. One matrix pins every point
   (`owner_code_is_contained_at_every_point`).

## Interim implementation

`EditableText` implements `TextStore` over `TextEditingController` and `RenderEditable`
(`crates/flui-widgets/src/text/text_store.rs`) until ADR-0092's `TextFieldState` replaces both
behind the same trait. The controller's `Arc<Mutex<…>>` survives behind the trait for now: the
store reads a snapshot of it when a lock opens and writes a read-write session back once,
checking the controller's generation in the same critical section (amendment item 3); its
listeners and `on_changed` run in `settle` (item 2). There is no undo stack. Converting offsets walks the text (O(n)), fine for single-line fields; multiline
needs a cached index. A push event queued behind a frame lands before a later key press (the key
handler runs queued grants first), but a programmatic `set_text` made while a grant is queued goes
ahead of it. App edits reach the observer at the next frame, key press or lock request, since
controller listeners are `Send + Sync` and the store is not.

No platform backend holds a store yet: `PlatformTextInput` is `Send + Sync` and the store is an
owner-thread `Rc`, so the pull connection waits for ADR-0082's owner-thread capability split. Until
then the production caller is the push projection. `TextInputOwner::active_store` is
`#[doc(hidden)]` until the Win32 TSF backend (§3) reads it, and the observer,
`rect_for_range`, `index_at_point` and `document_bounds` have no production caller before then
either.

## Divergences

- **Platform selection is exact.** A selection the platform sets is kept at any scalar
  boundary, including inside a grapheme cluster (TSF and AppKit address scalars); a tap or an
  arrow key still snaps to graphemes, as the controller always has. The platform half is pinned
  by the kit's `selection_inside_a_grapheme_is_kept_exactly` (`flui-widgets/ARCHITECTURE.md`
  Mapping decision #35); the tap and arrow-key snapping is **Unasserted:** no test pins this.
- **Obscured means protected.** An obscured field reports `status().protected`: text reads
  return `Protected`, while edits, selection and geometry (through the mask) work. Pinned by
  `obscured_editable_text_conforms_to_kit_v1`.
- **Composition tracking.** An edit that does not touch the composition shifts it; one that
  overlaps it, or inserts strictly inside it, clears it; no edit creates or extends one. Pinned
  by the kit's `edit_shifts_an_untouched_composition_and_clears_an_overlapped_one`.

## Platform mapping

No backend code exists for any of these yet; this is the shape each backend implements.

| Platform | Mapping |
|---|---|
| Windows, TSF `ITextStoreACP` (ACP offsets are UTF-16) | `RequestLock(TS_LF_READ\|TS_LF_READWRITE [\|TS_LF_SYNC])` → `LockGrant::{Read, ReadWrite}` with `LockTiming::{Sync, Async}`; `Granted` sets `*phrSession` to `OnLockGranted`'s HRESULT (the grant calls it), `Deferred` returns `TS_S_ASYNC`, `SyncLockUnavailable` returns `TS_E_SYNCHRONOUS`. `GetStatus` → `status()` (protected → an `IS_PASSWORD` input scope); `GetEndACP` → `document_len`; `GetText` → `text`, one `TS_RT_PLAIN` run; `GetSelection` → `selection()` (`TS_AE_START` when active < anchor, `fInterimChar` false). `SetSelection` → `set_selection`; `SetText` → `replace`; `InsertTextAtSelection` → `insert_at_selection` (`TF_IAS_QUERYONLY` answered from `selection()`); `QueryInsert` returns the range as given. `GetTextExt` → `rect_for_range` in screen physical pixels (`pfClipped` from `clipped`, `NoLayout` → `TS_E_NOLAYOUT`); `GetScreenExt` → `document_bounds`; `GetACPFromPoint` → `index_at_point` (`GXFPF_NEAREST` → `Nearest`, else `Exact`; `PointOutside` → `TS_E_INVALIDPOINT`). `AdviseSink`/`UnadviseSink` → `set_observer` (`OnTextChange`, `OnSelectionChange`, `OnLayoutChange(TS_LC_CHANGE)`, `OnStatusChange`). `ITfContextOwnerCompositionSink` `OnStart`/`OnUpdate`/`OnEndComposition` → `set_composition` inside the text service's read-write session. Embedded-object verbs return `E_NOTIMPL`; no attributes are reported. Errors: `Offset` → `TS_E_INVALIDPOS`, `Detached` → `E_UNEXPECTED`, `DeferredQueueFull` → `E_FAIL`. A panic in the owner's notification after a grant (amendment item 2) does not change the answer: `S_OK`, with `*phrSession` from `OnLockGranted`. No field is read-only yet, so `TS_SD_READONLY` and `TS_E_READONLY` have no source; the status flag and the error are added with the first read-only field. UIA `TextPattern`/`ValuePattern` read the same store under sync read locks. |
| macOS, `NSTextInputClient` (`NSRange` is UTF-16) | Every call is synchronous and uses `Sync`; a refused lock returns today's empty answers (`nil`, `{NSNotFound, 0}`), never blocks. `markedRange`/`hasMarkedText` → `composition`; `selectedRange` → `selection().range()`; `attributedSubstringForProposedRange:actualRange:` → `text` over the clamped range, returned as `actualRange`; `firstRectForCharacterRange:actualRange:` → `rect_for_range` through `convertRectToScreen` with a flipped y (single-line today); `characterIndexForPoint:` → `index_at_point(Nearest)` or `NSNotFound`. `setMarkedText:selectedRange:replacementRange:` → `replace` of the replacement range, else the composition, else the selection, then `set_composition(hides_caret: false)` and `set_selection`; `insertText:replacementRange:` → `replace` and clear the composition; `unmarkText` → clear the composition keeping the text (unlike winit's `Disabled`). `layout_changed` → `invalidateCharacterCoordinates`; an app edit overlapping the composition → `discardMarkedText`. The backend's own UTF-16 converter is replaced by `text_store::utf16`. |
| Linux, winit (X11 XIM, Wayland) | Push-only: through `project_ime_event`. The candidate area comes from `rect_for_range` over the composition or the caret. |
| Linux, native Wayland `text-input-v3` (if a backend bypasses winit) | `set_surrounding_text` takes UTF-8 byte cursor and anchor (at most 4000 bytes), read under a sync read lock; `delete_surrounding_text` → `replace`; `preedit_string` byte cursors are handled as the projection handles them; `done(serial)` is one read-write lock. |
| Linux, IBus over D-Bus; AT-SPI | IBus counts Unicode scalars, so a scalar↔UTF-16 helper is needed then. AT-SPI `Text` reads through the store. |
| Android | `InputConnection` maps onto the same verbs. |
| Web | A hidden-input bridge implements the same store. |

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

In place:

- §1: `flui-platform-api` `text_store::utf16::tests` (surrogates, combining marks, ZWJ, flags,
  round trips, refusals), `text_store::lock::tests` (a sync request inside a session is refused,
  deferred grants run FIFO, a full queue refuses, and `a_panicking_grant_releases_the_lock`), and the
  `compile_fail` doctest on `text_store::lock`.
- §2: `text_store::projection::tests` (preedit, cursor mapping and clamping, `cursor: None`,
  empty preedit with and without a composition, X11 start/end, commit, direct commit,
  `Disabled`, and a push event while commits are closed applying in order at the next anchor);
  `flui-widgets` `focus_gain_attaches_an_ime_client_and_routes_preedit_to_the_controller` (an
  IME event the realm receives is projected onto the attached store) and
  `typing_after_a_deferred_commit_lands_after_the_commit` (an attached store follows the
  owner's frame transaction); `flui-runtime`
  `a_text_store_lock_requested_during_a_frame_is_granted_after_the_drive_returns` (the grant
  runs in `Idle`); `flui-app` `runner_frame_ordering`'s scan that every runner drives frames
  through `UiRealm::drive_frame`; the existing `EditableText` IME tests, now through the projection.
- §4: `flui-testing` `tests/text_store_kit.rs` (`in_memory_store_conforms_to_kit_v1` and one
  `kit_fails_a_store_that_…` test per fault, including a store that ignores the commit gate it
  is handed and one that notifies inside a transaction); `flui-widgets` `tests/text_store_kit.rs`
  (`editable_text_conforms_to_kit_v1`, `obscured_editable_text_conforms_to_kit_v1`) and
  `tests/editable_text.rs`'s `text_store` module (offset mapping, one `on_changed` per session,
  exact platform selection, controller swap, `layout_changed`, `Detached` after dispose, a lock
  from a post-frame callback, typing after a deferred commit).
- Amendment items 1–3: `flui-platform-api` `tests/lock_gate.rs`'s
  `settling_runs_owner_code_outside_the_lock` (settle after each release and before the next
  grant, a panicking settle reaching the gate while the queue drains, and resuming when no owner
  installed a gate); kit version 2's `composition_over_a_selection_replaces_the_selection`,
  `composition_only_sessions_do_not_notify_the_owner` and
  `owner_notification_runs_after_release`, with `flui-testing`'s
  `conformance_fails_a_store_that_notifies_its_owner_of_a_composition`,
  `conformance_fails_a_store_that_notifies_its_owner_under_the_lock` and
  `a_pinned_conformance_version_does_not_grow`, and `reconverting_committed_text_notifies_only_on_commit`;
  `flui-widgets` `on_changed_runs_after_the_lock_is_released`,
  `an_app_edit_during_a_lock_is_not_overwritten`,
  `swapping_the_controller_during_a_grant_drops_the_session`,
  `a_panicking_on_changed_is_reported_once_and_the_field_keeps_working` (a failure reaching the
  next owner turn once, one raised by the dispatch, the first of two kept, and the observer
  told before the next grant) and `a_text_form_field_validates_and_saves_the_committed_text`.

Outstanding:

- Amendment items 4–7, with their tests: the Win32 text-services host and the removal of
  `active_store` (item 7), the widget's committing of a composition on blur, paste, undo and
  unmount (item 4), the runtime's pointer-down and close hooks, anchor debt and the gate's realm
  rule (items 4–6), and a failure the gate holds being reported through the anchor debt when no
  dispatch or anchor follows (items 2 and 5).

- Mock `ITextStoreACP` unit tests in the Win32 backend (§3), clippy-only in CI like the rest of
  Win32 until a Windows test job runs them.
- A winit backend test that a `Preedit`/`Commit` sequence produces the same store edits as the
  kit's `tsf_style_conversion_script` (§2). The projection's own tests pin each event; nothing
  yet compares the two paths end to end.
- `cargo xtask device windows-ime` on a host with ja-JP installed, and the recorded B1 evidence
  (§5).
