### Fixed

- **Android: a backgrounded app no longer wakes every display period** (`flui-platform`): the event
  loop blocks until something wakes it. A paused Activity no longer runs its loop 60 to 120 times a
  second, and a wake deadline that only a frame can serve no longer turns the poll into a busy loop
  while paused. Preferences are read every 500 ms only while resumed (ADR-0249).
- **Android: the backgrounded pump no longer sleeps 16 ms on the loop thread** (`flui-app`): it is
  paced by a deadline the wake hook reports, so input and lifecycle events keep arriving.
- **Android: safe-area insets follow a rotation** (`flui-platform`): the insets are asked for on
  every resize, configuration change and new native window, and a reply taken at the old size is
  asked for again, instead of waiting up to a second for the 500 ms poll.
- **Android: hidden system bars reserve no space** (`flui-platform`): from API 30 the safe area is
  the visible bars, so immersive and fullscreen content is not padded by a bar that is not shown.
- **Android: a window that changed size in the background is resized on return** (`flui-platform`):
  a new native window publishes its size before its surface is recreated.
- **Android: frame and resize callbacks no longer run with the window lock held**
  (`flui-platform`), so a callback that reaches the platform cannot deadlock the loop.
