# ADR-0124: Controlled numeric and disclosure actions preserve their meaning

- **Status:** Accepted
- **Date:** 2026-10-05
- **Extends:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) and [ADR-0095](ADR-0095-agent-protocol-schema-crate.md)

## Decision

Expand and collapse are distinct requests, never activation aliases. Numeric
setters carry a finite numeric payload independently of text editing. The
platform listener translates the entire request before admitting it to the
presentation inbox. Existing action bits remain unchanged; expand, collapse
and numeric setters use bits 27, 28 and 29, preserving reserved bits 23–25.

`NumericRange` is an immutable checked value: finite value and bounds,
inclusive membership, and a finite positive step. A zero span is valid.
The step describes adjustment proposals and does not quantize numeric setters.
The semantics owner validates incoming payloads against its current range
when it resolves the request, and the request is invoked as soon as it resolves.

`Slider` and `Disclosure` are controlled widgets: events propose changes through
`EventCx`; the parent commits them by supplying a new value or expansion state.
Their public types are available through the existing SDK widget module.
Focus belongs to the mounted lifecycle-acquired node. After a reentrant focus
request, input rechecks attachment and writer authority before proposing.
Disabling or unmounting retires callbacks and prevents retained actions from
reaching replacement controls. Independent callback and view owners enter
terminal custody before arbitrary user code or retirement; a single opaque
user aggregate retains its own double-panic limit.

## Verification

`semantics_translation_and_routing` exercises queued platform requests through
actual frame producers. `numeric_setters_are_checked_against_the_current_range`
checks numeric admission against the current range.
`focus_actions_and_shortcuts` includes real mounted platform focus.
`controlled_slider_input_and_geometry` covers allocated geometry, RTL, finite
extremes, controlled proposals and retired actions.
`controlled_disclosure_state_and_geometry` covers retained header focus,
controlled expansion, clipping, disabled actions and retirement competition.
Native accessibility execution remains a separate platform validation obligation.
