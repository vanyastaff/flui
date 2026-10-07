### Fixed

- **Per-window device pixel ratio** (`flui-runtime`, `flui-app`): a window's scale-factor change
  now reaches that window's own pipeline and semantics owner. Previously every change was applied
  to the primary window, so a secondary window kept its old ratio and published accessibility
  bounds at the wrong scale, and the primary took a scale reported by another monitor.
  `UiRealm::set_device_pixel_ratio` is replaced by `UiRealm::set_device_pixel_ratio_for`, which
  takes the presentation. `flui-testing`'s `HeadlessHost` can open further windows
  (`open_window`, `attach_to`, `enable_semantics_on`, `set_scale_factor`) and read what each
  window published to its accessibility adapter (`published_a11y_tree`).
