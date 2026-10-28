### Fixed

- The native Wayland host names the keyboard layout from where the keymap names it, found once per keymap, so a burst of layout switches no longer rescans a keymap of up to 8 MiB on each one, and the names cost no copy of the text.
- The native X11 host asks the server only for the devices a hierarchy change adds, as it happens, so a tablet or touch screen plugged in reaches its first stroke as itself; a device change takes the classes it carries without a query.
