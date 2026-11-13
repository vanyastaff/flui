### Fixed

- A paint pass keeps the room its shared fragment buffer needed in two of its last eight passes (up to twice 1024 operations), so a list scrolled frame after frame stops growing that buffer again on every frame, a small repaint between its frames included (`list_10k_scroll_warm`: 792 559 → 748 207 bytes, 126 → 121 reallocations).
