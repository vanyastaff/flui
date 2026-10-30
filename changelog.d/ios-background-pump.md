### Fixed

- iOS: a pump while frames are disabled no longer sleeps 16 ms on the main thread. The display link paces it, so touches and scene events are delivered meanwhile.
