### Fixed

- An offscreen slot whose frame panics or fails before its submission keeps the image its last completed frame drew on its target, instead of being left cleared to the background.
