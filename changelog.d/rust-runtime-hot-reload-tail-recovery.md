### Fixed
- Preserve redraw demand and retry owner delivery when a queued hot reload panics, keeping the accepted FIFO tail live and the original failure authoritative.
