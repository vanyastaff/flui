### Fixed

- Simple transform inversion accepts finite tiny scales with representable reciprocals and refuses non-finite translation, rotation and scale values. Analytical variants retain their shape; complex transforms keep the matrix computed-inverse policy.
