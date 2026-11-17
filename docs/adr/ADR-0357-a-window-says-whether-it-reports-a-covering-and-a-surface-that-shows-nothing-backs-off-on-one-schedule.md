# ADR-0357: A window says whether it reports a covering, and a surface that shows nothing backs off on one schedule

- **Status:** Accepted (2026-11-17)
- **Date:** 2026-11-17
- **Related:** [ADR-0254](ADR-0254-window-contract.md) §1 (the window's facts and capabilities),
  [ADR-0335](ADR-0335-a-withheld-frame-commits-nothing-and-joins-the-dropped-frame-backoff.md)
  §2 (the withheld retry), [ADR-0258](ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md) §6
  (the dropped-frame backoff), [ADR-0353](ADR-0353-production-is-paced-by-the-platform-and-a-frame-that-showed-nothing-waits-for-the-fallback.md)
  (what paces a frame), [ADR-0345](ADR-0345-the-lifecycle-starts-where-its-window-is-and-every-change-walks-the-ladder.md)
  §1 (`is_seen` drives the lifecycle)

## Context and problem statement

`WindowState::occluded` said "the window system keeps a shown window off the screen", and `false`
where the window system does not tell. A covering by other windows is told on some hosts and not
on others, so an unoccluded snapshot meant "seen" on AppKit and "nobody knows" on Wayland, and
nothing let a reader tell the two apart. A frame that a covered window's surface refused was
retried on the dropped-frame backoff; a Wayland window whose compositor stopped answering its
frame callbacks, without suspending it, was asked for a frame once per one-second grace forever,
and a late answer to an older callback released a newer one. The two waits were two hand-written
doublings in two crates.

A desktop-wide cover tracker on Win32 (a z-order walk with an out-of-context WinEvent hook) was
built and read: it ran `EnumWindows` and DWM queries on the owner thread, woke it for every show or
hide of any window of any process, missed an uncovering a program makes with `SetWindowPos` alone,
and counted windows that cover nothing (tool windows, per-pixel transparent DirectComposition
windows) as covering. A window that is seen and draws nothing is a worse defect than a covered
window that keeps drawing.

## Decision drivers

- A fact a host cannot know is stated as unknown, typed, not published as its opposite.
- "Not seen" stops frames, haptics and pointer sequences (ADR-0345 §1), so it comes only from facts
  the host knows for certain.
- A surface that shows nothing backs off on a bounded schedule armed as a deadline, never a poll,
  and the schedule ends the moment the surface shows a frame or is paced again.
- One rule in one place (AGENTS.md "Current forms").

## Considered options

1. A capability that says whether a covering reaches `occluded`, hosts reporting only what they
   know, and one backoff type for every wait on a surface that shows nothing (chosen).
2. A third value of `occluded` (seen, covered, unknown) in each snapshot.
3. A cover tracker on every host that lacks one, Win32 included, on the owner thread.

## Decision

Chosen option: 1, because what a host can see is fixed per host and window, as
`reports_keyboard_inset` is, and `is_seen` keeps one reading; option 2 repeats a per-host constant
in every snapshot, and option 3 is the tracker read above.

1. **`WindowCapabilities::reports_occlusion`** (a `WindowCapability` with its absence reason) says
   whether a covering by other windows reaches `occluded`. AppKit reports it (`occlusionState`).
   X11 reports it while no compositing manager owns `_NET_WM_CM_S<n>`, read with the window
   manager's support and again when a manager announces the selection; under one, the server reads
   every redirected window unobscured, and it is absent (`NOT_ON_THIS_DEVICE`). Wayland reports it
   where the window's `xdg_toplevel` is version 6 or newer, whose `suspended` state covers a
   toplevel the compositor does not show; before it, absent. iOS, Android and the Web never tell a
   covering (`NOT_ON_THIS_PLATFORM`). Headless reports what the test sets.
2. **Win32 tracks no covering.** It states "not seen" from what it knows for certain: hidden,
   minimized, cloaked by DWM, the session locked; a covering is `NOT_YET_ON_THIS_HOST`. A tracker
   that would be correct runs off the owner thread and excludes what covers nothing (tool windows,
   per-pixel transparent windows, a capture in progress), in one pixel space, and follows its own
   windows' moves; `docs/gaps.md` holds that entry.
3. **`flui_platform::Backoff`** is the one schedule a surface that shows nothing backs off on: a
   wait from a first value, doubled per attempt in a row that came to nothing, at most a ceiling.
   The UI runtime's retry of a dropped or withheld frame uses it from one display period to
   `FAILURE_RETRY_CAP`, with ADR-0258 §6's ten doublings, armed through `next_wake`; a present
   ends it, and ADR-0335's budget keeps the first withheld attempts at the display rate. The
   Wayland host's wait on an unanswered frame callback uses it from `CALLBACK_GRACE` (1 s) to
   `MAX_CALLBACK_GRACE` (8 s), as the loop's wait deadline; the compositor's next `done` ends it.
   The other hosts pace frames by a signal that always comes (a blocking present, AppKit's display
   pass, a display link, the choreographer, the browser's animation frame) and wait on nothing a
   surface can withhold.
4. **A frame callback's answer is matched by ticket.** A `done` releases the callback out only
   when it answers that callback or a later one; an older one ends the backoff and gives the
   callback out the first grace.
5. **A surface that withholds frames past the budget is reported once** per presentation, as the
   `flui.pace` warning `surface_withholds_frames`, with `host_reports_occlusion`.

### Consequences

- Good, because a reader of an unoccluded snapshot knows from the capabilities whether it means
  seen, and no host publishes a guess.
- Good, because an unanswered Wayland window costs a frame per eight seconds, not per second, and
  the backoff has one implementation with its ceiling.
- Bad, because a covered Win32 window keeps drawing frames nobody sees until a correct tracker is
  built; its surface's refusals, where the swapchain reports them, already back off.

## Confirmation

- The capability tests of each host:
  `an_x11_window_has_the_capabilities_it_names` and
  `a_compositing_manager_takes_away_the_covering_report` (X11),
  `a_wayland_window_has_the_capabilities_it_names` and `the_capabilities_follow_the_compositor`
  (Wayland), `a_window_state_round_trips_through_its_parts` (the contract).
- `unanswered_callbacks_back_off_to_the_ceiling_until_one_is_answered` (the Wayland model) and
  `the_record_backs_off_unanswered_callbacks_and_matches_answers_by_ticket` (its seam in the host).
- `a_withholding_surface_backs_off_to_the_ceiling_and_starts_over_once_it_shows` and
  `a_surface_that_keeps_withholding_frames_is_reported_once` (`flui-runtime`).
- `an_unseen_window_with_nothing_asked_runs_no_frame` (Win32) and
  `an_unseen_window_asks_for_no_wake_until_it_is_seen` (headless).
