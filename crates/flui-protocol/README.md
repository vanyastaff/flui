# flui-protocol

**The typed vocabulary FLUI shares with tests, devtools and agents.**

- `SemanticsRole` and `SemanticsAction` — FLUI's semantics vocabulary,
  modelled on Flutter's `dart:ui` enums. `flui-semantics` re-exports them.
- `Role`, `ActionName` and `Checked` — the agent-protocol wire names of
  [ADR-0080](../../docs/adr/ADR-0080-agent-protocol-desktop-contract.md), shared
  by the desktop agent server.

Every vocabulary enum is `#[non_exhaustive]` and has an `ALL` slice generated
from the same list as the enum. The crate depends on nothing in the workspace
and names no upstream type in its API
([ADR-0095](../../docs/adr/ADR-0095-agent-protocol-schema-crate.md)).

## Features

| Feature | Adds |
|---------|------|
| `serde` | `Serialize`/`Deserialize` for the wire vocabulary, in ADR-0080's spelling |
| `schemars` | `JsonSchema` for the wire vocabulary (implies `serde`) |

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace.

## License

MIT OR Apache-2.0, per the workspace license.
