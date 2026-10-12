# ADR-0203: FLUI's semantics vocabulary covers WAI-ARIA 1.2 and follows AccessKit's actions

- **Status:** Accepted (2026-10-12); implemented by the semantics-model work W4
- **Date:** 2026-10-12
- **Supersedes:** [ADR-0124](ADR-0124-controlled-numeric-and-disclosure-actions.md), in part:
  its action bit assignments (expand, collapse and numeric setters at bits 27–29, bits 23–25
  reserved). Its meaning stands: expand and collapse are distinct requests, numeric setters
  carry a checked finite payload, and the owner validates against the current range.
- **Related:** [ADR-0095](ADR-0095-agent-protocol-schema-crate.md) (the vocabulary lives in
  `flui-protocol`, beside the agent's wire `Role`), [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md),
  [ADR-0202](ADR-0202-typed-semantics-attributes.md).

## Context

`SemanticsRole` holds 34 structural roles; interactive kinds (button, checkbox, text field,
slider) are flags in `flui-semantics`, so the protocol vocabulary cannot name what most nodes
are. Of AccessKit's ~165 roles about 45 are reachable. `SemanticsAction` is Flutter's list:
`MoveCursor*`, `DidGain/LoseAccessibilityFocus`, `Copy`, `Cut`, `Paste` and `Dismiss` have no
path through AccessKit, while the inbound `ShowTooltip`, `HideTooltip`, `ReplaceSelectedText`,
`ScrollToPoint` and `SetSequentialFocusNavigationStartingPoint` are dropped. Bits 23–25 are
reserved to match `dart:ui`'s layout, which FLUI does not speak. Handlers receive
`(SemanticsAction, Option<ActionArgs>)` and decode their own arguments.

Pre-1.0, the owner chose to break this vocabulary now (spec D3).

## Decision

1. **Roles.** `SemanticsRole` lists every non-abstract WAI-ARIA 1.2 role with an AccessKit
   counterpart, the input kinds AccessKit distinguishes (text, multiline, password, search,
   email, number, phone, URL, date, time), and the desktop roles a window needs (window, pane,
   scroll view). DPUB-ARIA and Graphics-ARIA roles are out of scope. The enum stays flat:
   values and names are stable and carry no payload; data that ARIA ties to a role (`aria-level`
   and the rest) is an attribute ([ADR-0202](ADR-0202-typed-semantics-attributes.md)).
   Discriminants are dense from zero in declaration order.
2. **Actions.** `SemanticsAction` follows `accesskit::Action`: `Click`, `Focus`, `Blur`,
   `Expand`, `Collapse`, `Increment`, `Decrement`, `ScrollUp`, `ScrollDown`, `ScrollLeft`,
   `ScrollRight`, `ScrollIntoView`, `SetScrollOffset`, `SetTextSelection`,
   `ReplaceSelectedText`, `SetValue`, `SetNumericValue`, `ShowContextMenu`, `ShowTooltip`,
   `HideTooltip`, `CustomAction`. Bits are dense from zero; no bit is reserved. `LongPress`
   becomes `ShowContextMenu`; `MoveCursor*` become `SetTextSelection`; the rest without an
   AccessKit path are removed.
   - AccessKit's one `SetValue` carries a text or a number; FLUI keeps `SetValue` (text) and
     `SetNumericValue` apart, because which of them a node accepts decides what it publishes
     (the UIA value pattern it offers and whether a range is writable). Both publish as
     AccessKit's `SetValue`.
   - `ScrollToPoint` and `SetSequentialFocusNavigationStartingPoint` are not routed yet. The
     first carries a point in window pixels, which the semantics layer cannot convert without
     the presentation's scale; the second belongs to focus management rather than to a node.
     Each joins with its first consumer.
3. **Typed calls.** A handler receives an `ActionCall`: the action fused with exactly the
   payload it takes (`SetValue(String)`, `SetNumericValue(f64)`,
   `SetTextSelection(TextSelection)`, `ReplaceSelectedText(String)`,
   `SetScrollOffset(Offset)`, `ScrollIntoView(Option<Reveal>)`, `CustomAction(i32)`, the
   directional scrolls with their `ScrollUnit` (`Item` or `Page`, `Page` when the request has
   none), the rest bare), so a payload that does not fit its action cannot be expressed.
   `semantics_action_request_for` decodes a platform request into one and refuses a missing or
   mismatched payload with `ActionDecodeError` before any handler runs. One handler type per
   configuration, rather than one slot type per action, keeps the per-node ticket the widget
   layer advertises every action through (`flui-widgets` ARCHITECTURE §17); the widget
   builders are where the per-action argument types live.
4. **Names.** Each role and action keeps a `name()` in this vocabulary; the list of names is
   pinned by a test. The agent's wire `Role` stays a separate vocabulary that folds roles to
   what UI Automation reports, so the in-process and desktop backends stay comparable
   (ADR-0095 §4); it gains the values UI Automation distinguishes (`heading`, `tree`, `grid`,
   …) and `wire_role` is recomputed from the new mapping.
5. **One change.** `flui-semantics`, the agent projection, `tools/desktop-mcp`,
   `flui-devtools`, `flui-testing` and every widget call site move to this vocabulary in one
   change.

## Consequences

- Every interactive control has a role name tools and tests can query.
- Numeric values change once, before 0.2.0; nothing persisted depends on them.
- A request AccessKit can deliver always reaches a typed handler or a typed refusal.
- Behaviour that relied on removed actions (cursor movement by character) moves to
  `SetTextSelection`, which the text model ([ADR-0204](ADR-0204-synthetic-semantics-nodes.md))
  makes addressable.

## Verification

`every_role_maps_to_accesskit` and `every_action_round_trips_through_accesskit` walk `ALL`;
`every_aria_role_is_covered` checks the vocabulary against an independent ARIA 1.2 list;
`vocabulary_names_are_pinned`; `every_wire_action_routes_to_a_semantics_action` and
`wire_role_matches_the_windows_adapter_for_every_role_flui_publishes` recomputed for the new
roles; `platform_requests_decode_to_their_typed_call` for the refused missing and mismatched
payloads.
