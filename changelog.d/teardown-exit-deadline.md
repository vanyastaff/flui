### Changed

- **The exit has one deadline.** At loop exit the flush registry is written first, then the
  services and the background pools share one six-second budget (ADR-0258 §7) instead of
  five seconds each, one after the other. `ExecutionServices::shutdown` takes the
  `Instant` to finish by instead of a grace per pool.

### Added

- **`TeardownStep::Flush`**: the step of the exit order that writes the flush registry.
- **The exit watchdog.** An exit that has not ended 15 s after the main window shut down
  (an application destructor that never returns) is ended with `flui_app::EXIT_CODE_HUNG`
  (124) after the flush registry is written once more, instead of lingering with its window
  gone. `flui_native::end_process` ends the process without running destructors or `atexit`
  handlers.

### Fixed

- **A panic at the exit no longer skips the flush.** A destructor of the application's main
  window factory or its captures that panics while the application exits, or a panic out
  of the platform's own loop, is held until the exit has written the flush registry and
  stopped the services and pools, and is then resumed: the process exits with 101 and the
  panic's text. Before, such a panic was either dropped silently or resumed before the
  flush.
