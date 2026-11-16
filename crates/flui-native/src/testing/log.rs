//! How a unit test counts the log lines a host writes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};

/// How many events with `target` are written on this thread while `body`
/// runs.
pub(crate) fn lines_in(target: &'static str, body: impl FnOnce()) -> usize {
    struct Count {
        target: &'static str,
        lines: Arc<AtomicUsize>,
    }
    impl<S: tracing::Subscriber> Layer<S> for Count {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            if event.metadata().target() == self.target {
                self.lines.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    let lines = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(Count {
        target,
        lines: Arc::clone(&lines),
    });
    tracing::subscriber::with_default(subscriber, body);
    lines.load(Ordering::SeqCst)
}
