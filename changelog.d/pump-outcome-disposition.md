### Changed
- `PumpOutcome::Presented` carries the frame's `PresentDisposition`. The inline raster lane classifies a frame from it, so a rendered frame can no longer fall back to "no present" for a missing completion and lose its frame-aligned haptics.
