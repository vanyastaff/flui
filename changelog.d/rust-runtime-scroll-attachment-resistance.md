### Fixed

- Repeated outward bouncing scroll input now advances in the requested direction; stationary and reversed input preserve or consume overscroll without reapplying resistance to its accumulated distance.
- Replacing a Scrollable controller stops its retired trajectory and cancellation hook. Rebuilding with the same position keeps its run, and retiring an older attachment preserves a newer attachment's cancellation hook.
