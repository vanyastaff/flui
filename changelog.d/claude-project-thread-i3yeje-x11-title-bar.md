### Added

- X11: the native host's window takes `WindowFrame::AppTitleBar` as a window without decorations where the window manager lists `_NET_WM_MOVERESIZE`. A press on a region `WindowTitleBar` marks moves the window through the window manager once the pointer travels, a double-click maximizes or restores it, and a band along the edges or a `WindowResizeArea` resizes it. `input::availability::x11` answers `window_move` and `window_resize`.
