### Added
- The native Wayland host carries out `WindowCommand::Activate` through `xdg-activation`: the window asks the compositor for a token naming the last user input and hands it back once the compositor has shown the window, and the compositor decides whether it takes the focus or only asks for the user's attention (sway marks it urgent). `WindowCapabilities::activates` is `Emulated` there; a compositor without `xdg_activation_v1` (Weston 13) refuses the command with `NotOnThisDevice`. The token a launcher hands the process (`XDG_ACTIVATION_TOKEN`) activates its first window, and `xdg-open` started for a link is not handed it.
- `flui_native::testing::wayland::offers_activation`, and `CompositorReport::{activation_tokens, activations}`: the tokens that came for a window and the ones it handed back.

### Changed
- `WindowCommand::Activate` on Windows brings the window forward only where Windows' foreground rules let the process; where they refuse, the window's taskbar button flashes until it comes forward, and the foreground stays with the application the user works with. `WindowCommand::Show` asks for the foreground under the same rules.
- Opening a URL, a file or a folder on Windows (`open_url`, `open_path`, `reveal_path`) hands the foreground right to the application the shell gives it to (`AllowSetForegroundWindow(ASFW_ANY)`), so a handler already running can come forward.

### Fixed
- `Platform::activate` on Windows no longer calls `SetForegroundWindow` on whatever window is in front, which belonged to another application; it does nothing, as a Win32 window comes forward through `WindowCommand::Activate`.
