# ADR-0346: A key-down says its kind, a dead key its accent, and a scroll its direction

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md) §1 for a key event's
  state and repeat (they were private fields that had to agree), §2's keys bullet for `KeyRepeat`
  and a dead key as a `Named` key, and §3 for `KeyRepeat` among the flag enums
- **Superseded-by:** [ADR-0358](ADR-0358-a-touchpad-hold-stops-what-coasts-and-a-touchpad-swipe-is-a-gesture.md)
  for the recording format's version and the protocol version (§5): version 5 and 0.21
- **Related:** [ADR-0342](ADR-0342-a-value-added-to-a-closed-union-breaks-the-agent-protocol.md) (a variant added to a closed union is a break), [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md) §4 (the generated W3C
  tables), [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) (the recording mirrors
  the vocabulary variant by variant), [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (the
  protocol schema a break is listed in)

## Context and problem statement

The contract's input vocabulary (`flui-platform`'s `keyboard` and `pointer` modules) had four
shapes that let a value say less than, or something other than, what happened. Each was read on
`origin/main` before this record:

| Shape | What was wrong |
|-------|----------------|
| `KeyEvent { state, repeat, release }` | Three facts of one key-down kept apart. `with_repeat(AutoRepeat)` on a release kept `First` and `with_release(Never)` on a release kept `Follows`, silently; `AutoRepeat` with `Never` was representable. |
| `Key::Named(NamedKey::Dead)` | A dead key carried no accent. Android turned `KeyCharacterMap`'s `COMBINING_ACCENT` into a bare `Dead` and never composed it, so `´` then `e` typed `e`. Windows had the accent in `WM_DEADCHAR` and dropped it. |
| `ScrollEvent` | No field said whether the platform reversed the device's direction (natural scrolling). AppKit's `isDirectionInvertedFromDevice` and WebKit's `webkitDirectionInvertedFromDevice` were not read. |
| `PointerEvent::Revise(PointerRevision { pointer, sample })` and `PointerEvent::PenSqueeze` | The revision named the reading it corrected by pointer id and time. A `PointerId` is reused after `Up`, UIKit revises after `touchesEnded`, and two readings can share a coarse timestamp, so the key can name the wrong reading or none. No host produced either variant, and `docs/gaps.md` described a different planned shape. |

The question: what shape does each of these facts take so the type says exactly what happened?

## Decision drivers

- An invalid state is unrepresentable; a builder never discards an argument.
- A consumer that asks only "is the key down" keeps one pattern for it.
- A value carries what the platform reports and nothing a host must guess; where a platform
  does not say, the value says so.
- A public variant has a producer; one without is a promise no host keeps.
- The recording and the agent protocol mirror the vocabulary one to one (ADR-0215).

## Considered options

1. **Keep the fields, make the builders refuse** (`with_repeat` returns `Result` on a release).
   Keeps the shape; every caller handles an error that cannot happen when it builds a key-down,
   and `AutoRepeat` with `Never` stays representable.
2. **`KeyState::Down { repeat: KeyRepeat, release: KeyRelease } | Up`.** A release carries
   neither; `AutoRepeat` with `Never` is still representable, and two enums describe one fact.
3. **`KeyState::Down(KeyPress) | Up` with `KeyPress::{First, AutoRepeat, Unpaired}`.** Every
   state a host reports is one value, a release carries nothing, and `KeyState::Down(_)` asks
   whether the key is down.

For the dead key: (a) a `combining: Option<char>` field on `KeyEvent`, meaningful for one key
only; (b) `Key::Dead(Option<char>)`, as winit has it, with `Dead` left out of the generated
`NamedKey`. For the revision: (i) keep it keyed by an estimation ticket the host mints with the
sample, with no host minting one yet; (ii) remove both variants until a host produces them.

## Decision

Chosen: option 3, (b), a scroll direction enum, and (ii), because each makes the type carry the
rule and leaves no public shape without a producer.

### 1. A key-down says its kind

```rust
pub enum KeyState { Down(KeyPress), Up }
pub enum KeyPress { First, AutoRepeat, Unpaired }
pub struct KeyEvent { pub state: KeyState, pub key: Key, pub code: Code, /* … */ }
```

`KeyRepeat`, `KeyRelease`, `KeyEvent::{state, repeat, release, with_repeat, with_release}` are
gone; `state` is a public field, since nothing has to agree with it. `Unpaired` is Windows'
Alt+numpad character, which no repeat or release follows. Both enums are closed
(`#[expect(clippy::exhaustive_enums)]`): a key goes down or comes up, and a key-down is a first
press, a repeat of it or a press with no key behind it.

### 2. A dead key carries its accent

`Key::Dead(Option<char>)` carries the accent as the layout prints it (`´`, `^`, `¨`), or `None`
where the platform does not say; its release carries the same value: every host settles
its key events through one ledger, `PressedAs` in `flui-native`, which keeps the key a dead or
code-less key was pressed as until its release, whatever Shift, the layout or a compose sequence
made of the key since. The `key-vocabulary`
generator leaves `Dead` out of `NamedKey` (`carried_by_key`) and fails if keyboard-types renames
it. Windows reads the accent from `WM_DEADCHAR`, XKB from the dead keysym, Android from
`KeyCharacterMap`; the DOM names no accent, and AppKit and UIKit compose dead keys as marked
text. Android composes a hardware dead key with the next character key through
`KeyCharacterMap.getDeadChar`, types the accent and the letter when the pair composes nothing,
keeps the accent across a modifier and drops it on a key that types nothing, a Control or Meta
chord, an input method's composition or focus loss.

### 3. A scroll says its direction

`ScrollEvent::inversion: ScrollInversion` with `Unknown` (the default), `FromDevice` and
`InvertedFromDevice`. The delta stays in content terms on every platform; the direction is for a
control that maps a scroll onto a value and wants to move with the fingers. macOS reads
AppKit's `isDirectionInvertedFromDevice`, Safari `webkitDirectionInvertedFromDevice`
and the native Wayland host `wl_pointer.axis_relative_direction`; the native recording carries
the inversion of each scroll row. The type is not named `ScrollDirection`, which is the
viewport's axis direction in `flui-rendering`. No widget reads it yet; the slider's wheel handling is
its first consumer (`docs/gaps.md`).

### 4. No revision or pen squeeze until a host produces one

`PointerEvent::Revise`, `PointerRevision`, `PointerEvent::PenSqueeze`, `PenSqueeze` and
`PenSqueezePhase` leave the vocabulary, the listener callback, the binding's route and the
recording. When the iOS host reads `touchesEstimatedPropertiesUpdated:`, the revision returns
keyed by a ticket the host mints with the estimated sample (UIKit's `estimationUpdateIndex`),
not by pointer id and time; `docs/gaps.md` holds that shape.

### 5. The recording and the protocol follow

The recording's key state is `down`, `repeat`, `unpaired` or `up`, its key has a `dead` variant
and its scroll an optional `inversion`; the format is version 4 and refuses version 3. Protocol
0.16 is a break (ADR-0342 §1 for the added variants) and lists the removed names in `BREAKING`, which gains a `removed` list beside `renamed`.

### Consequences

- Good, because a release can no longer claim a repeat, and a consumer reads one field.
- Good, because a hardware dead key on Android types the composed character, and a field gets
  the accent to show while it waits.
- Bad, because every place that built a key event changed, and a 0.15 agent client is refused by
  a 0.16 application.
- Neutral, because Windows, X11, Android and iOS report no direction: their
  scrolls say `Unknown` (`docs/gaps.md`).

### Confirmation

- The compiler: `KeyState::Up(KeyPress::AutoRepeat)` does not compile (`KeyState`'s
  `compile_fail` doctest).
- `cargo xtask key-vocabulary` and its self-test: `NamedKey` has no `Dead`, and a carried
  variant gone upstream fails the run.
- Tests: `a_hardware_dead_key_composes_with_the_next_key` (flui-native, the Android composer over
  a stand-in key map), `a_repeat_and_a_missing_release_are_kinds_of_key_down` and
  `a_dead_key_carries_its_accent_and_no_named_key_spells_it` (flui-platform),
  `every_vocabulary_variant_round_trips_through_the_recording_dto` (flui-testing),
  `every_published_schema_is_additive_to_the_next` (flui-protocol),
  `a_scroll_carries_the_direction_the_compositor_reports` (flui-native, the Wayland wire),
  `a_held_key_is_released_as_it_was_pressed` (flui-native, the shared ledger: a press, a
  modifier or compose change, its release).
