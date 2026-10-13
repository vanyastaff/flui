# ADR-0215: Input is recorded as the `HostInput` stream, and backends are certified against one conformance table

- **Status:** Accepted (2026-10-13); implementation pending
  (`docs/plans/specs/flui-interaction/completeness/tasks.md`). Until it merges, `main` follows the
  current code: `flui_testing::replay::PointerScript` replay of pointer events, with no recordings.
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

1. **What is recorded.** The stream of `HostInput` a runtime handles. `InputTap` in `flui-app`
   observes that stream; each row holds the `InputSurface`/`WindowId`, `t` (the `EventTime`),
   the arrival time and the dispatch result. The decisions `GestureObserver` reports are recorded
   as `Decision` rows in the same stream, so decisions made later on deadlines, with no input of
   their own, are replayable. The header holds devices, scale factors, the
   behavior values in effect (the gesture preset), the clocks and a major format version.
   One writer per runtime on its owner thread; serialisation and IO run on a worker over owned
   bytes (on wasm, inline on the owner thread), with no lock shared between runtimes. The serialiser is compiled only into dev and
   tool builds; the facade's normal graph carries no test support. On wasm the writer is any
   `Write`, with no file system assumed.
   `InputTap` delivers as `GestureObserver` does. The dispatch result is committed and the owned
   row moved out before the tap runs; delivery follows dispatch with no runtime borrow held; each
   call is panic-contained, with the first failure authoritative; a tap replaced or removed from
   inside a callback takes effect from the next row. Registration returns a `#[must_use]` RAII
   handle whose `Drop` unregisters.
2. **DTO, not vocabulary.** The event vocabulary carries no `serde`. `flui-testing` holds private
   mirror enums with pinned `#[serde(rename)]` spellings; unknown fields are skipped within a major format version, an unknown variant is an error;
   `From<&HostInput>` and `TryFrom<Dto>` over `flui-runtime`'s `HostInput`, including its
   accesskit `ActionRequest` payload. Because the vocabulary is `#[non_exhaustive]`, a
   downstream `match` cannot be exhaustive, so drift is caught by a generated table of one sample
   per variant that must round-trip through the DTO; regeneration is gated as ADR-0143 §4's key
   tables are.
3. **Format.** JSON Lines: one header object, then one object per row, framed by `serde_json`
   and a newline. `RecordingReader<R: BufRead>` is an iterator of `Result<Record,
   RecordingError>`; `RecordingWriter<W: Write>` writes. `RecordingError::{Io, Json { line,
   source }, Version { found, supported }, UnknownVariant { line, tag }}`: an unknown variant is
   an error, never a skipped row. `bincode` is excluded (RUSTSEC-2025-0141, refused by
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
   one explicit opt-in per recording, and it covers all of them.
6. **Scripts.** `PointerScript` is built from per-device consuming builders — `TouchScript`,
   `MouseScript`, `PenScript`, `TrackpadScript`, `VirtualScript` — each `Into<PointerScript>`;
   `build() -> Result<_, ScriptError>` checks down/up pairing. The output is an owned
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
  `a_tap_replaced_mid_call_takes_effect_from_the_next_row`,
  `a_panicking_tap_is_contained_and_the_first_failure_kept`, and
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
