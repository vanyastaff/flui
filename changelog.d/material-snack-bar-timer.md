### Changed

- A shown `SnackBar` no longer asks for a frame while it rests: its display duration is counted by a timer on the UI runtime's clock instead of a ticking animation controller, so nothing is drawn between its entrance and its timeout.
- A `Scaffold` nested in another `Scaffold` under the same `ScaffoldMessenger` no longer shows the messenger's snack bar a second time: only the outermost scaffold of a nested set shows it.
- `flui_sdk::scheduler` re-exports `TimerHandle` and `Instant`, the UI runtime's timer and the instant its clock reads.
