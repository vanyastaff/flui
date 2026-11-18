### Added

- macOS: a window whose content lies under its screen's camera housing reports that band as its safe area (`WindowState::insets`, so `MediaQuery`'s view padding and `SafeArea`), from macOS 12 on: the screen's top inset for a window that covers the whole screen, nothing for a window below the housing. Built and type-checked, not yet run on a Mac with a housing.

### Changed

- macOS: the refresh period of the built-in display is 1/60 s or longer while Low Power Mode is on, and the display snapshot gets a new revision when switching Low Power Mode changes it (macOS 12 and later). A ProMotion panel reported 1/120 s there before. Built and type-checked, not yet run on a Mac.
- macOS: a view's contents keep their own size at its top-left corner when a frame is not the view's size yet, as during a live resize, where they were stretched to it. Built and type-checked, not yet looked at on a Mac.

### Fixed

- macOS: a display whose current mode reports 0 Hz has the nominal refresh period of its display link, where it had none and paced at the 60 Hz default. Built and type-checked, not yet run on such a display.
