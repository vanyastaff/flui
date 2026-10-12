# ADR-0201: Preserve modifier observations and interpret keyboard commands separately

- **Status:** Accepted
- **Date:** 2026-10-12
- **Supersedes:** ADR-0023 decision 2 and ADR-0156's modifier-comparison description,
  only for source composition and ordinary command interpretation. Character-only
  Shift opt-in, ASCII comparison, repeat and action/dispatch decisions remain.
- **Related:** ADR-0143 (owned source vocabulary), ADR-0090 and ADR-0142
  (text-store composition authority).

## Context

AltGraph can be observed independently or alongside Control/Alt aliases. A
four-flag producer loses that observation; a consumer matching raw aliases can
invoke Ctrl+A instead of inserting a glyph. Toggle states likewise belong to an
event but must not disable ordinary control navigation. A source composition
flag and a field's composing range represent different facts.

## Decision

Platform adapters preserve positive supported observations in the owned
Modifiers value. Web reads canonical getModifierState values on keyboard and
MouseEvent-based input, retaining convenience booleans and normalizing supported
legacy Hyper/Super observations to Meta. No producer invents AltGraph from a
Control+Alt chord, named-key sequence or physical position. Source key meaning,
metadata and raw modifiers remain unchanged through owner/runtime delivery.

The widget layer interprets four ordinary command dimensions separately. With
explicit AltGraph active, Control and Alt are excluded from that private view;
Meta and Shift remain. Locks, Fn and Symbol are observations rather than command
requirements. SingleActivator, field command/word movement and Slider use one
private projection. A produced-character shortcut can still work on a nontext
control; AltGraph is not blanket-rejected. The existing exact/ignored Shift and
ASCII-letter contracts remain.

An Active source composition prevents ordinary framework activators, field
keyboard editing/submit and Slider proposals. Unclaimed keys retain native
defaults. Existing controller-composition guards remain even when a source does
not observe composition. Raw input and authored raw Focus handlers retain the
complete event and their explicitly chosen policies. This decision does not
implement Web IME committed-text transport or outstanding text-store exits.

Source getter calls run without framework guards. Reentrant inner input may
finish before an outer observation. A retired registration cannot admit the
outer event to its replacement; the next healthy input remains deliverable.
There is no new mutable modifier tracker or delivery obligation.

## Compatibility costs

The current DTO cannot distinguish a genuine additional Control+AltGraph from
Control aliases. Ordinary activators choose glyph interpretation; raw handlers
can inspect the unchanged facts. Without explicit AltGraph, EditableText retains
its conservative Ctrl+Alt printable-text fallback. An authored Ctrl+Alt command
can therefore remain unreachable while a field is focused. Neither choice is a
claim of universal Ctrl+Alt/AltGraph equivalence.

Winit's pinned aggregate state exposes only four dimensions. Native observation
provenance, Win32/AppKit lock mappings, Android layout/meta meaning and release or
focus ordering remain separate work. Missing observations are not proof a
modifier is physically inactive. Apple-Web runtime platform policy also remains
unchanged.

## Alternatives and evidence

Observation-only repair would leave the field/shortcut and Slider problems.
Rewriting public Control/Alt would hide evidence from raw consumers. A richer
key/text/provenance DTO could distinguish more native cases but requires its own
producer-backed contract. A blanket AltGraph rejection would disable intentional
produced-character control bindings. The private projection provides a shared
interpretation while keeping source facts available.

The browser probe's observed-modifiers and modifier-getter recovery controls
exercise public DOM delivery. Mounted keyboard-modifier rows in the existing
text_editing, focus_actions_and_shortcuts and controlled_slider_input_and_geometry
tables distinguish field text/selection, actual actions, native-default refusal
and Slider proposals. Constructor/controlled-source tests do not establish
physical layouts, real browser IME commits or native ordering. Default wasm
panic-abort does not establish Rust unwind containment.

Source representation follows [UI Events, Working Draft](https://www.w3.org/TR/2026/WD-uievents-20260221/#interface-keyboardevent)
and [key values, Recommendation](https://www.w3.org/TR/2025/REC-uievents-key-20250422/#selecting-key-attribute-values).
Neither prescribes FLUI shortcut arbitration. [W3C AltGraph discussion](https://github.com/w3c/uievents/issues/147),
[Winit's Control/AltGraph limitation](https://github.com/rust-windowing/winit/issues/3012)
and [Slint's public cases](https://github.com/slint-ui/slint/blob/78c9731ea99886bbef547c72a30315ec7ce59006/tests/cases/input/altgr.slint)
inform the cases and costs rather than supplying a universal framework policy.
