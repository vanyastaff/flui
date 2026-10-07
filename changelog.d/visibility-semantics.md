### Fixed

- Hidden `Visibility` children retained for layout no longer appear in the accessibility tree by default. `Visibility` and `VisibilityGate` offer `maintain_semantics` for explicit retention, and changes to child accessibility invalidate semantics.
