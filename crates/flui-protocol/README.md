# flui-protocol

**The typed vocabulary FLUI shares with tests, devtools and agents.**

- `semantics`: `SemanticsRole` and `SemanticsAction`, FLUI's semantics vocabulary, which
  `flui-semantics` re-exports.
- `wire`: `Role`, `ActionName` and `Checked`, the agent-protocol names of
  [ADR-0080](../../docs/adr/ADR-0080-agent-protocol-desktop-contract.md).
- On them, the driver protocol of
  [ADR-0238](../../docs/adr/ADR-0238-flui-driver-protocol.md): the method table, the session,
  handles and trees, locators, actions and input, waits, events, captures, audits, the envelope
  and its framing, the error codes, and the input and native recording formats. The crate
  documentation lists the module of each.

## Versioning

The schema is versioned apart from this crate, by `version::PROTOCOL_VERSION`, with a golden
JSON schema per published version in `tests/schema/`. The rule for changing it, what counts as
additive and how a break is recorded, is in the `version` module's documentation. A new golden
file is written by `FLUI_PROTOCOL_BLESS=1 cargo nextest run -p flui-protocol --all-features`,
which writes a missing golden and never overwrites one.

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
