### Added

- **`flui-native`**: `PlatformProxy::open_window` opens a window from another thread on every
  host (Win32, AppKit, Wayland, X11, Android, UIKit, the web and headless). The request queues on
  the loop's owner signal, at most 64 at once (`ProxySendError::Full` hands it back), and opens in
  the owner's next turn; its `PendingWindow` answers the window's identity or why it did not
  open, and a quit answers what is still queued with `OpenWindowError::OwnerGone`.
- **`flui-foundation`**: `ClaimHandle::try_claim`, a non-blocking poll that tells "nothing yet"
  from every resolution, `OwnerGone` included.

### Fixed

- **`flui-native`**: `PendingWindow::try_take` answers `OpenWindowError::OwnerGone` when the
  owner stopped before answering, instead of `None` forever.
