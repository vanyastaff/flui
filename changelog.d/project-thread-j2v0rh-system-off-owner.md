### Changed

- `flui-system` bounds a UI runtime's work off its owner thread with one count on every platform: eight pieces at once (Android ran four, the Linux portal's requests and a save's write ran unbounded). A launch, an autostart change, a user's folder, a grant or a save made while every place is held is refused at once with `Unavailable` instead of queueing or starting another thread.
