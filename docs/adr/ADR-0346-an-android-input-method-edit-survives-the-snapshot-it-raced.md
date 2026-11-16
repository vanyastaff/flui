# ADR-0346: An Android input method edit survives the snapshot it raced, and only a touched composition restarts the keyboard

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0247](ADR-0247-android-java-view.md) item 6 only (its "generation-tagged
  snapshot", "a text change restarts the input method", and "an edit made on a snapshot older than
  the latest one published is dropped … and the owner restarts the input method"); the Android row
  of [ADR-0090](ADR-0090-ime-pull-text-store-contract.md)'s platform mapping for the same clauses
  ("generation-tagged snapshot", "an edit on an older snapshot is dropped")
- **Related:** [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) (identities
  come from `IdMint`, never a wrapping counter)

## Context and problem statement

`FluiView`'s `InputConnection` answers the input method from a mirror of the focused field on the
Java main thread and never waits for the owner (ADR-0247 item 5). The owner published a snapshot
of the store tagged with a `u32` generation minted by `wrapping_add`, recorded the generation it
expected on the owner thread at once, and posted the snapshot to Java. Three defects followed:

- An edit the keyboard made between the owner's recording and Java's taking the snapshot carried
  the previous generation. `Mirror::report` returned `None` for it, the owner never heard of it,
  and a selection-only snapshot then only called `updateSelection`: the typed character stayed in
  the keyboard's text and never reached the field. The restart ADR-0247 item 6 promised for this
  case ran only on a path the filtered report never reached.
- Every text change the application made (a formatter, a key the keyboard sent as a key event)
  restarted the input method, which drops a CJK composition and the keyboard's suggestions on
  every keystroke.
- The generation wrapped, and one view's mirror outlives the windows over it, each of which
  started its own counter at zero.

What rule keeps the two threads one document without losing an edit and without restarting the
keyboard on edits that do not concern it?

## Decision drivers

- The frame path is synchronous and the owner never waits for the Java main thread (ADR-0247
  item 5).
- Nothing the user typed is lost while the field keeps its text.
- The application's text is the authority when the two changed the text at once.
- A keyboard restart is visible (composition, suggestions, shift state); Flutter's embedder
  restarts only when the composing text changed.
- Identities are minted, never wrapped (ADR-0313, AGENTS.md "Current forms").

## Considered options

1. Keep the generation, and restart the input method on a report against an older generation.
2. Number both sides: publications minted by the owner, reports minted by the mirror, the
   snapshot installed on the Java main thread inside the posted call, each snapshot carrying the
   last report the owner had replayed.
3. Rebase a raced edit onto the application's new text (operational transform).

## Decision

Chosen option: 2, because it loses no edit in the common race (a caret move), keeps the
application authoritative when both changed the text, and is decidable in a pure model.

1. **Two ledgers.** `text::android::ime::Ledger` (owner thread, one per view, shared by the
   windows over it) mints a `Publication(NonZeroU64)` from an `IdMint` for every snapshot, keeps
   the last publication that replaced the text, and records the last `ReportId` it replayed or
   dropped. `text::android::ime::Mirror` (Java main thread) mints a `ReportId(NonZeroU64)` for
   every report of the view and keeps the last report made since it last took a snapshot's text.
   Java carries neither number.
2. **The view takes a snapshot where it reports.** `Mirror::install` runs inside the posted call
   that hands the snapshot to `FluiView`, so it is ordered with the view's reports, and answers an
   `Install`:
   - a focus or a resynchronisation replaces the editor and restarts the input method;
   - a snapshot that has not seen a report the view made on the text it holds raced the input
     method: if it only moved the selection or the composition, the view keeps its text
     (`Install::Keep`) and the owner still replays the edit; if it changed the text, the view takes
     it and restarts, and `Ledger::replay` drops the edit made before it (`Replayed::Stale`);
   - otherwise the view takes the snapshot, and restarts the input method only when the snapshot
     ends its composition or changes the composed text. A composition only moved by text inserted
     before it, and any change outside it, is told (`updateSelection`, and `updateExtractedText`
     for an input method that monitors it).
3. **Only a refused step asks the owner for a new snapshot** (`Replayed::restarts_the_view`):
   every other correction is already in flight.
4. **A tap on the focused field shows the keyboard.** A contact that goes down and comes up inside
   the focused field (`ime::KeyboardTap`, read by the owner before the window takes the contact)
   calls `FluiView.showKeyboard`, so a keyboard put away with Back returns, as on a platform
   `EditText`.
5. **Cut, Copy and extracted text.** `performContextMenuAction` copies the selection to the
   `ClipboardManager`, and Cut also deletes it as one input method edit; a protected field refuses
   both. `getExtractedText` honours `hintMaxChars` with a window around the selection and keeps a
   `GET_EXTRACTED_TEXT_MONITOR` request until its connection is replaced, sending the text on every
   change.

The bridge protocol (`FluiHost.PROTOCOL`, `owner::android::java_host::PROTOCOL`) rises with the
changed native and Java signatures.

### Consequences

- Good, because a character typed while the application moves the caret reaches the field.
- Good, because a formatter that rewrites the text on every key no longer restarts the keyboard,
  and a composition survives an edit outside it.
- Good, because publications and reports cannot wrap or collide across windows.
- Bad, because a selection change the application made while the keyboard typed is replaced by
  the keyboard's selection.
- Bad, because a character typed while the application rewrote the text is dropped (the keyboard
  restarts on the application's text, so the two still agree).
- Neutral, because the store contract (ADR-0090) is unchanged.

### Confirmation

`android_ime_adapter` (`crates/flui-native/src/text/android/ime/tests.rs`):
`a_selection_snapshot_that_raced_an_edit_keeps_it`,
`a_selection_snapshot_that_saw_every_edit_is_taken`,
`a_text_snapshot_that_raced_an_edit_wins_and_restarts`,
`a_report_on_replaced_text_races_no_later_snapshot`,
`the_input_method_restarts_only_when_its_composition_is_touched`,
`a_composition_the_owner_ends_restarts_the_input_method`,
`an_edit_for_the_field_focus_left_is_dropped`, `a_refused_step_asks_for_a_new_snapshot` and
`a_tap_on_the_focused_field_shows_the_keyboard`. The Java side is held by `cargo xtask android-dex`
(the dex is rebuilt from the sources) and the protocol handshake; the device runs are rows of
`crates/flui-native/platform-parity.toml`.

## Pros and cons of the options

### 1. Restart on a stale report

- Good, because it is the smallest change.
- Bad, because the restart drops the typed character the owner never replayed, and restarts the
  keyboard in the most common race.
- Bad, because the wrapping generation and the per-window counters stay.

### 2. Two ledgers

- Good, because no edit is lost in a selection race and the owner stays the authority on text.
- Good, because the rule is a pure function of two numbers and a change kind, tested on every
  host.
- Bad, because the mirror's install moves onto the Java main thread, inside the posted call.

### 3. Rebase the raced edit

- Good, because neither side loses anything.
- Bad, because a rebase of a composition onto an application's rewrite has no defined result, and
  the keyboard's own copy would still need a restart.

## More information

- Flutter 3.50 `TextInputPlugin.setTextInputEditingState` restarts only when the composing region's
  text changed (`composingChanged`), and `InputConnectionAdaptor.performContextMenuAction` handles
  select all, cut, copy and paste; Flutter does not honour `hintMaxChars`.
- `InputConnection.getExtractedText` and `InputMethodManager.updateExtractedText`
  (developer.android.com reference).
