### Changed
- The Android manifest `flui create` and `flui build` generate declares `locale` and `layoutDirection` in the Activity's `configChanges`, so a language or layout-direction change reaches the running app (the host reads the languages again and the font fallback follows) instead of recreating the Activity and restarting `android_main`.
