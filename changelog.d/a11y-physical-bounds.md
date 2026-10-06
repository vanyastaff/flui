### Fixed

- **Accessibility bounds on scaled displays** (`flui-semantics`, `flui-rendering`): the tree
  published to AccessKit now carries the window's device pixel ratio as a scale transform on its
  root, so screen readers receive bounds in physical pixels. At 150% UI Automation previously
  reported every control at two thirds of its size and offset toward the window's corner, so
  Narrator highlighted and hit-tested the wrong area. A scale-factor change, such as moving the
  window to another monitor, republishes the root. `SemanticsOwner::set_device_pixel_ratio` sets
  the ratio; `PipelineOwner::set_device_pixel_ratio` forwards it.
