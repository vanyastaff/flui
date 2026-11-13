### Fixed

- A debug build no longer builds an accessibility consumer tree after each full semantics publication when no two nodes share an identifier, so a frame asks the heap the same amount on every run: that tree's maps are seeded at random per run, and how often they grew decided the frame's allocation figures.
