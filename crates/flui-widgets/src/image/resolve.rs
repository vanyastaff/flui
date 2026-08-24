//! The resolve transaction behind [`Image`](crate::Image)'s async dispatch.
//!
//! Flutter parity: `_ImageState`'s `_updateSourceStream` / `_replaceImage`
//! pair (`widgets/image.dart`, 3.44.0). Every observable rule below is that
//! pair's, translated to a cache-key subscription instead of an
//! `ImageStream` listener:
//!
//! - A rebuild whose provider carries the **same** cache key is a no-op —
//!   Flutter's `if (_imageStream?.key == newStream.key) return;`. Provider
//!   *instance* identity is deliberately not consulted: two distinct
//!   `AssetImage`s for one path are one subscription.
//! - A key change cancels the live load and, unless
//!   [`gapless_playback`](crate::Image::gapless_playback) is set, clears the
//!   displayed frame in the same frame the swap lands — Flutter's
//!   `if (!widget.gaplessPlayback) _replaceImage(info: null);`.
//! - A failed load leaves the displayed frame alone. Flutter's `onError` sets
//!   `_lastException`; it never calls `_replaceImage`. With the default
//!   (non-gapless) policy there is nothing left to leave alone — the swap
//!   already cleared it.
//!
//! # Cancellation, and the generation guard behind it
//!
//! A swap and `dispose` both drop the `TaskToken`, which cancels the load
//! outright — the future is dropped, not merely ignored, so an abandoned load
//! stops costing whatever it held open. (What that does to the *shared* decode
//! cache is `decode_cache`'s business, not this module's: a key another widget
//! is still awaiting keeps loading and still lands in the cache; a key nobody
//! else wants is removed from the pending map. Either way this widget is no
//! longer a subscriber.)
//!
//! Every resolve additionally carries a monotonically increasing generation,
//! and a completion may publish only while its own generation is still
//! current — the same swap and `dispose` bump it. That makes cancellation's
//! correctness a property of this file rather than an argument about driver
//! internals: whatever order two loads settle in, and whether or not the
//! driver has already dropped the loser's task, only the live request can
//! reach the screen. The out-of-order case it exists for — a slow first
//! provider settling after a fast second one — is what a swap during a fast
//! scroll produces.

use std::sync::Arc;

use flui_scheduler::{AsyncDriver, TaskToken};
use flui_types::painting::Image as PixelImage;
use flui_view::context::BuildContext;
use flui_view::{RebuildHandle, RebuildReason};
use parking_lot::Mutex;

use super::cache_key::ImageCacheKey;
use super::decode_cache;
use super::provider::ImageProvider;

/// The frame the widget displays, plus the generation allowed to write it.
#[derive(Debug, Default)]
struct Published {
    /// The last frame published by a non-stale resolve. `None` is the
    /// empty-box placeholder.
    image: Option<PixelImage>,
    /// The current request generation. A completion carrying any other
    /// generation is stale and is dropped.
    generation: u64,
    /// Set while `spawn_local_eager` polls the freshly spawned task inline.
    /// A completion landing there must not schedule a rebuild: the build that
    /// will read it has not run yet. Same window `FutureBuilder` keeps.
    inline_window: bool,
}

impl Published {
    /// Publish `image` if `generation` is still current; returns whether a
    /// rebuild must be scheduled.
    fn publish(&mut self, generation: u64, image: PixelImage) -> bool {
        if generation != self.generation {
            return false;
        }
        self.image = Some(image);
        !self.inline_window
    }

    /// Whether `generation` is still the live request.
    fn is_current(&self, generation: u64) -> bool {
        generation == self.generation
    }
}

/// Owns one `Image` widget's subscription to a cache key.
///
/// Lives in the widget's `ViewState`, so it is created before `init_state`
/// and dropped on unmount. Not `Clone`: the subscription is the state's.
pub(super) struct ImageResolver {
    /// Shared with the in-flight task, which is why it is behind a lock:
    /// `ViewState::build` takes `&self`, and the task writes from the frame's
    /// async step.
    published: Arc<Mutex<Published>>,
    /// Captured in `init_state` — the only lifecycle hook handed a
    /// `BuildContext`. `did_update_view` and `dispose` receive none.
    handle: Option<RebuildHandle>,
    /// The binding's async driver, likewise captured in `init_state`.
    driver: Option<AsyncDriver>,
    /// Cancels the live load on drop.
    token: Option<TaskToken>,
    /// The key the live subscription was created for. `None` means no async
    /// subscription — either nothing has been resolved yet, or the provider
    /// is synchronous.
    key: Option<ImageCacheKey>,
    /// The provider the live subscription resolves through. Held because
    /// `init_state` is handed no view.
    provider: Arc<dyn ImageProvider + Send + Sync>, // PORT-CHECK-OK-DYN: the same erased provider handle `Image` itself stores
    /// Whether to keep the previous frame across a key change.
    gapless_playback: bool,
}

impl std::fmt::Debug for ImageResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let published = self.published.lock();
        f.debug_struct("ImageResolver")
            .field("key", &self.key)
            .field("generation", &published.generation)
            .field("has_frame", &published.image.is_some())
            .field("loading", &self.token.is_some())
            .field("gapless_playback", &self.gapless_playback)
            .finish_non_exhaustive()
    }
}

impl ImageResolver {
    /// Creates a resolver for `provider`. Nothing is resolved until
    /// [`init`](Self::init) runs — the capabilities a load needs are only
    /// reachable from `init_state`.
    pub(super) fn new(
        provider: Arc<dyn ImageProvider + Send + Sync>,
        gapless_playback: bool,
    ) -> Self {
        Self {
            published: Arc::new(Mutex::new(Published::default())),
            handle: None,
            driver: None,
            token: None,
            key: None,
            provider,
            gapless_playback,
        }
    }

    /// `_ImageState.initState` + `_resolveImage`: capture the lifecycle
    /// capabilities, then start the first resolve.
    pub(super) fn init(&mut self, ctx: &dyn BuildContext) {
        self.handle = Some(ctx.rebuild_handle());
        self.driver = ctx.async_driver();
        let key = self.provider.cache_key();
        self.start(key);
    }

    /// `_ImageState.didUpdateWidget`: re-resolve only when the *cache key*
    /// changes, not when the provider instance does.
    pub(super) fn did_update(
        &mut self,
        provider: Arc<dyn ImageProvider + Send + Sync>,
        gapless_playback: bool,
    ) {
        let key = provider.cache_key();
        self.provider = provider;
        self.gapless_playback = gapless_playback;
        if key == self.key {
            return;
        }
        self.start(key);
    }

    /// The frame to display right now.
    pub(super) fn frame(&self) -> Option<PixelImage> {
        self.published.lock().image.clone()
    }

    /// Whether this resolver holds a subscription at all — false for a
    /// provider that opted out of async resolution, whose widget resolves
    /// inline on every build instead. Read in `build`, where re-asking the
    /// provider for its key would allocate one every frame.
    pub(super) fn is_subscribed(&self) -> bool {
        self.key.is_some()
    }

    /// `_ImageState.dispose`: cancel the load and retire its generation, so a
    /// completion already in flight cannot publish into a disposed state.
    pub(super) fn dispose(&mut self) {
        self.token = None; // Drop cancels.
        self.key = None;
        self.published.lock().generation += 1;
    }

    /// Cancel the live load, open a new generation, and resolve `key`.
    ///
    /// A `None` key is a provider that has opted out of async resolution;
    /// there is nothing to subscribe to, and the widget resolves
    /// synchronously on every build instead.
    fn start(&mut self, key: Option<ImageCacheKey>) {
        self.token = None; // Drop cancels the previous load.
        self.key.clone_from(&key);

        let generation = {
            let mut published = self.published.lock();
            published.generation += 1;
            if !self.gapless_playback {
                published.image = None;
            }
            published.generation
        };

        let Some(key) = key else {
            return;
        };

        // Flutter's synchronously-completing `ImageStream`: a key already in
        // the decode cache resolves in this very frame, with no placeholder.
        if let Some(hit) = decode_cache::cached(&key) {
            self.published.lock().publish(generation, hit);
            return;
        }

        let Some(driver) = self.driver.clone() else {
            // No binding installed a driver: nothing will ever poll the load.
            // Report it rather than spawn into a driver nobody drives.
            tracing::warn!(
                "Image: no async driver on this BuildContext; the image load will \
                 never be polled. Is the tree bound to a binding?"
            );
            return;
        };
        // `start` runs only from `init` (which populates this immediately
        // above its own call) and from `did_update` (which runs strictly
        // after `init_state`), so an absent handle is an internal invariant
        // violation, not a reachable state.
        let handle = self
            .handle
            .clone()
            .expect("BUG: image resolve started before init_state captured the rebuild handle");

        let future = self.provider.resolve_async();
        let published = Arc::clone(&self.published);
        self.published.lock().inline_window = true;

        let token = driver.spawn_local_eager(Box::pin(async move {
            match future.await {
                Ok(image) => {
                    let schedule = published.lock().publish(generation, image);
                    if schedule {
                        handle.schedule(RebuildReason::AsyncCompletion);
                    }
                }
                Err(err) => {
                    if published.lock().is_current(generation) {
                        // Neither `?provider` nor `%err`: the provider's
                        // `Debug` holds the file path or URL, and the error's
                        // `Display` interpolates it. For a user-picked photo
                        // that is their data.
                        tracing::warn!(
                            error_kind = err.kind(),
                            "image provider failed to resolve asynchronously; showing \
                             empty placeholder box"
                        );
                    }
                }
            }
        }));

        self.published.lock().inline_window = false;
        self.token = token;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PixelImage`'s equality is buffer identity (an `Arc` clone is equal to
    /// its source; two separately built images are not), so a test that wants
    /// to name a specific frame keeps the handle it published.
    fn solid(width: u32, height: u32) -> PixelImage {
        PixelImage::from_rgba8(width, height, vec![0u8; (width * height * 4) as usize])
    }

    #[test]
    fn a_completion_from_the_live_generation_publishes_and_asks_for_a_rebuild() {
        let mut published = Published::default();
        let frame = solid(2, 2);

        assert!(published.publish(0, frame.clone()));
        assert_eq!(published.image, Some(frame));
    }

    #[test]
    fn a_completion_from_a_retired_generation_cannot_replace_the_frame() {
        let mut published = Published::default();
        let live = solid(2, 2);
        published.publish(0, live.clone());

        // A swap retires generation 0.
        published.generation += 1;

        assert!(
            !published.publish(0, solid(9, 9)),
            "a stale completion must neither publish nor schedule a rebuild",
        );
        assert_eq!(
            published.image,
            Some(live),
            "the frame the live generation published must survive a late \
             completion from a retired one",
        );
    }

    #[test]
    fn a_completion_inside_the_inline_poll_window_publishes_without_a_rebuild() {
        let mut published = Published {
            inline_window: true,
            ..Published::default()
        };
        let frame = solid(2, 2);

        assert!(
            !published.publish(0, frame.clone()),
            "the build that will read this frame has not run yet, so no \
             rebuild is owed",
        );
        assert_eq!(published.image, Some(frame));
    }
}
