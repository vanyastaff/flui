# ADR-0402: A binary links the development agent only where a hook is installed

- **Status:** Accepted (2026-11-24)
- **Date:** 2026-11-24
- **Supersedes:** [ADR-0288](ADR-0288-the-development-agent-is-one-sans-io-service.md) §1's last
  sentence only ("An app that never starts an agent creates no `Service`, so it pays for the code
  only"): such an app links none of the agent's owner side. Unchanged: the service is compiled in
  every build, with no feature.
- **Related:** [ADR-0239](ADR-0239-every-debug-app-hosts-the-driver-agent.md) (which builds
  install the driver agent's hook); [ADR-0398](ADR-0398-the-release-build-mode-leaves-trace-and-debug-events-out.md)
  and [ADR-0397](ADR-0397-an-icon-face-is-in-a-binary-only-where-a-package-or-the-application-asks-for-it.md)
  (the other parts of a release binary that only some applications use)
- **Sources:** `flutter/flutter@3.50.1` (`ae9b586f34`)
  `packages/flutter/lib/src/foundation/binding.dart:573-600` (`initServiceExtensions` registers
  the VM service extensions behind `kReleaseMode`) and `:930-950` (the guard exists "so that
  Dart's tree shaker can remove the code for the service extension in release builds")

## Context and problem statement

`flui-app` serves the driver agent only in a debug build or with the `driver` feature
(ADR-0239), and an application installs a tool of its own through `AppConfig::dev_agent`. Yet
every release binary carried the agent's owner side. What bears on that:

| Fact | Evidence |
|---|---|
| The UI runtime's own paths call into the agent directly: the command drain matches `UiCommand::Agent`, `AgentSubscribe` and `AgentRecord` and counts and releases leases; a frame's end completes agents' readings, publishes their events and evaluates their waits; an owner turn evaluates waits; platform input is journalled; a scene's settlement reads parked layer reads; a presentation's close answers every job an agent parked | `crates/flui-runtime/src/ui_runtime/commands.rs`, `frame.rs`, `pump.rs`, `input.rs`, `settlement.rs`, `presentation/close.rs` before this change |
| Every runner vends a window to the loop's hooks through `DevAgentHost::vend`, which named `UiRuntime::dev_agent_window`, whether or not a hook was installed | `crates/flui-app/src/app/runner/dev_agents.rs`, `crates/flui-runtime/src/dev_agent.rs` |
| A linker keeps what a reachable function names: a direct call, a match arm or a vtable keeps the window methods, the protocol's answers and their serialization, `flui-semantics`' agent reads and the recordings' wire forms | the release counter's symbols |
| Built as CI's `size` job builds it, the generated counter's Linux x86_64 release binary is 21,157,744 bytes on `main` and 20,572,784 with the seam below (584,960, 2.8%) | local builds of `main` at `40af5f05d` and of this change, 2026-11-24 |
| ADR-0288 §1 keeps the service out of a feature because a feature of `flui-runtime` cannot follow `debug_assertions`, and `flui-app`'s build would always turn it on | ADR-0288 §1 |
| Flutter registers its service extensions only where the build mode allows them, so that its tree shaker removes their code from a release build: removal by reachability, not by a second build of the framework | Sources |

What decides whether a binary links the agent's owner side?

## Decision drivers

- A binary that never hands an agent a window carries none of the agent's code.
- An application that installs a hook in any build gets a working agent: no build mode makes
  `AppConfig::dev_agent` a silent no-op.
- No feature or `cfg` in `flui-runtime`, `flui-view` or `flui-semantics`, which could disagree
  with `flui-app`'s build condition (ADR-0288 §1).
- The rule is held where the reference is made, so the next path that reaches the agent goes
  through the same seam.

## Considered options

1. Keep the direct calls (today).
2. A feature of the lower crates, on in a debug build (`cfg(any(debug_assertions, feature = "agent"))`)
   and forwarded from `flui-app`'s `driver`.
3. The agent's owner side is reached only through a reference that vending a window installs,
   and only the code that installs a hook names the vending.

## Decision

Chosen option: 3, because it makes the agent's code reachable exactly where a hook can be served,
in every build mode, with nothing for the build to agree on.

### §1. The runtime reaches the agent through the side a vended window installs

`flui-runtime`'s `ui_runtime::agent_side::AgentSide` is the one seam between the UI runtime's
own paths and the agent: the drain (leases counted and released, window calls, subscriptions,
recordings, the recordings swept), a frame's end, an owner turn, the next wait deadline, platform
input's journal, a presentation leaving the runtime, a scene's layer reads, and a presentation's
close. `UiRuntime` and each `PresentationState` hold an `Option<&'static dyn AgentSide>`, set by
`UiRuntime::dev_agent_window` (the only constructor of an `AgentWindow`) and never cleared, so the
jobs an agent left on a presentation are answered after its agent slot goes. Every path calls the
agent only through it. An agent command that reaches a runtime with no side was not sent through
its ports: it is dropped as stale, and its reply with it, which the caller reads as gone.

### §2. Only the code that installs a hook names the vending

`flui_runtime::dev_agent::WindowSource` holds `UiRuntime::dev_agent_window`, and
`WindowSource::dev_agent` is the one place that names it. `DevAgentHost::attach` takes the source
beside the hooks and vends through it. `flui-app` creates the source in `DevAgent::with`, behind
`AppConfig::dev_agent` (and the driver agent's own installation, under `cfg(driver_agent)`), and
`flui-testing`'s headless host where its test attaches a hook. A binary in which nothing installs
a hook names neither, and the linker drops the agent's owner side.

### Consequences

- Good, because a release application without a hook carries none of the agent: about 0.58 MB of
  the generated counter on Linux x86_64.
- Good, because a release application that installs its own hook keeps a working agent, and the
  debug and `driver` builds are unchanged.
- Good, because no lower crate has a feature or a `cfg` for the agent, so no two crates can
  disagree about whether it is built.
- Bad, because the rule is held by reachability, not by the compiler: a new direct call from the
  runtime's paths into an agent module links the agent again, and only the `size` job sees it
  (on `main`). The module documentation of `agent_side` states the rule for the reader.
- Bad, because each of the runtime's paths reads one more `Cell` and calls through a vtable where
  an agent is served; on the frame path that is a handful of reads per frame.
- Neutral, because `UiCommand::Agent` and the agent's state fields stay in every build; their
  drop glue is all a binary without an agent keeps.

### Confirmation

- `counter_release_bytes`, `counter_wasm_bytes` and `hot_reload_host_release_bytes` in
  `perf/size.toml` are measured with this change; a path that links the agent again moves them
  past the `size` job's 2%.
- Every test of the agent (`flui-runtime`'s agent and dev-agent suites, `flui-app`'s
  `a_loop_takes_every_installed_hook_once_and_one_that_panics_is_dropped_alone`,
  `flui-testing`'s and `flui-driver`'s headless runs) vends its windows through
  `dev_agent_window`, so a window that installed no side fails them all.
- A reader of a change to `flui-runtime`'s `ui_runtime` checks that a call into an agent module
  goes through `AgentSide` (§1).

## Pros and cons of the options

### 1. Keep the direct calls

- Good, because nothing changes.
- Bad, because every release binary carries 0.58 MB that only an agent reaches.

### 2. A feature of the lower crates

- Good, because the compiler would refuse a call into the agent from a build without it.
- Bad, because `debug_assertions` is per crate: a profile override of one crate makes
  `flui-runtime` and `flui-app` disagree, and a debug `flui-app` would vend windows a release
  `flui-runtime` cannot serve.
- Bad, because `AppConfig::dev_agent` would do nothing, or not exist, in a release build, so an
  application's own tool stops working there.
- Bad, because every agent module, state field and test carries the condition, in three crates.

### 3. The side a vended window installs

- Good, because the code an application links follows what it installs, in every build.
- Good, because the seam is one trait and one reference.
- Bad, because a direct call added later is not a compile error.

## More information

Revisit when the agent's state fields move into one value per presentation: that value can own the
side, and the fields' drop glue goes with it.
