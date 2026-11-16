# ADR-0350: A quit the user asks for asks every window, and a phone holds its background for the writes in flight

- **Status:** Implemented (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0259](ADR-0259-close-guard-close-reasons-and-flush-registry.md) §4 in part:
  its first sentence names only the application's own code as the source of a
  `CloseReason::Program` quit; §1 here adds the user's quit through the platform. The rest of §4
  stands.
- **Related:** [ADR-0254](ADR-0254-window-contract.md) §4 (`AppHandler`, one
  method per obligation), [ADR-0260](ADR-0260-session-end-flushes-before-it-tears-down.md)
  (the session end's budget, its inline flush, and which platforms end the application),
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (user code runs with nothing
  held), [ADR-0338](ADR-0338-the-host-acquires-capabilities-from-the-launch-registry.md) (the
  placement recorder, desktop only)

## Context and problem statement

Two exits lost what the application held.

- **A quit the user asks for.** AppKit reports Quit from the Dock, a quit Apple Event and
  `terminate:` from any sender to `applicationShouldTerminate:`. The host cancelled AppKit's
  request and stopped its loop in order, which cleared every window's handler and then closed
  each window. No window heard a close request, and a window holding unsaved work was closed
  past the handler that would have refused. The application's own quit,
  `AppHandle::request_quit`, already asked every presentation (ADR-0259 §4); the platform's quit
  did not reach it.
- **A phone leaving the screen.** iOS told the application its session was ending only at
  `applicationWillTerminate:`, which UIKit rarely sends to a scene-based application, and held
  no background task: a write the application had started when its scene went to the background
  was suspended with the process, or lost when the system ended it. Android reports `onStop` as
  the end of the session (ADR-0260), writes what the documents published within the budget, and
  hands an unfinished write to the IO pool with nothing saying whether it was stored.

How does a platform's quit reach the application's windows, and how long does a phone keep the
process running for the writes in flight?

## Decision drivers

- A window's handler and close guard decide whether it may close, whatever asked for the close
  (ADR-0259).
- One consultation of the windows, not one per host: the guards' records, the deferral and its
  cancellation live in `flui-app` already.
- A native callback runs no user code it does not have to (ADR-0287): AppKit's request is
  answered at once.
- The phone's own mechanism bounds the window: UIKit's background task, which the system may
  end sooner; Android keeps a stopped process until it reclaims it.
- What could not be written is reported by name, never dropped silently.

## Considered options

1. The host asks each window's `WindowHandler::close_requested` itself and quits when all grant.
2. The host asks the application (`AppHandler::quit_asked`), which runs the consultation its own
   quit runs; on a phone, the session end's answer to `Ending` holds the background.
3. A separate background-hold capability on `OwnerPlatform`, taken by the application around
   each write.

## Decision

Chosen option: 2, because the consultation, its records and its deferral exist once, in
`flui-app`, and a phone's hold reuses the session end's answer and release.

### §1. A quit the user asks for is the application's to decide

```rust
pub trait AppHandler: 'static {
    // ...
    fn quit_asked(&self, owner: &OwnerPlatform) -> QuitAnswer;
}

pub enum QuitAnswer {
    Quit,        // the platform closes every window and ends its loop
    KeepRunning, // the application asks its windows and quits itself if they let it
}
```

- AppKit answers `applicationShouldTerminate:` for a quit outside a session end with
  `NSTerminateCancel` and asks `quit_asked` in a task of its own on the owner lane, with no lock
  held; `Quit` stops the loop in order, `KeepRunning` leaves every window and its handler as it
  was. The headless host asks when a test reports a quit (`HeadlessHostEvents::quit`) and stops at
  its next owner turn on `Quit`.
- The Win32, X11 and Wayland hosts hear no application-wide quit: each of their closes reaches
  one window's handler, and a session end (Windows' Restart Manager included) reaches
  `session_end`. iOS, Android and the web cannot be asked to quit.
- With no handler bound the answer is `Quit`; a handler that panics answers `KeepRunning`, on every
  host, since a panic is no consent to close windows past their handlers.
- `flui-app` answers with the consultation `AppHandle::request_quit` runs: `KeepRunning`, and on
  the owner's next turn every presentation is asked as a `CloseReason::Program` close, in
  registration order, its guard first and then its handler; a refusal recorded on the guard defers
  the quit, one that records nothing (a panic) cancels it, and a handler that closes another
  window meanwhile leaves that window answering `Close`. A run with no main window to ask through
  answers `Quit`, and so does one that has shut down.

### §2. A phone holds its background for the writes in flight

```rust
pub enum SessionEndSupport {
    // Hold, Delay { max }, NotifyOnly
    Background { window: Duration },
}
```

- iOS and Android report `SessionEndPhase::Ending` once per trip to the background (iOS at the
  scene's `sceneDidEnterBackground:`, Android at `onStop` or `onSaveInstanceState`), and their
  answer to it is read: `Block` keeps the process running for at most `window` until the
  application calls `Platform::release_session_end`. The application stays up for the user's
  return: no window closes and the loop does not quit (ADR-0260).
- iOS begins a UIKit background task on `Block` and ends it exactly once: on release, when UIKit
  expires it (reported), or when the scene comes back. `IOS_BACKGROUND_WINDOW` is under the
  thirty seconds UIKit grants. Android holds nothing: `ANDROID_BACKGROUND_WINDOW` is under the
  delay after which the cached-app freezer stops a stopped process.
- `flui-app` flushes inline within the budget as before; when writes are still in flight it
  answers `Block`, and a thread waits for them (`WritesInFlight::wait_until`) until the window
  passes, logs by name every value still not stored, and wakes the owner, whose turn releases the
  platform. A later trip's session end replaces the hold, so an earlier trip's writes never
  release a later one.

### Consequences

- Good: a window with unsaved work refuses the Dock's Quit as it refuses its close button; one
  quit path for the application's code and the user's.
- Good: an iOS application's writes survive the trip to the background within UIKit's window,
  and an unfinished write is named in the log on both phones.
- Bad: the application's answer comes after AppKit's request is cancelled, so AppKit's own
  "quit" state is never entered (a document-based AppKit application's review of unsaved
  documents does not run); FLUI has no `NSDocument`.
- Bad: on Android the window bounds the report only; the process may still be reclaimed sooner.

### Confirmation

- `app_handler_obligation_table` ("a quit the user asks for is the handler's to decide"): kept
  running, panicking and granted answers on the headless host.
- `runner_close`: `a_quit_nothing_refuses_ends_the_loop`,
  `a_refused_quit_waits_for_the_record_then_exits` and
  `a_quit_the_window_stays_open_for_is_cancelled`, each for a quit from the application and from
  the platform.
- `runner_teardown`: `a_phone_holds_its_background_for_the_writes_in_flight`.
- `owner::background` tests: `background_window_ends_each_task_exactly_once`,
  `background_window_hands_back_a_task_it_cannot_hold`.
- The AppKit and UIKit paths are compiled by CI only; their device runs are rows of
  `crates/flui-native/platform-parity.toml`.

## Pros and cons of the options

### 1. The host asks each window's handler

- Good: a run that drives `flui-native` directly gets the consultation without writing one.
- Bad: a second consultation beside `flui-app`'s, without its guards, records or deferral; a
  window that refuses with a record would be closed later as a user close, not as the quit.

### 2. The application decides; a phone's session end holds its background

- Good: one consultation; the session end's answer and release already carry a hold.
- Bad: one more `AppHandler` method every handler writes.

### 3. A background-hold capability

- Good: an application could hold the background for work that is not a document's write.
- Bad: a second hold beside the session end's, a cross-thread handle in a public signature, and
  every host's lifecycle position grows a method for one platform's task.

## More information

Raven's Cocoa platform (`cocoa_platform.m`) answers `NSTerminateCancel` while the core
decides; Flutter's macOS embedder asks the framework through `System.requestAppExit`; Rune runs
every window's `on_quit` before any closes. No embedder of Flutter holds a UIKit background task
for the application's writes; that is left to plugins.
