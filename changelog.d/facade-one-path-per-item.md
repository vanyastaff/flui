### Changed

- **Each item the facade publishes has one path** (`flui`; breaking). The crate root keeps
  `run_app`, `main`, `Application` and `LaunchError`; `flui::prelude` carries those three beside
  the widget and Material names and no longer carries `AppConfig`, `AppWindowError`,
  `WindowPolicy`, `register_font`, `open_window` or `open_secondary_window`.
  - `AppConfig`, `AppWindowError`, `WindowPolicy`, `open_window`, `open_secondary_window`,
    `AppControlError`, `AppHandle`, `BackendFailure`, `GpuBackend`, `UnsupportedSystem`,
    `FontRegistrationError` and `register_font` leave the crate root for `flui::app`.
  - `flui::app` no longer re-exports `run_app`, `main`, `Application`, `LaunchError`: use
    `flui::run_app`, `flui::main`, `flui::Application`, `flui::LaunchError`.
  - `AppId` and `AppIdError` are `flui::app` only, no longer in `flui::foundation`.
  - `CursorIcon` is `flui::interaction::CursorIcon` only, no longer in `flui::rendering` or
    `flui::platform`.
  - `NumericRange`, `NumericRangeError`, `SemanticsConfiguration` and `SemanticsRole` are
    `flui::semantics` only, no longer in `flui::rendering`.
  - `PointerKindSet` is stable and `flui::interaction::PointerKindSet` only, no longer in
    `flui::platform::input_capabilities` (`flui-sdk` likewise).
  - `Inspect` (trait and derive) is `flui::foundation::inspect::Inspect`, no longer `flui::Inspect`.
  - `MotionOverride` is `flui::animation::MotionOverride`, no longer in `flui::app`.
