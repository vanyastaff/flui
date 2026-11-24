# ADR-0395: A rebuild is shown over the app it will replace

- **Status:** Accepted (2026-11-23)
- **Date:** 2026-11-23
- **Supersedes:** [ADR-0238](ADR-0238-flui-driver-protocol.md) §1, the list of effect classes
  (`ReadOnly`, `Acts`, `Destructive`): `Annotates` is a fourth (§1); ADR-0238 §5's "An `Acts`
  method is never resent on timeout", which §4 below restates for every effect that changes the
  app;
  [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §4, the timeout's two
  answers ("retry `soon` when it only reads, `may_have_run` when it acts"), which §4 below
  restates for the four effects; and ADR-0288 §8, the sentence on what an observer may call ("a
  call of its that acts is answered `not_driver`", which the code read as every call that is not
  read-only): an observer may also call a method whose effect is `annotates`. The rest of
  ADR-0288 §8 (one driver's seat, the hello's role, what an observer never claims) is unchanged.
- **Related:** [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (every debug build
  serves the agent); [ADR-0393](ADR-0393-a-development-reload-applies-one-of-three-tiers-chosen-from-the-shape-of-the-edit.md)
  and [ADR-0394](ADR-0394-application-code-reloads-as-an-image-the-element-tree-adopts.md) (the
  edit loop this reports on)

## Context and problem statement

`flui run` rebuilds the application on each save and keeps the running build until a new one
succeeds. Where the build failed was written only to the terminal: the author looking at the app
saw the old code keep running and no sign that the save had not reached it.

| Fact | Where it shows |
|---|---|
| Every debug build serves the agent on a local endpoint whose credentials `flui run` creates | ADR-0239; `flui-cli`'s `agent::Session` |
| A second driver's hello is refused, and `flui mcp` may hold the seat while `flui run` runs | ADR-0288 §8; `flui-runtime`'s `agent::dispatch::may_call` |
| An observer could call only `read_only` methods, so no tool but the driver could draw over a window | `may_call`, before this record |
| The runtime already draws one development overlay over every window's scene, outside the app's tree | `flui-runtime`'s `performance_overlay` |
| Vite's and Next.js's dev servers show a compile error over the page it would replace; Flutter's tool writes it to the console only | each tool's error overlay; `flutter run` |

Where does a rebuild's state reach the window the author is looking at, and which client may put
it there?

## Decision drivers

- The author sees a failed save in the window, with the file and line, without looking away from it.
- The report never changes the app: not its state, its tree or the input it receives, so a test
  or `flui mcp` that drives the app acts on the same app with the overlay shown or not (ADR-0288).
- The driver's seat stays single: `flui run` reporting a build must not take it from `flui mcp`.
- One rule over the effects for what an observer may call and what a client may resend, held by
  a type and the dispatcher; no list of method names (AGENTS.md, "Recurring defects": a check
  that covers only the forms its author wrote, a rule written twice).

## Considered options

1. An observer may call a method whose effect is `annotates`; `build.report` is the first (chosen).
2. A third role, `toolchain`, beside the driver and the observer.
3. A file the app watches, written by `flui run`.
4. A widget the application's template puts in its tree.
5. Option 1 with a panel the author dismisses with a tap.

## Decision

§1. **An effect that only annotates.** `MethodEffect` gains `Annotates`: the method changes only
what the development tools draw over the app's windows, never the app's own state, its tree or
its input. `MethodEffect::changes_app` is true for `acts` and `destructive` alone, and it is the
one rule: an observer may call a method that does not change the app (`may_call`), and §4's
timeout reads it. The runtime serves an `annotates` row through a trait whose handler is given
the window's build banner and nothing of the app, so a method declared `annotates` that would
reach the app's state does not build: the dispatch of an `annotates` row requires that trait
and hands it nothing else.

§2. **`build.report`.** Protocol 0.29 adds the window method `build.report { window, state }`.
`state` is `building`, `succeeded` or `failed { diagnostics, omitted }`; a diagnostic is the
compiler's first line and the primary span's `file:line:column`. A report carries at most
`MAX_DIAGNOSTICS` (8) diagnostics and counts the rest: the type `Diagnostics` holds the bound,
built or decoded, and decoding refuses the element past it before reading it, so a window refuses
a report with more before it holds it; the schema states the bound (`maxItems`). The window
draws the latest report beside the performance overlay: a chip while a build runs, a panel of the
errors while the last build failed, nothing after a success. The banner takes no input: every
pointer event reaches the app as it would without it.

§3. **`flui run` reports as an observer.** A debug `flui run` of a desktop application with a
process restart opens an agent session (unlisted, writing no session file, when the run has no
`--agent`), connects to it as an observer and calls `build.report` in every window
`windows.list` names: `building` when a rebuild starts, `succeeded` before the new build replaces
the app, `failed` with cargo's errors, read from its JSON messages while each message still
reaches the terminal as cargo writes it. It sends from a thread of its own and folds waiting
reports into the latest, so a report never holds up a build. `flui run --no-build-overlay`, a
release run, a profile other than `dev` and a run on Windows send none, and a run that cannot
open the session or start the thread warns and runs without the overlay.

§4. **A call that timed out.** A call with no answer within its budget is answered `timeout`:
retry `soon` when its effect does not change the app (a read or an annotation, which a resend
shows again), and `may_have_run` with no retry when it does. The server, the driver
(`flui-driver`'s connection), `flui mcp`'s resend over a broken connection and the trace's test
generator read the same `changes_app`.

### Consequences

- Good: a failed save shows in the window with its file and line; a driver's tests and
  `flui mcp` act on the same app with the overlay shown or not.
- Good: any later development annotation (a reload outcome, a lint) is one method row with the
  `annotates` effect, and an observer may call it without a change to the roles.
- Bad: the panel covers part of the window until the next build; the author cannot put it away.
- Bad: a capture of the presented frame shows the banner, as it shows the performance overlay.
- Bad: a window opened after a report shows nothing until the next one.
- Bad: on Windows the running executable cannot be overwritten, so `flui run` stops the app
  before it builds and no window is there to show a report; a run there sends none.
- Bad: the Android, browser and worker-host runs do not report yet
  ([gaps](../gaps.md#accepted-adr-sections-not-built-yet)).

### Confirmation

- `flui-protocol`'s `only_an_act_or_a_destructive_call_changes_the_app` (the rule over every
  effect) and its schema golden `protocol-0.29.json`, which lists `build.report` with the
  `annotates` effect.
- `flui-runtime`'s `an_observer_may_call_what_reads_or_annotates_and_nothing_else` (the
  dispatcher over every method of the table) and
  `an_observer_reports_a_build_and_is_still_refused_an_act` (through the service);
  `a_report_past_its_deadline_may_be_sent_again` (§4 on the server); the `Annotate` bound on the
  annotating rows' dispatch.
- `flui-protocol`'s `a_failed_build_keeps_the_errors_a_window_shows_and_counts_the_rest` and
  `a_report_past_the_bound_is_refused_before_its_next_diagnostic_is_read`;
  `an_observer_reports_a_build_and_is_still_refused_an_act` refuses one past the bound through the
  service.
- `a_report_replaces_or_clears` (a success clears the banner) and `a_report_shows_a_node_until_it_succeeds` (the window draws it
  and asks for a frame); `the_banner_and_the_performance_overlay_show_apart`,
  `a_banner_is_finite_inside_its_surface_and_clear_of_the_performance_overlay` and
  `zero_and_tiny_surfaces_publish_finite_geometry`;
  `a_tap_on_the_banner_reaches_the_app_under_it` (a tap inside the drawn panel reaches the
  app's widget under it, and the banner stays drawn).
- `flui-driver`'s `a_timed_out_call_is_resent_only_when_it_leaves_the_app_as_it_was` (§4 in the
  driver) and `codegen_marks_what_it_cannot_write_and_writes_the_main_window_as_the_tests_own`
  (a generated test leaves out a `build.report`, as it leaves out a read); `flui-cli`'s
  `a_call_whose_connection_broke_goes_again_only_when_it_leaves_the_app_as_it_was` (§4 in `flui
  mcp`, over a connection the app closes).
- `flui-cli`'s `a_report_reaches_every_window_as_an_observer`,
  `a_window_that_refuses_the_report_leaves_the_others_to_get_it`,
  `a_report_whose_app_was_replaced_reaches_its_successor` and
  `the_reports_that_waited_fold_into_the_newest` and
  `the_reports_made_before_the_app_listens_reach_it_folded` (the reporter);
  `the_errors_are_kept_and_every_message_still_reaches_the_terminal` (cargo's messages);
  `an_unlisted_session_writes_no_file_and_leaves_the_listed_one`; and
  `a_restart_replaces_the_running_build_only_with_one_that_succeeded`, where the running app
  serves the endpoint and shows `building`, then `failed` for a compile error, then `succeeded`.

## Pros and cons of the options

### 1. Observer and `annotates` (chosen)

- Good: the seat stays single and `flui mcp` keeps driving while `flui run` reports.
- Good: the rule is one predicate over the effects, held by the dispatcher and by the trait that
  serves an annotation.
- Bad: one more effect for every client to read in the method table; an older client sees an
  unknown string there.

### 2. A `toolchain` role

- Good: `flui run` is named for what it is.
- Bad: a third role with its own list of methods, which is the rule written a second time; any
  other tool that draws an annotation needs the role too.

### 3. A file the app watches

- Good: no protocol change.
- Bad: a second channel beside the agent, with its own credentials, location and races, and no
  answer to say that a window showed the report.

### 4. A widget in the template

- Good: the app's own code, styled by its theme.
- Bad: it lives in the app's tree, so it changes what a driver and a test see, and an app that
  removes it or was created before it loses the report.

### 5. A panel dismissed with a tap

- Good: the author puts the panel away while the error is fixed.
- Bad: the banner takes input the app would have received. A driver's tap on an element under
  the panel never reaches it while `element.actionability` reports the element reachable, and
  an observer that reports again after each dismissal keeps the area from the app.
