### Fixed

- Refuse nonfinite move, scroll, and gesture positions before hit testing or pointer delivery, preserving existing contacts and terminal delivery.
- Retain intermediate hardware samples when frame-coalescing moves and feed their production timestamps into drag, multi-drag, scale, and tap-and-drag velocity estimation.
