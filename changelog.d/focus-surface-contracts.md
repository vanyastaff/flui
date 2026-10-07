### Changed

- Focus key dispatch and global handlers preserve `KeyEventResult`, including propagation stops that leave native default handling available. Traversal resolution takes `TraversalDirection` instead of a boolean.
- Focus widgets retain weak `FocusSubscription` guards for node notifications; dropping a guard withdraws that listener while preserving callback failure ownership.
