//! The channel a loop pushes system notices through, as the owner sees it.
//!
//! Every loop keeps one, which its host's table makes: the lifecycle feature
//! implements it (`lifecycle::notice`), and the owner names no feature. A
//! host's lifecycle source that hears the operating system say the machine
//! woke, slept, locked or wants memory back dispatches the notice to it; so
//! does a loop that hears one in its own callbacks (a UIKit memory warning, an
//! Activity's trim level). The application's notice hook sits in it, and a
//! wake marks what the platform cached about the world stale before the hook
//! hears of it.
//!
//! A loop that hears its host's lifecycle in its own callbacks also keeps the
//! application's session-end and reopen hooks; `LifecycleHooks` is how the
//! lifecycle feature's position reaches them.

use std::sync::{Arc, Weak};

use flui_platform::SystemNotice;

#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    all(target_os = "linux", feature = "winit-backend")
))]
use crate::owner::handlers::SessionEndHook;

/// The callback a platform calls with each system notice.
pub(crate) type SystemNoticeHook = Box<dyn FnMut(SystemNotice) + Send>;

/// Something a platform caches about displays or windows, and can mark stale.
pub(crate) trait StaleOnWake: Send + Sync {
    /// Mark the cached value stale: the next read asks the operating system.
    fn mark_stale(&self);
}

/// A loop's system-notice channel: the application's hook, what a wake marks
/// stale before the hook hears of it, and the notices accepted but not yet
/// heard.
pub(crate) trait NoticeChannel: Send + Sync {
    /// Install the application's hook, replacing an earlier one.
    fn set_hook(&self, hook: SystemNoticeHook);

    /// Mark `cache` stale whenever the machine wakes, for as long as it lives.
    fn mark_held_stale_on_wake(&self, cache: Weak<dyn StaleOnWake>);

    /// Deliver `notice` to the application's hook. A wake marks every cache
    /// registered with the channel stale first, even while the hook is busy
    /// with an earlier notice.
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
/// Linux) and keeps the application's hooks for it. The lifecycle feature's
/// position registers through it.
#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    all(target_os = "linux", feature = "winit-backend")
))]
pub(crate) trait LifecycleHooks {
    /// The loop's notice channel.
    fn notices(&self) -> &dyn NoticeChannel;

    /// Keep `hook`, which the loop asks at each phase of a session end it
    /// hears, replacing an earlier one.
    fn set_session_end_hook(&self, hook: SessionEndHook);

    /// The application finished with a session end it answered with a block.
    /// A loop whose host cannot be asked to wait holds no end open, and this
    /// does nothing.
    fn release_session_end(&self);

    /// Keep `hook`, which the loop calls when the user reopens the
    /// application. A loop whose host has no reopen drops it unused.
    fn set_reopen_hook(&self, hook: Box<dyn FnMut() + Send>);
}

/// The notice channel the loops' tests give a loop that hears nothing.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Weak;

    use flui_platform::SystemNotice;

    use super::{NoticeChannel, StaleOnWake, SystemNoticeHook};

    /// A notice channel that keeps no hook and marks nothing stale.
    pub(crate) struct NoNotices;

    impl NoticeChannel for NoNotices {
        fn set_hook(&self, _: SystemNoticeHook) {}

        fn mark_held_stale_on_wake(&self, _: Weak<dyn StaleOnWake>) {}

        fn dispatch(&self, _: SystemNotice) {}
    }
}
