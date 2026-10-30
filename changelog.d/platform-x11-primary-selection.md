### Added

- **`flui-native`**: the native X11 host (`X11Platform`) has a primary selection.
  `Platform::primary_selection` hands out the `PRIMARY` selection, owned and read on the
  clipboard's own connection and thread. The application's selection stays lazy: its closure is
  asked for the text at the loop's turn, only when another application reads it.
