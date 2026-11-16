# ADR-0338: The host acquires capabilities for itself from the launch registry

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0152](ADR-0152-capability-seam-revised.md) §2 item 2 in part (that
  providers are acquired only per UI runtime; a widget still acquires in a lifecycle hook)
- **Related:** [ADR-0343](ADR-0343-the-owner-promises-turn-deadlines-and-the-window-states-user-placement.md)
  (the turn deadline and the user-placement fact placement relies on),
  [ADR-0237](ADR-0237-window-state-and-host-notices.md) decision 6 (window placement is FLUI's
  own value), [ADR-0262](ADR-0262-persisted-documents-over-a-byte-storage-capability.md)
  (documents reach their bytes through `StorageCapability`),
  [ADR-0260](ADR-0260-session-end-flushes-before-it-tears-down.md) (the session end's budget)

## Context and problem statement

ADR-0152 §2 states how a platform capability reaches code: a widget acquires it in a lifecycle
hook, from the registry its UI runtime holds, through a `ProviderContext` the UI runtime builds
(its completions, its runtime locals, the widget's window). An application replaces a built-in
provider with `Application::capability`, and the registry is checked for conflicts before the
platform starts.

The composition root also keeps state of its own in an application capability: a window that
remembers its placement (`AppConfig::restore_placement`) is read before the window opens, so
before any UI runtime exists, and written while it lives and when it closes. The desktop runner
used to read and write it through its built-in store directly. An application that provides its
own `StorageCapability` (an encrypted or sandboxed store, a test's memory store) then had its
window placement kept somewhere else, and nowhere when the run named no data directory. ADR-0152
names no acquisition outside a UI runtime. So the question is: how does the host acquire a
capability for itself, and in which context?

## Decision drivers

- The application's override decides where the application's data lives: one store per
  application, whoever writes to it.
- The read happens before the first window opens, when no UI runtime and no window exist.
- A provider sees the same contract wherever it is asked: a `ProviderContext` whose `owner()`
  poster delivers at the owner's next turn and whose completions retire with their owner (ADR-0152
  §2, items 5 and 6).
- The frame path is synchronous and the owner thread never blocks on storage (AGENTS.md, "Design
  stance"); a provider is the application's code and may refuse, panic or never answer.
- One acquisition path: the context construction and the handle's downcast exist once.

## Considered options

1. The host acquires from the launch registry through a scope of its own, with no window.
2. Placement is routed through a window's UI runtime: the window opens, its UI runtime acquires the
   storage, and the placement is applied after the window opened.
3. Placement is declared host data, kept outside the application's storage.

## Decision

Chosen option: 1, because it is the only one that honours the application's override before the
window opens without a second contract for providers.

1. **The scope.** `flui_runtime::capability::HostCapabilities` holds the run's
   `CapabilityRegistry` (the launch registry, the one conflicts were checked against) and an
   owner-job queue of its own. `HostCapabilities::acquire::<C>() -> Result<C::Handle, Unsupported>`
   asks the provider with a `ProviderContext` whose `window()` is `None`; a capability that needs a
   window answers `UnsupportedReason::NoWindow` itself, as it does for a widget before its first
   window. The context is built by the one function `PresentationCapabilities::acquire` uses
   (`acquire_with`), so a provider cannot tell the two callers apart except by the window.
2. **Its owner.** The composition root makes one scope per loop when the loop starts (flui-app's
   `runner::placement::install`), runs what providers posted with `run_posted` after every owner
   turn and while it waits for the last writes at the end of the loop, and retires it with
   `retire` when the loop ends, before the execution services close. A post after retirement is
   refused with `OwnerRetired`; a completion pending then ends with `Abandoned::OwnerRetired`.
3. **No cache.** The host acquires once per window that opens, as a widget's acquisition asks the
   provider each time (ADR-0152 §2, item 3).
4. **Failure costs the host's use, not the launch.** A refusal or a panic of the provider leaves
   the window opening from its configuration and unremembered; a panic is reported and its payload
   discarded (ADR-0281). Reading never blocks the owner: the window waits for the read at most
   `RECALL_BOUND` on the host's placement clock, through the owner's turn deadline
   (`flui_native::AppHandler::turn_deadline`), and then opens from its configuration.

### Consequences

- Good, because an application's storage override reaches the window placements, and an
  application with an override and no data directory has them.
- Good, because a provider has one contract: the same context, poster and retirement whether a
  widget or the host asks.
- Bad, because the host's scope is a second owner of completions besides UI runtimes, whose jobs
  the runner must run at its turns and at its final wait; a host that forgot `run_posted` would
  leave a provider's answer unrun (the runner's tests hold both places).
- Neutral, because no capability other than storage is acquired by the host yet; a new host use
  acquires through the same scope.

### Confirmation

- `a_host_scope_refuses_posts_after_retirement`, `a_host_scope_contains_a_panicking_job` and
  `a_host_scope_acquires_with_no_window` (flui-runtime's `tests/`).
- `an_application_storage_keeps_its_window_placement`,
  `the_host_acquires_the_storage_once_per_window`,
  `a_provider_that_fails_still_lets_the_first_window_open` and
  `the_final_flush_takes_answers_posted_to_the_owner` (flui-app's `runner::placement::tests`).
- `a_turn_deadline_gives_the_owner_a_turn_with_no_window` (flui-native's `app_handler` table, on
  Win32) for the owner turn the read's bound relies on.

## Pros and cons of the options

### 1. The host acquires from the launch registry through a scope of its own

- Good, because the override is honoured before the window opens, so the first reveal is already
  where the window was.
- Good, because the provider sees the contract ADR-0152 already states.
- Bad, because the runner owns a job queue and must turn it.

### 2. Placement routed through a window's UI runtime

- Good, because it needs no new owner of completions.
- Bad, because the window opens before its placement is known: it opens at its default and then
  moves, which is the visible jump the first reveal exists to avoid (ADR-0237 decision 5).
- Bad, because a window whose UI runtime is built later than its native window would read its
  placement through a runtime that does not exist yet.

### 3. Placement declared host data, outside the application's storage

- Good, because it is the cheapest: no acquisition at all.
- Bad, because the application's choice of store (encryption, a sandbox container, a test's
  memory store) does not reach data the framework writes on its behalf, and an application with an
  override and no data directory keeps no placement.

## More information

- AppKit's `setFrameAutosaveName:` keeps the frame in the application's user defaults, the
  application's own store, and refuses a name another window of the application already uses;
  FLUI keeps one window per name likewise (`runner::placement`'s claims).
- The rule as the runner builds it is in `crates/flui-app/ARCHITECTURE.md` ("The host keeps window
  placements in the application's storage").
