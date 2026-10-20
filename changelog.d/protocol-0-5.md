### Added

- **Driver protocol 0.5** (`flui-protocol`, ADR-0238): one method table, `METHODS`, with 40
  methods named `group.verb`, each a sealed `Method` with the side that serves it (agent, driver
  or lane), its effect class, its MCP tier (13 in the core set) and typed parameters and output.
  The types the methods take and answer arrive with it:
  - `Locator` (role, name, value, description, identifier, state, `within`, `has`, window and a
    strict-by-default `pick`) and `Target`;
  - `ElementRef` (`w3/e12`) and `TreeRevision`;
  - the input path and policy, the per-device `ActionSequence` in BiDi's `performActions` shape,
    the gestures and the input journal;
  - `Condition` and `WaitOutcome` with unsettled causes, and `SnapshotDelta`;
  - `Event`, `CaptureMeta`, `PerfReport` with budgets, `AuditReport` and `Verdict`;
  - `Hello` and `ErrorObject`.

  Four error codes are new: `ambiguous`, `not_actionable`, `stale` and `version`. `Node` gains
  `identifier`, and `Tree` gains `revision`. The schema is published as `protocol-0.5.json`.

### Changed

- **`InspectRect` is now `LogicalRect`** (`flui-protocol`), beside the new `LogicalPoint` and
  `LogicalSize`.
- **Protocol 0.5 renames the agent operations** (`flui-protocol`, ADR-0238): `windows`, `read`,
  `act` and `motion` become `windows.list`, `semantics.read`, `semantics.act` and `motion.set`.
  The development agent endpoint keeps answering the old names and says protocol 0.4 until the
  driver agent replaces it.
