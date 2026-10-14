# ADR-0215: Input is recorded as the `HostInput` stream, and backends are certified against one conformance table

- **Status:** Accepted (2026-10-13). §1–§3, §5 and §6 are implemented; the native codec of §4 and
  §7 are pending (`docs/plans/specs/flui-interaction/completeness/tasks.md`). §5's `--keep-text`
  is the API's `Privacy::KeepText`; the command-line flag arrives with a recording tool.
- **Date:** 2026-10-13
- **Related:** [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) (the boundary and
  timeline recorded here), [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md) (the
  vocabulary and its generated-table gate), [ADR-0090](ADR-0090-ime-pull-text-store-contract.md)
  (the text-store conformance this mirrors), [ADR-0163](ADR-0163-presentation-owned-pointer-resampling.md);
  spec [docs/plans/specs/flui-interaction/completeness/](../plans/specs/flui-interaction/completeness/design.md);
  research [trace format](../research/2026-10-12-interaction-input-followups.md),
  [synthetic-first testing](../research/2026-10-12-interaction-input-best-practices.md),
  [dependency selection](../research/2026-10-13-interaction-dependency-selection.md).

## Context

`flui_testing::replay::PointerScript` drives the production `GestureBinding` on a virtual clock,
but only for pointer events, through mouse-shaped constructors, and nothing records what a real
backend produced. Without recordings a gesture bug seen on a device cannot become a test, and a
backend's translation (Win32 `POINTER_INFO`, DirectManipulation viewports, AppKit, Android) is
only checked by hand. Serialising the event vocabulary directly would freeze it as a wire format
and put `serde` on the Stable contract crate.

## Decision

1. **What is recorded.** The stream of `HostInput` a runtime handles, with the non-input rows
   replay needs, in one order. `InputTap` observes `&InputRecord`, a
   `#[non_exhaustive]` enum of `Input`, `Decision`, `Preferences`, `Gap` and `End` rows, which the
   runtime emits on its owner thread in the order it applied them; the writer serialises exactly
   that sequence. The tap and its registry live beside the emitting runtime in `flui-runtime`
   (`UiRuntime::input_taps`, so a headless host records what a runner does); an application
   installs one per runtime through `flui-app`'s `AppConfig::with_input_tap`, which re-exports
   the tap and record types. An `Input` row holds the `InputSurface`/`WindowId` it addressed
   (the window, or a surface embedded in it, ADR-0207 §5), `t` (the `EventTime`), the arrival
   time, the dispatch result and the `InputSequenceId` dispatch placed it in. The arena's
   verdicts (the decisions `GestureObserver` reports, each naming its contest's sequence) are
   recorded as `Decision` rows in the same stream, so decisions made later on deadlines, with no
   input of their own, are replayable. A `Decision` row names its member by a number the
   recording gives each member in the order it first sees it (its join), never by its type: two
   members of one type stay apart, and the type name is only an annotation. A recording names
   sequences, embedded surfaces and members by value, since a reader cannot mint them; a replay
   resolves an embedded surface against the ones its host embedded in the same order. The
   header holds devices, scale factors, the behavior values in effect (each window's per-kind
   gesture values, as values and never a preset's name, and the `SystemPreferences` revision
   with its values), the clocks and a major format version; replay restores the gesture values
   and preferences before its first row (row
   `a_recording_replays_under_its_recorded_gesture_values`). Each preference revision the runtime applies while
   recording (ADR-0172: applied before the next input, seen by new sequences) is recorded as a
   `Preferences` row at its position in the stream, and replay applies it at the same position,
   so host-owned values are part of the replayed timeline (row
   `a_preference_revision_between_sequences_replays_to_the_same_decisions`).
   One writer per runtime on its owner thread; serialisation and IO run on a worker over owned
   bytes (on wasm, inline on the owner thread), with no lock shared between runtimes. The serialiser is compiled only into dev and
   tool builds; the facade's normal graph carries no test support. On wasm the writer is any
   `Write`, with no file system assumed.
   `InputTap` delivers as `GestureObserver` does. The dispatch result is committed and the owned
   row moved out before the tap runs; delivery follows dispatch with no runtime borrow held; each
   call is panic-contained, with the first failure authoritative and reported to the owner's
   diagnostics; the tap stays registered, the row that panicked is not re-delivered (the
   recording gets a `Gap { rows }` marker so a reader sees the loss) and later rows continue.
   The lost-row count is pending delivery kept apart from the records: right after the contained
   panic the runtime delivers it to the tap as an `InputRecord::Gap` row in a second contained
   call; if that call panics too, the count stays pending, grows with further losses and goes
   out before the next row, and finishing the recording offers it once more. Completeness does
   not depend on a tap that keeps failing: a recording is complete only when it ends with an
   `End` row, written at finish with the total lost-row count, after every pending `Gap` was
   delivered. If the final `Gap` or the `End` cannot be delivered, finishing returns
   `Err(RecordingError::Incomplete { lost_rows })` without retrying. Replay validates before it
   dispatches: it reads the whole recording through its `End` row (and checks it for `Gap` and
   `Redacted` rows) before handing any row to the runtime, so a stream with no `End` row (a
   failing tap, a crash, a truncated file) fails with `NonReplayable` and no input reaches the
   runtime (rows `a_panic_on_the_final_row_still_writes_its_gap`,
   `a_tap_that_fails_at_finish_reports_an_incomplete_recording`,
   `a_truncated_recording_dispatches_nothing`). Delivery to the tap is batched like the
   other owner-thread deliveries: a pass delivers only the records present when it started;
   records a tap callback admits reentrantly (by dispatching input) go to the next pass, which
   the runtime schedules as an owner-thread wake, so a tap that dispatches on every row cannot
   keep the owner loop from returning (row `a_tap_may_dispatch_reentrantly` admits recursively);
   a
   tap replaced or removed from
   inside a callback takes effect from the next row. An outgoing tap (replaced, removed, or the
   one that panicked after installing a replacement) is retired after the callback catch under
   the same containment custody: a panic while destroying its captures is contained separately,
   never replaces the first failure and never double-panics; the incoming tap still receives
   the next row. Registration returns a `#[must_use]` RAII
   handle bound to its own registration (id and generation); its `Drop` unregisters only that
   registration, so a stale handle dropped after a replacement never removes the new tap.
   Registration generations are never reused; at the terminal value registration returns
   `Err(IdExhausted)` permanently instead of wrapping.
2. **DTO, not vocabulary.** The event vocabulary carries no `serde`. `flui-testing` holds private
   mirror enums with pinned `#[serde(rename)]` spellings; unknown fields are skipped within a major format version, an unknown variant is an error;
   `From<&InputRecord>` and `TryFrom<Dto>` over every `InputRecord` variant (`Input` with its
   nested `HostInput` DTO, including the accesskit `ActionRequest` payload, `Decision`,
   `Preferences`, `Gap` and `End`). Because the vocabulary and `InputRecord` are
   `#[non_exhaustive]`, a downstream `match` cannot be exhaustive, so drift is caught by a
   generated table of one sample per vocabulary variant and per record variant that must
   round-trip through the DTO; regeneration is gated as ADR-0143 §4's key
   tables are.
3. **Format.** JSON Lines: one header object, then one object per row, framed by `serde_json`
   and a newline. `RecordingReader<R: BufRead>` is an iterator of `Result<Record,
   RecordingError>`; `RecordingWriter<W: Write>` writes. `RecordingError::{Io, Json { line,
   source }, Version { found, supported }, UnknownVariant { line, tag }, NonReplayable { line },
   Incomplete { lost_rows }}`: an unknown variant is an error, never a skipped row; `NonReplayable` is a row
   replay cannot reproduce: item 5's redacted row or a `Gap` marker from item 1. Replay stops
   there and never dispatches later rows against a gesture state that skipped the lost input
   (row `replay_stops_at_a_gap_with_non_replayable`). `bincode` is excluded (RUSTSEC-2025-0141, refused by
   `deny.toml`); a binary encoding, if a capture size ever needs one, is `rmp-serde` behind the
   same header and reader.
4. **Two recording levels.** `PlatformInput`-level recordings are `HostInput` streams that
   `flui-runtime`, `flui-interaction` and widget tests replay. Native recordings hold the backend's plain
   `NativeReading`s (no `windows::*` or `objc2` types), with clock frequency and anchor in the
   header; their codec lives in `flui-platform`'s tests and replays them through the pure
   decoders, so Linux CI exercises Win32 and Apple translation without those hosts.
5. **Privacy by default.** Key text, IME commit strings, accessibility action text
   (`ActionData::Value` for `SetValue` and `ReplaceSelectedText`, and any other text-carrying
   action data), window titles and device names are redacted when recorded. `--keep-text` is the
   one explicit opt-in per recording, and it covers all of them. A row whose payload was
   redacted is marked `Redacted`; replay refuses it with `RecordingError::NonReplayable { line }`
   instead of applying different text, so a redacted recording never claims to reproduce
   decisions that depended on that text. Rows: replaying a redacted assistive `SetValue` fails
   with `NonReplayable`; the same recording made with `--keep-text` replays to its decisions.
6. **Scripts.** `PointerScript` is built from per-device consuming builders — `TouchScript`,
   `MouseScript`, `PenScript`, `TrackpadScript`, `VirtualScript` — each converting into a
   `PointerScript` through `build() -> Result<_, ScriptError>` (and `TryFrom`), which checks
   down/up pairing: an unpaired script is an error, never a panicking conversion. The output is an owned
   `Vec<HostInput>` on virtual time with a readable text form beside its JSON Lines. Every row of
   the arena ownership table and the scroll-channel table has at least a touch and a mouse
   script. Harness finders look nodes up in the semantics tree by role and label; a "run until
   settled" step is bounded by a step limit.
7. **Backend conformance.** `assert_backend_conforms<B: BackendUnderTest>(&mut B)` in
   `flui-testing` is statically dispatched; `BackendUnderTest` has `type Reading` and
   `fn decode(&mut self, r: Self::Reading) -> impl Iterator<Item = HostInput> + use<'_, Self>`
   and is never `dyn`. Rows run through `run_table`: pointer assembly, clocks (per-device and
   per-sequence clock choice, QPC on Win32, monotonic order, counter wrap), the `Scroll` and
   `PanZoom` grammar, DnD, haptics and IME event order. Each feature adds its own rows.
8. **Native injection.** `cargo xtask device <os>-input` exists for every OS: `SendInput` and
   `InjectSyntheticPointerInput` on Windows, `CGEvent` on macOS, XCUITest on iOS, instrumentation
   on Android, uinput/libei on Linux, WebDriver BiDi on the web. It certifies translation (native
   input in, expected `HostInput` out), needs an interactive desktop, and its result is quoted in
   the PR. Paths no injector reaches — the Windows precision touchpad through DirectManipulation
   and macOS trackpad gestures — are certified only by recordings taken on hardware.

## Consequences and verification

A device bug becomes a checked-in recording; a new backend proves itself against one table; the
vocabulary evolves without a wire freeze, at the cost of a DTO layer kept in step by its gate.

- `every_vocabulary_variant_round_trips_through_the_recording_dto` over the generated table.
- `a_recording_replays_to_the_decisions_it_recorded`, which compares the replayed `Decision` rows
  with the recorded ones, and
  `an_unknown_variant_is_a_typed_error_not_a_skipped_row` in `flui-testing`.
- `recordings_redact_text_titles_and_device_names_by_default`, and
  `an_assistive_set_value_on_a_password_field_records_no_plaintext`.
- `InputTap` delivery rows: `a_tap_may_dispatch_reentrantly`,
  `a_tap_replaced_mid_call_takes_effect_from_the_next_row` (including dropping the replaced tap's stale handle, which leaves the new tap registered),
  `a_panicking_tap_is_contained_and_the_first_failure_kept` (the next row is still recorded after a `Gap` marker), `tap_registration_refuses_at_generation_exhaustion`, `a_tap_that_installs_a_replacement_then_panics_retires_contained_and_the_new_tap_records_the_next_row`, and
  `releasing_the_owner_during_a_tap_call_finishes_cleanly`.
- `a_script_with_an_unpaired_down_fails_to_build`; rows "double click within one frame",
  "N events in one frame" and "an event between vsyncs".
- `flui-platform` native codec rows: `win32_pointer_info_recordings_decode_to_their_host_input`
  and the per-OS equivalents, run on Linux CI over pure decoders.
- `assert_backend_conforms` runs for the headless backend in CI; for Win32, macOS, iOS,
  Android, Linux and the web it runs on a native host or device only, as do all
  `xtask device` checks.

## Alternatives rejected

- **`serde` on the vocabulary.** Freezes the Stable contract crate's types as a wire format.
- **`libinput record`-style raw OS capture as the only level.** Replay then needs the kernel
  device; FLUI keeps raw decoding to its own native level and records after translation.
- **Flutter's `PointerEventRecord`.** In memory only, no version, wall-clock replay.
- **`postcard`.** Not self-describing; a reader cannot skip an unknown field.
- **Mouse-shaped convenience helpers (Masonry), with touch only through raw events.** Touch, pen
  and trackpad scripts would stay second-class; the per-device builders follow Compose's
  modality injection scopes.
