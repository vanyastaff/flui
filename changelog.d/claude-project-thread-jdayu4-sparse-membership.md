### Changed

- Settling a render node's children no longer builds a membership set for a parent without lazy sliver children: scrolling the 10 000-row list by one screen makes 3 992 allocations instead of 4 132.
