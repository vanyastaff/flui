### Changed

- The semantics pass allocates a child-fragment list only for a node whose children contribute a fragment, and keeps its visited table between passes. A `SlideTransition` tick with semantics on allocates 21 times instead of 64 (5060 bytes instead of 42272), and a full reassemble allocates 1473 times instead of 1565. The accessibility tree is unchanged.
