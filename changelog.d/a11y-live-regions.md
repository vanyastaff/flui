### Changed

- **Live regions take a politeness** (`flui-semantics`, `flui-widgets`):
  `SemanticsConfiguration::set_live_region` takes `Option<Assertiveness>` and `live_region()`
  replaces `is_live_region()`; `Semantics::live_region` takes an `Assertiveness`. An assertive
  region reaches AccessKit as `Live::Assertive`.

### Fixed

- **Alerts and statuses announce themselves** (`flui-semantics`): a node with the `Alert` role is
  an assertive live region and one with `Status` a polite one unless set otherwise, as their ARIA
  roles imply.
- **Snack bars are announced** (`flui-material`): a shown `SnackBar` publishes one `Status` node,
  so a screen reader reads it when it appears.
