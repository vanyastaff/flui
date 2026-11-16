### Fixed

- Windows: work the host posts to its own windows is no longer lost when the thread's message queue is full. A hidden or minimized window's frame request, a delivery raised on another thread, a close the handler granted, the fullscreen hotkey, the exit decision after the last window closes and the low-memory notice now run from the owner's next turn instead of being dropped, so the loop still ends with its last window.
- Windows: a burst of frame requests on a hidden or minimized window (a background task waking many times between turns) is one frame, as it is for a visible window, instead of one frame per request.
