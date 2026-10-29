### Fixed
- A macOS app no longer aborts on start with "wake deadline read before owner checkout returned": the AppKit wake pump consults the wake-deadline hook on its own owner-queue turn instead of inside the redraw request.
