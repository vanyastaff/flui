### Changed

- Focus key dispatch and global handlers preserve `KeyEventResult`, including propagation stops that leave native default handling available. Traversal resolution takes `TraversalDirection` instead of a boolean.
- Focus widgets retain weak `FocusSubscription` guards for node notifications; dropping a guard withdraws that listener while preserving callback failure ownership.
- Focus-node observation uses subscription guards; raw listener IDs remain private implementation details, and the unused node listener-count utility is removed.
