# Async

FLUI's frame scheduler (`flui-scheduler`) paces async work that interacts with the frame and
rebuild cycle, rather than `tokio` running it detached. Loading state, an error-with-Retry path
and cancellation when a route is popped mid-fetch are each built on that scheduler.
