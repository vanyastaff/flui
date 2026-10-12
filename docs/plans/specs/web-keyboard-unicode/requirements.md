# Web keyboard Unicode values

Status: Approved, 2026-10-12. Implementation in progress.

## Problem

The Web backend's single-scalar character gate loses valid NFC key strings such
as `ḍ̇` and `का` and admits raw control values as characters. Actual registered
DOM delivery through the public window input callback reproduces both defects.
Constructor-driven browser checks establish translation, not physical layout
or hardware fidelity.

## Requirements

- R1: Preserve supported named keys and accepted Hyper/Super-to-Meta aliases.
- R2: Preserve complete nonempty NFC key strings according to the W3C grammar:
  zero or one non-control base followed by combining characters. Controls are
  category Cc; combining means category Mc or a nonzero canonical combining
  class. Admission does not depend on byte, scalar, UTF-16 or grapheme count.
- R3: Publish Unidentified for empty, raw-control, non-NFC, unknown named or
  otherwise invalid Web values. Do not normalize, truncate or rewrite them.
- R4: Preserve physical code, location, timestamp, modifiers, composition and
  the existing repeat/release contract; do not discard an entire event when
  its logical value is refused. The next healthy event must remain deliverable.
- R5: Keep the common KeyText nonempty-text contract and native mappings.
  Do not impose Web grammar on native or authored character strings.
- R6: Prove a complete admitted character through mounted focus/shortcut
  dispatch and its observable action result, with nonmatching and event-policy
  controls.
- R7: Use mature, coherent Unicode data and normalization facilities in one
  private Web conversion seam. Introduce no public parser, DTO or micro-crate.
- R8: Keep browser probe output safe for quote, backslash and raw-control
  strings. Record browser execution and wasm artifact cost; do not claim that
  compilation or Node execution establishes browser delivery.
- R9: Document the mapping and its observable witnesses. Preserve public-path
  RED, GREEN and counterfactual evidence and complete the scoped gate.

## Separate work

Web focus ownership (issue #1050), default-action outcomes and listener
retirement, Android layout translation, text-store/native protocols, browser
IME integration and physical device acceptance remain separate. This scope
does not supply text commits or close those defects.

## References

- [W3C key values, Unicode values](https://www.w3.org/TR/2025/REC-uievents-key-20250422/#key-string)
- [Unicode normalization](https://www.unicode.org/reports/tr15/tr15-58.html)
- [ADR-0143](../../../adr/ADR-0143-flui-owned-input-event-vocabulary.md)
- [ADR-0156](../../../adr/ADR-0156-character-shortcut-shift-policy.md)
