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
//! - [`TransferRequest`] — stage 4, the consumer half of a delivery: a future
//!   over the foundation claim slot whose producer half the source keeps,
//!   polled on the frame-driven `AsyncDriver` (no second async mechanism, no
//!   runtime dependency);
//! - [`TransferPayload`] / [`TransferError`] — stage 5 outcomes;
//! - [`DataTransferSource`] — the platform-side trait tying the stages
//!   together, with [`NullDataTransferSource`] as the honest inert
//!   implementation for backends that have no transport yet.
//!
//! Each platform instance mints [`DataTransferId`]s in one table inside its
//! one [`DataTransferSource`], so every id the app can observe is redeemable
//! at `Platform::data_transfer()` and a stale id fails the generation check
//! instead of aliasing a later offer. The table, like the producer half of a
//! request, is the host's.

use std::sync::Arc;

use flui_foundation::{ClaimHandle, ClaimOutcome, DataTransferId};

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
#[allow(
    clippy::exhaustive_enums,
    reason = "the transport names the formats it decodes; any other media type is `Custom`"
)]
pub enum TransferFormat {
    /// Plain UTF-8 text (`text/plain`).
    Text,
    /// HTML markup (`text/html`). Delivered raw; sanitization is the
    /// consumer's duty.
    Html,
    /// URIs, a local file as a `file:` URI (`text/uri-list`, RFC 2483).
    /// Delivered as [`TransferPayload::Custom`] of that type: one URI per
    /// CRLF-terminated line.
    UriList,
    /// An encoded image in the container named by `mime` (e.g. `image/png`).
    /// Delivered as [`TransferPayload::Custom`] of `mime`, the bytes as the
    /// source provided them; pixel decoding is the consumer's.
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

impl TransferFormat {
    /// Whether this is FLUI's private drag-session identity
    /// ([`DragSessionKey::representation`](crate::drag::DragSessionKey::representation)),
    /// whatever its parameters.
    #[must_use]
    pub fn is_drag_identity(&self) -> bool {
        matches!(self, Self::Custom { mime }
            if mime.as_str().split(';').next() == Some(crate::drag::IDENTITY_ESSENCE))
    }
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

crate::flag_set::flag_set! {
    /// Drop effects a DnD source permits / a target accepts. The empty set
    /// permits or accepts nothing: a target replying with it refuses the drop.
    pub struct TransferActions: u8 {
        /// Copy the data; the source keeps its copy.
        const COPY = 1;
        /// Move the data; the source is expected to remove its copy.
        const MOVE = 1 << 1;
        /// Link to the data rather than transferring it.
        const LINK = 1 << 2;
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

impl DropFeedback {
    /// The one action this answer takes while the source permits `allowed`:
    /// the accepted actions the source permits, narrowed to one (copy, then
    /// move, then link), since every drag protocol reports a single effect.
    /// `None` refuses the drop.
    #[must_use]
    pub fn resolve(self, allowed: TransferActions) -> Option<TransferActions> {
        let permitted = self.accept? & allowed;
        [
            TransferActions::COPY,
            TransferActions::MOVE,
            TransferActions::LINK,
        ]
        .into_iter()
        .find(|action| permitted.contains(*action))
    }
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
    /// Backends call this right after minting `id` in their offer table; the
    /// offer shares the table's representation slice, so the two can never
    /// disagree.
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

/// Stage-5 artifact: a decoded, typed payload.
///
/// The transport decodes text and markup; a URI list, an image and any other
/// media type arrive as the bytes of their type ([`TransferFormat::UriList`],
/// [`TransferFormat::Image`]).
#[derive(Debug, Clone)]
#[allow(
    clippy::exhaustive_enums,
    reason = "the transport decodes text and markup; every other media type is `Custom`"
)]
pub enum TransferPayload {
    /// Plain UTF-8 text.
    Text(String),
    /// Raw HTML markup (unsanitized — the consumer's duty).
    Html(String),
    /// The bytes of any other media type, undecoded.
    Custom {
        /// The media type of `bytes`: `text/uri-list` for a URI list, the
        /// container of an image, the type the source declared otherwise.
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
// TransferRequest — the stage-4 consumer half
// ============================================================================

#[derive(Debug)]
enum RequestState {
    Ready(Option<Result<TransferPayload, TransferError>>),
    Pending(ClaimHandle<Result<TransferPayload, TransferError>>),
}

/// Stage-4 artifact, consumer half: one in-flight delivery. A `Send` future
/// polled by the frame-driven async driver. Dropping it before completion
/// cancels the delivery, which the producer sees as its claim slot abandoned
/// ([`ClaimSlot::is_abandoned`](flui_foundation::ClaimSlot::is_abandoned)).
///
/// Executor clone, wake and retirement run outside the claim slot's locks.
/// Caught wake failures preserve the delivery and retain opaque executor
/// ownership; ordinary successful retirement still runs destructors.
#[must_use = "dropping a TransferRequest cancels the delivery"]
#[derive(Debug)]
pub struct TransferRequest {
    state: RequestState,
    resolved: bool,
}

impl TransferRequest {
    /// A delivery the producer completes through the claim slot `handle` was
    /// made with ([`claim_slot`](fn@flui_foundation::claim_slot)): the backend
    /// keeps the slot and returns the request from
    /// [`DataTransferSource::request`]. A value delivered after the request
    /// was dropped is discarded; a slot dropped without delivering resolves
    /// the request with [`TransferError::SourceGone`].
    pub fn pending(handle: ClaimHandle<Result<TransferPayload, TransferError>>) -> Self {
        Self {
            state: RequestState::Pending(handle),
            resolved: false,
        }
    }

    /// An already-resolved request, for data that is genuinely in memory.
    /// Polling this path does not clone or register an executor waker.
    pub fn ready(result: Result<TransferPayload, TransferError>) -> Self {
        Self {
            state: RequestState::Ready(Some(result)),
            resolved: false,
        }
    }
}

impl std::future::Future for TransferRequest {
    type Output = Result<TransferPayload, TransferError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        use std::task::Poll;
        assert!(
            !self.resolved,
            "BUG: TransferRequest polled again after it resolved"
        );
        let outcome = match &mut self.state {
            RequestState::Ready(result) => {
                result.take().expect("BUG: ready transfer lost its result")
            }
            RequestState::Pending(handle) => match std::pin::Pin::new(handle).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(ClaimOutcome::Delivered(result)) => result,
                Poll::Ready(ClaimOutcome::OwnerGone) => Err(TransferError::SourceGone),
                Poll::Ready(ClaimOutcome::AlreadyClaimed) => {
                    panic!("BUG: a TransferRequest's claim handle was claimed before the request")
                }
            },
        };
        self.resolved = true;
        Poll::Ready(outcome)
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
