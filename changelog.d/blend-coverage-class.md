### Added

- `flui_painting::paint::CoverageClass` and `BlendMode::coverage_class`: the one classification of blend modes by how they treat a transparent or partly covered source (`Absorbing`, `Saturating`, `Replacing`, `Masking`, `Advanced`). Reachable from the facade as `flui::painting::paint::CoverageClass`. Like `BlendMode`, it is exhaustive, so a new class is a breaking change.
- `flui_painting::testing::EVERY_BLEND_MODE` (`testing` feature): every `BlendMode` variant, kept complete by a compile-time match.

### Changed

- `BlendMode::is_porter_duff` and `keeps_destination_under_transparent_source`, `ColorFilter::modifies_transparent_black`, `Color::blend` and the engine's coverage, SSAA and blend-state routing match every blend mode by name. A new mode no longer falls into a default arm (`SrcOver` blending, no coverage correction, a source-over formula) and must be classified wherever it is routed. Behaviour is unchanged for every existing mode.
