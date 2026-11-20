# ADR-0390: A page journals its native layer as a native host does

- **Status:** Accepted (2026-11-20)
- **Date:** 2026-11-20
- **Supersedes:** [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §11, the
  sentence "A page records the input layer only" and the capture's restriction to "a debug build
  of a native host": a page's debug build journals its backend for the `native` layer too.
- **Related:** [ADR-0215](ADR-0215-input-recording-and-backend-conformance.md) §4 (the native
  journal), §5 (redaction) and §7 (the conformance table, whose recording rows replay each
  backend's journal on the headless host);
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §16 (a release build compiles no
  journal); [ADR-0241](ADR-0241-driver-trace-format.md) (the trace that holds the native
  streams); [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (the build that serves
  the agent)

## Context and problem statement

A driver's trace holds the app's `input` layer and, where the app journals its backend, the
`native` layer beside it: one stream per source, each a native recording that replays on the
headless host (ADR-0288 §11, ADR-0241). `flui-app`'s build script set `cfg(native_journal)` only on
targets with a local socket, and the web runner told the agent that it records no native layer,
so the page's hello listed `input` alone.

| Fact | Where it shows |
|---|---|
| The journal is not bound to a target: every host's streams open in shared code through the same `Journal`, the web's included | `flui-native`'s `owner/app_delivery.rs` (notices, session ends, reopens), `owner/window_delivery.rs` (input method events, store locks, close requests), `platform.rs` (`HostPlatform::try_from_parts`, built for wasm32: preference and display reads), `window/state.rs` (window snapshots), `input/assembly/registry.rs` (pointer registries) |
| A trace of a page through the Chrome lane held the input method events and the preference revisions of the input layer, and no lifecycle notice, display read, window snapshot or composition stream | `flui-app`'s `runner/web.rs`, which passed `false` for the native layer |
| A page's input method runs paths no other host has (`EditContext`, a hidden `<textarea>`), and their recordings could not be taken by a driver | `flui-native`'s `text::conformance::{WebEditContext, WebTextarea}` |

Should a page's debug build journal its backend for the driver's `native` layer, and if it does,
under which switch?

## Decision drivers

- One rule on every host: the hello lists the layers the app records the same way wherever it
  runs, and a trace holds the same streams whichever lane took it (AGENTS.md, "Recurring
  defects": a rule held by one host).
- A release build compiles none of the journal, and an app nobody drives pays nothing for it after
  its start (ADR-0151 §16, ADR-0288 §11).
- The page has one thread and no blocking call: the capture runs where its source runs, on the
  owner, and the page transport only moves lines (ADR-0288 §6).
- The capture must be installed before the platform is made, since a registry reports only to the
  capture present when it was created; the page's launch object is read from the platform as it
  is made (ADR-0288 §7).
- Text stays redacted unless the recording asks to keep it (ADR-0215 §5).

## Considered options

1. Keep a page's recording to the input layer (the rule this replaces).
2. Journal the page's backend in every debug build, as a native host does.
3. Journal the page's backend in a debug build behind an opt-in of its own: a launch key, or a
   Cargo feature.

## Decision

Chosen option: 2, because it is the native hosts' rule unchanged: the code that journals is
already built for wasm32, and only the runner's switch kept the page out.

§1. **The build.** `flui-app`'s build script sets `cfg(native_journal)` in every build that sets
`cfg(driver_agent)` with the profile's debug assertions, a page's included. A `compile_error!`
beside the agent's own (`app::agent`) fails a build where `cfg(native_journal)` and
`debug_assertions` disagree on a system with a transport (`unix`, `windows`, wasm32), so a
release build cannot journal and a debug page cannot lose its native layer to a misread profile.

§2. **The runners.** Every runner begins its journal through one guard, `flui-app`'s
`JournalStart`: begun on the owner thread before the platform is made, ended when the guard is
dropped unsettled (the platform failed to start, another instance took the launch, the runner
unwound), kept or ended by the read of the launch (`settle(served)`), or handed to a loop that
reads the launch later (`release_to_loop`, the desktop and iOS). The web runner starts its loop
through `start_journaled(Backend::Web, start, install)`: the journal of the web backend, then the
platform, then the agent installed from the page's launch and told the loop journals, the journal
kept only when the agent is served. A served page attaches the journal's `NativeLines` to the UI
runtime it hands the agent, so the hello lists `native` and a recording of `[input, native]`
holds every stream the page opens: its pointer registries (`web`), its application delivery (notices), its window
delivery (input method events, the text store's focus and locks under `EditContext` or the
textarea), its platform's reads (preferences, displays) and its windows' snapshots.

§3. **The transport.** The lines go as `record_native` events of the session, which the page
export carries to the script's `onLine` like any other line. The export's shape and
`PAGE_EXPORT_VERSION` do not change: the export moves lines and never reads them. Bounds, joining,
redaction and the share of a recording's queue are ADR-0288 §11's.

### Consequences

- Good, because a trace through the Chrome lane holds the page's lifecycle, input method, reads
  of the system and window snapshots, read back by the same reader as any other lane's.
- Good, because the hello's layers follow one rule on every host, and the web needs no switch of
  its own.
- Good, because the web's input method paths can now be recorded by a driver and replayed.
- Bad, because a debug page links the line capture and the native recording's serializer, which
  were built but unreferenced before, and its setup goes through the capture until the launch is
  read.
- Neutral, because a page nobody drives ends the capture at its start, as a native app does.

### Confirmation

- The `compile_error!` in `flui-app`'s `app::agent` (§1). Main's `wasm-check` job compiles
  `flui-app` for `wasm32-unknown-unknown` in a debug build: there the web runner's journal is
  built under `cfg(native_journal)`, and a build script that left the page out fails there.
- `flui-app`'s `a_loop_keeps_its_journal_only_when_its_platform_starts_and_the_agent_is_served`
  drives `start_journaled`, the web runner's start, on every host: a failed start leaves no
  journal, the agent hears that the loop journals, and the journal stays exactly when the agent
  is served. `a_journal_whose_start_goes_unsettled_ends_and_one_handed_to_its_loop_stays` holds
  the guard every runner uses (§2).
- `flui-app`'s `a_page_conversation_carries_the_native_layer_of_a_recording` runs a page
  conversation on the Linux host over a service that records the native layer, with the journal
  installed as the web runner installs it: the hello lists `native`, and the stream's header
  (`web`), its setup and a row journaled during the recording reach the script as `record_native`
  events. `a_loop_nobody_serves_ends_its_native_journal_so_no_line_reaches_it` holds the end of an
  unserved journal.
- `flui-driver`'s `a_trace_of_a_page_session_holds_the_pages_native_layer` asks a page through the
  Chrome lane for both layers and reads its application, window, platform and window state
  streams back through the protocol's schema.
- The conformance table's recording rows (ADR-0215 §7) drive a session of each kind through the
  journaled delivery code with a capture of the web backend, as of every other decoder's
  backend, and replay what the capture recorded on the headless host
  (`each_backend_s_recordings_replay_on_the_headless_host`); each row fails against a backend
  that installs its capture after its platform, journals redacted, or leaves a reopen
  undelivered (`each_conformance_row_fails_against_a_broken_backend`). What those rows check is
  the shared delivery code under each backend's journal: the browser's own translation in front
  of it, like each native host's OS translation, runs only in a browser.
- A FLUI web build driven in headless Chrome with a trace is a run owed on that environment: the
  row "Recorded native input replays deterministically on the headless host" of
  `docs/platform-parity.md`.

## Pros and cons of the options

### 1. Input only

- Good, because a page carries no capture.
- Bad, because the web is the one host whose native layer a driver cannot record: no lifecycle,
  displays, window snapshots or composition stream, and the hello's layers differ by host.
- Bad, because the web's input method paths, which no other host has, stay out of every trace.

### 2. Journal as a native host does (chosen)

- Good, because the switch, the end of an unserved journal and the hello are the native hosts'.
- Good, because nothing new is built for the web: the streams already open through the shared
  `Journal` on wasm32.
- Bad, because a debug page links the capture and its serializer.

### 3. Journal behind an opt-in of its own

- Good, because a debug page that is never recorded would link no capture.
- Bad, because a launch key cannot be read in time: the capture must exist before the platform
  is made, and the page's launch object is taken off the page by the platform as it is made.
- Bad, because a Cargo feature cannot follow the profile, and a second switch only the web has is a
  rule held by one host, which the driver would have to set apart from every other lane.
