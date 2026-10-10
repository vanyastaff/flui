### Fixed

- Preserve accepted resampler output after a callback panic, consume the failing packet once, and deliver its remaining tail through the next healthy sample or stop. Pending queries include prepared output; explicit clear cancels remaining debt.
- Reuse admission-reserved output buffers so unchanged-time measured sample and stop delivery preserve their allocation contract while retaining failed delivery tails and frozen reentrant ordering.
- Cancel already-prepared contact output on terminal withdrawal while preserving queued source flushing and the next frame's work for a reused pointer ID.
- Keep older resampled motion ahead of reentrant button edges and pending-input observation, while preserving each driving operation's frozen prefix and shared-handle recovery.
