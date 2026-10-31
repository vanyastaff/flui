### Fixed

- Native Wayland host: a second clipboard write made with no user input between replaces the text the selection answers. Weston up to 14 drops a second selection set under the same serial, so the second write was lost and the seat kept the first text or none. A write whose concealment differs from the selection's sets a new source, so the password-manager hint follows the latest write where the compositor takes it; where it keeps the earlier source, the concealed write goes unmarked and a warning says so.
