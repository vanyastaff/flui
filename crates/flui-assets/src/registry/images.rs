//! The decoded images a registry keeps, and the bridged loads that fill them.
//!
//! [`AssetRegistry::load_image_bridged`](super::AssetRegistry::load_image_bridged)
//! and its network sibling go through [`DecodedImages`]: a completed image is
//! served from a byte-budgeted LRU, a request for an image already loading
//! joins that load, and a cold request starts one task on the bridge runtime.
//!
//! # A load lives as long as someone waits for it
//!
//! Every [`ImageLoad`] waiting on a pending load is a subscriber. When the last
//! one is dropped the pending entry is removed, its [`LoadTicket`] is
//! cancelled and its task aborted, so an abandoned request stops reading,
//! stops fetching and never reaches the decoder. A request for the same source
//! after that starts a fresh load with a fresh ticket. The ticket's allocation
//! is the load's identity: a task completing after its entry was replaced
//! compares tickets and leaves the newer entry alone.
//!
//! # Decoding never runs on an async worker
//!
//! Bytes are read on the runtime's async workers; the decode runs on its
//! blocking pool behind the registry's decode permits, so a host runtime's
//! workers keep serving its own tasks while large images decode, and at most
//! `decode_threads` decodes run at once. A load whose subscribers all left
//! while it waited for a permit skips the decode.
//!
//! # Locking
//!
//! One mutex guards the cache and the pending map. Nothing that can run code
//! outside this module happens under it: senders are completed, aborts are
//! issued, tasks are spawned and evicted pixels are freed after it is released.

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use flui_foundation::memory::MemoryPressureLevel;
use parking_lot::Mutex;
use tokio::sync::{Semaphore, oneshot};
use tokio::task::AbortHandle;

use crate::Image;
use crate::error::{AssetError, Result};

/// Decoded bytes a registry keeps by default.
///
/// Phones and the browser get a quarter of the desktop budget: there the
/// system kills a process for its footprint long before it runs out of
/// address space.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
pub(crate) const DEFAULT_DECODED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// Decoded bytes a registry keeps by default.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub(crate) const DEFAULT_DECODED_IMAGE_BYTES: usize = 256 * 1024 * 1024;

/// The most decodes a registry runs at once by default: half the cores, at
/// most four, so decoding never takes the cores the owner and raster threads
/// need.
pub(crate) fn default_decode_threads() -> NonZeroUsize {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    NonZeroUsize::new(cores.div_ceil(2).clamp(1, 4))
        .expect("BUG: a value clamped to at least one is nonzero")
}

/// Where a decoded image comes from: the identity a registry caches it and
/// coalesces its loads under.
///
/// Two sources are the same image only within one registry; each registry
/// keeps its own decoded images.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageSource {
    /// An asset path, read from the file system.
    Asset(Arc<str>),
    /// An HTTP or HTTPS URL.
    #[cfg(feature = "network")]
    Network(Arc<str>),
}

impl ImageSource {
    /// The path or URL, for error messages.
    pub(crate) fn label(&self) -> &Arc<str> {
        match self {
            Self::Asset(path) => path,
            #[cfg(feature = "network")]
            Self::Network(url) => url,
        }
    }
}

/// The identity of one load attempt, shared by its pending entry, its
/// subscribers and its task.
#[derive(Debug, Default)]
pub(crate) struct LoadTicket {
    cancelled: AtomicBool,
}

impl LoadTicket {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// A load in flight and the subscribers waiting for it.
struct Pending {
    ticket: Arc<LoadTicket>,
    subscribers: usize,
    waiters: Vec<oneshot::Sender<Result<Image>>>,
    /// Set once the task is spawned; `None` only between admission and spawn.
    task: Option<AbortHandle>,
}

struct State {
    /// Unbounded by count: [`DecodedImages::budget`] bounds it by bytes.
    entries: lru::LruCache<ImageSource, Image>,
    /// Sum of the pixel bytes in `entries`.
    bytes: usize,
    pending: HashMap<ImageSource, Pending>,
}

impl State {
    /// Evicts least recently used images until at most `limit` bytes remain.
    fn trim_to(&mut self, limit: usize, retired: &mut Vec<Image>) {
        while self.bytes > limit {
            let Some((_, image)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes -= image.data().len();
            retired.push(image);
        }
    }
}

/// A registry's decoded images: the completed ones it keeps and the loads in
/// flight.
pub(crate) struct DecodedImages {
    state: Mutex<State>,
    /// The most pixel bytes `entries` holds; an image larger than this is
    /// handed to its waiters and not kept.
    budget: usize,
    /// One permit per decode allowed to run at once.
    permits: Arc<Semaphore>,
}

impl std::fmt::Debug for DecodedImages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock();
        f.debug_struct("DecodedImages")
            .field("images", &state.entries.len())
            .field("bytes", &state.bytes)
            .field("pending", &state.pending.len())
            .field("budget", &self.budget)
            .finish_non_exhaustive()
    }
}

/// What admitting a request found.
pub(crate) enum Admission {
    /// The request was served or joined a load already in flight.
    Served(ImageLoad),
    /// The request starts a load: spawn it with [`DecodedImages::attach`].
    Start(ImageLoad, Arc<LoadTicket>),
}

impl DecodedImages {
    pub(crate) fn new(budget: usize, decode_threads: NonZeroUsize) -> Self {
        Self {
            state: Mutex::new(State {
                entries: lru::LruCache::unbounded(),
                bytes: 0,
                pending: HashMap::new(),
            }),
            budget,
            permits: Arc::new(Semaphore::new(decode_threads.get())),
        }
    }

    /// The completed image for `source`, made most recently used.
    pub(crate) fn cached(&self, source: &ImageSource) -> Option<Image> {
        self.state.lock().entries.get(source).cloned()
    }

    /// Serves `source` from the cache, joins its load in flight, or opens a new
    /// pending entry the caller must start.
    pub(crate) fn admit(self: &Arc<Self>, source: ImageSource) -> Admission {
        let mut state = self.state.lock();
        if let Some(image) = state.entries.get(&source) {
            return Admission::Served(ImageLoad::ready(Ok(image.clone())));
        }
        let (tx, rx) = oneshot::channel();
        if let Some(pending) = state.pending.get_mut(&source) {
            pending.subscribers = pending
                .subscribers
                .checked_add(1)
                .expect("BUG: an image load has more subscribers than addressable memory");
            pending.waiters.push(tx);
            let ticket = Arc::clone(&pending.ticket);
            drop(state);
            return Admission::Served(ImageLoad::waiting(rx, Arc::clone(self), source, ticket));
        }
        let ticket = Arc::new(LoadTicket::default());
        state.pending.insert(
            source.clone(),
            Pending {
                ticket: Arc::clone(&ticket),
                subscribers: 1,
                waiters: vec![tx],
                task: None,
            },
        );
        drop(state);
        Admission::Start(
            ImageLoad::waiting(rx, Arc::clone(self), source, Arc::clone(&ticket)),
            ticket,
        )
    }

    /// Records the spawned task of the load `ticket` names, so its last
    /// subscriber can abort it. A load abandoned before this point is aborted
    /// here instead.
    pub(crate) fn attach(&self, source: &ImageSource, ticket: &Arc<LoadTicket>, task: AbortHandle) {
        let orphan = {
            let mut state = self.state.lock();
            match state.pending.get_mut(source) {
                Some(pending) if Arc::ptr_eq(&pending.ticket, ticket) => {
                    pending.task = Some(task);
                    None
                }
                // Completed already, or abandoned before the spawn returned.
                _ => ticket.is_cancelled().then_some(task),
            }
        };
        if let Some(task) = orphan {
            task.abort();
        }
    }

    /// Settles the load `ticket` names: removes its entry, keeps a decoded
    /// image within the budget, and hands `outcome` to every waiter. `None`
    /// closes the waiters' channels. A ticket that no longer owns the entry
    /// (abandoned, or replaced by a newer load) changes nothing.
    pub(crate) fn complete(
        &self,
        source: &ImageSource,
        ticket: &Arc<LoadTicket>,
        outcome: Option<Result<Image>>,
    ) {
        let mut retired = Vec::new();
        let waiters = {
            let mut state = self.state.lock();
            let owns_entry = state
                .pending
                .get(source)
                .is_some_and(|pending| Arc::ptr_eq(&pending.ticket, ticket));
            if !owns_entry {
                drop(state);
                drop(outcome);
                return;
            }
            let pending = state
                .pending
                .remove(source)
                .expect("BUG: the pending entry was present under the same lock");
            if let Some(Ok(image)) = &outcome {
                self.keep(&mut state, source.clone(), image.clone(), &mut retired);
            }
            pending.waiters
        };
        drop(retired);
        if let Some(outcome) = outcome {
            for waiter in waiters {
                // A waiter whose subscriber left before completion is closed.
                let _ = waiter.send(outcome.clone());
            }
        }
    }

    /// Adds `image` under `source` and evicts down to the budget.
    fn keep(&self, state: &mut State, source: ImageSource, image: Image, retired: &mut Vec<Image>) {
        let size = image.data().len();
        if size > self.budget {
            return;
        }
        if let Some(replaced) = state.entries.put(source, image) {
            state.bytes -= replaced.data().len();
            retired.push(replaced);
        }
        state.bytes += size;
        state.trim_to(self.budget, retired);
    }

    /// One subscriber of the load `ticket` names has left; the last one
    /// cancels it.
    fn unsubscribe(&self, source: &ImageSource, ticket: &Arc<LoadTicket>) {
        let abandoned = {
            let mut state = self.state.lock();
            match state.pending.get_mut(source) {
                Some(pending) if Arc::ptr_eq(&pending.ticket, ticket) => {
                    pending.subscribers -= 1;
                    if pending.subscribers == 0 {
                        ticket.cancel();
                        state.pending.remove(source)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        if let Some(mut pending) = abandoned
            && let Some(task) = pending.task.take()
        {
            task.abort();
        }
    }

    /// Gives back completed images: half the budget under moderate pressure,
    /// all of them under critical pressure. Loads in flight are left alone.
    pub(crate) fn reclaim(&self, level: MemoryPressureLevel) {
        let mut retired = Vec::new();
        {
            let mut state = self.state.lock();
            let limit = match level {
                MemoryPressureLevel::Moderate => self.budget / 2,
                _ => 0,
            };
            state.trim_to(limit, &mut retired);
        }
        drop(retired);
    }

    /// Decodes `bytes` on the current runtime's blocking pool once a decode
    /// permit is free. `None` when every subscriber left before the decode
    /// started.
    pub(crate) async fn decode(
        &self,
        source: &ImageSource,
        ticket: &Arc<LoadTicket>,
        bytes: Vec<u8>,
    ) -> Option<Result<Image>> {
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .expect("BUG: the decode semaphore is never closed");
        let ticket = Arc::clone(ticket);
        let label = Arc::clone(source.label());
        let decoded = tokio::task::spawn_blocking(move || {
            // Held until the decode returns, even if the task awaiting it is
            // aborted: the permit bounds decodes running, not decodes awaited.
            let _permit = permit;
            if ticket.is_cancelled() {
                return None;
            }
            Some(crate::assets::image::decode_rgba8(&label, &bytes))
        })
        .await;
        match decoded {
            Ok(outcome) => outcome,
            Err(error) => Some(Err(AssetError::LoadFailed {
                path: source.label().to_string(),
                reason: format!("the image decoder failed: {error}"),
            })),
        }
    }
}

/// Settles a load's entry when its task ends, however it ends: a task that
/// panics or is dropped by its runtime closes its waiters instead of leaving
/// them pending forever.
pub(crate) struct Completion {
    images: Arc<DecodedImages>,
    source: ImageSource,
    ticket: Arc<LoadTicket>,
    settled: bool,
}

impl Completion {
    pub(crate) fn new(
        images: Arc<DecodedImages>,
        source: ImageSource,
        ticket: Arc<LoadTicket>,
    ) -> Self {
        Self {
            images,
            source,
            ticket,
            settled: false,
        }
    }

    pub(crate) fn images(&self) -> &Arc<DecodedImages> {
        &self.images
    }

    pub(crate) fn source(&self) -> &ImageSource {
        &self.source
    }

    pub(crate) fn ticket(&self) -> &Arc<LoadTicket> {
        &self.ticket
    }

    /// Hands `outcome` to the load's waiters; `None` closes them.
    pub(crate) fn settle(mut self, outcome: Option<Result<Image>>) {
        self.settled = true;
        self.images.complete(&self.source, &self.ticket, outcome);
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if !self.settled {
            self.images.complete(&self.source, &self.ticket, None);
        }
    }
}

/// A bridged image load: resolves to the decoded image, or to the error the
/// load ended with.
///
/// Dropping the last `ImageLoad` waiting on a load cancels it: its read or
/// request is aborted and its decode, if not yet started, never runs.
/// Polling needs no tokio context.
#[must_use = "an image load is cancelled when dropped"]
pub struct ImageLoad {
    state: LoadState,
}

enum LoadState {
    Ready(Option<Result<Image>>),
    Waiting {
        rx: oneshot::Receiver<Result<Image>>,
        subscription: Subscription,
    },
}

/// A subscriber's claim on a pending load; dropping it unsubscribes.
struct Subscription {
    images: Arc<DecodedImages>,
    source: ImageSource,
    ticket: Arc<LoadTicket>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.images.unsubscribe(&self.source, &self.ticket);
    }
}

impl ImageLoad {
    pub(crate) fn ready(outcome: Result<Image>) -> Self {
        Self {
            state: LoadState::Ready(Some(outcome)),
        }
    }

    fn waiting(
        rx: oneshot::Receiver<Result<Image>>,
        images: Arc<DecodedImages>,
        source: ImageSource,
        ticket: Arc<LoadTicket>,
    ) -> Self {
        Self {
            state: LoadState::Waiting {
                rx,
                subscription: Subscription {
                    images,
                    source,
                    ticket,
                },
            },
        }
    }
}

impl std::fmt::Debug for ImageLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let waiting = matches!(self.state, LoadState::Waiting { .. });
        f.debug_struct("ImageLoad")
            .field("waiting", &waiting)
            .finish_non_exhaustive()
    }
}

impl Future for ImageLoad {
    type Output = Result<Image>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.state {
            LoadState::Ready(outcome) => Poll::Ready(
                outcome
                    .take()
                    .expect("BUG: an image load was polled after it completed"),
            ),
            LoadState::Waiting { rx, subscription } => match Pin::new(rx).poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(outcome)) => Poll::Ready(outcome),
                Poll::Ready(Err(_closed)) => Poll::Ready(Err(AssetError::LoadFailed {
                    path: subscription.source.label().to_string(),
                    reason: "the image-loading task was dropped before completing".to_owned(),
                })),
            },
        }
    }
}
