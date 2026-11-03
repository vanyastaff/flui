### Added

- Wayland: the native host's window takes `WindowFrame::AppTitleBar` where it can draw a frame of its own. It asks the compositor for no frame and keeps the resize margins of its own frame without its title bar; a press on a region `WindowTitleBar` marks moves the window through `xdg_toplevel.move` once the pointer travels, a double-click maximizes or restores it, and a secondary press shows the compositor's window menu. `input::availability::wayland` answers `window_move` and `window_resize`, and `set_input_regions` takes the regions.
