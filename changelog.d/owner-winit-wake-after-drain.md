### Fixed

- **winit loop no longer loses a wake that lands while it reads** (`flui-native`): the winit
  host reads what background sources recorded (an X11 window's mode properties, logind and
  memory-pressure notices, the desktop portal's appearance) before the application's turn. A
  wake that arrived during those reads was folded into the turn already under way and settled
  by it, so its change stayed unread until some unrelated event woke the loop: a window manager
  that minimized and then maximized an X11 window in quick succession could leave the window
  reporting itself minimized. The turn now begins before the reads, so such a wake posts a turn
  of its own, which reads again.
