# ADR-0401: `flui run` serves its reload at an endpoint the session file names

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Superseded-by:** [ADR-0403](ADR-0403-a-protocol-method-is-named-for-its-subject-or-its-instrument.md)
  for the method name `run.reload` and the tier name `session` (§1) and the tool name
  `flui_reload` (§4) only: the method is `app.reload`, its tier is named `control`, and its
  tool is named for it; §3's `run.reload` event of `flui run --json` keeps its name.
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md) (the method table and its
  conversation), [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (the session
  `flui run --agent` writes), [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md)
  §8 (who may call what), [ADR-0393](ADR-0393-a-development-reload-applies-one-of-three-tiers-chosen-from-the-shape-of-the-edit.md)
  (the tiers and the report of §5), [ADR-0395](ADR-0395-a-rebuild-is-shown-over-the-app-it-will-replace.md)
  (the build's diagnostics)

## Context and problem statement

An agent that edits an application's sources through `flui mcp` has no way to make the edit
run. `flui run --agent` rebuilds on each save, but the agent learns nothing of the rebuild: not
when it ended, not whether it failed, not the compiler's errors. Its only signal is the app's
endpoint, which a process restart closes.

| Fact | Where it shows |
|---|---|
| `flui mcp` reaches the app at the endpoint `target/flui-agent.json` names, with its token | ADR-0239; `flui-cli`'s `agent::Credentials` |
| The app cannot rebuild itself, and a process restart ends every connection to it | `flui-cli`'s `commands::run`, `ProcessRestart` |
| `flui run` already reads cargo's errors for a desktop build (`Diagnostics`, at most 8) | ADR-0395 §2; `build::util::compile_errors` |
| Each reload strategy wrote its own `run.reload` event (`{"kind": …, "ok": …}`), with no errors and no time | `commands::run`, before this record |
| An observer may call only what leaves the app as it was | ADR-0288 §8, ADR-0395 §1; `ClientRole::may_call` |
| `flutter run --machine` serves `app.restart {appId, fullRestart}` itself, the process that started the app; a request made before the first compile waits for it, and one reload runs at a time | `flutter/flutter@3.50.1`, `packages/flutter_tools/lib/src/commands/daemon.dart:840`, `:1085-1120` |

Which process answers a reload, how does a client find it, and what does it answer?

## Decision drivers

- The answer outlives the process the reload replaces.
- One report of a reload, whatever started it (a save, `r`, `R`, a call), so the terminal, `--json`
  and a client never disagree.
- The rule over who may call what is the protocol's, read the same at every endpoint.
- No second builder racing `flui run` on the target directory.
- The endpoint is as private as the app's: same token, same file, same local transport.

## Considered options

1. `flui run` serves `run.reload` at a run endpoint of its own, named in the session file (chosen).
2. The app relays `run.reload` to `flui run` over a supervisor connection.
3. `flui mcp` builds and restarts the app itself.
4. A request file `flui run` watches, answered by its `--json` events.

## Decision

§1. **`run.reload` is the supervisor's.** `ServedBy` gains `Supervisor`: the tool that started
the app and rebuilds it. Protocol 0.31 adds `run.reload { at_least? }`, served by the supervisor,
effect `destructive` (a restart loses the app's state), tier `session`. `at_least` is the
cheapest `ReloadTier` (`reassemble`, `restart`, `process_restart`) the reload may apply; absent,
the run places it. The answer is a `ReloadReport { outcome, elapsed_ms }`, its outcome
`applied { tier }`, `unchanged` or `failed(Diagnostics)`. No agent serves the method, and
`flui-driver` refuses it `not_supported`, naming the run endpoint.

§2. **The run endpoint.** A `flui run --agent` that rebuilds on saves (a desktop application,
an Android device or a browser, with hot reload and without `--release`; `--agent` refuses a
worker project) binds a fresh private
endpoint (a socket in a `0700` directory, or a pipe whose DACL grants the current user) and only
then rewrites the session file with its address as `run`, before `run.agent.ready` reports it
as `run`, so neither the file nor the event names an endpoint nobody serves. A run that does not
rebuild on saves names none. The conversation is ADR-0238 §2's: a hello with the session's
token, compared by `launch::same_token`, answered by a hello that names no app and lists the
methods the role may call; then requests. One thread serves every client, polling, and holds:

- every client that presents the token, a driver or an observer: who drives is the app's seat
  (ADR-0288 §8), and the run holds no second one. A seat in each process could be held by two
  clients, each refused at the other's endpoint, and a Windows pipe a client dropped keeps its
  seat until the run writes to it;
- `ClientRole::may_call`, moved into `flui-protocol` with the token comparison and
  `ServerLine::malformed`, so that the app's agent and the run endpoint read one rule: an
  observer's reload is refused `not_driver`, and a line that is not a request is answered by the
  id it carries;
- at most 8 clients, each with at most one reload waiting, and a second request on that
  connection refused `busy`, retry `soon`; at most 8 reloads untaken by the loop, a reload whose
  client went among them, and one past them refused `busy`;
- every reload handed to the loop is answered: with its report, or `shutting_down` when the loop
  drops it unanswered (the run ended, or the step failed), and so is one a client sent before the
  run ended that the loop never took. A client that goes leaves its reload to run.

§3. **One report.** Every step of the loop that builds returns a `Reloaded`, and the loop
writes one `ReloadReport` from it: the `run.reload` event, timed from the moment the loop took what
started the reload (a save, a key, a call; a call that waited behind a build is timed from when
the loop took it), and the answer of the request that started it. A strategy that reads cargo's
messages fills `failed` with its diagnostics; one that does not reports none. The tier is what the run
applied: `process_restart` for a new process (in a browser, a reloaded page), `reassemble` for
a worker build handed to the running host. Until ADR-0393 §3–§4 are built no mechanism restarts
in the process, so `at_least: restart` is applied as `process_restart` (ADR-0393 §2), and a
request at `restart` or above takes `R`'s path: the app stops, then builds. A failed build
leaves the running app as it was unless the reload stopped it first: a restart does, and on
Windows and Android every rebuild does. A build that succeeded and is the one the app runs is
`unchanged`: the desktop process restart starts the executable cargo wrote (as `cargo run` would,
with its library path) and starts it again only when cargo built it anew or the file changed
since the app started, so a save and a call for the same edit restart the app once; protocol
0.34 adds the outcome.

§4. **`flui mcp`'s `flui_reload`.** The tool calls `run.reload` over a connection of its own to
the run endpoint, kept beside the app's, and waits up to 600 s: a build whose dependencies
changed takes minutes. A session without `run` answers `not_supported`. The method is
destructive, so a call that timed out or whose connection broke is not sent again (ADR-0395 §4).
A connection whose other end closed since the last call (the app a reload restarted, a run that
ended) sends nothing: the call is unsent and goes on a new connection whatever its method, so the
first call after a restart reaches the new app.

### Consequences

- Good: an agent edits, reloads and reads the compiler's errors in one call, and the answer
  survives the restart.
- Good: the terminal, `--json` and `flui_reload` read one report; ADR-0393 §5's report gains its
  reason and the process's outcome as fields of it.
- Good: a later supervisor method is a row of the table and a branch of one dispatcher.
- Bad: the `run.reload` event's shape changed: a reader of `kind` and `ok` reads `outcome`.
- Bad: a second server of the protocol's conversation, in `flui-cli`, beside `flui-runtime`'s.
- Bad: the Android, browser and worker-host runs answer a failed build without its errors
  ([gaps](../gaps.md#accepted-adr-sections-not-built-yet)).
- Bad: an agent with the session's token reloads the app whoever drives it: the run admits it
  where the app's seat would refuse it.
- Bad: a Windows named pipe has no shutdown, so a client's reader ends only when the run closes
  its end, as for the app's endpoint.

### Confirmation

- `flui-protocol`'s schema goldens `protocol-0.31.json` (the method, its types and
  `supervisor`) and `protocol-0.34.json` (`unchanged`)
  and `what_ends_the_app_is_destructive_and_reads_never_act`.
- `flui-cli`'s `a_reload_reaches_the_loop_and_its_client_reads_the_report`,
  `a_reload_the_run_drops_is_answered_shutting_down`,
  `the_endpoint_refuses_what_it_may_not_run_before_the_loop_sees_it`,
  `a_second_reload_on_one_connection_is_busy_until_the_first_is_answered`,
  `the_hello_lists_the_reload_to_a_driver_alone`, `a_broken_request_is_answered_by_its_id` and
  `the_loop_holds_a_bounded_number_of_reloads` (§2);
  `a_reload_asks_the_run_with_its_tier_and_waits_for_the_build`,
  `a_reload_goes_to_the_run_the_session_file_names` and
  `a_request_after_the_app_closed_the_connection_is_unsent` (§4).
- `an_agent_reloads_the_app_through_the_run_that_started_it`, end to end: `flui run --agent`
  names the endpoint in the file and in `run.agent.ready`; a `flui_reload` of the build the app
  runs is `unchanged` and leaves it running, with the report the `run.reload` event carries,
  timed within the call; a save restarts the app once, on the executable its build wrote (no
  `cargo run` names the package to it) with the build's `deps` on its library path, and a
  `flui_reload` after it is `unchanged`; an executable changed since the app started, and an
  app that exited, are started again; the first call that acts after a restart reaches the new
  app; a broken build answers its compiler error; a restart stops the app before it builds; and
  an unknown tier is refused. `the_app_searches_the_libraries_cargo_run_would` and
  `an_example_runs_from_below_its_profile_output` hold the library path `cargo run` sets.
  `a_restart_replaces_the_running_build_only_with_one_that_succeeded` reads the report a save
  writes, and `an_agent_reads_and_taps_the_app_run_with_agent_through_mcp` that a run with
  `--no-hot-reload` names no run endpoint.
- `flui-runtime`'s `an_observer_may_call_what_reads_or_annotates_and_nothing_else`, through the
  moved `ClientRole::may_call`.

## Pros and cons of the options

### 1. A run endpoint (chosen)

- Good: the process that builds answers, and its connection outlives the app's.
- Good: the same conversation, token and file: `flui mcp`'s client is the app's.
- Bad: a second server of the conversation, small but its own (§2's bounds and refusals).

### 2. The app relays to the supervisor

- Good: one endpoint for a client.
- Bad: a process restart ends the connection that carries the answer; the app would hold a
  request for a peer that kills it.
- Bad: the app's agent gains a third role and a routing rule.

### 3. `flui mcp` builds

- Good: no new endpoint.
- Bad: two builders on one target directory, and `flui run`'s watcher rebuilds the same save
  again; the restart would be `flui mcp`'s, with no process of its own to restart.

### 4. A request file

- Good: no protocol change.
- Bad: no answer channel: a client parses another process's stdout, which `flui mcp` does not
  hold, and races other writers of the file.
