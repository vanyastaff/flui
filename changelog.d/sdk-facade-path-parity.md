### Added

- The facade exposes the custom text-store vocabulary under `interaction::text_input` and
  `ImagePlacement` under `painting`, matching the existing SDK owning-layer paths.

### Changed

- `flui-sdk` places painting values under the same `painting::{paint, styling, typography}`
  modules as the facade instead of duplicating them at a flat SDK-only path.
- `TextInputPurpose` is at `platform::text_store::TextInputPurpose`, and `Availability` is the
  ungated `platform::Availability` in both the SDK and facade. The surface identity test now
  requires the same path on both sides, not merely the same type at different paths.
