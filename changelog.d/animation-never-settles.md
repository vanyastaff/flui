### Added

- **A simulation that never settles is reported** (`flui-animation`, debug and `diagnostics`
  builds): a fling, custom simulation or animated-value segment that has run for a minute of its
  own time without settling is reported once on `flui_animation::never_settles`
  (`NEVER_SETTLES_TARGET`) with the controller's label.
