### Fixed

- RefreshIndicator accumulates pull distance across pointer updates and consumes it when the gesture reverses before scrolling content.
- Replacing a RefreshIndicator scroll position stops the retired position's fling and routes subsequent flings to the new position; rebuilding with the same position preserves its run.
- Gestures released while RefreshIndicator is refreshing no longer start a ballistic scroll; scrolling resumes after completion.
