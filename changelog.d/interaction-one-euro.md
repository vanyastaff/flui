### Changed

- One Euro filter tuning is private and validated; constructors and filtering return `Result` with `OneEuroFilterError` for invalid parameters, input or unrepresentable arithmetic.
- Rejected smoothing samples leave accepted history unchanged. The two-axis filter commits positions and timestamps together and preserves its anchor across duplicate or regressed timestamps.
