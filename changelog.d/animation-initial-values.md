### Fixed

- Controller builders apply the same value canonicalization as `set_value`: bounded positive infinity selects the upper endpoint, NaN and negative infinity select the lower endpoint, and unbounded non-finite inputs retain zero.
- Non-finite initial values issue the controller's existing warning once; subsequent direct assignments share that warning latch. Initialization commits the finite value before invoking diagnostics.
