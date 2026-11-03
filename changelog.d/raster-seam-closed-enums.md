### Changed

- `flui_engine::PumpOutcome`, `FrameDropReason` and `RasterSubmitError` are no longer `#[non_exhaustive]`, for ADR-0068 §2's reason: the raster lane matches every outcome by name, so a new one fails to compile there instead of being reported as a failed frame with no retry. `PumpOutcome`'s struct variants stay `#[non_exhaustive]`.
