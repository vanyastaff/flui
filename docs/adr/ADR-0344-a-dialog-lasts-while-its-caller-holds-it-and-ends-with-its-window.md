# ADR-0344: A dialog lasts while its caller holds it and ends with its window

- **Status:** Accepted (2026-11-16)
- **Date:** 2026-11-16
- **Supersedes:** [ADR-0152](ADR-0152-capability-seam-revised.md) §2 items 5 and 6 in part (a
  request its caller can call off answers through a `Dialog<T>` wrapping its `Completion<T>`, not
  the bare completion; `WindowRef` exposes the window's `WindowLifetime` beside its id, scale and
  raw handles)
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) §2 items 5 and 6 (the
  `Completion` and the `WindowRef` this extends), [ADR-0154](ADR-0154-capability-crates.md) §10
  and §11 (the scenario primitive and `flui-system`), ADR-0340 (an Android activity result
  lives while its provider holds its `ResultTicket`),
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (provider code runs
  with no borrow held), [ADR-0323](ADR-0323-a-value-stays-on-the-owner-thread-unless-its-signature-names-what-crosses.md)
  §1 (owner-thread state)

## Context and problem statement

`flui-system`'s file dialogs answered through a plain `Completion`, and nothing else ended a
dialog:

- `DialogError::WindowClosed` was declared and never built. A UI runtime outlives each of its
  windows but one, so a dialog outlived its window, delivered into the closed window's widget,
  and a handle of a closed window still asked the system.
- A dialog could not be called off. Dropping the completion left the portal's dialog on screen
  and the window `Busy`; a portal that never answered kept it `Busy` until the runtime retired.
  Nothing called the portal's `Request.Close`.
- `OpenResult` admitted `Some(vec![])` and several picks for a request that allowed one; a
  portal success without `uris` read as a cancel.
- `Failed { code }` promised the operating system's code, while the portal path passed `0` or
  the portal's response `2`, and the launcher guessed `NoHandler` for a response it could not
  read.

The Android host is gaining activity results (ADR-0340), where a request lives while its
provider holds a `ResultTicket`; a dialog shown through an activity has to end the same way.
How does a dialog end, and what may its answer be?

## Decision drivers

- Accepted work is delivered or refused on every exit (AGENTS.md "Before each push", item 3):
  the window closing and the caller leaving are exits.
- One cancellation shape across hosts: the portal's `Request.Close`, an activity's
  `ResultTicket`, a scripted double's held request.
- The frame path is synchronous and owner-thread state is `Rc`/`Cell` (ADR-0323 §1); a provider
  never runs inside a borrow of the runtime's state.
- A capability crate depends on the contract crate alone (ADR-0154 §1), so the window's
  lifetime has to reach it through `WindowRef`.
- Types carry the rule: an empty pick list or a fabricated code should not be representable.

## Considered options

1. **A liveness query on `Resolver`** (`Resolver::is_wanted`) and a "window closed" flag the
   backend polls.
2. **A cancellation token handed to the backend**, which it watches.
3. **A dialog value the caller holds, a hold the backend returns, and a window lifetime on
   `WindowRef`**: the handle ends a dialog by dropping what the backend gave for it.

## Decision

Chosen option: 3, because the end of a dialog is then one drop the handle performs, whatever
ended it, and that drop is exactly the drop of a `ResultTicket` or a portal request's guard.
Option 1 makes every backend poll and leaves a silent portal waiting; ADR-0340 already chose a
held ticket over a resolver query for the same reason. Option 2 hands each backend a second
channel to watch and gives it no place to keep the system's own handle.

§1. **A dialog lasts while its caller holds it.** `FileDialogs::open` and `save` return a
`Dialog<T>`; `on_complete`, `poll` and `into_future` consume it as a `Completion`'s do, and the
dialog stays up while the value, the closure or the future is held. Dropping it dismisses the
dialog, frees the window and delivers nothing.

§2. **A backend returns what the dialog lasts by.** `FileDialogBackend::open` and `save`
return a `DialogHold`, `DialogHold::new(keep: impl Any)`. The handle drops it, on the owner
thread with none of its state borrowed, when the dialog ends for any reason: the answer, the
caller's drop, the window's close, the runtime's retirement. A backend whose dialog is still up
dismisses it in that drop: the portal's guard wakes its request's task, which calls
`org.freedesktop.portal.Request.Close` (or never makes the call, when the drop came first); an
Android backend holds its `ResultTicket`; the scripted double holds the `RequestTicket` of
§5.

§3. **A provider learns that its window closed.** `WindowRef::lifetime()` is a
`WindowLifetime`: `is_closed()`, and `on_close(listener) -> WindowCloseWatch`, which runs the
listener once, inside the close, while the watch is held. The runtime keeps the window's
`WindowCloser` with its presentation and closes it when the close begins, on every path that
closes a presentation; a closer dropped unclosed closes. The file dialogs answer
`Err(DialogError::WindowClosed)` to a dialog in flight and drop its hold, and a handle of a
closed window answers `WindowClosed` without asking the backend.

§4. **An answer is one the request can have.** An open dialog answers `Option<Picks>`, a
non-empty list (`Picks::try_new` refuses an empty one); several picks for a request that did
not allow them, a success without picks, several files for a save are
`DialogError::Failed(SystemFailure::MalformedAnswer)`. A failure no variant names carries a
`flui_platform::SystemFailure`: `Os(code)`, `PortalResponse(code)`, `Bus` or
`MalformedAnswer`; `LaunchError::Failed` and `DialogError::Failed` take it in place of an
integer, and the launcher reports an unread portal response as `PortalResponse`, not
`NoHandler`.

§5. **The scenario primitive withdraws a request its caller calls off.**
`Script::receive_with_ticket` returns a `RequestTicket`; dropping it removes a held or scheduled
request, takes its answer off the timeline and counts it in `Script::withdrawn`, so a dismissed
dialog is neither `Unanswered` nor `NotReached`.

### Consequences

- Good, because a closed window and a caller that left both end the system's dialog and free
  the window on every host, the scripted double included.
- Good, because an Android dialog backend maps onto ADR-0340 with no new contract:
  `DialogHold::new(ticket)`.
- Good, because a diagnostic says what the system said, and an empty or oversized pick list
  cannot reach a caller.
- Bad, because `FileDialogBackend`, `OpenResult`, `LaunchError::Failed`, `DialogError::Failed`
  and `WindowRef::new` change shape: a third-party backend returns a hold, and a caller that
  matched `Failed { code }` matches the `SystemFailure`.
- Neutral, because `WindowRef` is now owner-thread (`!Send`), as the provider context it comes
  from already was.

### Confirmation

- The compiler: a backend that returns no `DialogHold` does not build; `Picks` has no empty
  value; `SystemFailure` is the only payload of `Failed`.
- Tests: `a_dialog_ends_with_its_window`, `a_dropped_dialog_is_dismissed_and_frees_its_window`,
  `a_window_shows_one_dialog_at_a_time` and
  `several_picks_for_a_single_pick_request_are_a_malformed_answer` in `flui-system`'s
  `system_it`; `a_dropped_dialog_closes_its_portal_request_and_frees_the_window` and
  `the_dialogs_reach_the_file_chooser_and_answer_its_picks` against the stand-in portal on
  Linux; `a_window_closes_once_and_tells_only_the_watches_still_held` in `flui-platform`;
  `a_dropped_ticket_withdraws_a_waiting_request_and_the_run_owes_nothing_for_it` in
  `flui-testing`.
