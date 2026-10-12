# Observed modifiers and composition-safe keyboard consumers

Status: Implemented; local validation and CI acceptance in progress.
Specification: [#1565](https://github.com/vanyastaff/flui/issues/1565).
Owner approval: 2026-10-12.

## Requirements

- **R1 — Source fidelity.** Web keyboard, pointer/mouse and wheel preserve the
  twelve canonical supported modifier observations. Supported legacy Hyper/Super
  normalize to Meta. Four convenience booleans remain source evidence. Do not
  infer AltGraph from Ctrl+Alt, AltRight or a named-key sequence.
- **R2 — Raw transport.** Owned input DTOs retain their shape and raw source flags.
  Key meaning, code, location, time, composition, state and release-not-repeat
  remain unchanged through window delivery, owner FIFO, runtime and focus.
- **R3 — Command interpretation.** One private widget interpretation excludes
  Control/Alt from ordinary command dimensions when explicit AltGraph is active,
  retaining Meta/Shift. Locks, Fn and Symbol do not become command requirements.
  It is consumed by SingleActivator, EditableText and Slider.
- **R4 — Glyph and command behavior.** Explicit AltGraph characters enter a
  focused field without Ctrl/Alt commands stealing them. Meta retains command
  priority. Produced-character shortcuts remain usable on nontext controls,
  including Slider +/-; exact/ignored Shift, ASCII case, complete strings,
  first-match action resolution, repeat and Up behavior retain their contracts.
- **R5 — Source composition ownership.** Active source composition cannot trigger
  ordinary framework shortcuts, traversal/activation, field keyboard insertion,
  editing or submit, or Slider proposals. Keys remain unclaimed for native
  defaults when no authored outer handler claims them. Controller composition
  guards remain even with an Inactive/unavailable source observation.
- **R6 — Incidental state.** Locks cannot disable conventional shortcuts or Slider
  arrows/Home/End. Produced logical keys remain authoritative; no extra casing or
  numpad reinterpretation is performed. Named word movement uses the same command
  interpretation and keeps Shift selection independent.
- **R7 — Accepted compatibility cost.** Without explicit AltGraph, preserve the
  current Ctrl+Alt printable-text fallback in EditableText. An authored Ctrl+Alt
  command may remain unreachable in a focused field. Genuine extra Control plus
  explicit AltGraph uses glyph interpretation in ordinary activators. Raw
  observations remain available through existing raw input/Focus interfaces.
- **R8 — Reentry and retirement.** Modifier getters run without callback/state
  guards. Reentrant inner delivery and next healthy input remain possible.
  Closing/replacing a registration during observation fences obsolete delivery.
  No global held-state tracker, new delivery debt or native-default bit changes
  are introduced.
- **R9 — Migration and evidence.** Migrate actual activator/field/control consumers,
  update durable architecture and public docs, add a changelog fragment and prove
  each behavior through public producer or mounted-consumer RED/GREEN and
  production-hunk counterfactuals. Report feature/platform execution limits.

## Accepted boundaries

This feature does not implement native modifier provenance/observation parity,
Winit release/focus ordering, richer key/text DTOs, Web IME committed-text
transport, outstanding ADR-0142 composition exits/anchor debt, Apple-Web runtime
platform policy, pointer command reinterpretation, wheel defaults or RAF design.
Source-confirmed native lock/layout gaps and the broader architecture backlog
remain separate work. Physical layouts/devices, other browser engines and native
AppKit/Android/UIKit execution are not established by synthetic tests.

## Evidence baseline

Main6a860e91b preserved source composition but lost Web modifier observations in
30 of 50 constructor/controlled-source DOM events. Public runtime/mounted
consumers reproduced composition ownership failures, Slider lock refusal,
AltGraph alias command collisions and the retained Ctrl+Alt field fallback.
Bare produced-character rejection was an investigation discriminator; the
approved policy preserves such shortcuts.
