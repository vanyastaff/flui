# ADR-0156: Character shortcuts may ignore the Shift needed to produce them

- **Status:** Accepted
- **Date:** 2026-10-07
- **Supersedes:** ADR-0023 decision 2, modifier matching for character triggers

## Decision

`SingleActivator::new`, `named` and `character` match every modifier exactly.
`character_ignoring_shift` admits only a character trigger and ignores Shift;
Control, Alt and Meta remain exact. Calling `shift` afterwards selects exact
Shift again. Repeat and ASCII-letter comparison retain their existing contracts.
The dispatch and Actions decisions of ADR-0023 remain in force, including the
amendments in ADR-0079 and ADR-0086.

A native key event carries the character that the platform produced. Slider
uses this policy for `+` and `-`, so Shift required by a layout does not disable
adjustment. Named arrows, Home, End and traversal still require exact modifiers.

## Validation

The shortcut character row checks Shift and extra modifiers; the mounted Slider
keyboard row checks actual proposals through focus dispatch. Native keyboard
translation remains the backend's existing logical-character path.
