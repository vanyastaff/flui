# ADR-0240: Semantics carry a stable identifier for tests and automation

- **Status:** Accepted (2026-10-20)
- **Date:** 2026-10-17
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md),
  [ADR-0203](ADR-0203-semantics-vocabulary.md),
  [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (`automation_id`)
- **Spec:** [flui-driver](../plans/specs/flui-driver/requirements.md)

## Context

FLUI semantics have no identifier. A test or agent can select only by role, name and value. These
change with localization and copy edits, and two "Delete" buttons in a list differ only by position.
ADR-0080 carries `automation_id`, but only UI Automation fills it, and FLUI never sets it. Every
mature automation stack has the escape hatch: Flutter's `Semantics.identifier` (and `ValueKey` for
its driver), Compose's `testTag`, Android `resource-id`, iOS `accessibilityIdentifier`, Playwright's
`getByTestId`.

## Decision

1. `SemanticsConfiguration` gains an optional `identifier: Option<Arc<str>>`. It is set by the
   widget author (`Semantics::identifier(..)` and an `identifier` builder on interactive widgets
   that already build semantics). It is never announced to assistive-technology users, never merged
   into a parent's label, and does not by itself make a node a semantics boundary.
2. Translation exports it as AccessKit `author_id`, which the AccessKit adapters map to UIA
   `AutomationId`, macOS `AXIdentifier` and AT-SPI's accessible id. When Android, iOS and web
   adapters exist they map it to `viewIdResourceName`, `accessibilityIdentifier` and a
   `data-flui-id` DOM attribute; that obligation is recorded in their gaps entries.
3. The wire `Node` gains `identifier` (schema `0.5`), and `Locator.identifier` matches it exactly.
   When the OS reports an `automation_id` equal to the identifier, the normalized projection
   (ADR-0095 §4) treats the two as one field.
4. Identifiers are not required to be unique. A strict locator on a duplicated identifier fails
   `ambiguous` like any other criterion.

## Consequences

- `interaction_gallery`, the shared fixture, gives every interactive widget an identifier, and
  the device scenarios select by it.
- Changing a widget's visible text no longer breaks its tests.

## Alternatives considered

- **Widget `Key` paths as selectors.** Rejected: only the in-app lane could resolve them, OS lanes
  never see keys, and keys are about reconciliation, not addressing.
- **Reuse `label` with a convention.** Rejected: it would be announced to users.

## Verification

`cargo nextest run -p flui-semantics identifier`:

- an identifier reaches the AccessKit node as `author_id`;
- it is absent from the label and from a merged parent;
- it does not create a boundary.

On Windows the a11y scenario reads it back as `AutomationId` through UIA.
