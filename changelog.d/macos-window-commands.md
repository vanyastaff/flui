### Fixed
- A macOS window's snapshot offers maximize, full screen, restyling, hiding and fixing the size only in the states AppKit carries them out in, so admission refuses the rest with `Unsupported` instead of `apply` failing with `ApplyError::Platform`.
- A macOS window maximized before full screen returns to normal mode, not maximized, when asked for normal mode from full screen.
