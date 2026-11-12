### Changed

- Paint records every render object's fragment into one buffer the pass shares, after its ancestors' operations, instead of a vector per render object: a node that paints only its child allocates nothing, and a label change costs 298 allocations instead of 366.
