### Changed

- The Android host platform is neither `Send` nor `Sync`: its loop keeps the Activity's window slot and stop flag in owner-thread cells (ADR-0327 §5).
