### Fixed

- Ignore contact bounce within 40 ms after a double tap's first release, preserving the first tap's pending verdict and timeout. A second press at exactly 40 ms remains eligible, and holding an earlier press past the boundary cannot admit it retroactively.
