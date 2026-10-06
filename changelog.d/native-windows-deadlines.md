### Fixed
- The native Windows message loop wakes for the frame deadlines FLUI wires today, gesture deadlines (long press, double tap) and the runner's own device-recovery, frame-pacing and first-reveal deadlines, so they resolve without additional input, minimized or hidden windows included. General application timers do not contribute a wake deadline yet.
