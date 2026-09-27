# ADR-0038: Data transfer — clipboard reachability, representations and system drag-and-drop

- **Status:** Accepted
- **Date:** 2026-07-28
- **Absorbs:** ADR-0034
- **Amended by:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (§4: the transport and its
  vocabulary live in `flui-platform-api`, re-exported at `flui_platform::data_transfer`; §9: the
  `Clipboard` trait moves there too, while the required `Platform::clipboard()` stays on
  `Platform` in `flui-platform`)

## Context

**Clipboard was synchronous, blocking and text-only.** The `Clipboard` trait
is `read_text`/`write_text`/`has_text`. On Wayland and X11 a read is an
asynchronous negotiation with another client over a pipe; the arboard-backed
winit implementation hides it behind a lock held for the whole round trip, so
a UI-thread `read_text()` stalls the frame on another process. On macOS and
Wayland the clipboard is main-thread-only, so moving reads to a thread is not
a fix everywhere.

**System drag-and-drop was absent.** winit's `HoveredFile`/`DroppedFile`/
`HoveredFileCancelled` fell into a catch-all; every "drag" in the UI crates was
an in-app gesture.

**Native DnD protocols demand synchronous hover answers.** Win32's
`IDropTarget::DragOver`/`Drop` return a `DROPEFFECT` on the message thread;
AppKit's `draggingUpdated:` returns an `NSDragOperation` during hover; Wayland
needs `wl_data_offer.accept`/`set_actions` during motion. A design without a
target→OS feedback channel during hover cannot be implemented on any of them.

Flutter is not the reference: its `Clipboard` is a `services` method channel
and it has no core DnD contract. `services` is dissolved into `flui-platform`
capability traits, and thread/window topology is a leapfrog zone (ADR-0027).

## Decision

### 1. One transport, seven stages

Every external datum — a paste, a drop — moves through the same stages:

| Stage | Artifact | Who acts |
|---|---|---|
| 1. Offer | `DataTransferOffer` (metadata only) | backend |
| 2. Negotiation | consumer reads `RepresentationDescriptor`s; for DnD the target sends `DropFeedback`, which the backend caches to answer the OS synchronously | consumer ↔ backend |
| 3. Request | `DataTransferSource::request(id, index, limits)` | consumer |
| 4. Async delivery | `TransferRequest` (consumer half) / `TransferCompleter` (backend half) | backend thread → frame thread |
| 5. Decoding | `TransferPayload`, or `TransferError::Decode` | transport |
| 6. Drop action | resolved at OS drop time from the cached feedback; `conclude_drop(id)` releases protocol resources | backend + realm |
| 7. Completion / cancel | per delivery: drop the request or its `TaskToken`; per offer: source retirement and a generation bump | either side |

Clipboard uses every stage but 6. The payload is lazy: a 10 000-file drop or a
200 MB image costs nothing until a consumer requests one representation.

**Accept/reject and copy/move are hover-time decisions.** The target keeps
answering "what would you do if dropped now"; the backend replies to the OS
from the cached answer and resolves the final action from the same cache,
never waiting on the app. The cache may lag behind deferred input dispatch —
the OS re-queries continuously, so the answer converges without blocking the
message thread. The initial value is *reject*.

### 2. `DataTransferId`: one minting authority per platform instance

`DataTransferId` is a generational ID from `flui-foundation`'s `ids!` macro. A
generation check only defends within one table — two tables both minting from
slot 0 produce ids that pass each other's checks — so:

> Each platform instance owns exactly one `OfferTable`, inside its one
> `DataTransferSource`. Every `DataTransferId` the app can observe, clipboard
> or DnD, is minted there and redeemable at `Platform::data_transfer()`.

`OfferTable` (mint / generation-checked get / retire-and-bump) is one shared
type in `flui_platform::data_transfer`, not reimplemented per backend. A
request for a mismatched id is `TransferError::StaleOffer`.

Offer lifecycle:

- **DnD:** one live session per window; a new drag retires its predecessor,
  so an unfetched drop costs at most one session record per window.
- **Clipboard:** one offer per observed clipboard state; a native backend
  keyed to the OS change token (Windows sequence number,
  `NSPasteboard.changeCount`, Wayland offer identity) re-mints on change. A
  source without change detection holds one long-lived offer whose payload is
  whatever the clipboard holds at fetch time (documented TOCTOU).
- **No consumer-facing offer cancellation.** Cancelling a delivery is dropping
  its request; retiring an offer belongs to the source and to the realm's
  single drop-conclusion authority. Letting any consumer retire an offer would
  cancel every other consumer's fetch.

`flui-platform` depends on `flui-foundation` for the ID — a downward edge.

### 3. Vocabulary

Plain data in `flui_platform::data_transfer`; no lock guard in any public
signature:

- `Mime` — type/subtype and parameter names lowercased, parameter values kept
  byte-for-byte (RFC 2045).
- `TransferFormat` — `Text`, `Html` (raw; sanitizing is the consumer's duty),
  `UriList`, `Image { mime }`, `Custom { mime }`.
- `RepresentationDescriptor { format, declared_len }`, `RepresentationIndex`;
  representations are in the source's preference order, as every platform
  convention treats them.
- `TransferActions` (COPY/MOVE/LINK bitset), `DropFeedback { accept }`
  (`None` = reject).
- `TransferPayload` — `Text`, `Html`, `UriList(Vec<TransferUri>)`,
  `Image(TransferImage)` (encoded bytes; pixel decoding belongs to the
  consumer), `Custom`.
- `TransferLimits { max_bytes }` (16 MiB default; `#[non_exhaustive]`), checked
  against the declared length at request time and the accumulated length
  during delivery.
- `TransferError` — `StaleOffer`, `UnknownRepresentation`, `TooLarge`,
  `Decode`, `SourceGone`, `Cancelled` (`#[non_exhaustive]`). Realm-lifecycle
  errors ("owner gone") live in `flui-interaction`, not in the platform
  vocabulary.

### 4. The transport trait, and no second async mechanism

`DataTransferSource: Send + Sync` has four methods: `clipboard_offer()`,
`request(id, index, limits) -> TransferRequest`, `update_drop_feedback(id,
feedback)` and `conclude_drop(id)`. None blocks and none is `async fn`.
`NullDataTransferSource` is the honest inert source for backends without a
transport.

`TransferRequest::channel()` returns a connected pair. The request is a `Send`
future; dropping it cancels, which the producer sees through
`TransferCompleter::is_cancelled()`. Completing after cancellation is a no-op;
dropping the completer without completing resolves the request with
`SourceGone`, so a crashed producer never leaves a consumer pending.
`TransferRequest::ready(..)` serves data already in memory.

The consumer spawns the request on the realm scheduler and holds the house
`TaskToken`; it is polled by the frame-driven async driver, and the
completer's wake requests a frame. This is the only delivery mechanism — not
`flui_platform::Task`, not the tokio background executor.

### 5. DnD enters as `PlatformInput::DragDrop`

`DragDropEvent` is `Entered { offer, allowed, position }`,
`Moved { id, allowed, position }`, `Dropped { id, action, position }` and
`Exited { id }`. It is deliberately not a pointer event: during an external
drag the OS owns the cursor, and gesture arenas and velocity trackers must not
run on it. Riding `PlatformInput` reuses the per-window dispatch path and its
re-entrancy queue; every exhaustive match must decide to route or ignore DnD.

Native mapping, chosen so no event or trait change is needed later:

- **Win32 `IDropTarget`:** Enter/Over/Leave/Drop → Entered/Moved/Exited/Dropped;
  effects answered from the cache; payload extracted via
  `IDataObjectAsyncCapability` or, failing that, on the message thread during
  `Drop`.
- **AppKit `NSDraggingDestination`:** entered/updated return the cached
  operation; `performDragOperation:` returns YES iff the cache accepts.
- **Wayland `wl_data_device`:** `accept`/`set_actions` re-sent on each cache
  update; the payload is a thread-safe pipe read; `conclude_drop` sends
  `finish`.

**Lock discipline.** A source releases its table lock before dispatching, and
no source method dispatches while holding it, so a consumer calling
`update_drop_feedback`/`conclude_drop` from inside an event callback cannot
deadlock.

**winit approximates.** It exposes files only, no negotiation and no drop-burst
terminator: offers carry one `text/uri-list` representation; `Dropped.action`
is `COPY`; `update_drop_feedback` is a no-op; the burst is frozen at the first
`about_to_wait` after a `DroppedFile`, and a straggler starts a new session, so
a split burst surfaces as two complete drops rather than one truncated one.
Native backends remove each of these limits.

### 6. Clipboard over the transport

The sync `Clipboard` trait stays as the backend substrate; the widget-facing
read path moves to the transport. `Platform::data_transfer()` is **required**
and returns clones of the platform's one source — a default body building a
fresh bridge per call would violate §9's single-instance rule and split the id
space (§2).

The clipboard half of a source has two modes:

- **Worker mode (winit on X11 and Windows).** A dedicated clipboard thread owns
  all arboard access. `request()` enqueues a read; the worker completes it.
  `write_text` becomes a fire-and-forget enqueue, so the UI thread never waits
  behind a hung read. The worker is detached at teardown: a hung read can leak
  one thread at exit, never hang exit. Not the shared tokio executor, where a
  non-interruptible read would starve async workers and turn runtime drop into
  an exit hang.
- **UI-thread mode (winit on Wayland and macOS).** The read stays on the UI
  thread and resolves immediately. The frame stall is **not** fixed there by
  this design; the fix is the native transport (Wayland's pipe read is
  asynchronous; AppKit has main-thread item providers). The API is already
  async-shaped, so consumers do not change when it lands.

The sync-backed source advertises a single `Text` representation without
probing (probing would block); an empty clipboard resolves to empty text.

### 7. Realm-side capability

Widgets reach the transport through a realm-local owner and a weak handle, the
`TextInputOwner`/`TextInputHandle` shape (ADR-0037 §5):

- `DataTransferOwner` (`flui-interaction`, `!Send`) holds the source and the
  realm scheduler's spawner, and is the realm's single drop-conclusion
  authority — widgets never retire offers.
- `DataTransferHandle` offers `clipboard_offer()`, `fetch(offer, index,
  limits, on_done) -> TaskToken`, `update_drop_feedback` and `conclude_drop`,
  over two thin facades: `ClipboardOffer` (no drop action) and `DragOffer`
  (offer plus permitted actions).
- The handle is a lifecycle capability on `LifecycleContext` (ADR-0078).

The completion callback is `Send` because it rides the scheduler's task. It
therefore captures a `Send` payload slot and the `RebuildHandle`, never the
`!Send` view state; `build` reads the slot. A first-class `!Send` delivery would
be its own ADR.

### 8. Web is out of scope

`navigator.clipboard` is a Promise and HTML5 DnD is its own event model; the
web backend returns `NullDataTransferSource`. The completer makes a real web
transport expressible on one thread, but its design (permissions, the web
`ClipboardItem` API, who owns offer identity) needs its own ADR. Nothing in the
trait assumes threads exist.

### 9. Clipboard reachability: resolve the capability, not the platform

`Platform::clipboard() -> Arc<dyn Clipboard>` is a **required** method, so the
compiler makes every backend provide one. The runner resolves it once per loop
at bootstrap, through the owner-thread capability (`OwnerPlatform::shared()`,
ADR-0039), and stores the `Arc` in `AppRuntime`
(`set_platform_clipboard` / `clear_platform_clipboard`, read with
`AppRuntime::clipboard()`, which clones the `Arc` out before returning so a
re-entrant caller never finds the slot locked). The clipboard belongs to the
loop, not to a window or realm.

- **No `PlatformHandle`.** A post-`run()` handle object with
  `clipboard() -> Option<…>` would need a `None` default across every backend,
  turning a compiler-enforced capability into one a backend can forget to
  wire.
- **One instance per platform.** A `Clipboard` that owns connection state (a
  socket, an X11 connection, a session token) keeps it behind an `Arc` the
  platform clones out on each call, so the resolved `Arc` and any later call
  reach the same connection. Reconstructing per call is only safe for a
  stateless wrapper over an OS singleton.
- **Install/teardown symmetry.** Teardown clears the slot, so a live platform
  resource (arboard's X11 connection) does not outlive the loop it belongs to.
- **macOS affinity is enforced by routing.** `NSPasteboard` is main-thread-
  only; `MacOSClipboard` dispatches every operation to the AppKit main thread,
  so callers need no affinity discipline (ADR-0039 §5).

## Implementation status

Built: the vocabulary, `OfferTable`, the request/completer pair,
`DataTransferSource` and `NullDataTransferSource`; `Platform::data_transfer()`
on every backend; `PlatformInput::DragDrop`; the winit file-drop source; realm
dispatch that logs and drops DnD events. The transport's state machines are
tested in `crates/flui-app/tests/data_transfer_transport.rs`. A plain-text
`ClipboardHandle` (`flui-interaction`) over the synchronous `Clipboard`
substrate reaches widgets through `LifecycleContext::clipboard_handle`; its
read is callback-shaped, so callers do not change when the §6 transport makes
it asynchronous. `EditableText`'s copy, cut and paste use it.

Not yet built: the clipboard half of the winit source (`clipboard_offer()`
returns `None`) and its worker/UI-thread modes (§6); the realm owner, handle
and facades (§7) and the first drop-target widget; native Win32/AppKit/Wayland
transports; image/HTML representations; write-side offers and drag sources.

## Consequences

- DnD exists, with a path from file drops to full MIME negotiation, and the
  frozen surface already carries the hover feedback native protocols need.
- One table per platform makes "one transport, two facades" structural.
- Staleness, cancellation and size limits are uniform across clipboard and
  DnD and reuse existing mechanisms (`GenId`, `TaskToken`).
- `data_transfer()` is required, so every `impl Platform` carries one small
  method. `PlatformInput` grew a variant, so every exhaustive match was
  touched.
- The frame stall on Wayland and macOS clipboard reads remains until native
  transports land.

## Alternatives rejected

- **An `async fn` clipboard trait.** A second async mechanism beside the
  scheduler's driver, and an executor question already answered.
- **DnD on the platform `WindowEvent` enum.** That channel is window
  lifecycle, not realm input; it bypasses the per-window re-entrancy queue.
- **Synthesized pointer events for drags.** The OS owns the cursor; gesture
  arenas would run on fiction.
- **`FileDropped(PathBuf)` minimalism, or eager payload in `Dropped`.** No
  growth path to other formats, no stale-offer defense, and O(payload) work on
  the UI thread for drops the target may reject.
- **Separate clipboard and DnD transports.** Duplicates negotiation, limits,
  staleness and cancellation; the semantics differ only at the facades.
- **A plain `u64` id inside `flui-platform`.** Forfeits the tested ABA
  defense to avoid a cheap, legal dependency edge.
- **Post-drop accept/reject.** Unimplementable on Win32, AppKit and Wayland,
  which all decide during hover.
- **Feedback as a return value of input dispatch.** Dispatch is deferred by
  the re-entrancy queue; a return channel would bypass it or block the message
  thread.

## Open questions

- Per-backend detection of a source vanishing mid-transfer (pipe EOF, timeout,
  protocol error).
- A delivery deadline: whether `TransferLimits` gains `max_duration`, with the
  worker abandoning and respawning — decided once real Wayland latencies are
  measured.
- Backpressure on concurrent fetches per offer or realm.
- Streaming for multi-gigabyte file contents; `max_bytes` is the only guard.
- The trust boundary for untrusted input (unsanitized HTML, attacker-shaped
  paths, image decode bombs), stated in one place.
- Whether sources grow clipboard change notification, or polling
  `clipboard_offer()` stays sufficient.
