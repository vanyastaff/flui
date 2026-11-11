### Changed

- An element that publishes a value no longer copies the table of every provider above it. Its scope shares that table and adds the few types the nearest providers publish; the table is rebuilt only when those outgrow four. Lookups hash a `TypeId` with `FxHasher`. A `RawButton` over a `Text` holds 0.35 KB less (8.4 to 8.05 KB) and makes one allocation fewer.
