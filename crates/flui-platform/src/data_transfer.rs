//! Data-transfer transport vocabulary (ADR-0038).
//!
//! Clipboard reads and system drag-and-drop share one seven-stage transport:
//! offer → negotiation → request → async delivery → decoding → drop action →
//! completion/cancellation. This module defines the stage artifacts:
//!
//! - [`DataTransferOffer`] — stage 1, metadata only, never bytes;
//! - [`RepresentationDescriptor`] / [`TransferFormat`] / [`Mime`] — stage 2
//!   negotiation vocabulary, plus [`DropFeedback`], the DnD reply half;
//! - [`TransferLimits`] — stage-3 resource bounds;
//! - [`TransferRequest`] / [`TransferCompleter`] — stage 4, a hand-rolled
//!   oneshot future/completer pair polled on the frame-driven `AsyncDriver`
//!   (no second async mechanism, no runtime dependency);
//! - [`TransferPayload`] / [`TransferError`] — stage 5 outcomes;
//! - [`DataTransferSource`] — the platform-side trait tying the stages
//!   together, with [`NullDataTransferSource`] as the honest inert
//!   implementation for backends that have no transport yet.
//!
//! [`OfferTable`] is the single minting authority for [`DataTransferId`]s:
//! each platform instance owns exactly one table inside its one
//! [`DataTransferSource`], so every id the app can observe is redeemable at
//! `Platform::data_transfer()` and stale ids fail the generation check
//! instead of aliasing a later offer.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Waker;

use flui_foundation::DataTransferId;
use parking_lot::Mutex;

// ============================================================================
// MIME + formats
// ============================================================================

/// A MIME type. Construction normalizes the type/subtype and parameter
/// *attribute names* to ASCII-lowercase; parameter *values* are preserved
/// byte-for-byte (RFC 2045 — e.g. `boundary` values are case-sensitive).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mime(Arc<str>);

impl Mime {
    /// Normalize `s` into a canonical MIME string.
    ///
    /// `type/subtype` and every parameter attribute name are lowercased and
    /// trimmed of surrounding whitespace; everything after a parameter's `=`
    /// is kept exactly as written.
    #[must_use]
    pub fn new(s: &str) -> Self {
        let mut out = String::with_capacity(s.len());
        let mut parts = s.split(';');
        if let Some(essence) = parts.next() {
            out.push_str(&essence.trim().to_ascii_lowercase());
        }
        for parameter in parts {
            out.push(';');
            match parameter.split_once('=') {
                Some((attribute, value)) => {
                    out.push_str(&attribute.trim().to_ascii_lowercase());
                    out.push('=');
                    out.push_str(value);
                }
                None => out.push_str(&parameter.trim().to_ascii_lowercase()),
            }
        }
        Self(Arc::from(out))
    }

    /// The normalized MIME string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Mime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The typed format of one representation a source offers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransferFormat {
    /// Plain UTF-8 text (`text/plain`).
    Text,
    /// HTML markup (`text/html`). Delivered raw; sanitization is the
    /// consumer's duty.
    Html,
    /// URIs / file paths (`text/uri-list`).
    UriList,
    /// An encoded image in the container named by `mime` (e.g. `image/png`).
    Image {
        /// Container format of the encoded bytes.
        mime: Mime,
    },
    /// Any other MIME type, delivered as raw bytes.
    Custom {
        /// The exact MIME type the source declared.
        mime: Mime,
    },
}

/// Stage-1/2 descriptor of one representation: format + size hint, no
/// payload.
#[derive(Debug, Clone)]
pub struct RepresentationDescriptor {
    /// Typed format of this representation.
    pub format: TransferFormat,
    /// Size in bytes if the source declared it; `None` = unknown until
    /// delivery.
    pub declared_len: Option<u64>,
}

/// Index of a representation within one offer, assigned by the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepresentationIndex(pub u16);

// ============================================================================
// Actions + feedback
// ============================================================================

/// Drop effects a DnD source permits / a target accepts. Plain `u8` bitset —
/// no external bitflags dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransferActions(u8);

impl TransferActions {
    /// No action — a target replying with this refuses the drop.
    pub const NONE: Self = Self(0);
    /// Copy the data; the source keeps its copy.
    pub const COPY: Self = Self(1 << 0);
    /// Move the data; the source is expected to remove its copy.
    pub const MOVE: Self = Self(1 << 1);
    /// Link to the data rather than transferring it.
    pub const LINK: Self = Self(1 << 2);

    /// Whether every action in `other` is present in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// The set union of the two action sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether the set is empty (no permitted/accepted action).
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for TransferActions {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for TransferActions {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// Stage-2 reply half (DnD): the target's current answer to "what would you
/// do if the user dropped now". Sent via
/// [`DataTransferSource::update_drop_feedback`]; the backend caches the
/// latest value per session and answers the OS's synchronous hover queries
/// from that cache. `accept: None` (the default) means reject.
///
/// `accept` should name a single action; the backend intersects it with the
/// source's permitted set and maps it to the protocol's single effect value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DropFeedback {
    /// The action the target would take on a drop right now, or `None` to
    /// reject.
    pub accept: Option<TransferActions>,
}

// ============================================================================
// Offer
// ============================================================================

/// Stage-1 artifact: what a source announces. Metadata only. Cheap to clone
/// (`Arc`-backed), immutable after mint.
#[derive(Debug, Clone)]
pub struct DataTransferOffer {
    id: DataTransferId,
    representations: Arc<[RepresentationDescriptor]>,
}

impl DataTransferOffer {
    /// Assemble an offer from its minted id and announced representations.
    ///
    /// Backends call this right after [`OfferTable::mint`]; the offer shares
    /// the record's representation slice, so the two can never disagree.
    #[must_use]
    pub fn new(id: DataTransferId, representations: Arc<[RepresentationDescriptor]>) -> Self {
        Self {
            id,
            representations,
        }
    }

    /// The generational id redeemable at the minting source.
    #[must_use]
    pub fn id(&self) -> DataTransferId {
        self.id
    }

    /// Representations in the source's declared order, which every platform
    /// convention (X11 TARGETS, Wayland offer order, NSPasteboard types)
    /// treats as the source's preference ranking — index 0 is what the
    /// source considers its best fidelity.
    #[must_use]
    pub fn representations(&self) -> &[RepresentationDescriptor] {
        &self.representations
    }

    /// First representation matching `format` in source-preference order.
    /// A consumer with its own cross-format ranking (e.g. prefer any `Image`
    /// over `Html`) iterates [`Self::representations`] itself.
    #[must_use]
    pub fn find(&self, format: &TransferFormat) -> Option<RepresentationIndex> {
        self.representations
            .iter()
            .position(|descriptor| descriptor.format == *format)
            .map(|index| {
                RepresentationIndex(
                    u16::try_from(index)
                        .expect("BUG: offer holds more than u16::MAX representations"),
                )
            })
    }
}

// ============================================================================
// Payload
// ============================================================================

/// One entry of a `text/uri-list` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferUri {
    /// A local filesystem path (`file://` URIs and platform-native drops).
    Path(PathBuf),
    /// Any other scheme, unparsed.
    Uri(String),
}

/// An encoded image payload. The transport hands over *encoded* bytes; pixel
/// decoding belongs to the consumer (flui-assets' decoders), not the
/// transport.
///
/// Declared as part of the frozen vocabulary, but **no backend delivers it
/// yet**: image representations arrive with the native transports (ADR-0038
/// §5 protocol mapping). Until then no source ever announces
/// [`TransferFormat::Image`], so a request for one cannot arise — there is
/// deliberately no stub that fabricates success.
#[derive(Debug, Clone)]
pub struct TransferImage {
    /// Container format of `bytes` (e.g. `image/png`).
    pub mime: Mime,
    /// The encoded image bytes, exactly as the source provided them.
    pub bytes: Arc<[u8]>,
}

/// Stage-5 artifact: a decoded, typed payload.
#[derive(Debug, Clone)]
pub enum TransferPayload {
    /// Plain UTF-8 text.
    Text(String),
    /// Raw HTML markup (unsanitized — the consumer's duty).
    Html(String),
    /// Entries of a `text/uri-list` representation.
    UriList(Vec<TransferUri>),
    /// An encoded image (see [`TransferImage`] for delivery status).
    Image(TransferImage),
    /// Raw bytes of any other MIME type.
    Custom {
        /// The exact MIME type the source declared.
        mime: Mime,
        /// The undecoded payload bytes.
        bytes: Arc<[u8]>,
    },
}

impl TransferPayload {
    /// Actual in-memory payload size in bytes, for limit enforcement against
    /// already-materialized data ([`TransferLimits::max_bytes`]).
    #[must_use]
    pub fn byte_len(&self) -> u64 {
        match self {
            Self::Text(text) | Self::Html(text) => text.len() as u64,
            Self::UriList(uris) => uris
                .iter()
                .map(|uri| match uri {
                    TransferUri::Path(path) => path.as_os_str().len() as u64,
                    TransferUri::Uri(uri) => uri.len() as u64,
                })
                .sum(),
            Self::Image(image) => image.bytes.len() as u64,
            Self::Custom { bytes, .. } => bytes.len() as u64,
        }
    }
}

// ============================================================================
// Limits + errors
// ============================================================================

/// Stage-3 resource bounds, enforced by the transport before and during
/// delivery. `#[non_exhaustive]`: deferred timeout work may add a field;
/// construct via `Default` + `with_*`.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TransferLimits {
    /// Deliveries larger than this fail with [`TransferError::TooLarge`].
    /// Checked against [`RepresentationDescriptor::declared_len`] at request
    /// time when known, and against actual accumulated bytes during delivery
    /// regardless.
    pub max_bytes: u64,
}

impl TransferLimits {
    /// Replace the byte ceiling.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }
}

impl Default for TransferLimits {
    /// 16 MiB — generous for text/uri-list, deliberately small for images;
    /// consumers expecting large payloads must opt in explicitly.
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Transport-stage failure. `#[non_exhaustive]`: deferred timeout/streaming
/// work implies new variants.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum TransferError {
    /// The offer id does not name a live offer (superseded, completed, or
    /// cancelled).
    #[error("offer {0} is stale (superseded, completed, or cancelled)")]
    StaleOffer(DataTransferId),
    /// The representation index is out of range for the offer.
    #[error("representation {index:?} out of range for offer {id}")]
    UnknownRepresentation {
        /// The offer that was addressed.
        id: DataTransferId,
        /// The out-of-range representation index.
        index: RepresentationIndex,
    },
    /// The payload exceeds the requested [`TransferLimits::max_bytes`].
    #[error("payload exceeds limit: {actual} > {limit} bytes")]
    TooLarge {
        /// Declared or accumulated payload size in bytes.
        actual: u64,
        /// The limit the request set.
        limit: u64,
    },
    /// The payload bytes failed to decode as the announced format.
    #[error("payload failed to decode as {format:?}")]
    Decode {
        /// The format the representation announced.
        format: TransferFormat,
    },
    /// The producing side vanished mid-transfer (window closed, client
    /// exited, completer dropped without completing).
    #[error("source vanished mid-transfer (window closed, client exited)")]
    SourceGone,
    /// The transfer was cancelled by the consumer.
    #[error("transfer cancelled")]
    Cancelled,
}

// ============================================================================
// OfferTable — the single minting authority
// ============================================================================

/// Source-side state for one live offer: the announced representations plus
/// the payload once (and if) the source has it in memory.
#[derive(Debug, Clone)]
pub struct OfferRecord {
    representations: Arc<[RepresentationDescriptor]>,
    payload: Option<TransferPayload>,
}

impl OfferRecord {
    /// A record announcing `representations`, with no payload yet.
    #[must_use]
    pub fn new(representations: Arc<[RepresentationDescriptor]>) -> Self {
        Self {
            representations,
            payload: None,
        }
    }

    /// The announced representations, in source-preference order.
    #[must_use]
    pub fn representations(&self) -> &Arc<[RepresentationDescriptor]> {
        &self.representations
    }

    /// The in-memory payload, if the source has materialized one.
    #[must_use]
    pub fn payload(&self) -> Option<&TransferPayload> {
        self.payload.as_ref()
    }

    /// Store the materialized payload (e.g. a frozen file-drop list).
    pub fn set_payload(&mut self, payload: TransferPayload) {
        self.payload = Some(payload);
    }
}

/// One slot of the offer slab. The generation survives the record: retiring
/// bumps it, so the next occupant of the slot mints a different id.
#[derive(Debug)]
struct OfferSlot {
    generation: NonZeroU32,
    record: Option<OfferRecord>,
}

/// Slab of live offers with generation-checked lookup. One per platform
/// instance, private inside its [`DataTransferSource`]. Not a public-API
/// lock: backends hold it behind their existing state lock (SP-6 — no guard
/// escapes a public signature).
///
/// This is the **single minting authority** for [`DataTransferId`]s
/// (ADR-0038 §2): a generational check is only a defense within one table,
/// so a platform instance must never operate two of these.
#[derive(Debug, Default)]
pub struct OfferTable {
    slots: Vec<OfferSlot>,
    free: Vec<u32>,
}

impl OfferTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a live offer, returning its generational id
    /// (`new_gen(slot, generation)`).
    pub fn mint(&mut self, record: OfferRecord) -> DataTransferId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(slot.record.is_none(), "free list held an occupied slot");
            slot.record = Some(record);
            return DataTransferId::new_gen(index, slot.generation);
        }

        let index =
            u32::try_from(self.slots.len()).expect("BUG: offer table exceeds u32::MAX slots");
        let generation = NonZeroU32::MIN;
        self.slots.push(OfferSlot {
            generation,
            record: Some(record),
        });
        DataTransferId::new_gen(index, generation)
    }

    /// Generation-checked lookup: slot **and** generation must match, so a
    /// retired id never addresses the slot's next occupant.
    #[must_use]
    pub fn get(&self, id: DataTransferId) -> Option<&OfferRecord> {
        self.slot(id).and_then(|slot| slot.record.as_ref())
    }

    /// Generation-checked mutable lookup (same rules as [`Self::get`]).
    pub fn get_mut(&mut self, id: DataTransferId) -> Option<&mut OfferRecord> {
        let slot_index = id.index() as usize;
        let slot = self.slots.get_mut(slot_index)?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.record.as_mut()
    }

    /// Retire a live offer: free the slot and bump its generation so the id
    /// is permanently stale. Returns the record, or `None` if `id` was
    /// already stale (idempotent).
    ///
    /// A slot whose generation would overflow is never reused — it is
    /// dropped from circulation rather than allowed to recycle generation 1.
    pub fn retire(&mut self, id: DataTransferId) -> Option<OfferRecord> {
        let slot_index = id.index() as usize;
        let slot = self.slots.get_mut(slot_index)?;
        if slot.generation != id.generation() || slot.record.is_none() {
            return None;
        }
        let record = slot.record.take();
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(id.index());
        }
        // else: generation space exhausted for this slot — leave it out of
        // the free list forever rather than risk ABA reuse.
        record
    }

    /// Number of live offers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.record.is_some())
            .count()
    }

    /// Whether the table holds no live offer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn slot(&self, id: DataTransferId) -> Option<&OfferSlot> {
        let slot = self.slots.get(id.index() as usize)?;
        (slot.generation == id.generation()).then_some(slot)
    }
}

// ============================================================================
// TransferRequest / TransferCompleter — the stage-4 oneshot pair
// ============================================================================

/// Shared oneshot state between a [`TransferRequest`] and its
/// [`TransferCompleter`]. Private; no guard ever escapes (SP-6).
#[derive(Debug)]
struct TransferShared {
    slot: Mutex<TransferSlot>,
}

#[derive(Debug)]
struct TransferSlot {
    /// The delivery outcome, present between completion and the consumer
    /// taking it in `poll`.
    outcome: Option<Result<TransferPayload, TransferError>>,
    /// Waker of the last poll, woken on completion.
    waker: Option<Waker>,
    /// Consumer dropped its half (stage-7 cancellation).
    cancelled: bool,
    /// Producer finished — `complete` ran or the completer was dropped.
    settled: bool,
    /// The consumer already took the outcome out of the slot.
    delivered: bool,
}

/// Stage-4 artifact, consumer half: one in-flight delivery. A `Send` future
/// so it can ride `BoxedTask` (`Pin<Box<dyn Future<Output = ()> + Send>>`,
/// flui-scheduler's `AsyncDriver` contour) after being wrapped by the
/// consumer. Dropping it before completion is stage-7 cancellation: the
/// shared state is marked cancelled, which the producer observes via
/// [`TransferCompleter::is_cancelled`].
#[must_use = "dropping a TransferRequest cancels the delivery"]
#[derive(Debug)]
pub struct TransferRequest {
    shared: Arc<TransferShared>,
}

impl TransferRequest {
    /// A connected pair: the backend keeps the completer and returns the
    /// request from [`DataTransferSource::request`].
    #[must_use = "dropping the pair immediately resolves it as cancelled"]
    pub fn channel() -> (TransferRequest, TransferCompleter) {
        let shared = Arc::new(TransferShared {
            slot: Mutex::new(TransferSlot {
                outcome: None,
                waker: None,
                cancelled: false,
                settled: false,
                delivered: false,
            }),
        });
        (
            TransferRequest {
                shared: Arc::clone(&shared),
            },
            TransferCompleter { shared },
        )
    }

    /// An already-resolved request, for data that is genuinely in memory.
    pub fn ready(result: Result<TransferPayload, TransferError>) -> Self {
        Self {
            shared: Arc::new(TransferShared {
                slot: Mutex::new(TransferSlot {
                    outcome: Some(result),
                    waker: None,
                    cancelled: false,
                    settled: true,
                    delivered: false,
                }),
            }),
        }
    }
}

impl std::future::Future for TransferRequest {
    type Output = Result<TransferPayload, TransferError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut slot = self.shared.slot.lock();
        if let Some(outcome) = slot.outcome.take() {
            slot.delivered = true;
            slot.waker = None;
            return std::task::Poll::Ready(outcome);
        }
        assert!(
            !slot.delivered,
            "BUG: TransferRequest polled again after it resolved"
        );
        slot.waker = Some(cx.waker().clone());
        std::task::Poll::Pending
    }
}

impl Drop for TransferRequest {
    fn drop(&mut self) {
        let mut slot = self.shared.slot.lock();
        slot.cancelled = true;
        // Free the payload and waker eagerly; the producer only reads flags.
        slot.outcome = None;
        slot.waker = None;
    }
}

/// Stage-4 artifact, producer half, held by the backend. `Send` — complete
/// from any thread. Completing after the consumer dropped the request is a
/// silent no-op; dropping the completer without completing resolves the
/// request with [`TransferError::SourceGone`], so a crashed producer can
/// never leave a consumer pending forever.
#[derive(Debug)]
pub struct TransferCompleter {
    shared: Arc<TransferShared>,
}

impl TransferCompleter {
    /// Resolve the paired request. A no-op if the consumer already cancelled
    /// (dropped its half).
    pub fn complete(self, result: Result<TransferPayload, TransferError>) {
        let waker = {
            let mut slot = self.shared.slot.lock();
            slot.settled = true;
            if slot.cancelled {
                None
            } else {
                slot.outcome = Some(result);
                slot.waker.take()
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// True once the consumer cancelled (dropped its half). A long-running
    /// producer polls this to abandon work early. Note: a blocking read
    /// already in progress (arboard) is *not* interruptible — cancellation
    /// then means the result is discarded, not that the thread unblocks.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.shared.slot.lock().cancelled
    }
}

impl Drop for TransferCompleter {
    fn drop(&mut self) {
        let waker = {
            let mut slot = self.shared.slot.lock();
            if slot.settled {
                None
            } else {
                slot.settled = true;
                if slot.cancelled {
                    None
                } else {
                    slot.outcome = Some(Err(TransferError::SourceGone));
                    slot.waker.take()
                }
            }
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

// ============================================================================
// DataTransferSource
// ============================================================================

/// The platform-side transport: stages 1, 2 (reply half), 3, 6 in one
/// object-safe trait. Exactly one instance per platform instance (ADR-0038
/// §2, §6). No method blocks; no method is `async fn` — the sync-pipeline
/// rule is honored by returning a pollable value instead.
pub trait DataTransferSource: Send + Sync {
    /// Stage 1, clipboard flavor: advertise the current clipboard offer.
    /// Stable id per observed clipboard state (ADR-0038 §2 lifecycle). DnD
    /// offers arrive by event (`DragDropEvent::Entered`) instead — pushed by
    /// the OS, not polled.
    fn clipboard_offer(&self) -> Option<DataTransferOffer>;

    /// Stage 3: request one representation of a live offer. Returns
    /// immediately; delivery completes through the returned future. A
    /// backend whose data is already in memory returns
    /// [`TransferRequest::ready`].
    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest;

    /// Stage 2, reply half (DnD only): the target's current hover answer.
    /// The backend caches the latest value per session and answers the OS's
    /// synchronous hover queries from the cache. Stale ids are a no-op.
    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback);

    /// Stage 6 (DnD only): the target has fetched what it needs; release
    /// protocol resources (Wayland `wl_data_offer.finish`, Win32 IDataObject
    /// release) and retire the offer. Idempotent; stale ids are a no-op.
    fn conclude_drop(&self, id: DataTransferId);
}

/// The inert source: [`clipboard_offer`](DataTransferSource::clipboard_offer)
/// → `None`; [`request`](DataTransferSource::request) →
/// `TransferRequest::ready(Err(TransferError::StaleOffer(id)))`; feedback and
/// conclusion are no-ops. For backends with no transport yet — honest
/// absence, not fake success.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullDataTransferSource;

impl DataTransferSource for NullDataTransferSource {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        _representation: RepresentationIndex,
        _limits: TransferLimits,
    ) -> TransferRequest {
        // This source mints no offers, so every id presented to it is by
        // definition not one of ours — stale, honestly.
        TransferRequest::ready(Err(TransferError::StaleOffer(id)))
    }

    fn update_drop_feedback(&self, _id: DataTransferId, _feedback: DropFeedback) {}

    fn conclude_drop(&self, _id: DataTransferId) {}
}
