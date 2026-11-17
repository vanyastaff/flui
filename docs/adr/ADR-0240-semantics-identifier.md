# ADR-0240: Semantics carry a stable identifier for tests and automation

- **Status:** Accepted (2026-10-20)
- **Date:** 2026-10-17
- **Amended:** 2026-10-21 — §4: an identifier is unique among its siblings, as AccessKit's
  `author_id` requires; a clash is reported, not repaired.
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
   A node named only by its identifier (no role, label or text) would publish as a
   `GenericContainer`, which every AccessKit filter drops with its `author_id`, so it publishes as
   a `Group`. Inside a control AccessKit names from its contents (a button, checkbox, radio button,
   link or menu item, reached through generic containers) it stays generic: that name comes from
   the labels beneath the control through generic containers only, and a `Group` would cut it.
   There the control carries the identifier, its own or else the first of the containers lifted
   into it, as a merging `Semantics` absorbs a descendant's identifier in Flutter; a test selects
   the control. The OS adapters and the agent read the same tree (ADR-0095 §4).
3. The wire `Node` gains `identifier` (schema `0.5`), and `Locator.identifier` matches it exactly.
   When the OS reports an `automation_id` equal to the identifier, the normalized projection
   (ADR-0095 §4) treats the two as one field.
4. An identifier is unique among its siblings by contract, as AccessKit requires of `author_id`:
   an OS automation client matching it on two siblings finds only one. It need not be unique
   anywhere else, so a row template can name its "delete" button in every row. A clash is the
   author's defect and is reported, never repaired: translation publishes the identifiers as
   written, debug builds report the clash to the log naming the parent, and a wire read lists a
   `duplicate_identifier` diagnostic naming the parent and the siblings (schema `0.6`), so a test
   or agent can assert on it. A strict locator on an identifier more than one node carries fails
   `ambiguous` like any other criterion.

## Consequences

- [`interaction_gallery`](https://github.com/vanyastaff/flui/tree/e8a42673ad23a2e88614940a266cd01224dcb25f/examples/interaction_gallery), the shared fixture, gives every interactive widget an identifier, and
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
- it does not create a boundary;
- a node named only by it is a `Group` an adapter keeps, and inside a button it names the button,
  which keeps its name, also after an incremental publish changes the ancestor's role
  (`a_container_named_only_by_its_identifier_is_a_group_or_names_its_control`).

On Windows the a11y scenario reads it back as `AutomationId` through UIA.
