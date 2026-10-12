# Web logical-key admission

Status: Approved behavior; implementation in progress.

The DOM producer resolves named values first, using the existing generated
vocabulary and accepted aliases. It then admits a nonempty character only if
the string is NFC and follows the W3C key-string grammar. A refused logical
value becomes Unidentified while the event retains its other observations.
Both Down and Up use the same private conversion seam.

The common owned character type retains nonempty native text. Validation belongs
to the producer that knows its source grammar, rather than narrowing every
backend and application-authored trigger. Existing focus, shortcut equality,
modifier and text-store authority contracts stay in force.

## Alternatives

- Keeping a one-scalar gate loses valid base-plus-mark values.
- Accepting every nonempty DOM value turns unknown names, raw controls and
  multiple-base strings into character keys.
- A private standards-aware Web parser with mature Unicode facilities preserves
  admitted meaning and refuses malformed values. This is the selected shape.

The implementation uses ICU4X 2.3 normalizer and properties compiled data on the
wasm target. Canonical combining classes come from the normalizer's borrowed
map, which shares its decomposition data, and general categories come from
the properties map. NFC is checked without rewriting. These facilities use the
same locked data cohort; there are no private character ranges or new public
parser. Record generated wasm artifact cost instead of claiming a performance
advantage without measurement.

## Acceptance seams

The owner approved the existing browser input probe (registered DOM events to
public on_input) and existing mounted Shortcuts/Focus consumer tables. Browser
checks compare source and translated values, include Down/Up and refused-value
recovery, and observe every existing event field. Mounted checks assert action
effects and handled/unhandled results. Probe output must escape strings safely.

A private parser-only test cannot replace these producer/consumer witnesses.
The existing wasm CI lane does not automatically execute the browser probe;
report its actual browser run separately. No new CI workflow is part of this
bounded repair.
