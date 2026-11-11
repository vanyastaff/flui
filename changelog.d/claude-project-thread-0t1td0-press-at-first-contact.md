### Changed

- A `GestureDetector` with tap, secondary-tap or long-press callbacks, and every control built on `RawButton`, builds its press recognizers at the first contact it is offered instead of at mount. A tap beside a double tap still composes its arena branch at mount. Mounting a `RawButton` over a `Text` makes 14 fewer allocations (84 to 70) and holds 1.2 KB less.
- `RecognizerSet` holds one attachment inline, so a listener that feeds one recognizer or one group allocates nothing for its set.
- A rebuild that names the recognizers a `RawGestureDetector` already has, in their order, allocates nothing to pair them.
