//! The decoded images a registry keeps, and the loads that fill them.
//!
//! [`AssetRegistry::load_image`](super::AssetRegistry::load_image) and its
//! network sibling go through [`DecodedImages`]: a completed image is served
//! from a byte-budgeted LRU, a request for an image already loading joins
//! that load, and a cold request starts one on the caller's background lanes.
//!
//! # The registry owns a load, a subscriber only waits for it
//!
//! The first request for a source reads it on its lanes' IO lane and decodes
//! it on their compute lane; the registry starts no runtime or thread of its
//! own. The load is the registry's, not that subscriber's: every
//! [`ImageLoad`] waiting on it is a subscriber, and only when the last one is
//! dropped is the pending entry removed, its [`LoadTicket`] cancelled and its
//! cancel signal sent. The IO future sees the signal at its next poll: an
//! HTTP request is dropped at once, a file read stops at its next chunk, and a
//! decode that has not started never runs. A request for the same source
//! after that starts a fresh load with a fresh ticket. The ticket's
//! allocation is the load's identity: a load completing after its entry was
//! replaced compares tickets and leaves the newer entry alone.
//!
//! # A load nobody runs is handed over
//!
//! A request with no lanes, or whose lanes are too full or unable to take
//! the load, runs it inside its own [`ImageLoad`], on the thread that polls
//! it. Should that subscriber leave first, or the lanes a load runs on shut
//! down and drop it, the load closes its waiters without an outcome; each
//! then admits itself again, and the first becomes the new lead, on its own
//! lanes. Only a subscriber whose own lanes are shutting down answers
//! [`AssetError::LanesRefused`].
//!
//! # Decoding never runs on the IO lane
//!
//! Bytes are read first; the decode runs on the compute lane behind the
//! registry's decode slots, so at most `decode_threads` decodes run at once.
//! Load slots are granted in admission order; a decode slot goes to the
//! earliest-admitted load whose bytes are ready ([`OrderedSlots`]), and a load
//! still fetching holds none, so the decoder never idles behind a slow read.
//! A load cancelled before its bytes are ready never asks for a decode slot,
//! and one cancelled while it waited for a slot skips the decode.
//!
//! # Locking
//!
//! One mutex guards the cache and the pending map. Nothing that can run code
//! outside this module happens under it: senders are completed, cancel
//! signals are sent, loads are spawned and evicted pixels are freed after it
//! is released.

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use flui_foundation::memory::MemoryPressureLevel;
use flui_platform::executor::{Executor, SpawnError};
use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::ordered_slots::{AdmissionOrder, OrderedSlots, Reservation};
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

/// Bytes one read takes of an image file: a cancelled load stops reading at
/// the next chunk instead of holding its lane for the whole file.
const FILE_READ_CHUNK: usize = 256 * 1024;

/// The largest encoded image file a load reads: 256 MiB, the same bound
/// `NetworkLoader::load_url` puts on a response body. A larger file fails
/// before it is buffered.
const MAX_ENCODED_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// How many times one [`ImageLoad`] takes over a load closed under it before
/// it answers with an error: lanes that keep dropping what they accept must
/// not keep a subscriber re-admitting forever.
const MAX_HANDOVERS: u8 = 4;

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

/// The background lanes a load runs on, or none to run it where its
/// [`ImageLoad`] is polled.
pub(crate) type Lanes = Option<Arc<dyn Executor>>;

/// The bytes of one source, read on the IO lane.
type Fetch = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>;

/// One load from its read to its settled waiters.
type Run = Pin<Box<dyn Future<Output = ()> + Send>>;

/// How a run comes by the load slot it holds from its read to its decode.
enum LoadSlot {
    /// Reserved at admission, for a run its lanes poll.
    Reserved(Reservation),
    /// Asked for at the run's first poll, in admission order, for a run
    /// that only progresses while its caller polls it.
    OnFirstPoll(Arc<OrderedSlots>, AdmissionOrder),
}

/// The identity of one load attempt, shared by its pending entry, its
/// subscribers and its run.
#[derive(Debug)]
pub(crate) struct LoadTicket {
    cancelled: AtomicBool,
    /// Where the load stands in admission order, for its decode slot.
    order: AdmissionOrder,
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
    /// Dropping it cancels a load spawned on the IO lane; `None` while the
    /// load runs inside its lead's [`ImageLoad`], or before its spawn returns.
    cancel: Option<oneshot::Sender<()>>,
}

struct State {
    /// Unbounded by count: [`DecodedImages::budget`] bounds it by bytes.
    entries: lru::LruCache<ImageSource, Image>,
    /// Sum of the pixel bytes in `entries`.
    bytes: usize,
    pending: HashMap<ImageSource, Pending>,
    /// The admission order the next started load takes.
    next_order: AdmissionOrder,
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
    /// One slot per load allowed past admission at once, held from before its
    /// read until its decode returns, so encoded bytes waiting to decode are
    /// bounded by count.
    loads: Arc<OrderedSlots>,
    /// One slot per decode allowed to run at once, handed out in admission
    /// order.
    decodes: Arc<OrderedSlots>,
    /// One HTTP connection pool per registry, initialized on the first IO
    /// lane that fetches an image.
    #[cfg(feature = "network")]
    network: Arc<tokio::sync::OnceCell<crate::NetworkLoader>>,
}

/// Loads a registry lets read or wait to decode per decode thread: enough to
/// keep the decoders fed past a slow read, few enough that the encoded bytes
/// waiting for them stay bounded.
pub(crate) const LOADS_PER_DECODE_THREAD: usize = 4;

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
enum Admission {
    /// The request was served from the cache.
    Cached(Image),
    /// The request joined a load already in flight.
    Joined(Subscription),
    /// The request starts a load: run `run`, which settles `subscription`'s
    /// waiters among the rest.
    Lead {
        subscription: Subscription,
        run: Run,
    },
}

impl DecodedImages {
    pub(crate) fn new(
        budget: usize,
        decode_threads: NonZeroUsize,
        #[cfg(feature = "network")] network: Option<crate::NetworkLoader>,
    ) -> Self {
        Self {
            state: Mutex::new(State {
                entries: lru::LruCache::unbounded(),
                bytes: 0,
                pending: HashMap::new(),
                next_order: 0,
            }),
            budget,
            loads: Arc::new(OrderedSlots::new(
                decode_threads.get().saturating_mul(LOADS_PER_DECODE_THREAD),
            )),
            decodes: Arc::new(OrderedSlots::new(decode_threads.get())),
            #[cfg(feature = "network")]
            network: Arc::new(tokio::sync::OnceCell::new_with(network)),
        }
    }

    /// The completed image for `source`, made most recently used.
    pub(crate) fn cached(&self, source: &ImageSource) -> Option<Image> {
        self.state.lock().entries.get(source).cloned()
    }

    /// Serves `source` from the cache, joins its load in flight, or starts
    /// one on `lanes`.
    pub(crate) fn load(self: &Arc<Self>, source: ImageSource, lanes: Lanes) -> ImageLoad {
        let mut load = ImageLoad {
            state: LoadState::Ready(None),
            images: Arc::clone(self),
            source,
            lanes,
            handovers: 0,
        };
        load.admit();
        load
    }

    /// Serves `source` from the cache, joins its load in flight, or opens a
    /// new pending entry and builds the run that settles it.
    fn admit(self: &Arc<Self>, source: &ImageSource, lanes: &Lanes) -> Admission {
        let mut state = self.state.lock();
        if let Some(image) = state.entries.get(source) {
            return Admission::Cached(image.clone());
        }
        let (tx, rx) = oneshot::channel();
        if let Some(pending) = state.pending.get_mut(source) {
            pending.subscribers = pending
                .subscribers
                .checked_add(1)
                .expect("BUG: an image load has more subscribers than addressable memory");
            pending.waiters.push(tx);
            let ticket = Arc::clone(&pending.ticket);
            drop(state);
            return Admission::Joined(Subscription::new(rx, Arc::clone(self), source, ticket));
        }
        let order = state.next_order;
        state.next_order = order
            .checked_add(1)
            .expect("BUG: a registry admitted 2^64 image loads");
        let ticket = Arc::new(LoadTicket {
            cancelled: AtomicBool::new(false),
            order,
        });
        state.pending.insert(
            source.clone(),
            Pending {
                ticket: Arc::clone(&ticket),
                subscribers: 1,
                waiters: vec![tx],
                cancel: None,
            },
        );
        // A load bound for lanes reserves under the lock that assigned
        // `order`, so load slots follow admission order however late each
        // lane first runs it. A load that runs where it is polled asks only
        // once it is polled: a future its caller holds unpolled must not
        // keep a slot from the load the caller awaits. The slot queue's lock
        // never takes this one, so the nesting cannot invert.
        let load_slot = match lanes {
            Some(_) => LoadSlot::Reserved(self.loads.reserve(order)),
            None => LoadSlot::OnFirstPoll(Arc::clone(&self.loads), order),
        };
        drop(state);
        let completion = Completion {
            images: Arc::clone(self),
            source: source.clone(),
            ticket: Arc::clone(&ticket),
            settled: false,
        };
        Admission::Lead {
            subscription: Subscription::new(rx, Arc::clone(self), source, ticket),
            run: self.run(completion, load_slot, lanes.clone()),
        }
    }

    /// Reads the source, decodes it and settles every waiter.
    fn run(&self, completion: Completion, load_slot: LoadSlot, lanes: Lanes) -> Run {
        let fetch = self.fetch(&completion.source);
        Box::pin(async move {
            // Granted before the read starts and held until the decode
            // returns: reads past admission, and the encoded bytes waiting
            // to decode, are bounded by count.
            let _load = match load_slot {
                LoadSlot::Reserved(reservation) => reservation.granted().await,
                LoadSlot::OnFirstPoll(loads, order) => loads.acquire(order).await,
            };
            let outcome = match fetch.await {
                Ok(bytes) => {
                    completion
                        .images
                        .decode(&completion.source, &completion.ticket, bytes, &lanes)
                        .await
                }
                Err(error) => Some(Err(error)),
            };
            completion.settle(outcome);
        })
    }

    /// The bytes of `source`, read where the run is polled.
    #[cfg_attr(
        not(feature = "network"),
        expect(
            clippy::unused_self,
            reason = "only a network fetch reads the registry's client"
        )
    )]
    fn fetch(&self, source: &ImageSource) -> Fetch {
        match source {
            ImageSource::Asset(path) => Box::pin(read_in_chunks(Arc::clone(path))),
            #[cfg(feature = "network")]
            ImageSource::Network(url) => {
                let network = Arc::clone(&self.network);
                let url = Arc::clone(url);
                Box::pin(async move {
                    // reqwest's connections live on a tokio reactor; on a
                    // lane without one it would panic at the first poll.
                    if tokio::runtime::Handle::try_current().is_err() {
                        return Err(AssetError::ReactorUnavailable {
                            url: url.to_string(),
                        });
                    }
                    let loader = network
                        .get_or_try_init(|| async {
                            crate::NetworkLoader::try_new().map_err(|error| {
                                AssetError::LoadFailed {
                                    path: url.to_string(),
                                    reason: format!("HTTP client initialization failed: {error}"),
                                }
                            })
                        })
                        .await?;
                    loader.load_url(&url).await
                })
            }
        }
    }

    /// Records the cancel signal of the load `ticket` names, spawned on an IO
    /// lane, so its last subscriber can cancel it. A load abandoned or
    /// settled before this point is left to finish or cancel on its own.
    fn attach(&self, source: &ImageSource, ticket: &Arc<LoadTicket>, cancel: oneshot::Sender<()>) {
        let mut state = self.state.lock();
        if let Some(pending) = state.pending.get_mut(source)
            && Arc::ptr_eq(&pending.ticket, ticket)
        {
            pending.cancel = Some(cancel);
        }
        // Otherwise `cancel` drops after the lock, cancelling a load that
        // settled already or whose last subscriber left.
    }

    /// Settles the load `ticket` names: removes its entry, keeps a decoded
    /// image within the budget, and hands `outcome` to every waiter. `None`
    /// closes the waiters' channels, and they take the load over. A ticket
    /// that no longer owns the entry (abandoned, or replaced by a newer load)
    /// changes nothing.
    fn complete(
        &self,
        source: &ImageSource,
        ticket: &Arc<LoadTicket>,
        outcome: Option<Result<Image>>,
    ) {
        let mut retired = Vec::new();
        let pending = {
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
            pending
        };
        drop(retired);
        let Pending {
            waiters, cancel, ..
        } = pending;
        drop(cancel);
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
        // Sends the cancel signal, outside the lock.
        drop(abandoned);
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

    /// Decodes `bytes` on the compute lane once a decode slot is free, behind
    /// every load admitted earlier that is also waiting with its bytes; loads
    /// still fetching are not waited for. Without lanes, or when the compute
    /// lane refuses the job, it decodes where the run is polled. `None` when
    /// every subscriber left before the decode started.
    async fn decode(
        &self,
        source: &ImageSource,
        ticket: &Arc<LoadTicket>,
        bytes: Vec<u8>,
        lanes: &Lanes,
    ) -> Option<Result<Image>> {
        if ticket.is_cancelled() {
            return None;
        }
        let slot = self.decodes.acquire(ticket.order).await;
        let label = Arc::clone(source.label());
        let ticket = Arc::clone(ticket);
        // Held until the decode returns, even if the run awaiting it is
        // dropped: the slot bounds decodes running, not decodes awaited.
        let decode = move || {
            let _slot = slot;
            if ticket.is_cancelled() {
                return None;
            }
            // A decoder that panics fails this load, not the lane or the
            // thread that polls it.
            Some(
                std::panic::catch_unwind(AssertUnwindSafe(|| {
                    crate::assets::image::decode_rgba8(&label, &bytes)
                }))
                .unwrap_or_else(|_| {
                    Err(AssetError::DecodeFailed {
                        path: label.to_string(),
                        reason: "the image decoder panicked".to_owned(),
                    })
                }),
            )
        };
        let Some(lanes) = lanes else {
            return decode();
        };
        // Shared with the job, so a refused job hands the decode back.
        let shared = Arc::new(Mutex::new(Some(decode)));
        let (tx, rx) = oneshot::channel();
        let job = {
            let shared = Arc::clone(&shared);
            Box::new(move || {
                let decode = shared.lock().take();
                if let Some(decode) = decode {
                    let _ = tx.send(decode());
                }
            })
        };
        match lanes.spawn_compute(job) {
            // `None` when the lane dropped the job unrun: it is shutting
            // down, and the load closes unsettled for a waiter on other
            // lanes to take over.
            Ok(()) => rx.await.ok().flatten(),
            // Lanes shutting down take no more work from this load, the
            // decode included: it closes unsettled, as above.
            Err(SpawnError::ShuttingDown) => None,
            // Lanes with no room now: decode where the load runs, as a load
            // given no lanes does.
            Err(_refused) => {
                let decode = shared.lock().take();
                decode.and_then(|decode| decode())
            }
        }
    }
}

/// Reads the file at `path` one [`FILE_READ_CHUNK`] at a time, yielding
/// between chunks so a cancelled load stops at the next one, and never more
/// than [`MAX_ENCODED_FILE_BYTES`].
async fn read_in_chunks(path: Arc<str>) -> Result<Vec<u8>> {
    use std::io::Read;

    let failed = |error: std::io::Error| AssetError::LoadFailed {
        path: path.to_string(),
        reason: error.to_string(),
    };
    let too_large = || AssetError::LoadFailed {
        path: path.to_string(),
        reason: format!("the file is larger than {MAX_ENCODED_FILE_BYTES} bytes"),
    };
    let not_a_file = || AssetError::LoadFailed {
        path: path.to_string(),
        reason: "not a regular file".to_owned(),
    };
    // Opening a FIFO or a device can block until another party acts, and
    // this runs on a shared IO lane: only a regular file is opened, checked
    // again once open in case the path was swapped in between.
    if !std::fs::metadata(path.as_ref()).map_err(failed)?.is_file() {
        return Err(not_a_file());
    }
    let mut file = std::fs::File::open(path.as_ref()).map_err(failed)?;
    let metadata = file.metadata().map_err(failed)?;
    if !metadata.is_file() {
        return Err(not_a_file());
    }
    let length = metadata.len();
    if length > MAX_ENCODED_FILE_BYTES {
        return Err(too_large());
    }
    // The metadata length is a hint, not a bound: the file can grow while it
    // is read, so every chunk is checked against the cap.
    let mut bytes = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    let mut chunk = vec![0_u8; FILE_READ_CHUNK];
    loop {
        let read = match file.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(failed(error)),
        };
        let total = u64::try_from(bytes.len().saturating_add(read)).unwrap_or(u64::MAX);
        if total > MAX_ENCODED_FILE_BYTES {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk[..read]);
        yield_now().await;
    }
}

/// Pending once, waking itself: the run's poller gets a turn to drop it.
fn yield_now() -> impl Future<Output = ()> {
    let mut yielded = false;
    std::future::poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}

/// Runs `run` until it finishes or `cancel` resolves, whichever is first;
/// `cancel` resolves when its sender is sent on or dropped.
async fn until_cancelled(mut run: Run, mut cancel: oneshot::Receiver<()>) {
    std::future::poll_fn(move |cx| {
        // The signal first: a cancelled load never reaches its next step.
        if Pin::new(&mut cancel).poll(cx).is_ready() {
            return Poll::Ready(());
        }
        run.as_mut().poll(cx)
    })
    .await;
}

/// Settles a load's entry when its run ends, however it ends.
///
/// A run that panics answers its waiters with the failure. A run dropped
/// unfinished, by the lead that polled it or by lanes shutting down, closes
/// them, and they take the load over.
struct Completion {
    images: Arc<DecodedImages>,
    source: ImageSource,
    ticket: Arc<LoadTicket>,
    settled: bool,
}

impl Completion {
    /// Hands `outcome` to the load's waiters; `None` closes them.
    fn settle(mut self, outcome: Option<Result<Image>>) {
        self.settled = true;
        self.images.complete(&self.source, &self.ticket, outcome);
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        // Dropped during an unwind: the run itself failed, and running it
        // again would fail the same way.
        let outcome = std::thread::panicking().then(|| {
            Err(AssetError::LoadFailed {
                path: self.source.label().to_string(),
                reason: "the image load panicked".to_owned(),
            })
        });
        self.images.complete(&self.source, &self.ticket, outcome);
    }
}

/// A decoded-image load: resolves to the decoded image, or to the error the
/// load ended with.
///
/// Dropping the last `ImageLoad` waiting on a load cancels it: its read or
/// request stops and its decode, if not yet started, never runs. Dropping
/// one of several leaves the load running for the rest. Polling needs no
/// tokio context.
#[must_use = "an image load is cancelled when dropped"]
pub struct ImageLoad {
    state: LoadState,
    images: Arc<DecodedImages>,
    source: ImageSource,
    /// The lanes this subscriber leads a load on, should it have to;
    /// `None` once they refused for want of room, so the next lead runs
    /// here.
    lanes: Lanes,
    /// Loads closed under this subscriber so far.
    handovers: u8,
}

enum LoadState {
    Ready(Option<Result<Image>>),
    /// Waiting for a load that runs elsewhere: on an IO lane, or in another
    /// subscriber.
    Waiting(Subscription),
    /// Running the load here, and waiting for it like any subscriber.
    Leading {
        /// `None` once it finished.
        run: Option<Run>,
        subscription: Subscription,
    },
}

/// A subscriber's claim on a pending load; dropping it unsubscribes.
struct Subscription {
    rx: oneshot::Receiver<Result<Image>>,
    images: Arc<DecodedImages>,
    source: ImageSource,
    ticket: Arc<LoadTicket>,
}

impl Subscription {
    fn new(
        rx: oneshot::Receiver<Result<Image>>,
        images: Arc<DecodedImages>,
        source: &ImageSource,
        ticket: Arc<LoadTicket>,
    ) -> Self {
        Self {
            rx,
            images,
            source: source.clone(),
            ticket,
        }
    }

    /// The outcome once the load settles; `None` when it closed unsettled.
    fn poll_outcome(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Image>>> {
        Pin::new(&mut self.rx).poll(cx).map(std::result::Result::ok)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.images.unsubscribe(&self.source, &self.ticket);
    }
}

impl ImageLoad {
    /// Joins or starts the load of this subscriber's source.
    fn admit(&mut self) {
        loop {
            self.state = match self.images.admit(&self.source, &self.lanes) {
                Admission::Cached(image) => LoadState::Ready(Some(Ok(image))),
                Admission::Joined(subscription) => LoadState::Waiting(subscription),
                Admission::Lead { subscription, run } => {
                    let Some(lanes) = &self.lanes else {
                        self.state = LoadState::Leading {
                            run: Some(run),
                            subscription,
                        };
                        return;
                    };
                    let (cancel, cancelled) = oneshot::channel();
                    match lanes.spawn_io(Box::pin(until_cancelled(run, cancelled))) {
                        Ok(()) => {
                            self.images
                                .attach(&self.source, &subscription.ticket, cancel);
                            LoadState::Waiting(subscription)
                        }
                        // The refused run closed the entry as it dropped.
                        // Lanes with no room now still have a thread here:
                        // lead again, on it.
                        Err(SpawnError::Saturated | SpawnError::Unavailable) => {
                            drop(subscription);
                            self.lanes = None;
                            continue;
                        }
                        Err(reason) => LoadState::Ready(Some(Err(AssetError::LanesRefused {
                            path: self.source.label().to_string(),
                            reason,
                        }))),
                    }
                }
            };
            return;
        }
    }

    /// Takes over a load closed unsettled, or answers once it has been
    /// closed under this subscriber too often.
    fn hand_over(&mut self) {
        self.handovers = self.handovers.saturating_add(1);
        if self.handovers > MAX_HANDOVERS {
            self.state = LoadState::Ready(Some(Err(AssetError::LoadFailed {
                path: self.source.label().to_string(),
                reason: format!("the load was dropped unfinished {MAX_HANDOVERS} times"),
            })));
            return;
        }
        self.admit();
    }
}

impl std::fmt::Debug for ImageLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self.state {
            LoadState::Ready(_) => "ready",
            LoadState::Waiting(_) => "waiting",
            LoadState::Leading { .. } => "leading",
        };
        f.debug_struct("ImageLoad")
            .field("state", &state)
            .field("handovers", &self.handovers)
            .finish_non_exhaustive()
    }
}

impl Future for ImageLoad {
    type Output = Result<Image>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            let settled = match &mut self.state {
                LoadState::Ready(outcome) => {
                    return Poll::Ready(
                        outcome
                            .take()
                            .expect("BUG: an image load was polled after it completed"),
                    );
                }
                LoadState::Waiting(subscription) => subscription.poll_outcome(cx),
                LoadState::Leading { run, subscription } => {
                    if let Some(future) = run
                        && future.as_mut().poll(cx).is_ready()
                    {
                        *run = None;
                    }
                    subscription.poll_outcome(cx)
                }
            };
            match settled {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(outcome)) => return Poll::Ready(outcome),
                Poll::Ready(None) => self.hand_over(),
            }
        }
    }
}
