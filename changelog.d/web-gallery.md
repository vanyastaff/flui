### Added

- The web runner drives an installed development agent hook (`AppConfig::with_dev_agent`): it attaches the hook before the page's window opens and hands it that window once it is installed, as the desktop and iOS runners do. The page's loop never ends, so the hook is never detached.

### Fixed

- Web: a touch that moves on the canvas stays the application's. The canvas sets `touch-action: none`, so the browser no longer takes a swipe, drag or pinch for its own panning and cancels the pointer a few pixels in.
- Web: `request_redraw` no longer runs the frame inside the call. A frame that asked for the next one, as a running animation does, ran again at once at the same frame time, and an animation started from a click (a swiped row closing, a page settling, a pull to refresh) held the page's thread for good; the next animation frame now serves it.
