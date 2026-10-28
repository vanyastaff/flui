### Fixed
- On Windows, a key an input method took (`VK_PROCESSKEY`) no longer reaches key handlers as an `Unidentified` key, nor its release as an ordinary key, and no longer takes a character queued for the key typed after it.
