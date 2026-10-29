### Changed
- `flui_platform::text_store::PointMode` is a struct of two independent choices, `round_to_nearest_edge` and `nearest_when_outside`, as TSF's `GXFPF_ROUND_NEAREST` and `GXFPF_NEAREST` are; `PointMode::EXACT` and `PointMode::NEAREST` replace the two variants. The text store kit is at version 3, which checks every combination.

### Fixed
- On Windows, a key an input method took (`VK_PROCESSKEY`) no longer reaches key handlers as an `Unidentified` key, nor its release as an ordinary key, and no longer takes a character queued for the key typed after it. A key pressed before the input method opened is still released.
- On Windows, TSF's `GetACPFromPoint` answers a point outside the text with the nearest position only when `GXFPF_NEAREST` asks for it, and rounds inside a character only for `GXFPF_ROUND_NEAREST`.
