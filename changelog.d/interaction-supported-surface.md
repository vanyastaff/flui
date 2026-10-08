### Removed

- Removed unused arena teams, multiple-winner resolution and the separate priority-based pointer-signal resolver, including its handler ID. Widget scroll and pan-zoom arbitration continues through the closest claiming hit-path consumer.
- Removed the unused raw-input wrapper and input-mode switch; Listener callbacks receive the canonical pointer dispatch directly.
- Removed the unused local input extrapolator. Owned hardware-predicted samples remain part of pointer events and localization.
