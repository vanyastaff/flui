### Fixed

- Refuse nonfinite move, scroll, and gesture positions before hit testing or pointer delivery, preserving existing contacts and terminal delivery.
- Retain intermediate hardware samples when frame-coalescing moves and feed their production timestamps into drag, multi-drag, scale, and tap-and-drag velocity estimation.
- Preserve measured excursions for tap, double-tap, long-press, multi-tap, drag, multi-drag, tap-and-drag, and scale admission when a pointer returns to its origin within one frame; predicted positions remain excluded and callbacks keep the current frame's geometry.
