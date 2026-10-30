### Added

- `AnimatedIcon` and `AnimatedIconData` in `flui-widgets`: vector icons whose paths move with an
  animation, mirrored under right-to-left text when the data says so, sized and colored by
  `IconTheme`. Constant data is built with `AnimatedIconData::from_static`, run-time data is
  checked by `AnimatedIconData::try_new`.
- `flui_material::AnimatedIcons`: the fourteen Material animated icon pairs (`PLAY_PAUSE`,
  `MENU_ARROW`, …).
- `cargo xtask icon-import`: converts SVG key frames or an Android animated vector drawable into
  `AnimatedIconData` constants.
