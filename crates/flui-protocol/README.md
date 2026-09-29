# flui-protocol

**The typed vocabulary FLUI shares with tests, devtools and agents.**

- `SemanticsRole` and `SemanticsAction` — FLUI's semantics vocabulary,
  modelled on Flutter's `dart:ui` enums. `flui-semantics` re-exports them.
- `Role`, `ActionName` and `Checked` — the agent-protocol wire names of
  [ADR-0080](../../docs/adr/ADR-0080-agent-protocol-desktop-contract.md), shared
  by the desktop agent server.

On that vocabulary sits the schema of what an agent reads and sends, in
ADR-0080's shapes:

- `ElementId` (`e12`) and `WindowId` (`w3`), the handles a reply names;
- `Node`, one element with its role, name, value, state and actions, and
  `Tree`, what a read returns (with `truncated`, and `coordinates` when the
  rectangles are not screen ones);
- `ReadQuery` (`root`, `max_depth`, `max_nodes`) and `ActionRequest`;
- `ErrorCode`, ADR-0080's fifteen codes, and `Retry`;
- `outline`, the one-line-per-element text an agent reads in place of JSON.

## Versioning

The schema is versioned apart from this crate: `PROTOCOL_VERSION` (`0.1`), and a
golden JSON schema per version in `tests/schema/`. Any change to the schema bumps
the minor version and publishes the new golden file (`FLUI_PROTOCOL_BLESS=1 cargo
nextest run -p flui-protocol --all-features`, which writes a missing golden and
never overwrites one). A change that only adds, in ADR-0080's sense, needs nothing
more: a test checks that every published schema is contained in the next. A change
that does not only add is listed in `version::BREAKING` with the ADR that decided
it; after 1.0 it also bumps the major version.

Every vocabulary enum is `#[non_exhaustive]` and has an `ALL` slice generated
from the same list as the enum. The crate depends on nothing in the workspace
and names no upstream type in its API
([ADR-0095](../../docs/adr/ADR-0095-agent-protocol-schema-crate.md)).

## Features

| Feature | Adds |
|---------|------|
| `serde` | `Serialize`/`Deserialize` for the wire vocabulary and schema, in ADR-0080's spelling |
| `schemars` | `JsonSchema` for the wire vocabulary and schema (implies `serde`) |

Part of the [FLUI](https://github.com/vanyastaff/flui) workspace.

## License

MIT OR Apache-2.0, per the workspace license.
