# ADR-0343: The owner promises turn deadlines, and the window states when the user places it

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Superseded-by:** [ADR-0354](ADR-0354-a-window-shows-the-cursor-its-region-asks-for-holds-the-pointer-only-while-focused-and-says-when-the-user-resizes-it.md) for decision 2's set of interactions and the rule a consumer counts the user's change by (`UserResizing` beside `UserPlacing`, read through `WindowInteraction::is_user`)
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

- A loop that delivers an instant already past on every iteration spins: the turn it gives does not
  move the instant unless the handler does.
- Work that is ready now is no deadline: the application does it in the turn it is in, or asks for
  one owner turn through the signal.
- One rule on every host, headless included, so an application test means what a desktop run does.
- A fact the snapshot gains reaches every host and every recording through the compiler
  (`ReportedFacts`, `WindowStateParts`).
- "Not the user" is the safe default where a host cannot tell.

## Considered options

1. A required `AppHandler::turn_deadline`, whose delivered turn serves every instant not later than
   its own time; and `WindowState::interaction` (`WindowInteraction`), a snapshot fact each host
   states on the report that carries the change.
2. One deadline with a "no frame" flag in its answer.
3. A per-window "the user did it" flag on `WindowHandler::state_changed`.
4. Keep inferring the user from what did not change.

## Decision

Chosen option: 1.

1. **The turn deadline.** `flui_native::AppHandler::turn_deadline(&self, &OwnerPlatform) ->
   Option<web_time::Instant>` is required, like every handler method (ADR-0254 decision 4). At the
   instant the loop gives the owner one turn through `OwnerSignal::wake`, with or without a window
   open, and asks no window for a frame. Every loop, headless included, arms and takes it with one
   type, `owner::wake_deadline::TurnDeadline`, by one rule: a turn delivered for the deadline
   serves every deadline not later than that turn's own time, and an answer not later than the
   last time served is not armed again. A handler that keeps answering an instant already past gets
   one turn for it. Work that is ready now is no deadline: the application does it in the turn it
   is in, or asks for one owner turn with `PlatformProxy::wake`. An answer may still be past (a
   held write whose interval ended before the change that owes it arrived): the change asked for
   one turn, that turn serves it, and the seam never arms an instant not later than the last time
   it served.
   - **When a loop asks.** After every owner turn. The Win32 loop and its modal loops and the X11,
     Wayland and Android loops ask before each wait, which follows every turn; AppKit, UIKit and the
     web ask after each turn and once the handler bootstrapped, and schedule the turn on the main
     queue or with a page timer; the headless host asks when a test drives
     `HeadlessOwnerTurns::drive_at(now)` with its manual clock. A handler that moves its turn
     deadline outside an owner turn asks for one with `PlatformProxy::wake`.
2. **Who places the window.** `flui_platform::WindowState::interaction: WindowInteraction`
   (`Unknown`, the default, `None` or `UserPlacing`), carried by `WindowStateParts`,
   `ReportedFacts` and the native recording (`user_placing`, absent for `Unknown`, which is how a
   recording older than the fact reads). A host states `UserPlacing` on each report it makes
   while the user places the window through the window system, and the snapshot it publishes
   when that ends says `None`:
   - Win32 inside its size-move loop (`WM_ENTERSIZEMOVE` to `WM_EXITSIZEMOVE`, which publishes),
     and while the default procedure carries out an `SC_MAXIMIZE`, `SC_MINIMIZE` or `SC_RESTORE`
     the user worked (publishing after it). A system command the window sends itself for an
     application's command (the restore before fullscreen) is marked by its sender and is not the
     user's;
   - the headless host states what its test simulates;
   - X11, Wayland, AppKit, UIKit, Android and the web read no signal for it and state `Unknown`.

   A consumer counts a change as the user's only from the report that carries it: `UserPlacing`
   there. Nothing is inferred from the report before, and `Unknown` counts as not the user.

### Consequences

- Good, because the main window waiting for its placement read opens at the bound on every host,
  and a sibling window gets no frame for it.
- Good, because a placement the run could not read survives every change no host attributes to the
  user.
- Good, because no answer of the handler can spin a loop: an instant a turn served is never
  delivered again.
- Bad, because work left at an instant already served waits for the application to ask for a
  turn itself; an answer repeated at the same past instant is not a request.
- Bad, because on the hosts that state `Unknown`, and on Win32 for a snap by keyboard, a
  placement the user makes after an unreadable one is not remembered that run; so is a drag that
  moved nothing during the loop (outline dragging, Snap Layouts) and ends in a snap applied after
  `WM_EXITSIZEMOVE`, while a drag that moved the window already counts as the user's and the
  snapped placement is written (`docs/gaps.md`). On Win32 an application's command applied in a
  turn the size-move loop gives counts as the user's.
- Neutral, because every `AppHandler` implementor answers `turn_deadline`, `None` when it has
  none.

### Confirmation

- `a_served_turn_deadline_is_not_armed_again` (flui-native's `wake_deadline`); in the
  `app_handler` table, `a_past_turn_deadline_gives_one_turn` (headless) and
  `a_past_turn_deadline_does_not_spin_the_win32_loop`, with
  `a_turn_deadline_gives_the_owner_a_turn_with_no_window` and
  `a_turn_deadline_asks_no_window_for_a_frame` on Win32;
  `a_turn_deadline_gives_the_owner_a_turn_with_no_window` in `x11_window` (Xvfb) and
  `wayland_window` (Weston); `a_due_turn_deadline_wakes_the_owner_from_a_modal_loop` (Win32
  modal frames); `a_window_opens_at_its_default_when_the_read_does_not_answer_in_time`
  (headless, through `drive_at`).
- `the_size_move_loop_states_the_users_placement_and_ends_it`,
  `a_system_command_states_the_users_placement_and_ends_it` and
  `the_applications_own_restore_is_not_the_users` (the Win32 window commands table).
- `an_unread_placement_is_overwritten_only_by_the_user` (flui-app's `placement_recorder` table):
  a display change, a scale change and the resize after it, a window manager's adjustment, an
  application's command in two reports and a change in the report after the user's write nothing;
  a drag and a caption button write. `a_window_that_could_not_read_its_placement_writes_only_what_the_user_places`
  (flui-app's runner, headless).
- The compiler: a host that builds `ReportedFacts` without `interaction`, and a mirror of
  `WindowStateParts` without it, do not compile.

## Pros and cons of the options

### 1. A required turn deadline served by time; a snapshot fact for the user

- Good, because each fact is the host's to state, with one rule everywhere.
- Good, because the rule needs no count of turns: the served time alone tells a stale answer.
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

- The Win32 size-move loop: `WM_ENTERSIZEMOVE`/`WM_EXITSIZEMOVE`. `ShowWindow` sends no
  `WM_SYSCOMMAND`; the window's own code sends one (the restore before fullscreen) and marks it.
  Win+Arrow snaps move the window outside both, so Win32 states `None` on them.
- The rule as flui-app keeps placements by it is in `crates/flui-app/ARCHITECTURE.md` ("The host
  keeps window placements in the application's storage"); the owner loops' rule is in
  `crates/flui-native/ARCHITECTURE.md`.
