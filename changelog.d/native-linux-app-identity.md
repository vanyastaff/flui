### Added

- A Linux window carries the application's id: Wayland windows set it as their app id, X11 windows write it as both halves of `WM_CLASS` and as `_GTK_APPLICATION_ID`, so a dock shows the desktop entry's name and icon and groups the window under its launcher. X11 windows also write `WM_HINTS` saying they take the input focus.
- `flui build linux` writes the desktop entry `share/applications/<id>.desktop` for every application, with `StartupWMClass=<id>`, and compiles the same id in, so the entry matches the window even for an application without a `flui.toml`.

### Changed

- `AppId` and `AppIdError` live in `flui-foundation` (still reachable as `flui::app::AppId`); `AppId::try_new` returns `AppIdError`, and `IdentityError` keeps only the display name's refusals.
- An application id is at most 247 bytes, so `<id>.desktop` is a valid file name; a longer one is refused.
- `CurrentHost::start_with` takes the application id (`Option<&AppId>`), and `CurrentHost::start_single` takes an `&AppId` instead of a string, so an id that is not reverse DNS can no longer reach a host.
