### Fixed

- Leaf layout failure reporting retains opaque panic payloads so their destructors cannot replace the layout error or abort recovery.