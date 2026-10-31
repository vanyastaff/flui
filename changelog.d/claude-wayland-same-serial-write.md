### Fixed

- Native Wayland host: a second clipboard write made with no user input between replaces the text the selection answers. Weston drops a second selection set under the same serial, so the second write was lost and the seat kept the first text or none.
