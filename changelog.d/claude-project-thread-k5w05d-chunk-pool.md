### Fixed

- A paint pass keeps the room its shared fragment buffer needed in the two passes before, so a list scrolled frame after frame stops growing that buffer again on every frame (`list_10k_scroll_warm`: 792 559 → 748 207 bytes, 126 → 121 reallocations).
