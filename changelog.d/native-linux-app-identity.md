### Added

- A Linux window carries the application's id: Wayland windows set it as their app id, X11 windows write it as both halves of `WM_CLASS` and as `_GTK_APPLICATION_ID`, so a dock shows the desktop entry's name and icon and groups the window under its launcher. Without an id a window carries its executable's name in ASCII. X11 windows also write `WM_HINTS` saying they take the input focus.
- `flui build linux` writes a desktop entry for every application, named by what its windows carry (`<id>.desktop` with `StartupWMClass=<id>`, or the executable's name when `flui.toml` declares no id), so two examples of one directory keep two entries.
- `flui::app::AppId` and `AppIdError`: the application id, at most 231 bytes so every name it is part of (the Windows single-instance pipe first) can hold it. `CurrentHost::start_with` takes it (`Option<&AppId>`) and `CurrentHost::start_single` takes an `&AppId`; `IdentityError` keeps only the display name's refusals.
