### Fixed

- Matrix inversion admits finite small transforms without an absolute determinant cutoff and refuses non-finite computed results. The boolean probe uses the same admission policy, and failed in-place inversion preserves its input.
