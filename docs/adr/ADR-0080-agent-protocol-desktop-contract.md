# ADR-0080: The agent protocol is one wire contract over MCP, backend-neutral

- **Status:** Accepted
- **Date:** 2026-09-24
- **Refs:** roadmap tracks G1 (devtools protocol) and G2 (`flui mcp`)

## Context

`tools/desktop-mcp` is an MCP server that lets an agent see and drive any desktop
application through the OS: UI Automation on Windows, real input, capture. The roadmap
puts `flui mcp` on top of the devtools protocol (G1) and says the agent protocol rides on
standards: MCP as the transport, AccessKit as the vocabulary for roles and properties, a
specification of our own only for what MCP lacks. The desktop server is the first backend
of that protocol, and the in-process FLUI backend (a realm's own semantics tree, driven
without the OS in between) is the second.

What agents and the skills written for them learn from the first backend becomes the
contract. Before this decision the server exposed UI Automation pattern names as
`patterns`, control-type names as `role`, native window handles as `window_id`, replies
built ad hoc with `json!`, and errors as English sentences in which the situation
("the process exited", "the handle was never issued", "the element is disabled") was
`invalid_argument` because that was the variant `wait_for` did not poll on. None of that
survives a second backend, and none of it lets an agent branch on anything but wording.

Two facts about the clients shaped the shapes: Claude Code hands the model only
`structuredContent` when a result carries it, and Codex drops `content[]` — the image
included — in the same case; and TypeScript-SDK clients validate an error result against
the tool's `outputSchema`.

## Decision

The wire contract of the desktop MCP server is defined in these terms, and the in-process
backend speaks the same one.

### Handles

Windows, elements and screenshots are session handles: `w3`, `e12`, `s2`. A handle names
one thing; the same window or element keeps its handle across reads; one whose window,
element or process is gone answers `gone` and is never re-bound. A window is identified
by its native id, owner, the owner's start time and its class; a native id the OS reuses
for a window that differs in any of them gets a fresh handle, and so does one whose
earlier window the session saw close (every call on a handle, and every listing, checks).
What no OS field can tell apart is a same-class window of the same process reusing the
native id with no call in between; on Windows the id's 16-bit reuse counter makes that
need 65536 reuses of one slot. A pid stays the OS number, bound to the
start time of the process it named when first handed out. Error codes name a handle's
kind (`unknown_handle` / `gone` with `kind: element | window | screenshot | process`).

### Targets and locations

Every tool that sends input (`click`, `drag`, `scroll`, `type_text`, `key`) requires a
safety target, exactly one of `window` or `pid`. Every pointer tool takes one location
shape: `element`, or `x`/`y` in screen coordinates, or `screenshot` with `x`/`y` in that
image's pixels (the server maps them, and refuses if the window moved since). `drag`
takes two such locations as `from` and `to`.

### Vocabulary

`role` is an AccessKit role name in snake case (`button`, `check_box`, `text_input`,
`list_item`, `window`, ...; `unknown` when the vocabulary has no name), with the OS's own
name in `native_role`. `actions` lists the tools the element supports (`invoke`,
`toggle`, `set_value`, `select`, `focus`, `expand`, `collapse`, `scroll_into_view`),
spelled as the tools are named. State is flat and left out at its default: `disabled`,
`focused`, `focusable`, `checked` (`true` / `false` / `"mixed"`), `expanded`, `selected`.
Native identifiers stay (`automation_id`, `class_name`): they are how applications are
tested.

### Replies

Every tool has a typed reply and publishes its output schema, widened with an `error`
branch so a failed call conforms too. A reply goes out as structured content and as the
same JSON in a text block. A tree or a list of matches defaults to `format: outline`,
one line per element (`- role "name" [ref=e12] [state] [actions=...]`, roots marked
`[window=w3]`), and `format: json` for the nodes. `screenshot` sends an image and a text
block and no structured content. Every tool carries a title and annotations
(`readOnlyHint` on the six readers).

### Errors

A failed call is a tool error (`isError`) whose structured content is
`{"error": {code, message, retry, effect?, ...fields}}`. The codes are fixed:
`invalid_argument`, `not_supported`, `busy`, `not_found`, `unknown_handle`, `gone`,
`disabled`, `action_unsupported`, `not_foreground`, `focus_elsewhere`, `outside_target`,
`timeout`, `input_held`, `shutting_down`, `platform`. `retry` is `never`, `soon` or
`when_appears`; polling tools poll on it. `effect` is present when part of the action
reached the OS or the application: an object `{kind, detail, sent?, total?, unit?}` whose
`kind` is `partial` (with `sent`, `total`, `unit`), `may_have_run`, `ran` or `incidental`,
and whose `detail` says what went out. A count outranks an inner effect: `partial` then
carries the next unit's own kind in its detail. Windows named in an error are data (`foreground`,
`covered_by`: `{window, pid, title}`). An action that ran but whose element could not be
read back afterwards is a success with `readback_failed`, not an error.

### Reads and waits

Reads take a scope (`window`, `pid`, or `root` for a subtree), `max_depth` and
`max_nodes`, and say `truncated` when they left anything out. `wait_for` waits for an
element (by criteria or by handle), for its `state`, or with `gone` for no match.
`launch` returns once the process is started and the session has checked that its PID
has no conflicting earlier identity. A busy desktop thread can delay that check by up
to 10 s; if it cannot finish, the launch is refused and its child is ended. This replaces
the earlier `bound: false` fallback for a busy worker: that fallback could publish a PID
already issued for another process and give `kill` and window tools different meanings
for the same number. `bound: false` remains available when the registry check succeeds
but the OS reports no start time. `wait_for_window` waits for the window, so a client
timeout on the window cannot end an application whose launch succeeded.

## Consequences

- Skills and prompts written against this server work against the in-process backend
  when it arrives; the vocabulary and codes do not change with the OS.
- Adding a backend means implementing `AccessibilityBackend` and mapping its roles onto
  the vocabulary; a role with no counterpart is `unknown` with the native name, not a new
  vocabulary.
- Changing a code, a role name or a reply field after 1.0 is a breaking change of the
  agent protocol and needs an ADR superseding this one.
- Additive work is free: new tools, new optional parameters, new optional reply fields,
  new `effect` details.

## Alternatives considered

- **Native handles and names on the wire.** Rejected: agents reason about them, they
  differ per OS, and a recycled native id had to be guarded by never re-binding it.
- **Prose errors with a distinguishing variant for polling.** Rejected: the meaning of
  an error lived in its wording, and the wording was chosen for control flow.
- **Text-only replies, as Playwright MCP.** Rejected for the tools that return data:
  typed replies validate, and structured content is what the main clients feed the model.
  Kept for `screenshot`, where structured content would hide the image.
- **An optional safety target.** Rejected: the one rule this server exists for cannot be
  a convention in the instructions.

## Not decided here

The in-process backend's transport (whether the devtools protocol carries these shapes or
the MCP server proxies a realm), server-side search (`FindAll` with a condition instead
of a client-side walk), modifiers on pointer tools, admitting a hosted child process for
keyboard input, and the vocabulary for properties beyond the ones listed. Each is an
additive change on this contract.
