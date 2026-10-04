### Changed

- Object keys derive allocation identity directly from their owning Arc, preserving reconciliation behavior without manual unsafe thread-safety implementations.
