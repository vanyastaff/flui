### Fixed

- The native Wayland host names the keyboard layout from names read once per keymap, so a burst of layout switches no longer rescans a keymap of up to 8 MiB on each one; the native X11 host reads the device list and the keymap at most twice per loop turn however many change notices the server sends.
