### Fixed

- macOS: the pointer entering or leaving a window no longer aborts the process. The view read
  the click count of every mouse event to decide whether a press lands on an application-drawn
  title bar, and AppKit raises for `clickCount` on the tracking area's enter and exit events; it
  now reads a press only from a press.
