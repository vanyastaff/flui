### Added
- The native Wayland host carries out `WindowCommand::Activate` through `xdg-activation`: the window asks the compositor for a token naming the last user input and hands it back once the compositor has shown the window, and the compositor decides whether it takes the focus or only asks for the user's attention (sway marks it urgent). `WindowCapabilities::activates` is `Emulated` there; a compositor without `xdg_activation_v1` (Weston 13) refuses the command with `NotOnThisDevice`. The token a launcher hands the process (`XDG_ACTIVATION_TOKEN`) activates the first window the host holds, and `xdg-open` started for a link is not handed it.
- `flui_native::testing::wayland::offers_activation`, and `CompositorReport::{activation_tokens, activations}`: the tokens that came for a window and the ones it handed back.

### Changed
- `WindowCommand::Activate` on Windows brings the window forward only where Windows' foreground rules let the process; where they refuse, the window's taskbar button flashes until it comes forward, and the foreground stays with the application the user works with. `WindowCommand::Show` asks for the foreground under the same rules.
- Opening a URL, a file or a folder on Windows (`open_url`, `open_path`, `reveal_path`) goes through `ShellExecuteExW` and hands the foreground right to the handler process the shell started, by its id; where the shell started none (the handler was already running) or failed, nothing is handed over.
- `Platform::activate` is documented as doing nothing on every host: an application comes forward through one of its windows.

### Fixed
- `Platform::activate` on Windows no longer calls `SetForegroundWindow` on whatever window is in front, which belonged to another application.
