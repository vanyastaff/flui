# ADR-0395: A rebuild is shown over the app it will replace

- **Status:** Accepted (2026-11-23)
- **Date:** 2026-11-23
- **Supersedes:** [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §8, the
  sentence "a call of its that acts is answered `not_driver`" read as "every call that is not
  read-only": an observer may also call a method whose effect is `annotates`. The rest of §8 (one
  driver's seat, the hello's role, what an observer never claims) is unchanged.
- **Related:** [ADR-0238](ADR-0238-flui-driver-protocol.md) (the method table and its effects);
  [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (every debug build serves the
  agent); [ADR-0393](ADR-0393-a-development-reload-applies-one-of-three-tiers-chosen-from-the-shape-of-the-edit.md) and
  [ADR-0394](ADR-0394-application-code-reloads-as-an-image-the-element-tree-adopts.md) (the edit
  loop this reports on)

## Context and problem statement

`flui run` rebuilds the application on each save and keeps the running build until a new one
succeeds. Where the build failed was written only to the terminal: the author looking at the app
saw the old code keep running and no sign that the save had not reached it.

| Fact | Where it shows |
|---|---|
| Every debug build serves the agent on a local endpoint whose credentials `flui run` creates | ADR-0239; `flui-cli`'s `agent::Session` |
| A second driver's hello is refused, and `flui mcp` may hold the seat while `flui run` runs | ADR-0288 §8; `flui-runtime`'s `agent::dispatch::may_call` |
| An observer may call only `read_only` methods, so no tool but the driver could draw over a window | `may_call`, before this record |
| The runtime already draws one development overlay over every window's scene, outside the app's tree | `flui-runtime`'s `performance_overlay` |
| Vite, Next.js and Webpack's dev servers show a compile error over the page it would replace; Flutter's tool writes it to the console only | each tool's error overlay; `flutter run` |

Where does a rebuild's state reach the window the author is looking at, and which client may put
it there?

## Decision drivers

- The author sees a failed save in the window, with the file and line, without looking away from it.
- The report never changes what the app does or what a driver sees of the app's own state: a
  test that drives the app passes the same with the overlay shown (ADR-0288).
- The driver's seat stays single: `flui run` reporting a build must not take it from `flui mcp`.
- One rule, held by a type and the dispatcher, for what an observer may call; no list of method
  names (AGENTS.md, "Recurring defects": a check that covers only the forms its author wrote).
- A release build compiles none of it.

## Considered options

1. An observer may call a method whose effect is `annotates`; `build.report` is the first (chosen).
2. A third role, `toolchain`, beside the driver and the observer.
3. A file the app watches, written by `flui run`.
4. A widget the application's template puts in its tree.

## Decision

§1. **An effect that only annotates.** `MethodEffect` gains `Annotates`: the method changes only
what the development tools draw over the app's windows, never the app's own state, its tree or
its input. An observer may call a `read_only` or an `annotates` method and no other
(`may_call`); the runtime serves an `annotates` method through a trait that is given the
window's development drawing and nothing of the app, so a method declared `annotates` that would
reach the app's state does not compile.

§2. **`build.report`.** Protocol 0.29 adds the window method `build.report { window, state }`.
`state` is `building`, `succeeded` or `failed { diagnostics, omitted }`; a diagnostic is the
compiler's first line and the primary span's `file:line:column`. A report carries at most
`MAX_DIAGNOSTICS` (8) diagnostics and counts the rest; a window refuses one with more. The window
draws the latest report beside the performance overlay: a chip while a build runs, a panel of the
errors while the last build failed, nothing after a success. A tap on the panel dismisses it and
reaches nothing under it; the next report shows again.

§3. **`flui run` reports as an observer.** A debug `flui run` of a desktop application always
creates its agent session, connects to it as an observer and calls `build.report` in every window
`windows.list` names: `building` when a rebuild starts, `succeeded` before the new build replaces
the app, `failed` with cargo's errors, read from its JSON messages while each message still
reaches the terminal as cargo writes it. It sends from a thread of its own and folds waiting
reports into the latest, so a report never holds up a build. `flui run --no-build-overlay`, a
release run and a profile other than `dev` send none.

### Consequences

- Good: a failed save shows in the window with its file and line; a driver's tests and
  `flui mcp` see the same app with the overlay shown or not.
- Good: any later development annotation (a reload outcome, a lint) is one method row with the
  `annotates` effect, and an observer may call it without a change to the roles.
- Bad: a window opened after a report shows nothing until the next one.
- Bad: on Windows the running executable cannot be overwritten, so `flui run` stops the app
  before it builds and no window is there to show the report.
- Bad: the Android, browser and worker-host runs do not report yet
  ([gaps](../gaps.md#accepted-adr-sections-not-built-yet)).

### Confirmation

- `flui-runtime`'s `an_observer_may_call_what_reads_or_annotates_and_nothing_else` (the
  dispatcher's rule over every effect) and `an_observer_reports_a_build_and_is_still_refused_an_act`
  (through the service); `a_report_past_its_deadline_may_be_sent_again` (an annotation that
  timed out did not act). The `Annotate` path's assertion fails the build of a window method
  declared `annotates` that is served through `Serve`.
- `a_report_replaces_refuses_or_clears` (a report with more than `MAX_DIAGNOSTICS` is refused, a
  success clears the banner) and `a_report_shows_a_node_until_it_succeeds` (the window draws it
  and asks for a frame); `the_banner_and_the_performance_overlay_show_apart`,
  `a_banner_is_finite_inside_its_surface_and_clear_of_the_performance_overlay` and
  `zero_and_tiny_surfaces_publish_finite_geometry`.
- `a_tap_inside_the_panel_dismisses_it_unseen_and_one_outside_reaches_the_app` and
  `a_dismissing_press_takes_its_contact_to_its_end`.
- `flui-cli`'s `a_report_reaches_every_window_as_an_observer` (the hello's role is `observer`,
  every listed window gets the report) and
  `the_errors_are_kept_and_every_message_still_reaches_the_terminal` (the errors kept, the rest
  counted, every message still written).
- `flui-protocol`'s schema golden `protocol-0.29.json` lists `build.report` with the
  `annotates` effect.

## Pros and cons of the options

### 1. Observer and `annotates` (chosen)

- Good: the seat stays single and `flui mcp` keeps driving while `flui run` reports.
- Good: the rule is one effect, held by the dispatcher and by the trait that serves it.
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
