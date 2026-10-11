### Changed

- Replaced the `Curves` namespace struct with the `curves` module and Rust-style constant names, for example `Curves::EaseInOut` → `curves::EASE_IN_OUT`. Rustdoc aliases retain search discoverability; the old type and paths are removed.
- Renamed the misleading `Curves::SlowOutFastIn` preset to `curves::LINEAR_OUT_SLOW_IN`. Curve parameters and sampled values are unchanged.
- Clarified that Bézier presets named after polynomial or trigonometric motion styles are approximations, while the four CSS easing presets use the CSS control points.
