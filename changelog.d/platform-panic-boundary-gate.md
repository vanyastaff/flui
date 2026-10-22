### Fixed
- A panic in an accessibility listener no longer unwinds into the screen reader's call: on Windows it no longer aborts the process from inside UI Automation, and on macOS it no longer unwinds through AppKit. The handler answers as if no listener ran.
- A panic in a macOS view, window-delegate or text-input method, an iOS view, scene or application-delegate method, a Core Haptics handler, a Win32 enumeration or low-memory callback, or an OLE drag-and-drop or TSF method that answers a constant is contained at the OS boundary, logged with its text, and answered with a failure the OS understands, instead of unwinding into the OS.
- A panicking wake-deadline hook on macOS no longer aborts the process from a GCD trampoline, and the next deadline is still scheduled.
