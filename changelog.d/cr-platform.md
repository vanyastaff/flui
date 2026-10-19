### Changed

- `HapticsHost` is configured through `HapticsHost::builder(..)` and `HapticsHostBuilder::build()`;
  the built host has no configuration methods, so no handle can exist while it is configured.
- `InputTapFactory::new` takes a closure of the `RecordingStart` the runtime's recording starts
  from (its windows' scales and gesture values, its preference revision), and `install_on`
  takes the `UiRuntime`; `flui_testing::recording::tap_factory` hands its closure the matching
  header. `HeadlessHost::install_input_tap` installs a factory as `flui-app` does.
- `DragOwner::deliver`'s hit test receives the pointer kind of the session an OS drag is, or
  `PointerKind::Unknown` for another application's drag.

### Fixed

- Configuring a haptics host after handing out a handle no longer leaves that handle
  answering `Unavailable`.
- Android's semantic haptics report what `performHapticFeedback` answered: `Suppressed` when
  the view declines it.
- Windows and Android report their window's refresh period, so the haptic frame deadline and
  the frame-pacing fallback follow the display; an unknown period is reported as a diagnostic.
- A `Draggable` row scrolled out of a lazy list's cache band keeps its drag.
- A touch drag of this runtime that reaches another of its windows through the OS is
  hit-tested with touch-sized targets.
- A replay moves each window to the scale its recording started at.
