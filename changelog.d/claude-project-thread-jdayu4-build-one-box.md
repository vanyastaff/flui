### Changed

- A build's child view is erased once, straight into its shared `BoxedView`, instead of a `Box` that was then copied into an `Rc`; an element's child list keeps its buffer across rebuilds. A label change allocates 406 times over four frames instead of 434, and a full reassemble 1840 instead of 2037.
