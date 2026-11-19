//! The decoded images a cache keeps, and the loads that fill them.
//!
//! [`ImageCache::load`](super::ImageCache::load), for an asset and a network
//! source alike, goes through [`DecodedImages`]: a completed image is served
//! from a byte-budgeted LRU, a request for an image already loading joins
//! that load, and a cold request starts one on the caller's background lanes.
//!
//! # The cache owns a load, a subscriber only waits for it
//!
//! The first request for a source reads it on its lanes' IO lane and decodes
//! it on their compute lane; the cache starts no runtime or thread of its
//! own. The load is the cache's, not that subscriber's: every
//! [`ImageLoad`] waiting on it is a subscriber, and only when the last one is
//! dropped is the pending entry removed, its [`LoadTicket`] cancelled and its
//! cancel signal sent. The IO future sees the signal at its next poll: an
//! HTTP request is dropped at once, a bundle's read stops at its next chunk, and a
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
//! [`ImageLoadErrorKind::LanesRefused`].
//!
//! # A load waits for its slot off the lanes
//!
//! A load bound for lanes is spawned on their IO lane only once it holds its
//! load slot: queued, it is a start the slot queue calls on the thread that
//! freed a slot, so loads waiting for one take no place in the lane's
//! admission window. Lanes with no room by then park the run and tell every
//! subscriber; the first one polled claims it and drives it where it is polled.
//!
//! # Decoding never runs on the IO lane
//!
//! Bytes are read first; the decode runs on the compute lane behind the
//! cache's decode slots, so at most `decode_threads` decodes run at once.
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
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use flui_foundation::memory::MemoryPressureLevel;
use flui_foundation::panic::{FirstPanic, drop_contained};
use flui_painting::paint::MAX_ENCODED_IMAGE_BYTES;
use flui_platform::assets::{AssetAuthority, AssetBundle, AssetKey};
use flui_platform::executor::{Executor, Lane, SpawnError};
use flui_platform::http::{Http, HttpAuthority, HttpRequest};
use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::ordered_slots::{AdmissionOrder, OrderedSlots, Reservation, Slot, Start};
use crate::error::{ImageLoadError, ImageLoadErrorKind};
use flui_painting::paint::Image;

/// Decoded bytes a cache keeps by default.
///
/// Phones and the browser get a quarter of the desktop budget: there the
/// system kills a process for its footprint long before it runs out of
/// address space.
#[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
pub(crate) const DEFAULT_DECODED_IMAGE_BYTES: usize = 64 * 1024 * 1024;
/// Decoded bytes a cache keeps by default.
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub(crate) const DEFAULT_DECODED_IMAGE_BYTES: usize = 256 * 1024 * 1024;

/// The most decodes a cache runs at once by default: half the cores, at
/// most four, so decoding never takes the cores the owner and raster threads
/// need.
pub(crate) fn default_decode_threads() -> NonZeroUsize {
    let cores = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    NonZeroUsize::new(cores.div_ceil(2).clamp(1, 4))
        .expect("BUG: a value clamped to at least one is nonzero")
}

/// How many times one [`ImageLoad`] takes over a load closed under it before
/// it answers with an error: lanes that keep dropping what they accept must
/// not keep a subscriber re-admitting forever.
const MAX_HANDOVERS: u8 = 4;

/// What a load ends with, shared by every request waiting on it; each wraps
/// a failure with its own source.
type Result<T> = std::result::Result<T, ImageLoadErrorKind>;

/// Where a decoded image comes from, and how its bytes are read: the
/// identity a cache keeps the image and shares its load under.
///
/// One constructor per kind of source. Two sources are the same image only
/// within one cache; each cache keeps its own decoded images.
#[derive(Clone)]
pub struct ImageSource {
    key: SourceKey,
    /// What the bytes are read with.
    reader: Reader,
}

/// What a source's image is kept and its load shared under. It holds no
/// bundle and no client, so a kept image keeps neither alive (ADR-0118).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SourceKey {
    /// An asset key, as one bundle answers it: two bundles may hold
    /// different images under one key, so they never share a decoded image
    /// or a load (ADR-0384 §6).
    Asset {
        key: AssetKey,
        authority: AssetAuthority,
    },
    /// A URL, as one client's authority answers it: two clients with
    /// different authorities may answer one URL with different images, so
    /// they never share a decoded image or a load (ADR-0280 §6).
    Network {
        url: Arc<str>,
        authority: HttpAuthority,
    },
}

/// The handle a source's bytes are read through.
#[derive(Clone)]
enum Reader {
    Bundle(AssetBundle),
    Http(Http),
}

impl ImageSource {
    /// The application's asset `key`, read through `bundle` (ADR-0384).
    #[must_use]
    pub fn asset(key: AssetKey, bundle: &AssetBundle) -> Self {
        Self {
            key: SourceKey::Asset {
                key,
                authority: bundle.authority(),
            },
            reader: Reader::Bundle(bundle.clone()),
        }
    }

    /// The image at the HTTP or HTTPS `url`, fetched with `http`.
    #[must_use]
    pub fn network(url: impl Into<Arc<str>>, http: &Http) -> Self {
        Self {
            key: SourceKey::Network {
                url: url.into(),
                authority: http.authority(),
            },
            reader: Reader::Http(http.clone()),
        }
    }

    /// The key, for an asset source.
    #[must_use]
    pub fn asset_key(&self) -> Option<&AssetKey> {
        match &self.key {
            SourceKey::Asset { key, .. } => Some(key),
            SourceKey::Network { .. } => None,
        }
    }

    /// The URL, for a network source.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match &self.key {
            SourceKey::Asset { .. } => None,
            SourceKey::Network { url, .. } => Some(url),
        }
    }
}

impl PartialEq for ImageSource {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for ImageSource {}

impl std::hash::Hash for ImageSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl std::fmt::Debug for ImageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.key.fmt(f)
    }
}

/// The asset key or the URL.
impl std::fmt::Display for ImageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.key {
            SourceKey::Asset { key, .. } => std::fmt::Display::fmt(key, f),
            SourceKey::Network { url, .. } => f.write_str(url),
        }
    }
}

/// The background lanes a load runs on, or none to run it where its
/// [`ImageLoad`] is polled.
type Lanes = Option<Arc<dyn Executor>>;

/// What an image load runs on: the caller's background lanes.
///
/// A request that becomes a load's lead runs it on its own services, so a
/// load handed over runs on those of the request that took it.
///
/// ```
/// use std::sync::Arc;
/// use flui_assets::{ImageCache, ImageSource, LoadServices};
/// use flui_platform::assets::{AssetBundle, AssetKey};
/// use flui_platform::executor::Executor;
///
/// fn show(cache: &ImageCache, bundle: &AssetBundle, lanes: Arc<dyn Executor>) {
///     let logo = AssetKey::try_new("assets/logo.png").expect("a key inside the bundle");
///     let load = cache.load(&ImageSource::asset(logo, bundle), &LoadServices::new().lanes(lanes));
///     # drop(load);
/// }
/// ```
#[derive(Clone, Default)]
pub struct LoadServices {
    lanes: Lanes,
}

impl LoadServices {
    /// No lanes: a load runs where its [`ImageLoad`] is polled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read on the IO lane of `lanes` and decode on their compute lane.
    #[must_use = "the builder returns the updated services"]
    pub fn lanes(mut self, lanes: Arc<dyn Executor>) -> Self {
        self.lanes = Some(lanes);
        self
    }
}

impl std::fmt::Debug for LoadServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadServices")
            .field("lanes", &self.lanes.is_some())
            .finish()
    }
}

/// The bytes of one source, read on the IO lane.
type Fetch = Pin<Box<dyn Future<Output = Result<Vec<u8>>> + Send>>;

/// One load from its read to its settled waiters.
type Run = Pin<Box<dyn Future<Output = ()> + Send>>;

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
    /// The lead's first, as admitted.
    waiters: Vec<Waiter>,
    /// Dropping it cancels a load spawned on the IO lane; `None` while the
    /// load waits for its load slot, runs inside a subscriber's
    /// [`ImageLoad`], or before its spawn returns.
    cancel: Option<oneshot::Sender<()>>,
    /// The run the lead's lanes had no room for when its load slot came
    /// free, for the first subscriber polled to drive.
    parked: Option<Run>,
}

/// One subscriber's ends of its channels.
struct Waiter {
    /// Settles the subscriber.
    settle: oneshot::Sender<Delivery>,
    /// Tells the subscriber the run is parked for it to claim; taken when
    /// it is told. Apart from `settle`, so the outcome still reaches a
    /// subscriber told after another claimed and finished the run.
    claim: Option<oneshot::Sender<()>>,
}

/// What a subscriber's settle channel delivers. A channel closed with none
/// means the load closed unsettled, and the subscriber takes it over.
enum Delivery {
    /// The load's outcome.
    Settled(Result<Image>),
    /// The lead's lanes are shutting down and took no run (to the lead
    /// only; the other subscribers take the load over).
    Refused(SpawnError),
}

/// Where a run takes its load slot from.
enum LoadSlot {
    /// Awaited when the run starts.
    Reserved(Reservation),
    /// Granted already, and taken when the run starts: a run dropped
    /// unstarted leaves it for its starter to give back.
    Held(Arc<Mutex<Option<Slot>>>),
}

struct State {
    /// Unbounded by count: [`DecodedImages::budget`] bounds it by bytes.
    entries: lru::LruCache<SourceKey, Image>,
    /// Sum of the pixel bytes in `entries`.
    bytes: usize,
    pending: HashMap<SourceKey, Pending>,
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

/// A cache's decoded images: the completed ones it keeps and the loads in
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
}

/// Loads a cache lets read or wait to decode per decode thread: enough to
/// keep the decoders fed past a slow read, few enough that the encoded bytes
/// waiting for them stay bounded.
pub(crate) const LOADS_PER_DECODE_THREAD: usize = 4;

impl std::fmt::Debug for DecodedImages {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Read under the lock and written after it: the formatter's writer
        // is the caller's code, which may call back here.
        let (images, bytes, pending) = {
            let state = self.state.lock();
            (state.entries.len(), state.bytes, state.pending.len())
        };
        f.debug_struct("DecodedImages")
            .field("images", &images)
            .field("bytes", &bytes)
            .field("pending", &pending)
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
    /// The request starts a load with no lanes: run `run` where it is
    /// polled, which settles `subscription`'s waiters among the rest.
    Lead {
        subscription: Subscription,
        run: Run,
    },
    /// The request starts a load on its lanes: call `start` with the slot
    /// granted now, or the queue calls it once one is freed for it.
    Started {
        subscription: Subscription,
        start: Option<(Slot, Start)>,
    },
}

impl DecodedImages {
    pub(crate) fn new(budget: usize, decode_threads: NonZeroUsize) -> Self {
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
        }
    }

    /// The completed image for `source`, made most recently used.
    pub(crate) fn cached(&self, source: &ImageSource) -> Option<Image> {
        self.state.lock().entries.get(&source.key).cloned()
    }

    /// Serves `source` from the cache, joins its load in flight, or starts
    /// one on `services`.
    pub(crate) fn load(self: &Arc<Self>, source: ImageSource, services: LoadServices) -> ImageLoad {
        let mut load = ImageLoad {
            state: LoadState::Unadmitted,
            images: Arc::clone(self),
            source,
            services,
            handovers: 0,
        };
        if load.services.lanes.is_some() {
            load.admit();
        }
        load
    }

    /// Serves `source` from the cache, joins its load in flight, or opens a
    /// new pending entry and builds the run that settles it.
    fn admit(self: &Arc<Self>, source: &ImageSource, services: &LoadServices) -> Admission {
        let mut state = self.state.lock();
        if let Some(image) = state.entries.get(&source.key) {
            return Admission::Cached(image.clone());
        }
        let (waiter, channels) = Waiter::new();
        if let Some(pending) = state.pending.get_mut(&source.key) {
            pending.subscribers = pending
                .subscribers
                .checked_add(1)
                .expect("BUG: an image load has more subscribers than addressable memory");
            let ticket = Arc::clone(&pending.ticket);
            // A run parked before this request joined is its to claim too.
            // Told on a channel nobody polls yet, so no waker runs here.
            let waiter = if pending.parked.is_some() {
                waiter.tell_parked()
            } else {
                waiter
            };
            pending.waiters.push(waiter);
            drop(state);
            return Admission::Joined(Subscription::new(
                channels,
                Arc::clone(self),
                source,
                ticket,
            ));
        }
        let order = state.next_order;
        state.next_order = order
            .checked_add(1)
            .expect("BUG: a cache admitted 2^64 image loads");
        let ticket = Arc::new(LoadTicket {
            cancelled: AtomicBool::new(false),
            order,
        });
        state.pending.insert(
            source.key.clone(),
            Pending {
                ticket: Arc::clone(&ticket),
                subscribers: 1,
                waiters: vec![waiter],
                cancel: None,
                parked: None,
            },
        );
        let completion = Completion {
            images: Arc::clone(self),
            source: source.clone(),
            ticket: Arc::clone(&ticket),
            settled: false,
        };
        let subscription = Subscription::new(channels, Arc::clone(self), source, ticket);
        // Reserved under the lock that assigned `order`, so load slots follow
        // admission order however late each load first runs. A load with no
        // lanes is admitted only once polled, so it reserves no slot while
        // its caller holds it unpolled. A load on lanes is spawned on them
        // only once it holds its slot: queued, it holds no place in their
        // admission window. The slot queue's lock never takes this one, so
        // the nesting cannot invert.
        if let Some(lanes) = &services.lanes {
            let start = Self::start(completion, Arc::clone(lanes), services.clone());
            let start = self.loads.reserve_start(order, start);
            drop(state);
            return Admission::Started {
                subscription,
                start,
            };
        }
        let load_slot = self.loads.reserve(order);
        drop(state);
        Admission::Lead {
            subscription,
            run: Self::run(completion, LoadSlot::Reserved(load_slot), services.clone()),
        }
    }

    /// Spawns a load that holds its load slot on `lanes`' IO lane. Lanes
    /// with no room for it park the run for a subscriber to claim and drive;
    /// lanes shutting down refuse the lead, and the rest take the load over.
    /// A load whose subscribers all left while it waited, or that no one is
    /// left to drive, hands the slot back: the queue passes it on in its own
    /// loop, never from inside this start.
    fn start(completion: Completion, lanes: Arc<dyn Executor>, services: LoadServices) -> Start {
        Box::new(move |slot: Slot| {
            if completion.ticket.is_cancelled() {
                return Some(slot);
            }
            let images = Arc::clone(&completion.images);
            let source = completion.source.clone();
            let ticket = Arc::clone(&completion.ticket);
            // Taken by the run when it first runs; until then a run dropped
            // unstarted gives the slot back here.
            let held = Arc::new(Mutex::new(Some(slot)));
            let run = Self::run(completion, LoadSlot::Held(Arc::clone(&held)), services);
            // Shared with the spawned task, so a refused spawn hands the run
            // back instead of dropping it.
            let shared = Arc::new(Mutex::new(Some(run)));
            let (cancel, cancelled) = oneshot::channel();
            let task: Run = {
                let shared = Arc::clone(&shared);
                Box::pin(async move {
                    let run = shared.lock().take();
                    if let Some(run) = run {
                        until_cancelled(run, cancelled).await;
                    }
                })
            };
            let unrun = match lanes.spawn_io(task) {
                Ok(()) => {
                    images.attach(&source, &ticket, cancel);
                    return None;
                }
                Err(SpawnError::Saturated | SpawnError::Unavailable) => {
                    let run = shared.lock().take();
                    match run.map(|run| images.park_run(&source, &ticket, run)) {
                        // Parked with its slot, held until the run that
                        // claims it finishes.
                        Some(None) => return None,
                        Some(Some(unparked)) => Some(unparked),
                        None => None,
                    }
                }
                Err(reason) => {
                    images.refuse_lead(&source, &ticket, reason);
                    shared.lock().take()
                }
            };
            let slot = held.lock().take();
            // Dropped after the lead heard why, with its slot out: it closes
            // the load, and the rest take it over.
            drop(unrun);
            slot
        })
    }

    /// Reads the source, decodes it and settles every waiter.
    fn run(completion: Completion, load_slot: LoadSlot, services: LoadServices) -> Run {
        let fetch = contained(fetch(&completion.source));
        let lanes = services.lanes;
        Box::pin(async move {
            // Granted before the read starts and held until the decode
            // returns: reads past admission, and the encoded bytes waiting
            // to decode, are bounded by count.
            let _load: Option<Slot> = match load_slot {
                LoadSlot::Reserved(reservation) => Some(reservation.granted().await),
                LoadSlot::Held(held) => held.lock().take(),
            };
            let outcome = match fetch.await {
                Ok(bytes) => {
                    completion
                        .images
                        .decode(&completion.ticket, bytes, &lanes)
                        .await
                }
                Err(error) => Some(Err(error)),
            };
            completion.settle(outcome);
        })
    }

    /// Records the cancel signal of the load `ticket` names, spawned on an IO
    /// lane, so its last subscriber can cancel it. A load abandoned or
    /// settled before this point is left to finish or cancel on its own.
    fn attach(&self, source: &ImageSource, ticket: &Arc<LoadTicket>, cancel: oneshot::Sender<()>) {
        let mut state = self.state.lock();
        if let Some(pending) = state.pending.get_mut(&source.key)
            && Arc::ptr_eq(&pending.ticket, ticket)
        {
            pending.cancel = Some(cancel);
        }
        // Otherwise `cancel` drops after the lock, cancelling a load that
        // settled already or whose last subscriber left.
    }

    /// Parks the run of the load `ticket` names for the first of its
    /// subscribers polled next to claim, and tells every one of them; the run
    /// back when no subscriber is left to tell.
    fn park_run(&self, source: &ImageSource, ticket: &Arc<LoadTicket>, run: Run) -> Option<Run> {
        let told = {
            let mut state = self.state.lock();
            match state.pending.get_mut(&source.key) {
                Some(pending)
                    if Arc::ptr_eq(&pending.ticket, ticket) && !pending.waiters.is_empty() =>
                {
                    pending.parked = Some(run);
                    pending
                        .waiters
                        .iter_mut()
                        .filter_map(|waiter| waiter.claim.take())
                        .collect::<Vec<oneshot::Sender<()>>>()
                }
                _ => return Some(run),
            }
        };
        // Told outside the lock: a send wakes its subscriber's task. One
        // that left changes nothing; one never polled again leaves the run
        // to the others, and every one still hears the outcome. A waker that
        // panics leaves the rest told; the first panic is raised once every
        // one is (ADR-0281).
        let mut first = FirstPanic::new();
        for claim in told {
            first.contain(|| {
                let _ = claim.send(());
            });
        }
        first.resume();
        None
    }

    /// Tells the lead of the load `ticket` names that its lanes refused the
    /// load; the caller then drops the run, and the rest take it over.
    fn refuse_lead(&self, source: &ImageSource, ticket: &Arc<LoadTicket>, reason: SpawnError) {
        let lead = {
            let mut state = self.state.lock();
            match state.pending.get_mut(&source.key) {
                Some(pending)
                    if Arc::ptr_eq(&pending.ticket, ticket) && !pending.waiters.is_empty() =>
                {
                    Some(pending.waiters.remove(0))
                }
                _ => None,
            }
        };
        // Sent and dropped outside the lock. A lead that left before hearing
        // it changes nothing.
        if let Some(lead) = lead {
            let _ = lead.settle.send(Delivery::Refused(reason));
        }
    }

    /// Takes the run parked for `subscription`'s load, unless another
    /// subscriber claimed it first.
    fn claim(&self, subscription: &Subscription) -> Option<Run> {
        let mut state = self.state.lock();
        match state.pending.get_mut(&subscription.source.key) {
            Some(pending) if Arc::ptr_eq(&pending.ticket, &subscription.ticket) => {
                pending.parked.take()
            }
            _ => None,
        }
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
                .get(&source.key)
                .is_some_and(|pending| Arc::ptr_eq(&pending.ticket, ticket));
            if !owns_entry {
                drop(state);
                drop(outcome);
                return;
            }
            let pending = state
                .pending
                .remove(&source.key)
                .expect("BUG: the pending entry was present under the same lock");
            if let Some(Ok(image)) = &outcome {
                self.keep(&mut state, source.key.clone(), image.clone(), &mut retired);
            }
            pending
        };
        drop(retired);
        let Pending {
            waiters, cancel, ..
        } = pending;
        drop(cancel);
        // Each waiter answered, or closed when there is no outcome so it takes
        // the load over, in turn. Its outcome is sent, and its claim channel
        // closed, each under containment of its own: a waker that panics
        // costs no subscriber its outcome, and the first panic is raised
        // once every one is answered (ADR-0281).
        let mut first = FirstPanic::new();
        for waiter in waiters {
            let Waiter { settle, claim } = waiter;
            let delivery = outcome.clone().map(Delivery::Settled);
            first.contain(move || {
                // A waiter whose subscriber left before completion is closed.
                if let Some(delivery) = delivery {
                    let _ = settle.send(delivery);
                }
            });
            first.contain(move || drop(claim));
        }
        first.resume();
    }

    /// Adds `image` under `key` and evicts down to the budget.
    fn keep(&self, state: &mut State, key: SourceKey, image: Image, retired: &mut Vec<Image>) {
        let size = image.data().len();
        if size > self.budget {
            return;
        }
        if let Some(replaced) = state.entries.put(key, image) {
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
            match state.pending.get_mut(&source.key) {
                Some(pending) if Arc::ptr_eq(&pending.ticket, ticket) => {
                    pending.subscribers -= 1;
                    if pending.subscribers == 0 {
                        ticket.cancel();
                        state.pending.remove(&source.key)
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
    /// still fetching are not waited for. A compute lane with no room is
    /// waited for, so the decode never runs on the IO lane that read the
    /// bytes. Without lanes, or when the compute lane's workers could not
    /// start, it decodes where the run is polled. `None` when every
    /// subscriber left before the decode started.
    async fn decode(
        &self,
        ticket: &Arc<LoadTicket>,
        bytes: Vec<u8>,
        lanes: &Lanes,
    ) -> Option<Result<Image>> {
        if ticket.is_cancelled() {
            return None;
        }
        let slot = self.decodes.acquire(ticket.order).await;
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
            let mut caught = FirstPanic::new();
            let decoded =
                caught.contain(|| Image::decode(&bytes).map_err(ImageLoadErrorKind::Decode));
            Some(decoded.unwrap_or_else(|| Err(panicked(caught))))
        };
        let Some(lanes) = lanes else {
            return decode();
        };
        // Shared with the job, so a refused job hands the decode back.
        let shared = Arc::new(Mutex::new(Some(decode)));
        loop {
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
                Ok(()) => return rx.await.ok().flatten(),
                // Lanes shutting down take no more work from this load, the
                // decode included: it closes unsettled, as above.
                Err(SpawnError::ShuttingDown) => return None,
                // A lane with no room now: wait for room rather than decode
                // where the load runs, which may be an IO worker, then spawn
                // again. Room is not reserved, so another caller may take it
                // first.
                Err(SpawnError::Saturated) => match lanes.room(Lane::Compute).await {
                    Ok(()) => {}
                    Err(SpawnError::ShuttingDown) => return None,
                    Err(_unavailable) => break,
                },
                // Workers that could not start give no room to wait for:
                // decode where the load runs, as a load given no lanes does.
                Err(_unavailable) => break,
            }
        }
        let decode = shared.lock().take();
        decode.and_then(|decode| decode())
    }
}

/// The bytes of `source`, read where the run is polled: an asset through
/// the bundle it was built with, a network image with its client.
fn fetch(source: &ImageSource) -> Fetch {
    match (&source.key, &source.reader) {
        (SourceKey::Asset { key, .. }, Reader::Bundle(bundle)) => {
            // The read is made inside the future: a source's `load` is host
            // code, and only the run's polls are contained.
            let (key, bundle) = (key.clone(), bundle.clone());
            Box::pin(async move {
                bundle
                    .load(&key, MAX_ENCODED_IMAGE_BYTES)
                    .await
                    .map_err(ImageLoadErrorKind::from_asset)
            })
        }
        (SourceKey::Network { url, .. }, Reader::Http(http)) => {
            Box::pin(fetch_over_http(Arc::clone(url), http.clone()))
        }
        (SourceKey::Asset { .. }, Reader::Http(_))
        | (SourceKey::Network { .. }, Reader::Bundle(_)) => {
            unreachable!("BUG: a source is built with the reader of its kind")
        }
    }
}

/// The body of a `GET` for `url`, within [`MAX_ENCODED_IMAGE_BYTES`].
async fn fetch_over_http(url: Arc<str>, http: Http) -> Result<Vec<u8>> {
    let response = http
        .send(HttpRequest::get(url.as_ref()).body_limit(MAX_ENCODED_IMAGE_BYTES))
        .await
        .map_err(ImageLoadErrorKind::Http)?;
    let status = response.status();
    if !status.is_success() {
        return Err(ImageLoadErrorKind::HttpStatus(status));
    }
    response
        .into_body()
        .into_bytes()
        .await
        .map_err(ImageLoadErrorKind::Http)
}

/// `fetch` with each poll contained: a bundle or a client that panics
/// fails this load with [`ImageLoadErrorKind::Panicked`] instead of
/// unwinding through the lane or the request that polls it. The fetch is
/// dropped, under containment too, as soon as it ends.
fn contained(fetch: Fetch) -> impl Future<Output = Result<Vec<u8>>> {
    let mut fetch = Some(fetch);
    std::future::poll_fn(move |ctx| {
        let running = fetch
            .as_mut()
            .expect("BUG: a contained fetch is not polled after it ended");
        let mut caught = FirstPanic::new();
        let outcome = match caught.contain(|| running.as_mut().poll(ctx)) {
            Some(Poll::Pending) => return Poll::Pending,
            Some(Poll::Ready(outcome)) => outcome,
            None => Err(panicked(caught)),
        };
        if let Some(ended) = fetch.take() {
            drop_contained(ended);
        }
        Poll::Ready(outcome)
    })
}

/// The error a caught panic answers.
fn panicked(caught: FirstPanic) -> ImageLoadErrorKind {
    caught.into_payload().map_or(
        ImageLoadErrorKind::Panicked { message: None },
        ImageLoadErrorKind::panicked,
    )
}

/// Runs `run` until it finishes or `cancel` resolves, whichever is first;
/// `cancel` resolves when its sender is sent on or dropped.
async fn until_cancelled(mut run: Run, mut cancel: oneshot::Receiver<()>) {
    std::future::poll_fn(move |ctx| {
        // The signal first: a cancelled load never reaches its next step.
        if Pin::new(&mut cancel).poll(ctx).is_ready() {
            return Poll::Ready(());
        }
        run.as_mut().poll(ctx)
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
        // The read, the request and the decode answer their own panics
        // (`contained`, `decode`), so a panic here is this module's own.
        // Dropped during its unwind, the load answers it, since running it
        // again would fail the same way; dropped after an executor caught it,
        // `panicking()` is false and the waiters take the load over, at most
        // `MAX_HANDOVERS` times.
        let outcome =
            std::thread::panicking().then_some(Err(ImageLoadErrorKind::Panicked { message: None }));
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
    /// What this subscriber runs the load on when it becomes its lead: at
    /// admission, or when it takes over a load closed under it.
    services: LoadServices,
    /// Loads closed under this subscriber so far.
    handovers: u8,
}

enum LoadState {
    /// Not yet admitted: a load that runs where it is polled joins or opens
    /// a pending entry only at its first poll. Opened earlier, an entry a
    /// caller holds unpolled would hold a load slot, and a request for the
    /// same source would join a run nobody drives.
    Unadmitted,
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
    rx: oneshot::Receiver<Delivery>,
    /// Told when the load's run is parked; `None` once told or closed.
    claim: Option<oneshot::Receiver<()>>,
    images: Arc<DecodedImages>,
    source: ImageSource,
    ticket: Arc<LoadTicket>,
}

/// A subscriber's ends of the channels its [`Waiter`] sends on.
struct Channels {
    settle: oneshot::Receiver<Delivery>,
    claim: oneshot::Receiver<()>,
}

impl Waiter {
    fn new() -> (Self, Channels) {
        let (settle, settle_rx) = oneshot::channel();
        let (claim, claim_rx) = oneshot::channel();
        (
            Self {
                settle,
                claim: Some(claim),
            },
            Channels {
                settle: settle_rx,
                claim: claim_rx,
            },
        )
    }

    /// This waiter, already told a run is parked.
    fn tell_parked(mut self) -> Self {
        if let Some(claim) = self.claim.take() {
            let _ = claim.send(());
        }
        self
    }
}

impl Subscription {
    fn new(
        channels: Channels,
        images: Arc<DecodedImages>,
        source: &ImageSource,
        ticket: Arc<LoadTicket>,
    ) -> Self {
        Self {
            rx: channels.settle,
            claim: Some(channels.claim),
            images,
            source: source.clone(),
            ticket,
        }
    }

    /// Whether this subscriber should look for a parked run: it was told one
    /// is parked, or its claim channel closed without telling, which leaves
    /// it to look rather than miss a run. Looking finds nothing when none is
    /// parked for its load.
    fn poll_told(&mut self, ctx: &mut Context<'_>) -> bool {
        let Some(claim) = &mut self.claim else {
            return false;
        };
        match Pin::new(claim).poll(ctx) {
            Poll::Pending => false,
            Poll::Ready(_) => {
                self.claim = None;
                true
            }
        }
    }

    /// What the load delivers; `None` when it closed unsettled.
    fn poll_outcome(&mut self, ctx: &mut Context<'_>) -> Poll<Option<Delivery>> {
        Pin::new(&mut self.rx)
            .poll(ctx)
            .map(std::result::Result::ok)
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
        self.state = match self.images.admit(&self.source, &self.services) {
            Admission::Cached(image) => LoadState::Ready(Some(Ok(image))),
            Admission::Joined(subscription) => LoadState::Waiting(subscription),
            Admission::Lead { subscription, run } => LoadState::Leading {
                run: Some(run),
                subscription,
            },
            Admission::Started {
                subscription,
                start,
            } => {
                // A slot free now: spawn here. Lanes with no room, or
                // shutting down, answer through the subscription.
                if let Some((slot, start)) = start {
                    drop(start(slot));
                }
                LoadState::Waiting(subscription)
            }
        };
    }

    /// Drives the load's parked run here, if this subscriber was just told
    /// of it and no other claimed it first.
    fn claim_parked(&mut self, ctx: &mut Context<'_>) -> bool {
        let LoadState::Waiting(subscription) = &mut self.state else {
            return false;
        };
        if !subscription.poll_told(ctx) {
            return false;
        }
        let Some(run) = self.images.claim(subscription) else {
            return false;
        };
        let LoadState::Waiting(subscription) =
            std::mem::replace(&mut self.state, LoadState::Unadmitted)
        else {
            unreachable!("BUG: the state was waiting under the same borrow");
        };
        self.state = LoadState::Leading {
            run: Some(run),
            subscription,
        };
        true
    }

    /// Takes over a load closed unsettled, or answers once it has been
    /// closed under this subscriber too often.
    fn hand_over(&mut self) {
        self.handovers = self.handovers.saturating_add(1);
        if self.handovers > MAX_HANDOVERS {
            self.state = LoadState::Ready(Some(Err(ImageLoadErrorKind::Abandoned {
                handovers: MAX_HANDOVERS,
            })));
            return;
        }
        self.admit();
    }

    /// This subscriber's answer: `outcome` with a failure named by its
    /// source.
    fn answer(&self, outcome: Result<Image>) -> std::result::Result<Image, ImageLoadError> {
        outcome.map_err(|kind| ImageLoadError::new(self.source.clone(), kind))
    }
}

impl Delivery {
    /// The outcome the delivery answers: the load's, or the refusal of the
    /// subscriber's lanes.
    fn into_outcome(self) -> Result<Image> {
        match self {
            Self::Settled(outcome) => outcome,
            Self::Refused(reason) => Err(ImageLoadErrorKind::LanesRefused(reason)),
        }
    }
}

impl std::fmt::Debug for ImageLoad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self.state {
            LoadState::Unadmitted => "unadmitted",
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
    type Output = std::result::Result<Image, ImageLoadError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            if self.claim_parked(ctx) {
                continue;
            }
            let settled = match &mut self.state {
                LoadState::Unadmitted => {
                    self.admit();
                    continue;
                }
                LoadState::Ready(outcome) => {
                    let outcome = outcome
                        .take()
                        .expect("BUG: an image load was polled after it completed");
                    return Poll::Ready(self.answer(outcome));
                }
                LoadState::Waiting(subscription) => subscription.poll_outcome(ctx),
                LoadState::Leading { run, subscription } => {
                    if let Some(future) = run
                        && future.as_mut().poll(ctx).is_ready()
                    {
                        *run = None;
                    }
                    subscription.poll_outcome(ctx)
                }
            };
            match settled {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(delivery)) => {
                    return Poll::Ready(self.answer(delivery.into_outcome()));
                }
                Poll::Ready(None) => self.hand_over(),
            }
        }
    }
}
