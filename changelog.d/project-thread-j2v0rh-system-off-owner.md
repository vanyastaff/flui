### Changed

- `flui-system` bounds a UI runtime's work off its owner thread with one count on every platform: eight pieces at once (Android ran four), a save's write of its contents among them. A save made while every place is held is refused at once with `Unavailable` instead of starting another thread.
