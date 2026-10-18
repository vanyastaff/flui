# ADR-0238: One driver protocol and one `flui-driver` crate for tests, people and agents

- **Status:** Proposed
- **Date:** 2026-10-17
- **Supersedes in part:** [ADR-0080](ADR-0080-agent-protocol-desktop-contract.md): the desktop
  MCP server as the backend of the wire contract (retired), the closed error-code list (four codes
  added), and the session-handle spelling of elements across windows. The rest of ADR-0080 stays:
  its vocabulary, typed replies widened with an error branch, outline as the default reading,
  `effect`, `retry` and `readback_failed`.
- **Settles:** the still-Proposed items of [ADR-0095](ADR-0095-agent-protocol-schema-crate.md)
  §1 (finder criteria, handle kinds, `effect` as a schema type, event shapes), §3 (`flui mcp` and
  the handle table across windows) and §5 (one finder shared by tests and agents). §4's projection
  stays as ADR-0095 wrote it and becomes a driver method.
- **Related:** [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (the app's half),
  [ADR-0240](ADR-0240-semantics-identifier.md), [ADR-0241](ADR-0241-driver-trace-format.md),
  [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md),
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md)
- **Spec:** [flui-driver](../plans/specs/flui-driver/design.md)

## Context

Three stacks drive FLUI apps and none of them composes with the others:

- `tools/desktop-mcp` observes any Windows app from outside, through MCP;
- the devtools agent serves one app's semantics from inside it (`windows`, `read`, `act`,
  `motion`);
- the xtask device drivers each hand-roll their input, their result channel and their verdict
  format, together with Python and Swift scripts.

Rust tests use a fourth shape again, `flui-testing`'s AccessKit queries. ADR-0080 made MCP the wire
and ADR-0095 made `flui-protocol` its schema, but neither gives the operations a test needs beyond
reading and acting: locating, waiting on frames, synthetic and real input with verified effect,
trees other than semantics, pixels and recordings. Nor does either let one definition serve a Rust
API, a CLI and MCP.

## Decision

1. **One method table.** `flui-protocol` (schema `0.5`) defines every operation as a sealed
   `Method` with a wire name (`group.verb`), the side that serves it (`Agent`, `Driver` or
   `Lane`), an effect class (`ReadOnly`, `Acts`, `Destructive`) and typed `Params`/`Output`. The
   generated `METHODS` list is the single source of the MCP tool list (deterministic order, tool name
   `group_verb`, schemas from `schemars`, annotations from the effect class), the CLI's subcommands
   (`flui drive <group> <verb>`, plus `flui drive call <method> --params <json>` for every method),
   the agent's dispatch and the schema golden file. A test fails when a method lacks a CLI subcommand
   or an MCP tool.
2. **Envelope.** NDJSON lines of at most 1 MiB: a `hello` with token, protocol version and client;
   then `{id, method, params}` requests answered by `{id, result}` or `{id, error}`, and
   `{event, params}` after `events.subscribe` (WebDriver BiDi's shape). Binary payloads go in
   length-prefixed frames that a result announces (`blob`); the BiDi transport base64-encodes them.
   MCP's JSON-RPC exists only in the `flui mcp` adapter.
3. **Errors.** ADR-0080's error object and codes stay, and four codes are added:
   - `ambiguous`, with `candidates`;
   - `not_actionable`, with `failed: attached | visible | stable | enabled | receives` and
     `covered_by`;
   - `stale`, a handle of an earlier generation that cannot be resolved;
   - `version`, no shared protocol version.

   A reader that predates them reads `platform`. `not_supported` names the missing `capability`.
4. **Locators, not handles, are what callers write.** A `Locator` is role, name, value and
   description (`exact`, `contains`, `prefix`, optionally case-insensitive), `identifier`
   (ADR-0240), state filter, `within`, `has`, window and `pick`. `pick` defaults to strict: an
   action through a locator with several matches fails `ambiguous`. A locator is re-resolved at
   every use. A resolved element is an `ElementRef` (`w3/e12`), whose element part is the app's
   generational id. Across windows the window part disambiguates, which is the handle table §3 of
   ADR-0095 left open. The app mints every id; clients and the MCP adapter never do.
5. **Input paths are explicit.** Every action names `semantic`, `synthetic` or `os`. A session has a
   preferred path, `synthetic` by default on every target. Each result names the path used and
   its `effect`, and `require` refuses a fallback. OS input is opt-in and never takes a
   developer's desktop unasked:
   - it is announced before the first event (devices and estimated duration);
   - any input the lane did not inject aborts the run, releases everything and ends it
     `cannot_verify` (`interrupted_by_user`);
   - a lane that cannot tell the user's input from its own refuses OS input on a shared desktop;
   - the session and report name an isolation mode (Windows Sandbox or a second session, Xvfb or a
     separate seat, an emulator, the Simulator, headless Chrome) or `shared`. For `os` input, `ran` means the app's input journal observed the event; otherwise the
   effect is `may_have_run`. An `Acts` method is never resent on timeout. Input is a typed
   per-device action sequence in BiDi `input.performActions` form (pointer: mouse, touch or pen;
   wheel; keys; text; IME; trackpad). Gestures (`element.tap`, `drag_to`, `pinch`, …) are driver
   methods composed from locate, actionability, perform and observe.
6. **Actionability before pointer input.** The app answers attached, visible, stable over two
   frames, enabled and receives (its hit-test at the point reaches the element) in one frame.
   The driver repeats until the action's timeout, and `force` skips all but attached.
7. **Waits are conditions on the frame and event timeline.** `wait.until` takes a `Condition`:
   appears, gone, matches an expectation, settled, N frames, an event, `all`, `any`. The app
   evaluates it at frame boundaries and after input. A timeout answers the last observation and the
   unsettled causes. Retrying assertions are waits, not client polls (default timeout 5 s).
8. **The crate.** `flui-driver` is a new published crate in tier H, between `flui-app` and
   `flui-cli`, versioned `0.N` (Evolving). It depends on `flui-protocol`, on `flui-testing` behind
   `headless`, and on OS crates for its lanes. It never depends on `flui-platform` or `flui-app`.
   Its `allowed-dependents` are `flui-cli`, tools and dev-dependencies, so no framework crate can
   link it. ADR-0082's rule, that OS types stay inside `flui-platform`, governs framework crates;
   `flui-driver` is a client that is never linked into an app, which is why its OS code is allowed.
   If `cargo xtask workspace` refuses an Evolving crate in tier H, the implementing PR amends
   ADR-0081's kind table here rather than reshaping the crate.
9. **Front ends.** The Rust API (`Driver`, `Window`, `Element`, `expect`), `flui drive` and
   `flui mcp` (MCP revision 2026-07-28) all call the same driver methods. The MCP adapter keeps no
   handle state.
   - Each method has a tier. MCP lists a core set of at most 13 tools by default: launch, close,
     list windows, read, locate, act, tap, type, keys, scroll into view, wait, capture, audit a screen. The rest
     sit behind `--caps` groups (`input`, `inspect`, `pixels`, `record`, `perf`, `audit`,
     `explore`, `layout`, `vision`, the last holding coordinate targets).
   - After an action, the reading is a snapshot delta by default (appeared, gone, changed against
     the action's starting revision). The full outline and none are options.
10. **Checks beyond correctness are methods too.**
    - `perf.start`/`stop`/`expect`: frame times, dropped and janky frames against the refresh
      period, input-to-present latency from the journal, per-target budgets. A target without a
      budget is cannot-verify.
    - `audit.a11y` and `audit.alternatives`: names, focus order, target size, gesture
      alternatives. The rules live in `flui-semantics` and `flui-testing` shares them.
    - `explore.run`/`shrink`: seeded random actions with invariants, and delta-debugging to a
      minimal trace.
    - `layout.*` and `audit.layout`: exact geometry facts from the render tree and text layout
      (boxes, clips, ink bounds, baselines, colours), assertions such as `to_be_centered_in`, a lint
      for off-centre text, near-miss alignment, uneven gaps, overlap, truncation and contrast, a
      layout-snapshot golden diffed in numbers, and annotated captures as evidence. Findings come
      from facts, never from looking at an image.
    - `audit.screen`: the layout and accessibility audits together, answering what was checked
      and what was not. A requested check that did not run keeps a report from `pass`.
    - Scenarios declare the parity rows they prove, and `cargo xtask parity` refuses a ✅ cell in
      `docs/platform-parity.md` that cites no scenario run.
11. **OS-lane determinism is a rule.**
    - A scenario fixes its window size and refuses another.
    - A lane sends single-step moves separated by explicit pauses, never an interpolated
      multi-step move.
    - Each lane declares its replay fidelity (exact positions, or order and kinds only); replay and
      golden holds compare positions only where the lane is exact.
12. **`tools/desktop-mcp` is retired.** It is not generalized: its OS input safety rules,
    worker model, UIA backend and error mapping move into `flui-driver`'s Windows lane, and FLUI
    drives FLUI apps only.

## Consequences

- One finder serves `flui test`, the CLI and agents with one meaning (ADR-0095 §5), now resolved in
  the app (`flui-semantics::agent`), not re-implemented per client.
- Schema `0.5` renames the agent ops (`semantics.read`, `semantics.act`, `motion.set`,
  `windows.list`); pre-1.0 the rename is a `version::BREAKING` entry citing this ADR.
- The xtask device drivers and the scripts under `tools/device-checks/` are deleted once their
  scenarios run on `flui-driver`.
- A coordinate in a public call is the exception: points exist (`Target::Point`) but are never
  produced by codegen from a locator-based trace.

## Alternatives considered

- **Grow desktop-mcp.** Rejected: it sees from outside only, has no widget, render or frame
  knowledge, and an MCP-shaped core would make Rust tests speak MCP.
- **JSON-RPC 2.0 end to end.** Rejected: the app would carry MCP-adjacent framing for no client
  but the adapter. BiDi's shape is what the web lane speaks already.
- **Client-side locator resolution over a fetched tree.** Rejected: waits would poll, a tree read
  per check is large, and check-then-act would span frames.
- **Regex in `TextMatch`.** Rejected: it brings a regex engine into every debug app for little a
  test needs; `contains` and `prefix` cover the cases seen in the suites.

## Verification

- `cargo nextest run -p flui-protocol`:
  - schema golden `protocol-0.5.json`;
  - 0.4 contained in 0.5 except the listed renames;
  - `METHODS` covered by the CLI and MCP.
- `cargo nextest run -p flui-semantics agent`: the locator and actionability tables.
- The new driver crate's headless suite (feature `headless`): the spec's failure matrix
  and every gesture through `semantic` and `synthetic`.
- Each lane's scenario on its platform, listed in the PR that lands it.
