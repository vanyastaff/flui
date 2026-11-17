### Fixed

- **Windows: a modal loop left idle costs nothing.** A system menu, a window move or resize, or a
  drag held still no longer asks the application for its deadlines and repaints every 16 ms. The
  loop asks again at the next deadline, and soon after anything on the UI thread ran (an owner
  turn, a window message, a drop target's event), so a deadline set meanwhile is still delivered
  inside the loop.
- **Windows: one cloak subscription per UI thread.** Every window used to install its own
  process-wide WinEvent hook for cloak changes, so a virtual-desktop switch was heard once per
  window for every window it cloaked. The thread now holds one hook while it has a window.
- **A tree published from inside the accessibility activation listener answers the screen
  reader.** Publishing there re-entered the adapter's own state (a contained panic under UI
  Automation and NSAccessibility, a deadlock under AT-SPI), and the window then reported no
  assistive technology attached while the adapter was active.
