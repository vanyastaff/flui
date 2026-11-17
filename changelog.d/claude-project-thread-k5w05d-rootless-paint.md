### Fixed

- A frame that runs while the render tree has no root no longer reports `painted = true` for a patch an earlier failed frame staged, and that patch is dropped instead of being committed by a later report.
- A change made while the render tree has no root (a patch a failed frame left staged, or a paint mark made after the root was removed) is painted by the first frame after the root returns, instead of being lost until the same node changes again.
