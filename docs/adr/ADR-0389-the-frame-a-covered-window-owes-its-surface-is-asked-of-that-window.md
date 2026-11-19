# ADR-0389: The frame a covered window owes its surface is asked of that window

- **Status:** Accepted (2026-11-18)
- **Date:** 2026-11-18
- **Related:** [ADR-0357](ADR-0357-a-window-says-whether-it-reports-a-covering-and-a-surface-that-shows-nothing-backs-off-on-one-schedule.md)
  (a window says whether it reports a covering; `OccludedSurface`),
  [ADR-0254](ADR-0254-window-contract.md) §4 (`AppHandler::wake_deadline`) and §5 (a window's
  frame request, and an absence stated by the host),
  [ADR-0353](ADR-0353-production-is-paced-by-the-platform-and-a-frame-that-showed-nothing-waits-for-the-fallback.md)
  (what paces a frame)

## Context and problem statement

Where the window system keeps a covered window's last frame and shows it again on uncovering
(AppKit, `OccludedSurface::Kept`), a covered window whose content changed draws one frame once it
has drawn nothing for a quiet period, so its uncovering shows the content of then
(`flui-runtime`'s `ARCHITECTURE.md`, "An unseen window waits, and a covered one owes its surface a
frame"). That frame's instant was one more term of `UiRuntime::next_wake`, which `flui-app` folds
into the application's single answer to `AppHandler::wake_deadline`. The answer names no window,
so every native host delivers a due one as a frame request to every window, and arms the same
instant again once a frame of any window ran (`WakeDeadline`, the rule of every native host). With
one window covered and another shown on AppKit, the shown window's frame ran for the request, the
covered window drew nothing when AppKit ran no display pass for it, and the owed instant was
delivered again after each of the shown window's frames: the loop turned at that window's frame
rate until the covered window drew or was uncovered.

The question: how does a frame one window owes reach that window, and only it, on the host that
owes one?

## Decision drivers

- An obligation that belongs to one window is attributed to that window by a typed identity, not
  by "whichever frame ran" (`AGENTS.md`, "Recurring defects").
- A wake reported where nothing serves it is a defect: no deadline may be re-armed by frames of a
  window it does not belong to.
- The UI runtime has no platform edge (ADR-0083); a host's code runs with no runtime checked out
  and no registry borrow held.
- One rule in one place, headless included: the runtime decides what is owed and when, every host
  only serves it.
- The absence of a host capability is stated by the host, not inherited from a default
  (ADR-0254 §5).

## Considered options

1. A timed frame request of the window: `PlatformWindow::request_frame_at`, which the runtime
   makes of the presentation's own window and the host serves for that window alone (chosen).
2. Keep the merged wake deadline and let its answer name its windows
   (`wake_deadline -> Option<(Instant, WindowSet)>`), with a frame count per window in
   `WakeDeadline`.
3. Keep the merged deadline and stop re-arming a delivered instant on any frame: arm it again
   only after an owner turn that changed the answer.
4. Draw the owed frame inside an owner turn, with no frame request of the window at all.

## Decision

Chosen option: 1, because the frame is the window's, so the request that brings it is made of
the window; option 2 widens an application-level answer every host shares into a per-window
table each host must keep consistent with its windows, option 3 loses a frame that a hold left
due at the instant just served (the reason `WakeDeadline` re-arms after a frame), and option 4
runs a frame outside the window's frame path, which a host paces (a display pass, a present that
blocks) and the runtime does not.

1. **`PlatformWindow::request_frame_at(&self, at: Option<EventTime>)`** is a required method of
   the window contract. `Some(at)` asks for a frame of this window once `at` has come on the event
   timeline: the host then makes the request `request_redraw` makes, of this window alone, once. A
   later call replaces the instant; `None` withdraws it; a window closed before it is served
   reaches nothing. A host whose windows keep no surface while covered is never asked and says so
   in its body, returning at once; a double that takes no frame verbs gets that body from
   `flui_platform::window_takes_no_frame_verbs!`. `OccludedSurface::Kept` names a window that
   serves it.
2. **The runtime asks, once per instant.** `UiRuntime::owed_frame_requests()` returns, for each
   presentation whose owed instant (`occluded_frame_at`, on the event timeline counted from the
   runtime clock's epoch) differs from what its window was last asked, an `OwedFrameRequest`: the
   instant, or a withdrawal once nothing is owed (seen again, drawn with nothing left, refused, a
   full sink, closing). `OwedFrameRequest::deliver` calls the window's `request_frame_at` and then
   records the instant as asked, so a host that panics is offered the same instant by the next
   call, and an instant once asked is not asked again whatever other windows draw.
   `UiRuntime::next_wake` no longer carries the owed instant; `runs_frames` and
   `has_pending_due_work` still count it once due, so the window's frame, when served, runs it.
   **A held frame is asked again once its hold may end.** When the instant has come and the
   presentation's frame is held (its text preparation waits on an answer or is parked), the frame
   the served request brought draws nothing; the presentation marks what its window was asked as
   held (`AskedFrame::hold`), and the same instant is asked again once the preparation may proceed
   (`text_preparation::may_proceed`: none saved, its answer came, or a build, an invalidation or a
   changed premise the next frame's reconciliation withdraws it for). Not before: a preparation
   parked with nothing changed would otherwise be asked for at the frame rate of its own held
   frames.
3. **Every host delivers at the end of a work item.** `OwnerDelivery` takes the requests under the
   lease of each runtime work item that is not terminal, and after an installation that succeeded,
   and delivers them once the runtime is back, each contained in the work item's `FirstPanic`.
   `flui_testing::HeadlessHost` delivers after each pump, window observation and system notice, and
   before it decides how long its loop waits (`time_to_next_frame`) or whether it has settled
   (`pump_until_settled`), so a change written between two pumps is asked for as the work item
   that wrote it would ask on a runner.
4. **The host serves a request for its window alone.** AppKit keeps the window's pending instant
   (`TimedFrame`) and keeps one tick on the owner lane live for it: a request for the instant the
   live tick serves arms none; a tick serves only the instant it was armed for while that is still
   the one asked, makes the window's `request_redraw`, and clears it, so the same instant asked
   again (a held frame) is armed again; a tick that runs early waits out the rest; one for a
   replaced or withdrawn instant does nothing. Nothing of it reaches the wake pump's deadline. The
   headless host's loop waits no later than the earliest unserved instant of an open window, and
   the first pump at or after it serves it; the owed frame is not work its loop wakes for in any
   other way, so a covered window alone gets its frame only through the request, as on AppKit.

### Consequences

- Good, because the frames of a shown window never deliver a covered window's owed frame again: an
  instant is asked of one window, once.
- Good, because the application's wake deadline again holds only work any window's frame serves.
- Good, because the headless host serves the request through the same runtime seam a runner does,
  so a test drives it through the public API.
- Good, because a frame held when its served request came (text preparation in flight) is asked
  for again once the hold may end, and a hold that lasts asks nothing meanwhile.
- Bad, because a window whose host serves the request and whose frame then does not run at all
  (AppKit running no display pass for a covered window) is not asked again for that instant: no
  frame of it ran, so nothing marks it held. Its surface keeps the content of before the covering
  until what it owes changes, for one when it is uncovered, and no loop turns for it. Whether
  AppKit runs that pass is owed a run on a Mac (`docs/platform-parity.md`).
- Neutral, because headless pumps every presentation of its one runtime at each frame, so a covered
  window beside a shown one has its owed frame granted by the shown window's frames as soon as it
  is due, as in a runtime of several presentations on a runner; a covered window alone is served
  only through its request.
- Neutral, because every window type gains one method, and the hosts that keep no covered surface
  state their absence in one line.

### Confirmation

- The compiler: `request_frame_at` has no default, so every window type states what it does.
- `the_owed_frame_is_asked_of_its_window_and_not_of_the_wake_deadline`,
  `a_hold_leaves_the_owed_frame_due`,
  `an_owner_work_item_asks_the_covered_window_for_its_owed_frame` and
  `a_window_installed_covered_is_asked_for_its_owed_frame_by_the_install` (`flui-runtime`,
  `occluded_frame`); `a_held_instant_is_asked_again_once_its_hold_may_have_ended` (its unit
  tests).
- `a_covered_window_beside_a_shown_one_is_asked_for_its_owed_frame_alone` and
  `a_change_between_pumps_is_still_owed_its_frame` (`flui-testing`, `occluded_window`), with the
  other `occluded_window` cases.
- `a_timed_request_is_served_once_at_its_instant`,
  `a_served_instant_asked_again_is_served_again` and
  `a_shown_windows_frames_do_not_serve_a_covered_windows_request_again` (`flui-native`, AppKit's
  `timed_frame`, run on every host).

## Pros and cons of the options

### 1. A timed frame request of the window

- Good, because the obligation and its delivery name the same window.
- Good, because the runtime keeps the rule and each host only times a request.
- Bad, because the window contract gains a method every window type implements.

### 2. A wake deadline that names its windows

- Good, because the loops keep one deadline to wait on.
- Bad, because `AppHandler::wake_deadline` becomes a per-window table, each host's `WakeDeadline`
  a per-window frame count, and every host must keep both consistent with the windows it has.
- Bad, because every other term of the deadline is any window's work and gains nothing.

### 3. Re-arm a delivered instant only when the answer changed

- Good, because it is a one-line change of `WakeDeadline`.
- Bad, because a frame a hold left due at the instant just delivered (a full sink, text
  preparation) is never delivered again: the reason the rule re-arms after a frame.

### 4. Draw the owed frame inside an owner turn

- Good, because no host is involved.
- Bad, because the frame runs outside the window's frame path, unpaced by the host, and on AppKit
  outside the display pass its layer expects.

## More information

`docs/gaps.md` ("Occlusion") and the parity row "Visibility, minimize, occlusion" hold what has run
and what a Mac run still owes.
