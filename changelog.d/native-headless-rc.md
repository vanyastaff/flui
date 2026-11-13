### Changed

- `flui_native::HeadlessHostWindow` and the headless platform's owner-thread handles (`HeadlessInputDevices`, `HeadlessDragAndDrop`, `HeadlessExitReevaluation`, `HeadlessDeferredWindowOpens`) are no longer `Send`: the headless host keeps its state on its owner thread (ADR-0327 §5). `HeadlessDisplayControls`, `HeadlessPreferences` and `HeadlessHostEvents` stay `Send + Sync`, as the operating-system sources they stand in for.
