### Added

- **iOS keyboard insets** (`flui-native`): the iOS window reads
  `UIKeyboardWillChangeFrameNotification`, so its insets carry the keyboard's occlusion and its
  motion (duration and curve), and `reports_keyboard_inset` is `Native`.
- **Web keyboard insets** (`flui-native`): the Web window reports the keyboard over its canvas,
  from `navigator.virtualKeyboard` where the browser has it (the window asks it to overlay the
  page, and the capability is `Native`) or else from the visual viewport (`Emulated`).
