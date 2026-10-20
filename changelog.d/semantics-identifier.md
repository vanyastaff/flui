### Added

- **Semantics identifier** (`flui-semantics`, `flui-objects`, `flui-widgets`, `flui-material`,
  `flui-cupertino`, ADR-0240): `SemanticsConfiguration::set_identifier`/`identifier` give a node a
  stable id for tests and automation. It is published as AccessKit `author_id` (UI Automation's
  `AutomationId`, macOS `AXIdentifier`, AT-SPI's accessible id) and as the wire `Node.identifier`
  of an agent read, and it is never announced. It names only the node its own configuration
  forms: it makes no boundary and annotates nothing, and a parent or `MergeSemantics` that absorbs
  the configuration does not take it.
- `Semantics::identifier`, `MergeSemantics::identifier` (with `RenderMergeSemantics::set_identifier`)
  and an `identifier` builder on `RawButton`, `Disclosure`, `EditableText` and
  `RawTextField`; in `flui-material` on `ElevatedButton`, `FilledButton`, `OutlinedButton`,
  `TextButton`, `IconButton`, `Checkbox`, `Switch`, `Radio`, `Chip`, `FilterChip` and `TextField`;
  in `flui-cupertino` on `CupertinoButton`.

### Changed

- `CupertinoButton`'s button semantics wrap its gesture detector instead of sitting inside it, so
  the button's own configuration forms its node; the published node is unchanged.
