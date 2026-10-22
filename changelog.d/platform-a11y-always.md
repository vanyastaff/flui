### Removed

- **`flui`, `flui-app`, `flui-native`**: the `a11y` feature. Native accessibility is always on:
  every Win32 and AppKit window hands its semantics tree to UI Automation and NSAccessibility,
  and on Linux the winit host (`flui-native`'s `winit-backend`, which `flui-app` enables there)
  brings the AT-SPI adapter. A manifest that names `features = ["a11y"]` no longer resolves;
  drop the feature.
