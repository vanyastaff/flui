//! The channel a loop pushes system notices through, as the owner sees it.
//!
//! Every loop keeps one, which its host's table makes over the loop's
//! application delivery: the lifecycle feature implements it
//! (`lifecycle::notice`), and the owner names no feature. A host's lifecycle
//! source that hears the operating system say the machine woke, slept, locked
//! or wants memory back dispatches the notice to it; so does a loop that hears
//! one in its own callbacks (a UIKit memory warning, an Activity's trim level).
//! A wake marks what the platform cached about the world stale before the
//! application's handler hears of it.
//!
//! A loop that hears its host's lifecycle in its own callbacks also answers
//! the application's release of a session end; `LifecycleHooks` is how the
//! lifecycle feature's position reaches it.

use std::sync::{Arc, Weak};

use flui_platform::SystemNotice;

/// Something a platform caches about displays or windows, and can mark stale.
pub(crate) trait StaleOnWake: Send + Sync {
    /// Mark the cached value stale: the next read asks the operating system.
    fn mark_stale(&self);
}

/// A loop's system-notice channel: what a wake marks stale before the
/// application's handler hears of it, and the way to that handler.
pub(crate) trait NoticeChannel: Send + Sync {
    /// Mark `cache` stale whenever the machine wakes, for as long as it lives.
    fn mark_held_stale_on_wake(&self, cache: Weak<dyn StaleOnWake>);

    /// Report `notice` to the application's handler. A wake marks every cache
    /// registered with the channel stale first, even while the handler is busy
    /// with an earlier notice. A panic of the handler resumes here.
    fn dispatch(&self, notice: SystemNotice);
}

impl dyn NoticeChannel {
    /// [`NoticeChannel::mark_held_stale_on_wake`] for a cache its owner holds
    /// by its own type.
    pub(crate) fn mark_stale_on_wake<T: StaleOnWake + 'static>(&self, cache: &Arc<T>) {
        let cache: Weak<T> = Arc::downgrade(cache);
        self.mark_held_stale_on_wake(cache);
    }
}

/// A loop that hears its host's lifecycle in its own callbacks (AppKit's
/// application delegate, UIKit's, the Activity's, the winit loop's turns on
/// Linux). The lifecycle feature's position reaches it through this.
#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    all(target_os = "linux", feature = "wayland")
))]
pub(crate) trait LifecycleHooks {
    /// The application finished with a session end it answered with a block.
    /// A loop whose host cannot be asked to wait holds no end open, and this
    /// does nothing.
    fn release_session_end(&self);
}

/// The notice channel the loops' tests give a loop that hears nothing.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Weak;

    use flui_platform::SystemNotice;

    use super::{NoticeChannel, StaleOnWake};

    /// A notice channel that marks nothing stale and reports nothing.
    pub(crate) struct NoNotices;

    impl NoticeChannel for NoNotices {
        fn mark_held_stale_on_wake(&self, _: Weak<dyn StaleOnWake>) {}

        fn dispatch(&self, _: SystemNotice) {}
    }
}
