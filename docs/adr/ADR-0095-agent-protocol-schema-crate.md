# ADR-0095: flui-protocol is the typed schema shared by tests, devtools and agents

- **Status:** Proposed
- **Date:** 2026-09-25
- **Amends (on acceptance):** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (settles its "Not decided
  here" in-process transport; the wire contract is unchanged)
- **Related:** [ADR-0040](ADR-0040-tree-observation-seam.md),
  [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md)
- **Refs:** decision D16 in the [decision index](../../design/decisions.md); the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md)

Nothing in `crates/` or `tools/` changes as part of this ADR.

## Context

ADR-0080 fixed one wire contract for agents: MCP as the transport, AccessKit role names as the
vocabulary, typed replies, fixed error codes, session handles. It named the in-process FLUI
backend as the second backend and left its transport open. Today the contract has exactly one
implementation, and the types that define it live inside that implementation:

- **The vocabulary is private to the desktop server.** The wire `Role` — AccessKit names in
  snake case, `#[non_exhaustive]`, with `serde` and `schemars` derives — is declared in
  `tools/desktop-mcp/src/a11y/role.rs:17`, and the action names beside it at `role.rs:139`.
  The backend seam is `AccessibilityBackend` (`tools/desktop-mcp/src/a11y/mod.rs:498`) and the
  outline format is `a11y::outline` (`a11y/mod.rs:388`). `tools/desktop-mcp` is a binary
  workspace member (`Cargo.toml:63`); nothing else can depend on those types.
- **FLUI's own semantics speak a different vocabulary.** `SemanticsRole`
  (`crates/flui-semantics/src/role.rs:30-32`) is a 33-variant `#[repr(u32)]` enum, not
  `#[non_exhaustive]`, that describes structural roles only; interactive kinds are flags
  (`role.rs:25-29`). It reaches AccessKit through `explicit_role`
  (`crates/flui-semantics/src/accesskit_translation.rs:141`), not through the wire names.
- **Tests read a third shape.** `flui-testing` re-exports AccessKit's own `Action`, `Role`,
  `NodeId` and friends for its accessibility queries (`crates/flui-testing/src/a11y.rs:36-39`),
  and the agent-workflow test asserts against an unbounded diagnostics string
  (`tests/agent_workflow.rs:136-140`, `to_string_deep`).
- **There is no in-process server.** `flui-devtools` says of itself that it walks no tree and
  opens no port (`crates/flui-devtools/src/lib.rs:15-24`); its tree access is limited to a
  counting observer over the ADR-0040 seam (the `inspector` feature,
  `crates/flui-devtools/Cargo.toml:62-65`).

So a finder written for `flui test`, a query an agent sends through the desktop server and a
DevTools view of the same app share no type, and nothing can check that the two backends
describe one app the same way. The review weighed making a FLUI protocol primary with MCP as a
projection of it; that reverses ADR-0080 and was withdrawn. Embedding an MCP server inside the
application (Slint's approach) was also considered.

## Decision

### 1. `flui-protocol` holds the schema; MCP stays the transport

A new crate, `flui-protocol`, holds the typed schema of what MCP itself does not define:
queries (criteria, scope, `max_depth`, `max_nodes`), nodes and the outline, actions, handle
kinds, error codes with `retry` and `effect`, the widget catalog descriptors, and event-log
entries. Types derive `serde` and `schemars`, as the desktop server's already do, behind the
crate's `serde` and `schemars` features (ADR-0089 §2 allows both only as derive impls behind a
feature).

- **Placement.** A contract crate in the C tier (ADR-0081), with no dependency on any runtime,
  platform or OS crate, so `flui-testing`, `flui-devtools` and the MCP server can all depend on
  it. Its kind is `stable` from creation (ADR-0081 §3); like the other two Stable crates its
  surface may still break in `0.x` releases and freezes at H3. It is versioned on its own, not
  with AccessKit or with the MCP SDK.
- **No upstream type in its signatures** (ADR-0089): no `accesskit`, no `rmcp`, no OS type.
- **ADR-0080 is the wire.** Every shape `flui-protocol` defines serializes to exactly the
  replies, codes and names ADR-0080 fixed. Anything this crate adds is additive in ADR-0080's
  own sense: new tools, new optional parameters, new optional fields.

### 2. The role enum is lifted out of the desktop server

The desktop server's `Role` and action names move into `flui-protocol` unchanged in their wire
spelling. `tools/desktop-mcp` becomes a library plus a thin binary (the MCP server package,
`flui-mcp`) that uses them. The in-process backend maps FLUI semantics (role plus flags) onto
the same wire `Role`, and the mapping is a function in `flui-protocol`, pinned by a test over
every semantics role (the own-vocabulary rule and the move of `SemanticsRole` itself are
ADR-0089's). `flui-testing`'s queries take `flui-protocol` types instead of re-exporting
AccessKit's.

### 3. The in-process transport

This settles ADR-0080's open item:

- `flui-devtools` becomes the in-process protocol server: a second `AccessibilityBackend` over
  the realm's own semantics tree, compiled only into development builds that add the package.
- It listens on a local endpoint only — a named pipe on Windows, a Unix domain socket elsewhere
  — authenticated by a token generated at launch and handed to the tool that launched the app.
  No TCP port.
- `flui mcp` is an MCP server over stdio that proxies to that endpoint. The agent sees one MCP
  server whether it drives a FLUI app in-process or any app through the OS.
- The handle table belongs to the backend, not to the MCP session, which keeps the design
  compatible with a stateless MCP transport.

### 4. A normalized outline projection is the cross-backend contract

`flui-protocol` defines a pure function from a node tree to a **normalized outline**:

- roles folded to the subset UI Automation can express;
- OS chrome (window frame, title bar, system menu buttons) removed;
- native fields (`native_role`, `automation_id`, `class_name`) dropped;
- handles renumbered in traversal order, one node per line, deterministic.

For the same application state, the projection of the UIA backend's tree and the projection of
the in-process backend's tree are equal. The projection is a test artifact and a golden-file
format, not the wire: replies still carry `role` and `native_role` exactly as ADR-0080 defines.

### 5. Tests, devtools and agents share artifacts

A finder built from `flui-protocol` query types runs in `flui test` against the headless
backend and through `flui mcp` against a live app with the same meaning. A normalized outline
recorded by one is a valid golden file for the other.

## Alternatives considered

- **FLUI protocol primary, MCP as a projection.** Rejected: it reverses ADR-0080, whose wire
  shapes agents and skills already use, for no capability MCP lacks.
- **An MCP server inside the application.** Rejected: every app would carry an MCP SDK and a
  listener, and the agent would see a different server per app. A stdio proxy keeps one server.
- **Merge the schema into `flui-devtools`.** Rejected: tests and the desktop server would then
  depend on the in-process server to name a role. Schema and server are separate.
- **Use AccessKit's types as the schema.** Rejected: AccessKit shipped 13 breaking releases
  between January 2024 and September 2026 (review count), and a Stable crate cannot follow
  that cadence (ADR-0089).
- **Require identical raw outlines on both backends.** Rejected: UIA reports window chrome and
  control types a FLUI tree does not have. Equality holds only after normalization, and the
  normalization is written down so it cannot drift silently.

## Consequences

- `tools/desktop-mcp` is restructured into the `flui-mcp` package with a library; its wire
  behaviour does not change.
- `flui-testing`'s public accessibility helpers change type: code that names `accesskit::Role`
  through `flui_testing` must name the `flui-protocol` type instead.
- `flui-semantics` gains a dependency on `flui-protocol`.
- `flui-devtools` gains a real server and a security boundary: debug-only, local endpoint,
  launch token.
- The CLI's `--json` event stream (`crates/flui-cli/src/commands/run.rs:420`, consumed by
  `cargo xtask device`) is a candidate for the event-log schema; moving it is a separate,
  additive change.
- Bounded reads, action replies that return the post-action outline, and a catalog index for
  agents are follow-ups that fit this crate; they are not decided here.

## Verification

None of these exists yet.

- **Round trip against the wire.** Every reply type in `flui-protocol` serializes to the JSON the
  desktop server emits today; the server's existing reply tests pass unchanged after it switches
  to the lifted types.
- **Mapping pinned.** A test walks every semantics role and asserts it maps to a wire `Role`
  other than `unknown`, except the roles documented as having no counterpart.
- **Projection equality.** On Windows, `cargo xtask device windows-a11y` runs a FLUI example,
  reads it through the UIA backend and through the in-process backend, and asserts the two
  normalized outlines are equal. A deliberately broken mapping (one role swapped) must make the
  check fail.
- **Shared finder.** One finder, written once with `flui-protocol` types, passes in a headless
  `flui-testing` test and through `flui mcp` against the same example.
- **No leak.** The upstream-type gate of ADR-0089 covers `flui-protocol` and finds no
  `accesskit` or `rmcp` path in its public API.
