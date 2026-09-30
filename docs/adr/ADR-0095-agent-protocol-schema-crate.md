# ADR-0095: flui-protocol is the typed schema shared by tests, devtools and agents

- **Status:** Accepted in part (2026-09-26, extended 2026-09-29): §1 as far as the crate
  itself goes (tier C, `stable`, `serde` and `schemars` behind features, no upstream type),
  and §1's schema for handles, nodes, the tree, the read query, the action request, the error
  codes and the protocol version; from §2 the lift of the wire `Role`, `ActionName` and
  `Checked` out of the desktop server, the move of `SemanticsRole` and `SemanticsAction` into
  the crate (with ADR-0089 §3's `ALL` rule), and the semantics-to-wire mapping, amended below
  to live in `flui-semantics`; from §3 the realm half of the in-process backend (a
  `SemanticsAgent` that reads and acts through the realm's owner inbox), and the in-process
  server in `flui-devtools` with its local endpoint (a named pipe or Unix socket, a launch
  token, debug builds only), amended below. Still Proposed: §1's finder criteria, handle kinds
  beyond elements and windows, `effect` as a schema type, widget catalog and event-log shapes;
  the `flui-mcp` library; `flui-testing`'s query types; §3's `flui mcp` and the handle table
  across windows; and §§4–5.
- **Date:** 2026-09-25
- **Amends:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md) (settles its "Not
  decided here" in-process transport; the wire contract is unchanged)
- **Related:** [ADR-0040](ADR-0040-tree-observation-seam.md),
  [ADR-0079](ADR-0079-keyboard-activation-and-focus-for-assistive-technology.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md)
- **Refs:** decision D16 in the [decision index](../../design/decisions.md); the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md)

The accepted part added `crates/flui-protocol` (tier C, order 2, `stable`), moved
`SemanticsRole` and `SemanticsAction` into it (`flui-semantics` re-exports them), and made
`tools/desktop-mcp` take its wire vocabulary from it. It then added the schema a read and an
action need (`ElementId`, `WindowId`, `Node`, `Tree`, `ReadQuery`, `ActionRequest`,
`ErrorCode`, `Retry`, `outline`, `PROTOCOL_VERSION`), the semantics-to-wire projection in
`flui-semantics` (`SemanticsOwner::read_wire` and `resolve_wire_action`), and
`flui_runtime::SemanticsAgent`. `flui-app` now vends it through the development-agent hook
(`flui_view::dev_agent::DevAgentHook`), and `flui-devtools`' `agent` feature serves it over a
local endpoint. Line citations in Context are to `d7007f547`, before those changes.

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
  opens no port (`packages/flui-devtools/src/lib.rs:16-23`); its tree access is limited to a
  counting observer over the ADR-0040 seam (the `inspector` feature,
  `packages/flui-devtools/Cargo.toml:63-66`).

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
the same wire `Role`, pinned by a test over every semantics role (the own-vocabulary rule and
the move of `SemanticsRole` itself are ADR-0089's). `flui-testing`'s queries take
`flui-protocol` types instead of re-exporting AccessKit's.

*Amended on acceptance:* the mapping is a function in `flui-semantics`, not in
`flui-protocol`. It projects the AccessKit node `flui-semantics` publishes to the platform
adapter, read through `accesskit_consumer` with the adapters' own filter and pattern
predicates, and folds each AccessKit role to the wire role the desktop server reads for it
through UI Automation. `flui-protocol` can see neither FLUI's flags nor AccessKit (ADR-0089),
and a second cascade over the flags beside the one that feeds AccessKit would drift from what
the OS reports (`crates/flui-semantics/ARCHITECTURE.md`, mapping decision 7).

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
  compatible with a stateless MCP transport. The realm's `SemanticsAgent` reports render
  identities scoped to one presentation (another window can report the same `e<n>`); the
  backend's table maps them when a session spans windows.

**Amended on acceptance (the server and its endpoint).**

- *The seam.* `flui-devtools` is an official package and cannot name `flui-runtime`, and
  `flui-app` cannot name an official package. So the application installs the server as a
  `flui_view::dev_agent::DevAgentHook` on its configuration (`AppConfig::with_dev_agent`), which
  the package reaches through `flui-sdk`; the host attaches it once per event loop and hands it
  an `AgentWindow` per window with content. `flui_runtime::dev_agent::DevAgentHost` holds the
  hook's containment for every host (the desktop and iOS runners, and `flui-testing`'s headless
  realm, which is how CI exercises it); no transport is in the runtime. An `AgentWindow` holds
  its window's agent weakly, so a closed window answers `gone` with kind `window` and needs no
  close notification, and holds the window's semantics handle strongly, so semantics are
  collected only while the tool keeps the window. `DevAgentHook::attach` answers whether the
  hook serves: one that does not (a release build, missing or short credentials, a failed bind)
  stays unattached and is handed no window, so it costs the application nothing. Android and
  web drive no hook.
- *Framing.* Newline-delimited JSON, lines of at most 1 MiB, a `{"hello":{"token":…}}` first
  line, then `windows`, `read` and `act` requests; every `read` and `act` names its window,
  because element ids are scoped to a window. Errors use ADR-0080's error object with `kind`
  and `handle` for a `gone` or `unknown_handle`.
- *Credentials.* The launching tool passes the endpoint and the token in the environment
  (`FLUI_AGENT_ENDPOINT`, `FLUI_AGENT_TOKEN`); the token is at least 32 bytes and compared in
  constant time. It defends against other users and remote callers, not the same user's other
  processes, which can read the environment. The Windows pipe admits the current user only (the
  process token's user SID, not the object owner, which is the Administrators group for an
  elevated process) and refuses remote clients; a Unix socket must sit in a `0700` directory the current user owns.
- *Timeouts.* The server waits a bounded time for the owner's answer and never makes the owner
  wait on it. A read that times out answers `timeout` (retry `soon`); an action that times out
  answers `timeout` with the `may_have_run` effect, because it stays queued and runs at the next
  drain.
- *Builds.* The server is compiled only with the package's `agent` feature, which an
  application enables behind its own development feature, and it stays inert outside
  `debug_assertions`.

### 4. A normalized outline projection is the cross-backend contract

`flui-protocol` defines a pure function from a node tree to a **normalized outline**:

- roles folded to the subset UI Automation can express;
- OS chrome (window frame, title bar, system menu buttons) removed;
- native fields (`native_role`, `automation_id`, `class_name`) dropped;
- handles renumbered in traversal order, one node per line, deterministic.

For the same application state, the projection of the UIA backend's tree and the projection of
the in-process backend's tree are equal. The projection is a test artifact and a golden-file
format, not the wire: replies still carry `role` and `native_role` exactly as ADR-0080 defines.
Under §2 as amended, the in-process backend's replies already carry the role UI Automation
would report (the desktop backend's `role`), with the AccessKit name in `native_role`, so the
projection's fold is the identity on them.

### 5. Tests, devtools and agents share artifacts

A finder built from `flui-protocol` query types runs in `flui test` against the headless
backend and through `flui mcp` against a live app with the same meaning. A normalized outline
recorded by one is a valid golden file for the other.

### Versioning

The schema carries its own version, `flui_protocol::PROTOCOL_VERSION` (`0.1`), serialized
as the `protocol` field of a `Tree`. Each version's JSON schema is a golden file
(`crates/flui-protocol/tests/schema/protocol-<version>.json`), and a test refuses a schema that
differs from its version's file. Any change to the schema bumps `minor` and adds a golden
file; a change additive in ADR-0080's sense needs nothing more, and a test checks that each
published schema is contained in the next. A change that is not additive is listed in
`version::BREAKING` with the ADR that decided it; after 1.0 it also bumps `major` and that ADR
supersedes ADR-0080. A change that gives an existing field a new meaning is not additive
either, though the schema's shape does not show it; it goes under a new name instead. An older
reader still reads a newer reply: an unknown field is ignored, an unknown role reads as
`unknown`, an unknown action name is dropped from a node's `actions`, and an unknown error code
reads as `platform`. Two optional fields are new here: `protocol` on a `Tree` (a reply without it
reads as unversioned), and `surface_rect` on a `Node`, the bounds from the window's drawing
surface that the in-process backend reports in place of `rect`, which stays screen pixels: the
realm knows no window position, and a client that reads `rect` as screen pixels must find none
rather than a misplaced one.

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

For the accepted part:

- `cargo nextest run -p flui-protocol --all-features`:
  `the_advertised_action_names_are_adr_0080s`,
  `every_wire_name_serializes_to_its_vocabulary_string` (which also checks the schemars
  schemas when that feature is on). The serde test compiles only with its feature.
- `cargo nextest run -p flui-semantics`: `roles_and_checkbox_states_translate_to_accesskit` (the
  pin for the mapping now that `explicit_role` ends in a wildcard) and
  `every_wire_action_routes_to_a_semantics_action` (every `ActionName` reaches a FLUI action
  through AccessKit's Windows adapter).
- `cargo nextest run -p flui-desktop-mcp`: `every_action_name_has_a_uia_pattern`, and the
  server's reply and schema tests pass unchanged on the lifted types.
- `cargo xtask workspace` places `flui-protocol` in tier C, its `tier` in the manifest.

- `cargo nextest run -p flui-protocol --all-features`: `the_wire_schema_is_the_one_its_version_published`,
  `every_published_schema_is_additive_to_the_next` and
  `the_additivity_check_refuses_a_removed_field_a_respelled_name_and_a_new_required_one`
  (`tests/wire_schema.rs`); `the_error_codes_are_adr_0080s`,
  `element_ids_serialize_as_e_handles_and_refuse_other_spellings`,
  `a_node_at_its_defaults_serializes_only_role_id_and_native_role` and
  `outline_is_one_line_per_element`, `a_newer_role_or_action_name_does_not_fail_the_read`,
  `surface_bounds_are_not_spelled_rect` and
  `a_tree_says_its_version_and_a_reply_without_one_reads_as_unversioned`.
- **Mapping pinned.** `cargo nextest run -p flui-semantics agent`:
  `every_role_but_the_documented_ones_reads_as_a_wire_role` walks every semantics role (`none`,
  `drag_handle` and `hot_key` are the documented exceptions, published as a container the wire
  lifts), with `every_role_bearing_flag_reads_as_the_role_uia_reports`,
  `wire_role_matches_the_windows_adapter_for_every_role_flui_publishes`,
  `the_role_fold_was_transcribed_from_the_locked_windows_adapter`,
  `generic_containers_are_lifted_and_hidden_subtrees_dropped`,
  `advertised_actions_follow_the_uia_patterns`, `expand_on_an_expanded_node_is_action_unsupported`,
  `set_value_reaches_set_text_with_its_text`, `a_disabled_node_refuses_with_disabled`,
  `read_honours_max_depth_and_max_nodes_and_says_truncated` and
  `a_read_tree_round_trips_through_json`.
- **The realm half of §3.** `cargo nextest run -p flui-runtime agent_semantics`: a counter's
  tree read as wire nodes, a tap through the agent delivered to the widget's handler (a
  `GestureDetector` runs it in the next frame, so the reply's `Ok` means delivered, and the
  signal's new value reads two frames on), `busy` before the first semantics frame, collection
  stopping with the last agent even while a reply is unanswered, `gone` for a node that left the
  tree and for a closed window, `unknown_handle` for a handle no read reported (for a read scoped
  to it as for an action), a record of issued handles bounded by render slots, `busy` on a full
  inbox, a panicking handler failing its reply first and leaving the queued read for the next
  drain, a panic before the handler reported as `ResolvePanicked` rather than the handler's, and
  traces without labels or values. The in-process `expand`/`collapse` check reads the committed
  tree and does not close the double-toggle race (`flui-semantics` mapping decisions 5 and 7).
- **The server of §3.** `cargo nextest run -p flui-devtools --features agent --test
  agent_endpoint`: `reads_the_counter_and_taps_it_over_the_endpoint` (a headless counter served
  over a real pipe or socket: the hello, `windows`, a read retried while `busy`, a tap, the new
  count read back, the handler run once) and the rows of `the_endpoint_contains_every_failure`
  (`a_connection_without_the_right_token_is_closed_unread`,
  `a_client_leaving_mid_request_does_not_stall_the_next`,
  `an_unanswered_request_times_out_and_a_timed_out_act_still_runs`,
  `malformed_and_oversized_lines`, `a_closed_window_answers_gone_and_leaves_the_list`,
  `detach_closes_the_endpoint`, `a_bind_failure_leaves_the_realm_running`,
  `traces_carry_no_labels_or_values`); `dev_agent_host_contains_its_hook` in `flui-runtime`
  (each hook method panicking alone, a panicking `Drop`, a nested call, a refused second
  attach, a new loop, nothing vended while unattached); `an_agent_for_a_closed_presentation_answers_gone`
  (a development window goes with its presentation); and in `flui-app`
  `main_window_agent_hook_stays_attached_across_failed_reopens_and_detaches_with_loop`. The
  windowed runners' hand-over (`desktop.rs`, `ios.rs`) runs behind GPU initialisation and is
  not executed in CI.
- **Round trip against the wire, in part.** `cargo nextest run -p flui-desktop-mcp`:
  `the_desktop_node_is_a_protocol_node` (the desktop server's node JSON reads as
  `flui_protocol::Node`, writes back unchanged, and outlines the same) and
  `every_code_is_a_protocol_error_code`.

Not yet built:

- **Round trip against the wire, the rest.** The desktop server switches its replies to the
  lifted types and its existing reply tests pass unchanged.
- **Projection equality.** On Windows, `cargo xtask device windows-a11y` runs a FLUI example,
  reads it through the UIA backend and through the in-process backend, and asserts the two
  normalized outlines are equal. A deliberately broken mapping (one role swapped) must make the
  check fail.
- **Shared finder.** One finder, written once with `flui-protocol` types, passes in a headless
  `flui-testing` test and through `flui mcp` against the same example.
- **No leak.** The upstream-type gate of ADR-0089 covers `flui-protocol` and finds no
  `accesskit` or `rmcp` path in its public API.
