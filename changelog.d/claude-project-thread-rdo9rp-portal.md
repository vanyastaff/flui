### Added

- **System services on Linux** (`flui-system`): `SystemPlugin` provides the launcher and the file
  dialogs through the XDG desktop portal (`OpenURI`, `Trash`, `FileChooser`) instead of refusing
  them. Dialogs take their title, filters, folder, accept label and suggested name, and are modal to
  an X11 window. `testing::PortalOnBus` runs these providers against a portal on another bus.
