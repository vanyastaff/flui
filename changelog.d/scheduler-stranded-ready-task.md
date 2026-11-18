### Fixed

- **`flui-scheduler`**: when an async task's poll panics and aborts the frame, the ready tasks
  the abort left unpolled request the next frame; they used to wait for a frame someone else
  asked for. A task that panics alone still requests no frame.
