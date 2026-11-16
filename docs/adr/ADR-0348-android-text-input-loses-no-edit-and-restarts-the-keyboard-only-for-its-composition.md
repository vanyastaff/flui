# ADR-0348: Android's Java view serves the input method from a mirror that loses no edit, and restarts the keyboard only for its composition

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0247](ADR-0247-android-java-view.md) whole: what ADR-0255 left of its
  items 1 and 2, and its items 3 to 8 and 10, are restated here, item 6 rewritten (the rest of
  items 1 and 2, and item 9, were replaced by [ADR-0255](ADR-0255-mobile-and-web-hosts.md)); the
  Android row of
  [ADR-0090](ADR-0090-ime-pull-text-store-contract.md)'s platform mapping, which §3 states
- **Related:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) (how the dex reaches the APK and the
  Activity), [ADR-0313](ADR-0313-a-generational-slot-never-mints-an-id-twice.md) (identities come
  from `IdMint`), [ADR-0135](ADR-0135-win32-text-services-hold-the-text-store-on-the-owner-thread.md)
  (the `TextStoreHost` a window offers), [ADR-0211](ADR-0211-unified-drag-and-drop.md) (the drag
  contract §6 serves), [ADR-0340](ADR-0340-the-android-activity-forwards-what-has-a-consumer-and-routes-results-by-token-and-epoch.md)
  (the soft input mode and the keyboard inset)

## Context and problem statement

Input methods, drag and drop, pasted content and stylus handwriting reach an Android app only
through a Java `View` (`onCreateInputConnection`, `onDragEvent`, `OnReceiveContentListener`). FLUI
renders a `NativeActivity`'s surface and reads input from the native queue, so it adds one Java
view, `FluiView`, whose `InputConnection` answers the input method on the Java main thread from a
mirror of the focused field, while the field's `TextStore` lives on FLUI's owner thread.
ADR-0247 decided that view; its input method rule lost edits:

- the owner recorded each snapshot's `u32` generation (minted by `wrapping_add`) on its own thread
  and posted the snapshot, which Java took later; an edit the keyboard made in between was
  reported against the old generation and dropped, and a selection-only update left the character
  in the keyboard and out of the field;
- an edit inside a batch the keyboard had not ended was not reported yet, so a snapshot taken in
  the middle of the batch overwrote it with nothing to show it was lost;
- every text change the application made restarted the input method, which drops a composition
  and the suggestions, and a keyboard put away with Back stayed away on a tap on the field.

What rule keeps the two threads one document, loses no edit, and restarts the keyboard only when
it must?

## Decision drivers

- The owner never waits for the Java main thread, and the input method never waits for the owner
  (it gives up after 2 s).
- Nothing the user typed is lost while the field keeps its text; the application's text is the
  authority when both changed it at once.
- A keyboard restart is visible (composition, suggestions, shift state); Flutter 3.50 restarts only
  when the composed text changed (`TextInputPlugin.composingChanged`).
- Identities are minted, never wrapped (ADR-0313).
- A host decides only what its platform needs; a gesture is recognised once, by the framework's
  arena, after hit-testing.

## Considered options

1. Keep the generation, and restart the input method on a report against an older generation.
2. Number both sides: publications minted by the owner, reports minted by the mirror; the view
   reports what the keyboard did before it takes a snapshot, decides against it, and the mirror
   follows only once the view's editor took it.
3. Rebase a raced edit onto the application's new text.

## Decision

Chosen option: 2, because it loses no edit in a selection race, keeps the application
authoritative when both changed the text, and its rule is a pure function tested on every host.

1. **FLUI ships Java code**, under `crates/flui-native/android/java` in the package
   `dev.flui.platform`, compiled to one checked-in dex, `crates/flui-native/android/classes.dex`,
   which reaches the APK as ADR-0255 decides; `classes.dex.toml` records the digests of the
   sources and the dex and the tools that compiled them, `cargo xtask android-dex` checks both
   digests (no JDK needed), and the `Android dex` workflow rebuilds them on a GitHub runner, so
   building or changing FLUI for Android needs no local JDK. At platform start the backend posts
   the view's installation to the Java main thread. **One `FluiView` per Activity**, laid over the whole window as the decor view's first child,
   focusable and drawing nothing, so its pixels are the surface's (the caret and character boxes,
   a drop's point and the semantics bounds need no offset). It takes no touch FLUI handles. It is
   the only Java input method target, drop target, drag source, receiver of pasted and keyboard
   content, and handwriting editor. Its class is loaded through the Activity's class loader
   (`loadClass`, never `FindClass`), its native methods bound with `RegisterNatives` after the
   bridge protocol (`FluiHost.PROTOCOL`, `owner::android::java_host::PROTOCOL`) matched; it holds a
   handle to the native bridge, which the platform clears on the Java main thread before it frees
   the bridge.
2. **Threads.** Owner-to-view calls are posted through `JavaView::post`/`post_or`, inside the
   panic boundary; the view's native methods only queue data for the owner and wake it. What the
   system needs answered at once (the input method's reads, a drag's acceptance, a drop's result)
   comes from state the owner published earlier.
3. **The input method's mirror.** `text::android::ime::Ledger` (owner thread, one per view,
   shared by the windows over it) mints a `Publication(NonZeroU64)` from an `IdMint` for each
   snapshot of the focused store, keeps the last publication that replaced the text, the last
   report it replayed or dropped (`seen`), and the composition as it last published or replayed
   it. `text::android::ime::Mirror` (Java main thread) mints a `ReportId(NonZeroU64)` for each
   report of the view and keeps the last report made on the text it holds. Java carries neither
   number. A snapshot reaches the view in one posted call, in this order:
   1. the view reports what the input method did since its last report, an edit inside a batch
      it has not ended included (`FluiView.reportPending`); the batch's later edits are reported
      from there when it ends;
   2. the mirror decides (`Mirror::decide`): a focus or a resynchronisation replaces the editor
      and restarts the input method; a snapshot that has not seen a report the view made on the
      text it holds raced the input method: if it only moved the selection, the view keeps its
      text (`Install::Keep`) and the owner still replays the edit; if it changed the text, the view
      takes it and restarts, and `Ledger::replay` drops the edit made before it
      (`Replayed::Stale`); otherwise the view takes it and restarts the input method only when it
      ends the composition or changes the composed text, and is told the rest (`updateSelection`,
      and `updateExtractedText` to an input method that monitors it);
   3. the view's editor takes it (`focusEditor`, `pushState`), and only then the mirror
      (`Mirror::take`). A call that fails leaves the mirror on what the view held, and the window's
      field publishes the store once more as a resynchronisation; that one's failure is logged.

   A selection change that also moved or ended the composition is the application's edit of what
   the input method composes: the ledger publishes it as a text change. Only a step the store
   refuses asks the owner for a new snapshot. A protected field is mirrored as one bullet per code
   unit. A key the native queue delivers while the view reports a composition is the input
   method's, marked `ImeComposition::Active`. The editor action arrives as Enter (Next and
   Previous as Tab and Shift+Tab), after the owner commits a composition still open. Offsets cross
   JNI in UTF-16 code units; the store contract (ADR-0090) is unchanged.
4. **The keyboard on a tap.** `TextStoreHost::show_keyboard(store)` is the seam: a field whose tap
   the gesture arena gave it asks for the soft keyboard through its `TextInputHandle`
   (`show_keyboard(token)`), queued with the presentation's other host operations. Android's host
   calls `FluiView.showKeyboard` (`showSoftInput`) for the focused store, as a tap on an
   `EditText` does; the other hosts' platforms show it by themselves or have none, and do nothing.
5. **Cut, Copy and extracted text.** `performContextMenuAction` copies the selection to the
   `ClipboardManager`, and Cut also deletes it as one input method edit; a protected field refuses
   both. `getExtractedText` answers at most `hintMaxChars` characters around the selection, never
   splitting a surrogate pair, and keeps a `GET_EXTRACTED_TEXT_MONITOR` request of the current
   connection until it is replaced, sending the text on every change.
6. **Drag and drop.** `onDragEvent` is the target: an offer is minted from the `ClipDescription`
   when a drag enters, and the drop is answered from the target's cached feedback. The view holds
   the drop's `ClipData` and its URI permissions under the offer's key before it asks for the
   drop, so nothing of the clip crosses JNI then; a request reads one representation from the held
   clip under its limits, the length counted before anything is copied: the text, HTML and URI
   list at once, a URI's bytes through the content resolver on a thread of their own, at most four
   at once, each answered within five seconds of being asked, its wait for a place included; past
   them an open it started is cancelled and its stream closed by a cancel worker, off the owner
   thread; at most 64 more wait, and a request past them is refused `Busy`. Concluding the drop
   lets go of its clip and permissions, and of nothing else; a refused drop, or the view leaving
   the window, lets go of them at once, and a read whose offer ended answers `SourceGone`. A drag
   from another app permits a copy; one this process started permits what its source advertised.
   `startDragAndDrop` (API 24) is the source when a drag leaves its window
   (`OsDragStart::OnLeavingWindow`); Android takes the whole clip at the start, `file:` URIs and
   bytes stay out of it, and the drag is global. Below API 24, or without the view,
   `outgoing_drag()` is `Unavailable`. `OnReceiveContentListener` (API 31) receives only pasted and
   keyboard text, which reaches the focused field as one input method edit; other content is
   handed back.
7. **Handwriting.** Android's automatic initiation sees only the stylus events native code leaves
   unhandled, so the view's automatic handwriting is off, and a pen tip that touches down inside
   the focused field's bounds (`TextStoreRead::document_bounds`, the field's box) asks
   `startStylusHandwriting` (API 33).
8. **No AndroidX and no GameActivity dependency.**

### Consequences

- Good, because a character typed while the application moves the caret reaches the field, and
  an edit inside an open batch is never overwritten by a snapshot.
- Good, because an application edit made with no composition open, or outside the composition, no
  longer restarts the keyboard, and the composition survives it.
- Good, because publications and reports cannot wrap or collide across windows.
- Good, because a keyboard put away with Back returns on the field's own tap, and only on it: a
  drag from the field, a scroll, or a tap on something drawn over it asks nothing.
- Bad, because a selection change the application made while the keyboard typed is replaced by
  the keyboard's selection.
- Bad, because a character typed while the application rewrote the text is dropped (the keyboard
  restarts on the application's text, so the two still agree). A formatter that rewrites the text
  in answer to each replayed edit makes this the common case under fast typing, and one that
  rewrites the composed word (a keyboard that composes every Latin word, as Gboard does) still
  restarts the keyboard on every key
  ([gaps](../gaps.md#an-android-formatter-loses-keys-typed-faster-than-it-answers)).

### Confirmation

- `android_ime_adapter` (`crates/flui-native/src/text/android/ime/tests.rs`):
  `a_selection_snapshot_that_raced_an_edit_keeps_it`,
  `a_selection_snapshot_that_saw_every_edit_is_taken`,
  `a_text_snapshot_that_raced_an_edit_wins_and_restarts`,
  `a_report_on_replaced_text_races_no_later_snapshot`,
  `an_application_composition_end_wins_a_race`,
  `a_snapshot_the_view_did_not_take_leaves_the_mirror`,
  `the_input_method_restarts_only_when_its_composition_is_touched`,
  `a_composition_the_owner_ends_restarts_the_input_method`,
  `an_edit_for_the_field_focus_left_is_dropped`, `a_refused_step_asks_for_a_new_snapshot`, with
  the surrogate, protected field, key rule, `EditorInfo`, editor action, handwriting and cursor
  anchor cases; the Android drag model in `src/drag/android/clip/tests.rs`
  (`android_drag_mapping`).
- `flui-widgets`: `a_tap_on_the_focused_field_asks_for_the_keyboard` and
  `a_fields_bounds_are_its_box`.
- The Java side: `cargo xtask android-dex` (the dex is built from the sources) and the protocol
  handshake. Device runs are rows of `crates/flui-native/platform-parity.toml`.

## Pros and cons of the options

### 1. Restart on a stale report

- Good, because it is the smallest change.
- Bad, because the restart drops the character the owner never replayed, restarts the keyboard
  in the most common race, and still misses an edit inside an open batch.

### 2. Two ledgers, report before install

- Good, because no edit is lost in a selection race, and the owner stays the authority on text.
- Bad, because the snapshot's install runs on the Java main thread, inside the posted call, and
  calls back into the view's native method for the pending report.

### 3. Rebase the raced edit

- Good, because neither side loses anything.
- Bad, because rebasing a composition onto an application's rewrite has no defined result, and the
  keyboard's own copy would still need a restart.

## More information

- Flutter 3.50 `TextInputPlugin.setTextInputEditingState` (restart only on `composingChanged`) and
  `InputConnectionAdaptor.performContextMenuAction` (select all, cut, copy, paste); Flutter does
  not honour `hintMaxChars`.
- `InputConnection.getExtractedText`, `InputMethodManager.updateExtractedText` and
  `InputMethodManager.showSoftInput` (developer.android.com reference).
