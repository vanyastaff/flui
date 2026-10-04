### Fixed
- Preserve lifecycle callback ownership after an earlier failure or during unwinding, keeping cancellation, terminal cleanup and queued notifications usable without replacing the first panic.
