### Fixed

- Refuse GlobalKey identity allocation permanently after issuing `u64::MAX`, preventing wraparound from issuing zero and reusing earlier keys.
