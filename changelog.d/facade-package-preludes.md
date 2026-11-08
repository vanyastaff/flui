### Added

- `flui_material::prelude`, also available as `flui::material::prelude`, curates the everyday Material widgets and theming types for an explicit glob import.

### Changed

- `flui::prelude` is now design-system-neutral and independent of Cargo features. Material applications import both `flui::prelude::*` and `flui::material::prelude::*`.
- First-party `material`, `cupertino`, `devtools`, and `hot-reload` facade features are standing train-aligned package re-exports under ADR-0315. Aggregated packages participate in the facade's semver contract and leave the facade before adopting an independent release cadence.

### Removed

- The empty `localizations` compatibility feature. Global widget localizations remain available through `flui::widgets` without a feature.
