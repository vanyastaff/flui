# ADR-0343: The owner promises turn deadlines, and the window states when the user places it

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Related:** [ADR-0254](ADR-0254-window-contract.md) decision 4 (one required handler method
  per obligation; `turn_deadline` is one more), [ADR-0058](ADR-0058-the-platform-paces-production-not-a-sleep.md)
  (the wake deadline a frame serves), [ADR-0338](ADR-0338-the-host-acquires-capabilities-from-the-launch-registry.md)
  (window placement, the first client of both),
  [ADR-0237](ADR-0237-window-state-and-host-notices.md) (the window snapshot)

## Context and problem statement

The composition root keeps state for the user that is no frame's business: a window's remembered
placement waits for a read with a bound and writes with a throttle. Two facts it needs were missing
from the owner and window contracts.

- **An owner turn at an instant.** `AppHandler::wake_deadline` is work a frame serves: a due
  instant asks the windows for frames. With no window open (the main window waiting for its
  placement read) no frame runs, so no owner turn runs and the window never opens; with a window
  open, an instant that is no frame's work asks for a frame it does not need.
- **Who placed the window.** A placement the run could not read is replaced only by what the user
  did. The snapshot said what the window is, not why it changed: inferring "the user" from what did
  not change (scale, displays, commands) calls a window manager's reparenting, a compositor's first
  configure, AppKit's constraint after show or the second report of a DPI resize the user's.

Which contract additions make both facts the host's to state, on every host?

## Decision drivers

- An obligation is cleared by the code that served it, never by its delivery (AGENTS.md, "Recurring
  defects").
- A loop that delivers the same unserved instant on every iteration spins; one that delivers it once
  and forgets it drops accepted work.
- One rule on every host, headless included, so an application test means what a desktop run does.
- A fact the snapshot gains reaches every host and every recording through the compiler
  (`ReportedFacts`, `WindowStateParts`).
- "Not the user" is the safe default where a host cannot tell.

## Considered options

1. A required `AppHandler::turn_deadline`, keyed by owner turns as the wake deadline is by frames;
   and `WindowState::interaction` (`WindowInteraction`), a snapshot fact each host states.
2. One deadline with a "no frame" flag in its answer.
3. A per-window "the user did it" flag on `WindowHandler::state_changed`.
4. Keep inferring the user from what did not change.

## Decision

Chosen option: 1.

1. **The turn deadline.** `flui_native::AppHandler::turn_deadline(&self, &OwnerPlatform) ->
   Option<web_time::Instant>` is required, like every handler method (ADR-0254 decision 4). At the
   instant the loop gives the owner one turn through `OwnerSignal::wake`, with or without a window
   open, and asks no window for a frame. The loop arms and takes it with
   `owner::wake_deadline::WakeDeadline`, the wake deadline's own type, keyed by
   `OwnerSignal::turns()` (the application's turns that ran) where the wake deadline is keyed by
   frames: an instant delivered once is delivered again only after a turn ran, so a handler that
   leaves an unserved instant in place gets its turn again, and a handler that has served it moves
   it.
   - **When a loop asks.** After every owner turn. The Win32 loop and its modal loops and the X11,
     Wayland and Android loops ask before each wait, which follows every turn; AppKit, UIKit and the
     web ask after each turn and once the handler bootstrapped, and schedule the turn on the main
     queue or with a page timer; the headless host asks when a test drives
     `HeadlessOwnerTurns::drive_at(now)` with its manual clock. A handler that moves its turn
     deadline outside an owner turn asks for one with `PlatformProxy::wake`.
2. **Who places the window.** `flui_platform::WindowState::interaction: WindowInteraction`
   (`None`, the default, or `UserPlacing`), carried by `WindowStateParts`, `ReportedFacts` and the
   native recording (`user_placing`). A host states `UserPlacing` while the user places the window
   through the window system: Win32 inside its size-move loop and while the default procedure
   carries out `SC_MAXIMIZE`, `SC_MINIMIZE` or `SC_RESTORE` (an application's command goes through
   `ShowWindow`, which sends none). Every other host states `None` until it reads its own signal
   (AppKit's live resize and drag, a move or resize X11 or Wayland starts for the user); UIKit,
   Android and the web place their windows themselves. A consumer counts a change as the user's
   only from this fact: `UserPlacing` in the report or in the one before it, whose placement it
   ends.

### Consequences

- Good, because the main window waiting for its placement read opens at the bound on every host,
  and a sibling window gets no frame for it.
- Good, because a placement the run could not read survives every change no host attributes to the
  user.
- Bad, because a handler that keeps answering an instant it cannot serve gets a turn after every
  turn until it moves it, as a wake deadline a frame never serves gets a frame after every frame.
- Bad, because on X11, Wayland and AppKit a move the user makes after an unreadable placement is
  not remembered until those hosts read their signal (`docs/gaps.md`).
- Neutral, because every `AppHandler` implementor answers `turn_deadline`, `None` when it has
  none.

### Confirmation

- `a_turn_deadline_gives_the_owner_a_turn_with_no_window` and
  `a_turn_deadline_asks_no_window_for_a_frame` (the `app_handler` table, on Win32);
  `a_turn_deadline_gives_the_owner_a_turn_with_no_window` in `x11_window` (Xvfb) and
  `wayland_window` (Weston); `a_due_turn_deadline_wakes_the_owner_from_a_modal_loop` (Win32
  modal frames); `a_window_opens_at_its_default_when_the_read_does_not_answer_in_time`
  (headless, through `drive_at`).
- `an_unread_placement_is_overwritten_only_by_the_user` (flui-app's `placement_recorder` table):
  a display change, a scale change and the resize after it, a window manager's adjustment and an
  application's command in two reports write nothing; a drag, a caption button and the report that
  ends a drag write.
- The compiler: a host that builds `ReportedFacts` without `interaction`, and a mirror of
  `WindowStateParts` without it, do not compile.

## Pros and cons of the options

### 1. A required turn deadline keyed by turns; a snapshot fact for the user

- Good, because each fact is the host's to state, with one rule everywhere.
- Good, because the turn deadline reuses the wake deadline's delivery rule rather than a second one.
- Bad, because every host and every recording carry one more field.

### 2. One deadline with a "no frame" flag

- Good, because there is one handler method.
- Bad, because the two obligations are served by different things (a frame, a turn), so one
  instant cannot carry both generations, and the nearer of two deadlines decides the flag of both.

### 3. A "the user did it" flag on `state_changed`

- Good, because it rides on the notice of the change.
- Bad, because every `WindowHandler` implementor changes, the flag is lost to a reader of the
  snapshot (`PlatformWindow::state`), and recordings do not carry it.

### 4. Keep inferring

- Good, because it needs no contract change.
- Bad, because it calls host adjustments the user's, which is the defect this decision fixes.

## More information

- The Win32 size-move loop: `WM_ENTERSIZEMOVE`/`WM_EXITSIZEMOVE`; the system commands reach the
  window as `WM_SYSCOMMAND` only from the user's input, `ShowWindow` sends none.
- The rule as flui-app keeps placements by it is in `crates/flui-app/ARCHITECTURE.md` ("The host
  keeps window placements in the application's storage"); the owner loops' rule is in
  `crates/flui-native/ARCHITECTURE.md`.
