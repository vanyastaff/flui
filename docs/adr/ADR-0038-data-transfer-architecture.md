# ADR-0038: Data-transfer architecture — clipboard representations and system drag-and-drop

*Clipboard reads and system drag-and-drop become one seven-stage transport (offer →
negotiation → request → async delivery → decoding → drop action → completion/cancellation)
carried by a generational `DataTransferId`, a metadata-only `DataTransferOffer`, and a
`TransferRequest`/`TransferCompleter` pair polled on the existing frame-driven
`AsyncDriver`/`TaskToken` contour. Each platform instance owns exactly **one** transport
source with **one** offer table — `Platform::data_transfer()` is a required method returning
that single instance, so every id the app can observe is redeemable at the one place the app
looks. DnD negotiation is bidirectional: the target's `DropFeedback` is cached by the backend
and answers the OS's *synchronous* hover queries (Win32 `DROPEFFECT`, AppKit
`NSDragOperation`, Wayland `accept`/`set_actions`), which is what makes the trait surface
frozen in slice 1 implementable on every native protocol later. Clipboard and DnD are two
facades (`ClipboardOffer`, `DragOffer`) over that single transport; the existing sync
`Clipboard` trait survives as the backend substrate, with the Wayland/X11 blocking-read
problem (U18) quarantined per backend where the U7 affinity audit permits it — a dedicated
clipboard worker thread on winit-X11/Windows, an explicit stays-on-UI-thread mode on
winit-Wayland/macOS until native transports land. DnD events enter through a new
`PlatformInput::DragDrop` variant on the per-window dispatch path that already reaches the
realm.*

---

- **Status:** Proposed (2026-07-28)
- **Date:** 2026-07-28
- **Deciders:** @vanyastaff
- **Scope:** `crates/flui-foundation/src/id.rs` (`DataTransferId`); `crates/flui-platform/src/data_transfer.rs` (new — vocabulary + `OfferTable` + `DataTransferSource` + `NullDataTransferSource`); `crates/flui-platform/src/traits/{platform.rs,input.rs}` (`Platform::data_transfer()` required method, `PlatformInput::DragDrop`); all 8 `impl Platform for` sites (Null wiring at minimum); `crates/flui-platform/src/platforms/winit/{platform.rs,clipboard.rs}` (DnD arms, `WinitDataTransfer`, clipboard worker); `crates/flui-app/src/app/binding.rs` (new `DragDrop` match arm — slice 1, the exhaustive `PlatformInput` match at `binding.rs:1417` does not compile without it); `crates/flui-app/tests/` (gate-visible transport tests — see Slices); later slices: `crates/flui-interaction/src/data_transfer.rs`, `crates/flui-view/src/context/build_context.rs`, `crates/flui-app/src/app/{binding.rs,runner.rs}` (capability install), `scripts/check-frame-capability-scope.sh`
- **Related:** ADR-0034 (clipboard reachability — the `AppBinding` slot, pre-`run()` resolution, and the §3 single-instance contract this ADR now obeys for the transport too); ADR-0030 (`PlatformTextInput` — the owner/handle capability template); ADR-0027 (owner-affine `UiRealm` — sanctioned leapfrog zone this design lives in); ADR-0018 (async-builder seam — the `TaskToken` consumption discipline and the `RebuildHandle` completion pattern); audit `docs/audits/2026-07-25-upgrade-pack-audit.md` §22 (U6+U18) and §23 (U7 — event-loop affinity, load-bearing for the clipboard quarantine)

---

## Context

**Clipboard today is synchronous, blocking, and text-only.** The `Clipboard` trait
(`crates/flui-platform/src/traits/platform.rs:473-484`) is three methods:
`read_text(&self) -> Option<String>`, `write_text(&self, String)`, and a `has_text()`
default that calls `read_text()`. `ClipboardItem` (`platform.rs:491`) carries
`text: Option<String>` plus `metadata: Option<String>` documented as "MIME type hints" —
MIME modeled as an unstructured string. `Platform::write_to_clipboard`/`read_from_clipboard`
(`platform.rs:268-279`) default-bridge to the same text path. Access is wired per ADR-0034:
`Platform::clipboard()` (`platform.rs:232`) is resolved before `Platform::run()` consumes the
platform box and stashed in `AppBinding`. On Wayland and X11 a clipboard read is an
asynchronous negotiation with the source client over a pipe; the `arboard`-backed winit
implementation hides that behind a lock — `ArboardClipboard` holds
`Mutex<arboard::Clipboard>` (`platforms/winit/clipboard.rs:13`) and every read/write takes it
for the full OS round trip — so a UI-thread `read_text()` is a frame stall waiting on another
process (audit §22, item **U18** — architecturally unavoidable with a sync signature, not yet
measured). The affinity audit (§23, item **U7**) additionally classifies `clipboard()` as
**main-thread-required on macOS and Wayland** with UB-class risk off-main-thread — any fix
that moves reads to a background thread must be gated per backend, not applied blindly.

**System drag-and-drop is absent entirely** (audit §22, item **U6**). The winit backend's
`WindowEvent` match (`crates/flui-platform/src/platforms/winit/platform.rs:540-763`)
handles ~20 variants and ends in `_ => {}` (`:762`) — winit's `DroppedFile`/`HoveredFile`/
`HoveredFileCancelled` (winit `0.30.12`, root `Cargo.toml:181`) are silently discarded.
Every "drag" in the UI crates is an in-app gesture (`Draggable`, `Dismissible`), not OS DnD.

**Native DnD protocols demand synchronous hover answers.** Win32's
`IDropTarget::DragOver`/`::Drop` must return a `DROPEFFECT` on the message thread before
returning; AppKit's `draggingUpdated:` must return an `NSDragOperation` during hover;
Wayland requires `wl_data_offer.accept`/`set_actions` during motion. Any event surface that
lacks a target→OS feedback channel during hover — or that defers the accept/reject decision
until after the OS-level drop has resolved — cannot be implemented on any of the three.
This constraint shapes stages 2 and 6 below and is the reason the feedback channel is part
of the trait from slice 1, not a later addition.

**What the audit requires** (§22, "Требуемая архитектура"): a seven-stage model — offer,
negotiation, request, async delivery, decoding, drop action, completion/cancellation — with
`DataTransferId` built on the house generational-ID mechanism
(`crates/flui-foundation/src/id.rs`, `GenId<T>` at `:783-851`), delivery over the existing
`TaskToken` cancel-on-drop contour (`crates/flui-scheduler/src/async_driver.rs:135-171`)
rather than a second async mechanism, and **one transport with two facades** (clipboard
without a drop action, DnD with `TransferActions`). Multiple typed representations per
offer, lazy payload, size limits, and stale-offer defense are all currently absent (audit
§22 requirements table).

**House constraints that shape the answer:** no locks in public API (SP-6); no `async fn`
in the sync pipeline; new `dyn` boundaries need FR-036 sanction (`scripts/port-check.sh:1090-1231`);
capability handles are acquired in `init_state`/`did_change_dependencies` only (trigger #22,
`scripts/check-frame-capability-scope.sh:49`); Flutter is *not* the reference here — its
`Clipboard`/`services` layer is dissolved by `docs/FOUNDATIONS.md:142` into capability traits
on `flui-platform`, and process/thread/window topology is a sanctioned leapfrog zone
(ADR-0027).

## Decision

### 1. One transport, seven explicit stages

Every external datum entering the app — a paste, a drop — moves through the same stages,
each with a named artifact:

| Stage | Artifact | Who acts |
|---|---|---|
| 1. Offer | `DataTransferOffer` (metadata only, no payload) | platform backend |
| 2. Negotiation | consumer inspects `RepresentationDescriptor`s; **DnD adds the reply half:** target sends `DropFeedback`, backend caches it and answers OS hover queries synchronously | widget/realm ↔ backend |
| 3. Request | `DataTransferSource::request(id, index, limits)` | consumer via handle |
| 4. Async delivery | `TransferRequest` (consumer half) / `TransferCompleter` (backend half) | backend thread → frame thread |
| 5. Decoding | `TransferPayload` (typed) or `TransferError::Decode` | transport |
| 6. Drop action | resolved **at OS drop time from the cached feedback**; delivered as `Dropped { action }`; `conclude_drop(id)` releases protocol resources — DnD only | backend + realm authority |
| 7. Completion/cancel | per-delivery: `TaskToken`/`TransferRequest` drop; per-offer: source-side retirement + generation bump | either side |

Clipboard uses stages 1–5 and 7 (no drop action); DnD uses all seven. The payload is
**lazy**: stage 1 never carries bytes, so a 10 000-file drop or a 200 MB image on the
clipboard costs nothing until a consumer requests exactly one representation.

Two consequences of the stage-2/stage-6 shape, stated up front because they are the
protocol-compatibility core:

- **The accept/reject and copy/move decision is a hover-time decision, not a post-drop
  decision.** The target continuously answers "what would you do if the user dropped now"
  via `DropFeedback`; the backend caches the latest answer per session and replies to the
  OS's synchronous queries from that cache. At OS drop time the backend resolves the final
  action from the same cache — it never waits on the app.
- **The cache is allowed to lag.** Input dispatch may be deferred by the per-window
  reentrancy queue (`shared/handlers.rs:330-392`); the backend still answers the OS
  immediately from the cache as of the last processed hover. The OS re-queries continuously
  during motion, so the answer converges without ever blocking the message thread. Initial
  cache value is *reject* — a window with no interested target refuses drops, which is the
  correct default on every protocol.

### 2. `DataTransferId`: the house generational ID, minted by exactly one table per platform instance

`DataTransferId` joins the `generational:` block of the `ids!` macro invocation
(invocation at `crates/flui-foundation/src/id.rs:659`, generational block at `:712`,
alongside `RenderId` and `RealmId`):

```rust
// crates/flui-foundation/src/id.rs — inside ids! { generational: { ... } }

/// Data-transfer ID — **generational** key identifying one live transfer
/// offer: a clipboard snapshot or an in-progress system drag session.
///
/// Generational ([`GenId`]): offers are short-lived and their slots are
/// reused; a stale id held by a widget across a drag-cancel or a clipboard
/// change must fail the generation check instead of silently addressing the
/// next offer (ABA).
pub type DataTransferId DataTransfer;
```

**Single minting authority.** A generational check is only a defense within one table: two
independent tables both minting from slot 0/generation 1 would produce ids that pass each
other's generation checks — a cross-table collision the `GenId` encoding cannot catch.
Therefore the invariant, load-bearing for the whole "one transport" claim:

> **Each platform instance owns exactly one offer table, inside its one
> `DataTransferSource` instance. Every `DataTransferId` observable by the app — clipboard
> or DnD — is minted by that table and redeemable at `Platform::data_transfer()`.**

The winit backend's DnD event pump and its clipboard path mint through the *same* table
(both reach it through the backend's shared `Arc`'d state, the same shape as
`WinitPlatformState`). There is no per-facade table and no bridge-local table.

**`OfferTable` is one shared type, written and tested once.** The `GenId` *encoding* comes
for free; the bump-on-retire and generation-checked-lookup logic does not — so it is not
reimplemented per backend. `flui_platform::data_transfer` provides:

```rust
/// Slab of live offers with generation-checked lookup. One per platform
/// instance, private inside its DataTransferSource. Not a public-API lock:
/// backends hold it behind their existing state Arc (SP-6 — no guard escapes).
pub struct OfferTable { /* slab of OfferSlot { generation: NonZeroU32, record: Option<OfferRecord> } */ }

impl OfferTable {
    pub fn mint(&mut self, record: OfferRecord) -> DataTransferId;      // new_gen(slot, generation)
    pub fn get(&self, id: DataTransferId) -> Option<&OfferRecord>;      // slot AND generation must match
    pub fn retire(&mut self, id: DataTransferId) -> Option<OfferRecord>; // bump generation, free slot
}
```

Stage-3 requests validate slot *and* generation; a mismatch is
`TransferError::StaleOffer` — the same ABA discipline `RenderTree` and realm incarnations
already follow.

**Offer lifecycle** (previously unspecified; now the rules):

- **DnD: one live session slot per window.** Minting a new session (drag enters) retires
  the predecessor, whatever state it was in — so a `Dropped`-but-never-fetched offer is
  bounded to one session record per window (a `Vec<PathBuf>`-scale cost), never an
  unbounded leak, even in slice 1 where no consumer exists. Eager retirement happens at
  `conclude_drop` (slice 3's realm authority) or on `Exited`.
- **Clipboard: one offer per observed clipboard state.** `clipboard_offer()` returns the
  *same* id until the source observes a change; a native backend keyed to the OS change
  token (Windows clipboard sequence number, `NSPasteboard.changeCount`, Wayland
  `wl_data_offer` identity) re-mints on change and retires the predecessor — holders of the
  old id get `StaleOffer`, which is the correct semantics. The sync-clipboard mode (§6) has
  no change detection, so it holds a single long-lived offer with shared fate: the payload
  is whatever the clipboard contains at fetch time (TOCTOU, documented degraded mode).
- **No consumer-facing offer cancellation.** Stage-7 cancellation of a *delivery* is
  dropping the `TransferRequest`/`TaskToken` — naturally per-consumer. Offer *retirement*
  is the source's prerogative (session end, clipboard supersede) plus the single realm
  drop-conclusion authority (§7). An earlier draft had `cancel(id)` on the trait; it is
  removed — with several consumers sharing one offer (or several realms sharing the
  `Arc<dyn DataTransferSource>` under ADR-0027), any one consumer retiring the offer would
  cancel everyone else's in-flight fetches.

**Dependency consequence:** `flui-platform` today depends only on `flui-types`
(`crates/flui-platform/Cargo.toml:17`). It gains a `flui-foundation` dependency — a
downward L2→L1 edge in the layer DAG (`docs/FOUNDATIONS.md:146-156`), acyclic
(`flui-foundation` depends only on `flui-types`, `crates/flui-foundation/Cargo.toml:54`).

### 3. The vocabulary: typed representations, in `flui-platform::data_transfer`

New module `crates/flui-platform/src/data_transfer.rs`. All types are plain data — no lock
guards escape any public signature (SP-6).

```rust
use std::{path::PathBuf, sync::Arc};
use flui_foundation::DataTransferId;

/// A MIME type. Construction normalizes the type/subtype and parameter
/// *attribute names* to ASCII-lowercase; parameter *values* are preserved
/// byte-for-byte (RFC 2045 — e.g. `boundary` values are case-sensitive).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Mime(Arc<str>);

impl Mime {
    pub fn new(s: &str) -> Self;
    pub fn as_str(&self) -> &str;
}

/// The typed format of one representation a source offers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransferFormat {
    /// Plain UTF-8 text (`text/plain`).
    Text,
    /// HTML markup (`text/html`). Delivered raw; sanitization is the consumer's duty.
    Html,
    /// URIs / file paths (`text/uri-list`).
    UriList,
    /// An encoded image in the container named by `mime` (e.g. `image/png`).
    Image { mime: Mime },
    /// Any other MIME type, delivered as raw bytes.
    Custom { mime: Mime },
}

/// Stage-1/2 descriptor of one representation: format + size hint, no payload.
#[derive(Debug, Clone)]
pub struct RepresentationDescriptor {
    pub format: TransferFormat,
    /// Size in bytes if the source declared it; `None` = unknown until delivery.
    pub declared_len: Option<u64>,
}

/// Index of a representation within one offer, assigned by the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepresentationIndex(pub u16);

/// Drop effects a DnD source permits / a target accepts. Plain `u8` bitset —
/// no external bitflags dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransferActions(u8);

impl TransferActions {
    pub const NONE: Self = Self(0);
    pub const COPY: Self = Self(1 << 0);
    pub const MOVE: Self = Self(1 << 1);
    pub const LINK: Self = Self(1 << 2);
    #[must_use] pub const fn contains(self, other: Self) -> bool;
    #[must_use] pub const fn union(self, other: Self) -> Self; // + BitOr/BitOrAssign impls
}

/// Stage-2 reply half (DnD): the target's current answer to "what would you
/// do if the user dropped now". Sent via `update_drop_feedback`; the backend
/// caches the latest value per session and answers the OS's synchronous hover
/// queries from that cache. `accept: None` (the default) means reject.
///
/// `accept` should name a single action; the backend intersects it with the
/// source's permitted set and maps it to the protocol's single effect value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DropFeedback {
    pub accept: Option<TransferActions>,
}

/// Stage-1 artifact: what a source announces. Metadata only. Cheap to clone
/// (`Arc`-backed), immutable after mint.
#[derive(Debug, Clone)]
pub struct DataTransferOffer {
    id: DataTransferId,
    representations: Arc<[RepresentationDescriptor]>,
}

impl DataTransferOffer {
    #[must_use] pub fn id(&self) -> DataTransferId;
    /// Representations in the source's declared order, which every platform
    /// convention (X11 TARGETS, Wayland offer order, NSPasteboard types)
    /// treats as the source's preference ranking — index 0 is what the
    /// source considers its best fidelity.
    #[must_use] pub fn representations(&self) -> &[RepresentationDescriptor];
    /// First representation matching `format` in source-preference order.
    /// A consumer with its own cross-format ranking (e.g. prefer any `Image`
    /// over `Html`) iterates `representations()` itself.
    #[must_use] pub fn find(&self, format: &TransferFormat) -> Option<RepresentationIndex>;
}

/// One entry of a `text/uri-list` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferUri {
    /// A local filesystem path (`file://` URIs and platform-native drops).
    Path(PathBuf),
    /// Any other scheme, unparsed.
    Uri(String),
}

/// An encoded image payload. The transport hands over *encoded* bytes; pixel
/// decoding belongs to the consumer (flui-assets' decoders), not the transport.
#[derive(Debug, Clone)]
pub struct TransferImage {
    pub mime: Mime,
    pub bytes: Arc<[u8]>,
}

/// Stage-5 artifact: a decoded, typed payload.
#[derive(Debug, Clone)]
pub enum TransferPayload {
    Text(String),
    Html(String),
    UriList(Vec<TransferUri>),
    Image(TransferImage),
    Custom { mime: Mime, bytes: Arc<[u8]> },
}

/// Stage-3 resource bounds, enforced by the transport before and during
/// delivery. `#[non_exhaustive]`: the open timeout question (see Open
/// questions) may add a field; construct via `Default` + `with_*`.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TransferLimits {
    /// Deliveries larger than this fail with [`TransferError::TooLarge`].
    /// Checked against `declared_len` at request time when known, and against
    /// actual accumulated bytes during delivery regardless.
    pub max_bytes: u64,
}

impl TransferLimits {
    #[must_use] pub fn with_max_bytes(mut self, max_bytes: u64) -> Self;
}

impl Default for TransferLimits {
    /// 16 MiB — generous for text/uri-list, deliberately small for images;
    /// consumers expecting large payloads must opt in explicitly.
    fn default() -> Self { Self { max_bytes: 16 * 1024 * 1024 } }
}

/// `#[non_exhaustive]`: deferred timeout/streaming work implies new variants.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum TransferError {
    #[error("offer {0} is stale (superseded, completed, or cancelled)")]
    StaleOffer(DataTransferId),
    #[error("representation {index:?} out of range for offer {id}")]
    UnknownRepresentation { id: DataTransferId, index: RepresentationIndex },
    #[error("payload exceeds limit: {actual} > {limit} bytes")]
    TooLarge { actual: u64, limit: u64 },
    #[error("payload failed to decode as {format:?}")]
    Decode { format: TransferFormat },
    #[error("source vanished mid-transfer (window closed, client exited)")]
    SourceGone,
    #[error("transfer cancelled")]
    Cancelled,
}
```

(Realm-lifecycle errors — "owner gone" — live in `flui-interaction`, not here: the platform
vocabulary crate must not encode realm semantics. Same layering `TextInputError` already
follows, `crates/flui-interaction/src/text_input.rs:291`.)

### 4. The transport trait, its future, and its completer — no second async mechanism

The delivery is a oneshot pair. The earlier draft specified only the consumer half; the
producer half is the actual hard part of the transport and is part of the frozen surface:

```rust
/// Stage-4 artifact, consumer half: one in-flight delivery. A `Send` future
/// so it can ride `BoxedTask` (`Pin<Box<dyn Future<Output = ()> + Send>>`,
/// flui-scheduler/src/async_driver.rs:53) after being wrapped by the consumer.
/// Dropping it before completion is stage-7 cancellation: the shared state is
/// marked cancelled, which the producer observes via
/// [`TransferCompleter::is_cancelled`].
#[must_use = "dropping a TransferRequest cancels the delivery"]
pub struct TransferRequest { /* private oneshot state shared with the completer */ }

impl TransferRequest {
    /// A connected pair: the backend keeps the completer and returns the
    /// request from [`DataTransferSource::request`].
    #[must_use] pub fn channel() -> (TransferRequest, TransferCompleter);
    /// An already-resolved request, for data that is genuinely in memory.
    #[must_use] pub fn ready(result: Result<TransferPayload, TransferError>) -> Self;
}

impl std::future::Future for TransferRequest {
    type Output = Result<TransferPayload, TransferError>;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output>;
}

/// Stage-4 artifact, producer half, held by the backend. `Send` — complete
/// from any thread. Completing after the consumer dropped the request is a
/// silent no-op; dropping the completer without completing resolves the
/// request with [`TransferError::SourceGone`], so a crashed producer can
/// never leave a consumer pending forever.
pub struct TransferCompleter { /* shared oneshot state */ }

impl TransferCompleter {
    pub fn complete(self, result: Result<TransferPayload, TransferError>);
    /// True once the consumer cancelled (dropped its half). A long-running
    /// producer polls this to abandon work early. Note: a blocking read
    /// already in progress (arboard) is *not* interruptible — cancellation
    /// then means the result is discarded, not that the thread unblocks.
    #[must_use] pub fn is_cancelled(&self) -> bool;
}

/// The platform-side transport: stages 1, 2 (reply half), 3, 6 in one
/// object-safe trait. Exactly one instance per platform instance (§2, §6).
/// No method blocks; no method is `async fn` — the sync-pipeline rule is
/// honored by returning a pollable value instead.
pub trait DataTransferSource: Send + Sync {
    /// Stage 1, clipboard flavor: advertise the current clipboard offer.
    /// Stable id per observed clipboard state (§2 lifecycle). DnD offers
    /// arrive by event (`DragDropEvent::Entered`, §5) instead — pushed by
    /// the OS, not polled.
    fn clipboard_offer(&self) -> Option<DataTransferOffer>;

    /// Stage 3: request one representation of a live offer. Returns
    /// immediately; delivery completes through the returned future. A backend
    /// whose data is already in memory returns `TransferRequest::ready(..)`.
    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest;

    /// Stage 2, reply half (DnD only): the target's current hover answer.
    /// The backend caches the latest value per session and answers the OS's
    /// synchronous hover queries from the cache (§1). Stale ids are a no-op.
    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback);

    /// Stage 6 (DnD only): the target has fetched what it needs; release
    /// protocol resources (Wayland `wl_data_offer.finish`, Win32 IDataObject
    /// release) and retire the offer. Idempotent; stale ids are a no-op.
    fn conclude_drop(&self, id: DataTransferId);
}

/// The inert source: `clipboard_offer` → `None`; `request` →
/// `TransferRequest::ready(Err(TransferError::StaleOffer(id)))`; feedback and
/// conclusion are no-ops. For backends with no transport yet — honest
/// absence, not fake success.
pub struct NullDataTransferSource;
```

**Delivery rides the existing contour and nothing else.** The consumer wraps the
`TransferRequest` in a `BoxedTask` and hands it to `Scheduler::spawn_local`
(`crates/flui-scheduler/src/scheduler.rs:1085`, forwarding to `AsyncDriver::spawn_local`,
`async_driver.rs:222`), receiving the house `TaskToken` (`async_driver.rs:135-171`):
cancel-on-drop, explicit `cancel()`, polled by `drive_async_tasks` on the frame thread
(called from the frame flow at `scheduler.rs:700`). When the producer completes from its own
thread, the future's waker fires, the driver's coalesced frame-request hook wakes the event
loop (`async_driver.rs:106-128`), and the payload is observed on the frame thread — never by
blocking it. Cancellation composes: dropping the `TaskToken` drops the task's future, which
drops the `TransferRequest`, which flips the flag `TransferCompleter::is_cancelled` reports.
This is the audit's requirement verbatim: the `TaskToken` contour is the delivery mechanism;
no second async machinery (and explicitly not `flui-platform`'s tokio-backed `Task<T>` — see
Alternatives).

### 5. DnD enters as `PlatformInput::DragDrop` on the per-window dispatch path

`PlatformInput` (`crates/flui-platform/src/traits/input.rs:120-132`) gains a fourth variant:

```rust
// crates/flui-platform/src/traits/input.rs
pub enum PlatformInput {
    Pointer(PointerEvent),
    Keyboard(KeyboardEvent),
    Ime(flui_types::ImeEvent),
    /// System drag-and-drop. Deliberately NOT a pointer event: during an
    /// external drag the OS owns the cursor, and the gesture-arena semantics
    /// of the pointer pipeline (capture, velocity) do not apply.
    DragDrop(DragDropEvent),
}

/// The push half of the transport: stage-1 arrival and stage-2/6 progress for
/// a drag session over one window. The target's reply half flows the other
/// way, through `DataTransferSource::update_drop_feedback` (§4).
#[derive(Debug, Clone)]
pub enum DragDropEvent {
    /// A drag entered the window. Carries the full offer (stage 1) and the
    /// actions the source currently permits.
    Entered {
        offer: crate::data_transfer::DataTransferOffer,
        allowed: crate::data_transfer::TransferActions,
        /// Logical-pixel position when the backend knows it. The winit
        /// backend stamps the last tracked cursor position
        /// (`platforms/winit/platform.rs:661`) — `None` before any cursor
        /// event, and possibly stale on Wayland where an external drag grabs
        /// the cursor (documented backend limitation, see Open questions).
        position: Option<Point<Pixels>>,
    },
    /// The drag moved while over the window. `allowed` is re-stamped on
    /// every event: modifier-driven copy/move/link changes mid-drag arrive
    /// here, and the target answers them with fresh `DropFeedback`.
    Moved {
        id: DataTransferId,
        allowed: crate::data_transfer::TransferActions,
        position: Point<Pixels>,
    },
    /// The user released and the backend resolved the drop from the cached
    /// feedback (§1). `action` is the effect reported to the OS. The payload
    /// is NOT here — stage 3/4 fetches it lazily; the target calls
    /// `conclude_drop` when done.
    Dropped {
        id: DataTransferId,
        action: crate::data_transfer::TransferActions,
        position: Option<Point<Pixels>>,
    },
    /// The drag left the window or the source cancelled; the offer is retired.
    Exited { id: DataTransferId },
}
```

Why this channel and not a new one: per-window input already flows
`WindowCallbacks::dispatch_input(PlatformInput)` (`crates/flui-platform/src/shared/handlers.rs:401-408`)
— including the reentrancy-deferral queue (`handlers.rs:330-392`) — and `flui-app` already
forwards every `PlatformInput` into the realm as `RealmEvent::Input`
(`crates/flui-app/src/app/runner.rs:1931-1934`). A `DragDrop` variant needs **no new
transport plumbing** across backends, the dispatch queue, and realm routing — though it does
require one new match arm everywhere `PlatformInput` is matched exhaustively, including
`flui-app`'s realm input dispatcher (`binding.rs:1417`), which is therefore in slice-1 scope.
The platform-level `WindowEvent` enum (`traits/platform.rs:368`) is the window-lifecycle
channel (resize/focus/fullscreen) consumed by `PlatformHandlers`, not routed into realm
input — the wrong bus (see Alternatives). The `as_pointer`/`as_keyboard`/`as_ime` accessors
(`input.rs:133-159`) each gain the new arm; exhaustive matches downstream surface every
place that must decide to route or explicitly ignore DnD — that ripple is the point.

**Native protocol mapping** (to be verified in detail when each native backend lands, but
the shape is chosen so no trait/event change is needed):

- **Win32** (`IDropTarget`): `DragEnter`/`DragOver`/`DragLeave`/`Drop` →
  `Entered`/`Moved`/`Exited`/`Dropped`. `DragOver`'s `*pdwEffect` out-param and `Drop`'s
  final effect are answered synchronously from the cached `DropFeedback`. Payload
  extraction after `Drop` returns uses `IDataObjectAsyncCapability` when the source
  supports it; otherwise the backend extracts on the message thread during `Drop` and the
  subsequent `request()` resolves from memory.
- **AppKit** (`NSDraggingDestination`): `draggingEntered:`/`draggingUpdated:` return the
  cached `NSDragOperation`; `performDragOperation:` returns YES iff the cached feedback
  accepts; the pasteboard remains readable for stage-3 fetches afterward.
- **Wayland** (`wl_data_device`): `accept`/`set_actions` are re-sent on each feedback-cache
  update during motion; after the drop, stage 3 reads the pipe (a plain fd read, off-thread
  safe even though the control protocol is loop-affine), and `conclude_drop` sends
  `wl_data_offer.finish`.

**Lock discipline at the winit source** (rule, since a consumer may call
`update_drop_feedback`/`conclude_drop` synchronously from inside an event callback): the
offer-table/state lock is released before any `dispatch_input` call — clone out, then
dispatch, the pattern the existing arms already follow (`winit/platform.rs:659-669`) — and
no `DataTransferSource` method ever calls back into dispatch while holding the table lock.
Re-entering the source from a callback is therefore lock-safe by construction.

### 6. Clipboard migration: one source per backend, sync trait as substrate, quarantine where affinity permits

The `Clipboard` trait (`platform.rs:473-484`) is **not** deleted or made async:

- It remains the low-level backend contract and the ADR-0034 pre-`run()` reachability story
  (`AppBinding::clipboard()`), which some callers legitimately use synchronously at
  bootstrap. ADR-0034's Arc-shaped resolution and install/teardown symmetry are untouched.
- `ClipboardItem` and `write_to_clipboard`/`read_from_clipboard` (`platform.rs:268-279`)
  are frozen as the legacy text path; new consumers use the offer model. Write-side
  multi-representation offering (lazy source-side providers) is deferred — see Slices.
- The *widget-facing read path* moves exclusively to the transport.

**`Platform::data_transfer()` is a required method — no default body:**

```rust
// crates/flui-platform/src/traits/platform.rs
/// The data-transfer transport (ADR-0038). Contract: returns clones of ONE
/// source instance per platform instance — the source owns connection-like
/// state (the offer table), and per ADR-0034 §3 such state must live behind
/// an Arc the platform clones out, never be reconstructed per call.
/// Backends without a transport return `NullDataTransferSource` (inert,
/// honest — not fake success).
fn data_transfer(&self) -> Arc<dyn DataTransferSource>;
```

An earlier draft gave this a default body constructing a fresh clipboard bridge per call.
That shape is rejected twice over (see Alternatives #10): a stateful bridge reconstructed
per call violates the exact ADR-0034 §3 contract ("Reconstructing per call is only safe
when the thing being reconstructed is a stateless handle onto an OS singleton",
`docs/adr/ADR-0034-clipboard-reachability-without-a-platform-handle.md:51`), and two
bridges with disjoint offer tables split the id space — an offer minted by one is
stale-or-aliased in the other, which breaks the §2 single-authority invariant. The cost of
the required method is one small `fn` per `impl Platform for` site (8 sites; the
platform-init stubs return `Arc::new(NullDataTransferSource)`).

**The winit source, `WinitDataTransfer`,** is constructed once at platform init, held in
the backend's `Arc`'d state, and returned by `data_transfer()` as clones. It owns the one
`OfferTable` (§2) — the DnD event pump mints drag sessions through it, and its clipboard
half mints the clipboard offer through it. The clipboard half has two modes, selected at
platform init from the detected display backend, because the U7 affinity audit
(§23) makes one mechanism per platform wrong:

- **Worker mode — winit on X11 and Windows** (backends where the audit does *not* classify
  clipboard access as main-thread-required): a **dedicated clipboard worker thread** owns
  all arboard access. `request()` enqueues a read job; the worker performs the blocking
  `get_text()` and calls `TransferCompleter::complete` from its thread; the waker/frame
  contour (§4) does the rest. The legacy winit `Clipboard` impl is rewired through the same
  worker: `write_text` becomes a fire-and-forget enqueue (returns immediately — the UI
  thread never touches the arboard mutex at all, closing the porosity where a UI-thread
  write would block behind an in-flight hung read), and legacy `read_text` becomes a
  blocking round-trip to the worker (unchanged contract for the bootstrap path; the widget
  path never calls it). Ordering is preserved by the single queue. This deliberately does
  **not** use `background_executor()`: `PlatformExecutor::spawn` for `BackgroundExecutor`
  is `runtime.spawn` on the shared num_cpus tokio runtime (`executor.rs:38-43, 106-111`) —
  a hung, non-interruptible arboard read would permanently eat an async worker shared with
  timers and file dialogs, and tokio's runtime drop joins workers, turning a hung read into
  a process-exit hang. The dedicated worker is instead **detached at platform teardown**:
  teardown signals stop and does not join, so a read hung on an unresponsive clipboard
  source can leak one thread at exit but can never hang exit. At most one read is in flight
  at a time (single worker, single queue): a hung negotiation stalls *subsequent clipboard
  operations' completions* (they queue), never the UI thread.
- **UI-thread mode — winit on Wayland and macOS** (audit §23: clipboard main-thread-required,
  UB-class off-main): the blocking read is **not** moved to a thread. `request()` performs
  the read synchronously on the UI thread at request time and returns
  `TransferRequest::ready(..)`. **This means U18 — the frame stall — is *not* fixed on
  Wayland by this ADR's clipboard bridge**, and cannot be fixed by any thread-shuffling of
  the sync trait without manufacturing a U7 hazard. The honest fix on these platforms is
  the native transport (slice 4): Wayland's `wl_data_offer` pipe read is asynchronous by
  nature (the fd read is thread-safe; only the control protocol is loop-affine), and
  NSPasteboard has main-thread item-provider patterns. The widget-facing *API* is already
  async-shaped on these platforms — only the backend under it is still synchronous — so
  consumers need no change when the native transport lands.

Bridge honesty, stated up front: the sync-clipboard half advertises a single `Text`
representation **without probing** (`has_text()` would block — its default calls
`read_text()`, `platform.rs:481-483`); truth emerges at fetch, where an empty clipboard
resolves to `TransferPayload::Text(String::new())`. Its stale-offer defense is weak by
construction: no change notification exists in the sync trait, so it holds one long-lived
offer with shared fate (§2 lifecycle). Native backends key generations to the OS change
token — that is precisely what the native slices buy.

`AppBinding` gains a `platform_data_transfer` slot mirroring ADR-0034's clipboard slot:
resolved via `platform.data_transfer()` once, before `run()` consumes the box (consistent
with the single-instance contract — the stored Arc *is* the one source),
`set_platform_data_transfer`/`clear_platform_data_transfer` symmetric with
`teardown_platform_realm`, `data_transfer(&self) -> Option<Arc<dyn DataTransferSource>>`
clone-then-release (no guard escapes — SP-6).

### 7. Realm-side capability: owner + handle, on the ADR-0030 template

Widget access follows `TextInputOwner`/`TextInputHandle`
(`crates/flui-interaction/src/text_input.rs:285-316`): a realm-local, `!Send` owner holding
the platform capability, exposed to widgets as a `Weak` handle.

```rust
// crates/flui-interaction/src/data_transfer.rs
use flui_platform::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, TransferActions,
    TransferError, TransferLimits, TransferPayload, RepresentationIndex,
};

/// Realm-layer error. `OwnerGone` lives HERE, not in the platform vocabulary
/// — same layering as `TextInputError` (text_input.rs:291).
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum DataTransferError {
    #[error("realm data-transfer owner is gone")]
    OwnerGone,
    #[error(transparent)]
    Transport(#[from] TransferError),
}

/// Realm-scoped owner. Constructed at realm bind time (the
/// `TextInputPlatformBridge` install point in flui-app) with the platform
/// transport and the realm scheduler's spawner. `!Send` — realm-affine per
/// ADR-0027, asserted like TextInputOwner (text_input.rs:327-328).
///
/// The owner is also the realm's **single drop-conclusion authority**: DnD
/// sessions are per-window, each window belongs to exactly one realm, and
/// only this owner (driven by the realm's DragDrop routing) calls
/// `conclude_drop` — individual widgets never retire offers (§2 lifecycle).
pub struct DataTransferOwner { /* source: Arc<dyn DataTransferSource>  // PORT-CHECK-OK-DYN: platform transport boundary, same shape as PlatformTextInput (text_input.rs:91) */ }

impl DataTransferOwner {
    pub fn new(
        source: Arc<dyn DataTransferSource>, // PORT-CHECK-OK-DYN: see above
        spawn: Box<dyn Fn(flui_scheduler::BoxedTask) -> flui_scheduler::TaskToken>,
    ) -> std::rc::Rc<Self>;
}

/// Completion callback. `Send` because it is moved into a `BoxedTask`
/// (`Pin<Box<dyn Future + Send>>`); it runs on the frame thread when
/// `drive_async_tasks` polls the completed request. See the canonical
/// consumer pattern below for how `!Send` widget state stays uncaptured.
pub type TransferDone = Box<dyn FnOnce(Result<TransferPayload, TransferError>) + Send + 'static>;

/// Weak, realm-local data-transfer capability stored by mounted widgets.
#[derive(Clone)]
pub struct DataTransferHandle { /* owner: Weak<DataTransferOwner> */ }

impl DataTransferHandle {
    /// Stage 1 (clipboard facade): the current clipboard offer, if any.
    pub fn clipboard_offer(&self) -> Result<Option<ClipboardOffer>, DataTransferError>;

    /// Stages 3–5+7: fetch one representation. Spawns the delivery on the
    /// realm scheduler and returns the house cancel-on-drop token — store it;
    /// dropping it (e.g. in `ViewState::dispose`) cancels the transfer.
    pub fn fetch(
        &self,
        offer: &DataTransferOffer,
        representation: RepresentationIndex,
        limits: TransferLimits,
        on_done: TransferDone,
    ) -> Result<flui_scheduler::TaskToken, DataTransferError>;

    /// Stage 2, reply half (drag facade): the target's current hover answer.
    pub fn update_drop_feedback(&self, offer: &DragOffer, feedback: DropFeedback)
        -> Result<(), DataTransferError>;

    /// Stage 6 (drag facade): done fetching; release the session. Routed
    /// through the owner's single-authority conclusion (see above).
    pub fn conclude_drop(&self, offer: &DragOffer) -> Result<(), DataTransferError>;
}

/// Clipboard facade: an offer with no drop action.
#[derive(Debug, Clone)]
pub struct ClipboardOffer { /* offer: DataTransferOffer */ }
impl ClipboardOffer {
    #[must_use] pub fn offer(&self) -> &DataTransferOffer;
}

/// Drag facade: an offer plus the source's currently permitted actions,
/// built and re-stamped by the realm's input routing from
/// `DragDropEvent::Entered`/`Moved`.
#[derive(Debug, Clone)]
pub struct DragOffer { /* offer: DataTransferOffer, allowed: TransferActions */ }
impl DragOffer {
    #[must_use] pub fn offer(&self) -> &DataTransferOffer;
    #[must_use] pub fn allowed_actions(&self) -> TransferActions;
}
```

**The canonical consumer pattern.** `TransferDone` is `Send`, so it must not capture
`!Send` widget internals (`Rc`/`RefCell` `ViewState`) — `move |r| self.text.set(r)` does
not compile, by design. The supported pattern is the ADR-0018 completion discipline, using
exactly two `Send`-safe captures: a payload slot and the `RebuildHandle` (which is
`Clone + Send + Sync + 'static`, `crates/flui-view/src/owner/rebuild_handle.rs:60-85`, and
whose own docs show this shape):

```rust
// init_state — trigger #22: capabilities acquired here, stored:
self.transfers = ctx.data_transfer_handle();
self.rebuild   = ctx.rebuild_handle();
self.pasted    = Arc::new(Mutex::new(None));   // Arc<Mutex<Option<Result<TransferPayload, TransferError>>>>

// later, from an event callback (never from build/layout/paint):
let slot = Arc::clone(&self.pasted);
let rebuild = self.rebuild.clone();
self.paste_token = Some(handle.fetch(
    offer.offer(), index, TransferLimits::default(),
    Box::new(move |result| {
        *slot.lock() = Some(result);                 // Send-safe slot
        rebuild.schedule(RebuildReason::StateChange); // frame-thread rebuild
    }),
)?);
// build() reads `self.pasted`; the !Send ViewState is never captured.
```

If this two-capture pattern proves too noisy in practice, a first-class `!Send` delivery
(owner-mediated completion routing, so the callback could borrow `ViewState` directly) is a
real design problem of its own — it interacts with the bound-drop seams and the frame
phases — and would need its **own follow-up ADR**; this ADR deliberately does not
fake-resolve it inline.

`BuildContext` (`crates/flui-view/src/context/build_context.rs`) gains, next to
`text_input_handle()` (`build_context.rs:131`):

```rust
/// Lifecycle-only capability (trigger #22): acquire in `ViewState::init_state`
/// / `did_change_dependencies`, store, use from event callbacks — never from
/// build/layout/paint. `None` when the realm has no platform transport
/// (headless without an installed source).
fn data_transfer_handle(&self) -> Option<flui_interaction::DataTransferHandle>;
```

Per `AGENTS.md`'s trigger-#22 rule, the same change appends `data_transfer_handle` to the
`capabilities=` list in `scripts/check-frame-capability-scope.sh:49`.

### 8. wasm32/web: explicitly out of this ADR's design scope

The web backend is the one platform where the async transport model is *mandatory*
(`navigator.clipboard.readText()` is a Promise; HTML5 DnD is its own event model), and
none of this ADR's backend machinery applies there: `WebExecutor::spawn` runs its `FnOnce`
inline on the only thread (`platforms/web/executor.rs:23-29`), so no "worker" exists, and
the current `WebClipboard` only reads back this app's own write-cache
(`web/clipboard.rs:5-9`). Day one, `WebPlatform::data_transfer()` returns
`NullDataTransferSource` — inert and honest, not a degraded fake.

The `TransferRequest::channel()` completer makes a real web transport *expressible* — a
`wasm_bindgen` Promise callback can call `TransferCompleter::complete` on the single
thread, no executor involved — but the actual design (permissions prompts, the
`ClipboardItem` web API, HTML5 drag events, what `OfferTable` means when the browser owns
offer identity) **needs its own follow-up ADR**. This ADR only commits to not blocking it:
nothing in the frozen trait assumes threads exist.

### 9. Gate compliance summary

- **SP-6:** every public signature above returns plain data, `Arc`s, futures, or tokens —
  no `MutexGuard`, no public lock fields; `OfferTable` lives behind each backend's private
  state lock.
- **Sync pipeline:** no `async fn` anywhere; `request()`/`fetch()` return pollable values;
  polling happens only in `drive_async_tasks`, outside build/layout/paint.
- **FR-036:** `Arc<dyn DataTransferSource>` appears in `flui-platform` and `flui-app`
  (both outside `fr036_scope`, `port-check.sh:1219-1231` — no marker needed) and in
  `flui-interaction` (in scope — per-site `// PORT-CHECK-OK-DYN:` markers, the exact
  precedent of `Arc<dyn PlatformTextInput>` at `text_input.rs:91`). No allowlist edit.
- **Trigger #22:** new capability token registered in the same change (§7).
- **IDs:** generational `GenId` mechanism reused; single minting authority per platform
  instance (§2); no bespoke ID scheme, no `Identifier::get()`.
- **TaskToken as the only async mechanism:** delivery is `TransferRequest` polled on
  `AsyncDriver`; the clipboard worker is a plain thread completing a oneshot, not an
  executor; `flui-platform`'s tokio `Task<T>` is untouched and unentrenched.

## Alternatives considered and rejected

1. **`async fn` clipboard trait (native `async fn in dyn Trait` or `async-trait`).**
   Creates a second async mechanism next to `AsyncDriver` — the audit forbids exactly this —
   and an executor question the frame-driven driver has already answered. Also collides
   with the sync-pipeline rule the moment any await leaks toward frame phases.
2. **DnD as new variants on the platform-level `WindowEvent` enum** (`platform.rs:368`).
   That channel feeds `PlatformHandlers::invoke_window_event` — window lifecycle, not realm
   input; it would need a parallel realm entry path duplicating what
   `RealmEvent::Input` (`runner.rs:1931-1934`) already provides, and it bypasses the
   per-window reentrancy-deferral queue (`handlers.rs:330-408`) that input correctness
   depends on.
3. **Synthesizing pointer events for drags.** During an external drag the OS owns the
   cursor; `CursorMoved` may not even fire (Wayland grabs it). Pretending a drop is a
   pointer-up would run gesture arenas and velocity trackers on fiction. W3C likewise
   separates drag events from pointer events.
4. **`Event::FileDropped(PathBuf)` minimalism.** The audit's brief warns against this shape
   by name: no offer/negotiation stage means no non-file growth path (HTML, images, custom
   MIME), eager payload, no stale-offer defense, no cancellation. It is the shape every
   toolkit later regrets.
5. **Eager payload in `Dropped`.** Forces allocation and decoding for drops the target may
   reject, and makes a 10 000-file drop (the audit §30 benchmark row) O(payload) at event
   time on the UI thread. Lazy stage-3 requests make cost consumer-driven.
6. **Two transports (clipboard-specific and DnD-specific).** Duplicates negotiation,
   limits, staleness, and cancellation machinery; diverges error types; the audit mandates
   one transport, two facades. The facades are where the semantics differ (drop action),
   and they are thin.
7. **Delivery over `flui_platform::Task<T>`** (`task.rs:74`). It is tokio-`JoinHandle`-backed,
   already flagged `PORT-CHECK-OK-SP3` as a parallel definition pending consolidation,
   absent on wasm32 except for ready values, and not frame-driven — using it would entrench
   the very duplication the marker tracks and constitute the forbidden second mechanism.
   The same reasoning rejects parking blocking clipboard reads on `background_executor()`:
   its `PlatformExecutor::spawn` is `runtime.spawn` on the shared tokio runtime
   (`executor.rs:106-111`), where a hung non-interruptible read starves async workers and
   runtime-drop-joins-workers turns it into an exit hang (§6's dedicated worker instead).
8. **Bespoke `DataTransferId` (plain `u64`) inside `flui-platform`** to avoid the new
   `flui-foundation` dependency. Forfeits the house ABA defense and its tested
   generation-check discipline; the L2→L1 edge is DAG-legal and cheap, and `RealmId`
   already documents platform-adjacent generational identity (`id.rs:725-740`).
9. **Making `Clipboard::read_text` deprecated/removed now.** ADR-0034's reachability story
   (pre-`run()` resolution, `AppBinding` slot) is live and correct; bootstrap-time sync
   reads on backends where reads are cheap are legitimate. Migration pressure belongs on
   the widget-facing surface, not on the backend substrate.
10. **A default `data_transfer()` body constructing a clipboard bridge per call** (an
    earlier shape of this ADR). Rejected: the bridge owns an offer table — connection-like
    state — so per-call reconstruction violates ADR-0034 §3's own rule, and two
    independently minted tables split the id space: cross-table ids *pass* each other's
    generation checks (both start at slot 0/generation 1), so a DnD offer id presented to
    a bridge instance could silently redeem an unrelated clipboard offer. The single
    per-platform source (§2, §6) is the repair.
11. **Post-drop accept/reject (`finish_drop(id, action)` after `Dropped`)** (also an
    earlier shape of this ADR). Unimplementable on every native protocol named as the
    fidelity endgame: Win32 and AppKit must answer effect/acceptance synchronously during
    hover and at drop; Wayland requires `accept`/`set_actions` during motion. The
    cached-`DropFeedback` model (§1, §4, §5) is the repair; it is also how embedders that
    face the same constraint (browser engines) bridge an async app to a sync protocol.
12. **Synchronous feedback as a return value from input dispatch.** Would require
    `dispatch_input` to return target answers inline, but the reentrancy-deferral queue
    (`handlers.rs:330-392`) makes dispatch asynchronous by design — a return channel would
    either bypass the queue or block the message thread. The cached-feedback side channel
    tolerates deferral (§1).

## Consequences

**Positive.** DnD exists at all, with a growth path from file drops to full MIME
negotiation, and the trait/event surface frozen in slice 1 already carries the
hover-feedback stage native protocols require — Win32/AppKit/Wayland backends slot in
without breaking changes (§5 mapping). One offer table per platform instance makes every
observable id redeemable at the one `data_transfer()` source — the "one transport, two
facades" claim is structural, not aspirational. On winit-X11 and winit-Windows, clipboard
reads move off the UI thread onto a dedicated worker whose failure mode is a leaked thread,
not a hung exit or a starved async runtime. Staleness, cancellation, and size limits are
uniform across both facades and reuse audited house mechanisms (`GenId`, `TaskToken`);
every new seam lands behind existing gates (SP-6, FR-036 markers, trigger #22 token).

**Negative / cost.** `flui-platform` gains a `flui-foundation` dependency (one new L2→L1
edge). `data_transfer()` is a required method: all 8 `impl Platform for` sites are touched
(stubs return the inert `NullDataTransferSource`). `PlatformInput` grows a variant — every
exhaustive match in the input path must be touched once, including `flui-app`'s realm
dispatcher in slice 1 (deliberate, but a real diff). **U18 is *not* fixed on winit-Wayland
or winit-macOS by this ADR**: the U7 affinity audit forbids off-main clipboard access
there, so the frame stall remains until native transports land (§6 UI-thread mode) — the
widget API is async-shaped from day one, but the stall moves nowhere on those two. The
winit DnD backend can only approximate the model: no reject/negotiate (winit exposes no
feedback channel — `DropFeedback` is accepted and unused there), positions possibly stale
on Wayland, files only, and the drop-burst boundary is heuristic (see Slices). Full
fidelity requires native backends (Win32 `IDropTarget`, AppKit `NSDraggingDestination`,
Wayland `wl_data_device`), which is exactly the depth the native backends were created
for. The sync-clipboard mode cannot detect external clipboard changes (TOCTOU) until
backends expose change tokens. Slice-1 tests that must gate CI live in `flui-app/tests`
rather than next to the code, because `flui-platform` tests are excluded from CI (see
Slices) — an accepted awkwardness until that exclusion lifts.

**Neutral.** Flutter parity is not at stake: Flutter has no core DnD contract (embedder
plugins) and its `Clipboard` is a `services/` MethodChannel — both sit in the dissolved
zone (`docs/FOUNDATIONS.md:142`) and the ADR-0027 leapfrog sanction covers the realm-scoped
shape. The write side and drag-source (dragging *out* of the app) are consciously deferred,
not designed-by-accident.

## Incremental delivery

**Slice 1 — winit file drops become typed platform events (no widget API, no clipboard
change).** The smallest unit that is honestly verifiable *at the CI gate*:

1. `flui-foundation`: `DataTransferId` in the `ids!` generational block (+ the macro's
   existing doc/test pattern).
2. `flui-platform`: `data_transfer.rs` with the §3 vocabulary, `OfferTable`,
   `TransferRequest`/`TransferCompleter`, `DataTransferSource`, and
   `NullDataTransferSource` (types compile; `TransferImage` declared, image delivery
   unimplemented and *documented* as such — no stub that fakes success).
3. `flui-platform`: `Platform::data_transfer()` required method; all 8 implementors wired —
   winit returns its `WinitDataTransfer` (DnD-capable; `clipboard_offer()` returns `None`
   until slice 2), every other backend returns `NullDataTransferSource`.
4. `flui-platform`: `PlatformInput::DragDrop(DragDropEvent)` + accessor arms
   (`input.rs:133-159`).
5. winit backend: the one `OfferTable` behind the state Arc; arms for `HoveredFile`
   (first path mints the session — with **defensive minting**: a `DroppedFile` arriving
   with no prior `HoveredFile`, which some backends produce, also mints — and emits
   `Entered`; subsequent paths accumulate into the session record),
   `HoveredFileCancelled` (emit `Exited`, retire, bump generation), and `DroppedFile`
   (accumulate). **Drop-burst boundary, honestly:** winit delivers per-file `DroppedFile`
   events with no end marker, and `about_to_wait` cannot *know* a burst is complete — so
   the rule is: at the first `about_to_wait` after ≥1 `DroppedFile`, freeze the
   accumulated list as the offer's `UriList` snapshot, emit `Dropped`, and retire the
   session's *accepting* state. A straggler `DroppedFile` arriving after the freeze mints
   a **new** defensive session (its own `Entered` + `Dropped`), so a pathological
   split burst surfaces as two complete drops rather than one truncated one — a
   documented winit approximation ceiling, removed by native backends whose protocols have
   real drop boundaries. `request()` on a frozen file offer returns
   `TransferRequest::ready(Ok(TransferPayload::UriList(..)))` — genuinely in memory, not
   fake async. `update_drop_feedback`/`conclude_drop` are no-ops (winit cannot reject or
   negotiate — documented); `Dropped.action` is stamped `COPY`, the file-manager
   convention, documented as an approximation. Lock discipline per §5.
6. `flui-app`: the exhaustive `PlatformInput` match in the realm input dispatcher
   (`binding.rs:1417`) gains a `DragDrop` arm that logs-and-drops (stated, not hidden);
   routing itself needs no new plumbing (`runner.rs:1931-1934` forwards all
   `PlatformInput`).
7. **Tests — placed where CI actually runs them.** `flui-platform`'s own tests are
   excluded from CI (AGENTS.md: STATUS_HEAP_CORRUPTION investigation), so any test living
   there is green-by-vacuity at the merge gate. Therefore the gate-visible tests live in
   `crates/flui-app/tests/data_transfer_transport.rs` (flui-app is CI-covered and already
   depends on flui-platform + flui-scheduler): `OfferTable` mint/lookup/retire/generation
   semantics (stale request → `StaleOffer`; cross-generation lookup fails),
   `TransferRequest`/`TransferCompleter` state machine (complete, drop-completer →
   `SourceGone`, drop-request → `is_cancelled`, complete-after-cancel no-op), and a mock
   `DataTransferSource` driving all seven stages including cancel-on-drop through a real
   `AsyncDriver`. These move home to flui-platform when the CI exclusion lifts. winit
   arm-conversion tests live next to the winit code in flui-platform — valuable locally
   (`just test`), explicitly **not** gate evidence, and said so.

**Slice 2 — clipboard transport + per-backend U18 quarantine.** `WinitDataTransfer`'s
clipboard half minting from the same table; the dedicated clipboard worker on X11/Windows
with the legacy `write_text` rerouted through it (porosity fix) and detach-at-teardown;
the documented UI-thread mode on Wayland/macOS (§6 — with the explicit statement that U18
is not fixed there); headless backend wired to its test clipboard; `AppBinding`
`platform_data_transfer` slot with ADR-0034 install/teardown symmetry.

**Slice 3 — realm capability.** `DataTransferOwner`/`DataTransferHandle`/facades in
`flui-interaction` (owner as the single drop-conclusion authority),
`BuildContext::data_transfer_handle()`, trigger-#22 token in
`check-frame-capability-scope.sh:49`, realm routing of `DragDrop` events to a drop-target
resolution seam (feedback loop: route `Entered`/`Moved` → target's `DropFeedback` →
`update_drop_feedback`), first consumer widget (`DropRegion`-style) using the canonical
completion pattern, with harness evidence.

**Slice 4 — depth.** Native backend transports keyed to OS change tokens and implementing
the real feedback loops per the §5 protocol mapping; image/HTML representations;
write-side multi-representation offers (lazy providers); drag-source support; macOS
pasteboard-affinity `debug_assert` (ADR-0034's deferred hazard) aligned with the
event-loop-affinity ADR (U7). **Separately and explicitly: the web transport
(navigator.clipboard, HTML5 DnD) is not a slice of this ADR — it needs its own follow-up
ADR (§8).**

## Open questions

- **Source vanishing mid-transfer.** A Wayland client or source window that exits between
  `request()` and delivery must resolve to `TransferError::SourceGone`, but per-backend
  detection (pipe EOF vs. timeout vs. protocol error) is unverified until a native backend
  exists. The sync-clipboard mode cannot distinguish "empty" from "gone".
- **Timeout.** The `TaskToken` contour has no deadline concept, and a hung clipboard read
  on the dedicated worker stalls all *queued* clipboard operations behind it (§6 — one
  worker, one queue). Should `TransferLimits` gain `max_duration` (it is `#[non_exhaustive]`
  for exactly this), enforced by the source rather than the consumer, with the worker
  abandoning-and-respawning on expiry? Deferred until real Wayland latencies are measured;
  no invented numbers.
- **Backpressure.** Nothing bounds concurrent in-flight requests per offer or per realm.
  A misbehaving widget could spawn hundreds of fetches; whether the owner should cap and
  queue (and at what number) is open.
- **Streaming.** `TransferPayload` is fully materialized; `max_bytes` is the only guard.
  Multi-GB file *content* transfer (as opposed to path lists) needs a chunked delivery
  shape this ADR deliberately does not design — it changes the trait surface and should
  arrive with a real consumer.
- **Security of untrusted input.** Byte limits are the only defense specified. HTML is
  delivered unsanitized (consumer's duty — where is that documented for widget authors?);
  `TransferUri::Path` from an external source is attacker-influenced (traversal,
  UNC/`file://` host tricks on Windows); image decoding bombs are pushed to flui-assets'
  decoders. A follow-up should define the trust boundary in one place.
- **winit position fidelity.** Stamping last-known cursor position is documented-stale on
  Wayland during external drags. Is `Moved` worth emitting at all on winit, or only
  `Entered`/`Dropped`/`Exited` until native backends?
- **Concurrent sessions.** One drag session per window is assumed (matches every OS today);
  the offer table supports more, but realm routing for multi-window simultaneous drags
  (two windows of one realm) is unspecified until the multi-window slices of ADR-0027 land.
- **Clipboard change detection.** Whether `DataTransferSource` should grow an optional
  change-notification (`on_clipboard_changed(callback)`) for offer invalidation, or
  whether polling-at-`clipboard_offer()` stays sufficient — decide when the first native
  transport lands.
- **First-class `!Send` completion delivery** (§7): follow-up ADR if the canonical
  two-capture pattern proves too noisy.
- **Web transport** (§8): follow-up ADR; nothing in the frozen surface may assume threads.
