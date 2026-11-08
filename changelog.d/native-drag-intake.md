### Changed

- Drag intake now shares one bounded type-admission and fetch policy across native backends, including direct transport requests. Declared, accumulated and decoded sizes use the same transfer-limit check while protocol-specific enumeration bounds remain in place.
