### Added

- `flui_driver::lane::macos`: the macOS lane. `MacosLane` posts Quartz events to the app's
  process from a private, tagged event source (the mouse and its wheel, keys with the character
  they type, text as Unicode runs, a pen as tablet events), reads the window's accessibility
  tree and projects it against the app's (`Window::os_tree`, `compare_projection`), presses an
  element through `AXPress` (`os_invoke`), and captures the window through ScreenCaptureKit from
  macOS 14 (`capture_os`). `MacosGuard` ends an OS run on the first event a listen-only session
  tap sees that the lane did not send. Before every event the lane reads the window, the
  foreground, the window under the point and the hardware's held input again, and before a
  sequence it refuses a system shortcut (the app switcher, Spotlight, Force Quit, Mission Control,
  the screenshot tool, VoiceOver, the input-source switch, Siri and the accessibility taps). A
  missing Accessibility, Input Monitoring or Screen Recording permission is `not_supported` naming
  `macos_permission_<name>`, never a prompt the run waits on.
- A macOS app the driver launches leads a process group of its own, which ends with the session.
