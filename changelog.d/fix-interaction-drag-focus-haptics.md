### Changed

- **Entering an OS drag session** (`flui-interaction`, `flui-runtime`): a host starts each
  `PendingOsDrag` with `PendingOsDrag::start(source)` and reads its identity with
  `PendingOsDrag::key`; its `request` field is no longer public. Dropping a pending OS drag
  without starting it ends its session cancelled.

### Removed

- `DragSession::os_plan`, `HapticsHost::frame_deadline`, and the public `receivers_on_path` and
  `ResolvedReceiver` (`flui-interaction`), which nothing outside the crate used.

### Fixed

- **An OS drag whose backend never reports its end stayed running forever** (`flui-interaction`):
  a backend that drops the end hook uncalled, after a start that succeeded or while the OS runs
  the drag, now ends the session cancelled, and its observer hears it. A hook a refusing backend
  kept ends nothing once the session is back in its window.
- **A panicking OS drag start left the drag stuck in the OS phase** (`flui-runtime`): the start
  is treated as a refusal, so the drag returns to its window, the starts after it still run, and
  the first panic propagates once all are done. `UiRuntime::enter_pending_os_drags` now contains
  each start as the owner host does.
- **A backend panic while a frame plays haptics lost every queued outcome** (`flui-interaction`):
  each request leaves the queue only as its outcome enters the mailbox, so outcomes resolved before
  the panic are delivered and the rest stay queued for the next frame.
- **A key pressed again after its release was lost never ended the first press**
  (`flui-interaction`): when focus moved to another unit in between, the first press's path is
  told its key was cancelled, so a keyboard scroll it began ends.
- **A failing OS drop could report the backend's failure instead of the receiver's**, and an
  observer could miss a drag's end when releasing the drag's values failed (`flui-interaction`):
  the first failure propagates, and the observer hears the end first.
- **The same OS offer over two windows shared one record on exit** (`flui-interaction`): offers
  are keyed by presentation and id everywhere, so closing one window no longer ends the other
  window's hover. A hover over a window the pointer left no longer withdraws the drop feedback,
  and a dropped offer is told done only once no window's fetch from it is outstanding, even after
  the window it dropped in closes.
- **A haptic suppressed while the backend played it still played, or resolved out of ticket
  order** (`flui-interaction`).
