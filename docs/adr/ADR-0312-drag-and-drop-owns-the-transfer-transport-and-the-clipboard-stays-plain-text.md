# ADR-0312: Drag and drop owns the transfer transport, and the clipboard stays plain text until a widget reads more

- **Status:** Accepted (2026-11-04)
- **Date:** 2026-11-04
- **Supersedes:** [ADR-0038](ADR-0038-data-transfer-architecture.md) as a whole: its transport,
  vocabulary and drag-and-drop input are restated in §1–§5; its clipboard over the transport (§6,
  §7 and the clipboard half of §2) is replaced by §6 and §7; its §8 (the Web out of scope) by §5,
  and its §9 (clipboard reachability) by §6.
- **Related:** [ADR-0152](ADR-0152-capability-seam-revised.md) §5 and §6 (the plain-text
  clipboard's read and write, which §6 keeps); [ADR-0211](ADR-0211-unified-drag-and-drop.md)
  (sessions, the source side, concurrent offers and the Web, which run on §1–§5);
  [ADR-0151](ADR-0151-platform-layer-boundary-and-names.md) §7 (the offer table is the core
  host's; a URI list and an image arrive as `Custom` payloads);
  [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md) (no lock or borrow held
  across user code, the rule §5's dispatch follows)

## Context and problem statement

ADR-0038 put the clipboard and system drag-and-drop on one transport: an offer with a
generational id, read one representation at a time. Drag and drop was built on it; the clipboard
was not. At `vanyastaff/flui@24a496f4449bef02a26d3b67b13f93d5abc29068`:

| Fact | Evidence |
|---|---|
| The transport trait still requires a clipboard offer | `DataTransferSource::clipboard_offer`, `crates/flui-platform/src/data_transfer.rs:492` |
| Every implementation answers `None`: the six hosts' transports, the inert one and three test doubles | `drag/{windows,wayland,x11,android,web,headless}.rs`, `data_transfer.rs:525`, `owner/windows/testing.rs`, `flui-app/tests/data_transfer_transport.rs`, `flui-interaction/tests/drag_sessions.rs` |
| The clipboard is its own capability and reads plain text | `ClipboardRead = Result<Option<Arc<str>>, ClipboardError>` (`flui-platform/src/clipboard.rs:13`), `ClipboardHandle::read_text` (`flui-interaction/src/text_input/clipboard.rs:114`), `NativeClipboard` (`flui-native/src/host/clipboard.rs:17`), ADR-0152 §5 and §6 |
| No widget reads anything but text | `EditableText`'s copy, cut and paste go through `ClipboardHandle::read_text` and `write_text` |
| ADR-0038 describes forms that are gone | §6 and §9: the `arboard` worker, winit and `Arc<dyn Clipboard>`; §8, "the web backend returns `NullDataTransferSource`", against `drag/web.rs` |

ADR-0038 also carries three amendments and a partial replacement (ADR-0082, ADR-0151, ADR-0211,
ADR-0152), so no one record states the rule. Which object carries drag and drop, what does the
clipboard read, and what shape does a richer clipboard read take when a widget needs one?

## Decision drivers

- A contract nobody consumes is a promise nobody tests (ADR-0151, option 11): a method every
  implementation answers `None` is that.
- Polling an offer needs a change notice, which not every host has: the Web ships none, Android
  denies a clipboard read to an application without input focus from API 29, and macOS offers
  only `NSPasteboard.changeCount` to poll.
- One request that names the formats it takes, in order, works on every host: it is
  `NSPasteboard.availableType(from:)`, Java's `DataFlavor` list, the Web's
  `ClipboardItem.types` with `getType`, and Android's `ClipDescription.hasMimeType`.
- Untrusted input is refused by size before it is copied (`AGENTS.md`, "Before each push", item
  6): the request has to carry its limit.
- The clipboard and drag and drop read the same OS objects (Win32 `IDataObject`, Wayland
  `wl_data_offer`, X11 selections, Android `ClipData`), so their payload vocabulary is one; their
  delivery is not.
- A record states the current rule (`docs/adr/README.md`, "Rewrite, don't stack").

## Considered options

1. Drag and drop keeps the transport alone, the clipboard stays plain text, and the shape of a
   richer read is fixed now in the drag-and-drop vocabulary, with no offer ids.
2. Build the rich clipboard on the transport now: `clipboard_offer` returns real offers, read
   through `request`.
3. Build the rich read on `ClipboardHandle` now, before a widget consumes it.
4. Keep ADR-0038 and `clipboard_offer` as they are.

## Decision

Chosen option: 1, because it deletes the one method nobody answers, keeps the clipboard's tested
plain-text surface, and fixes the later read so that it lands by addition, on every host, with
its first consumer.

### §1. One transport, seven stages, for drag and drop

A drop moves through seven stages:

| Stage | Artifact | Who acts |
|---|---|---|
| 1. Offer | `DataTransferOffer` (metadata only), pushed in `DragDropEvent::Entered` | backend |
| 2. Negotiation | the target reads `RepresentationDescriptor`s and sends `DropFeedback`, which the backend caches to answer the OS synchronously | target ↔ backend |
| 3. Request | `DragTransport::request(id, index, limits)` | target |
| 4. Async delivery | `TransferRequest` (consumer half) and `TransferCompleter` (producer half) | backend thread → frame thread |
| 5. Decoding | `TransferPayload`, or `TransferError::Decode` | transport |
| 6. Drop action | resolved at the OS drop from the cached feedback; `conclude_drop(id)` releases protocol resources | backend and UI runtime |
| 7. Completion or cancel | per delivery: drop the request; per offer: retirement and a generation bump | either side |

The payload is lazy: a drop of ten thousand files or a 200 MB image costs nothing until a target
requests one representation. Accepting, refusing and choosing copy or move are hover-time
answers: the target keeps answering "what would you do if dropped now", the backend replies to
the OS from the cached answer and resolves the final action from the same cache, never waiting
on the application. The cache starts at *reject*.

The clipboard is not on this transport (§6).

### §2. One offer table per platform instance

`DataTransferId` is a generational id of `flui-foundation`. A generation check defends only
within one table, so each platform instance owns one `OfferTable` (the core host's, `pub(crate)`
in `flui-native`'s drag model, ADR-0151 §7), inside its one `DragTransport`, and every
`DataTransferId` the application can observe is minted there and redeemable at
`Platform::data_transfer()`. A request for an id the table no longer holds is
`TransferError::StaleOffer`.

- A platform that delivers one OS offer at a time retires a window's live offer when the next
  arrives, so an unfetched drop costs one record per window; a platform that delivers several at
  once (iPadOS) keys each by its own id (ADR-0211 item 5).
- No consumer retires an offer. Cancelling a delivery is dropping its request; retiring an offer
  belongs to the backend and to the UI runtime's one drop-conclusion authority, since a consumer
  that retired one would cancel every other consumer's fetch.

### §3. The vocabulary

Plain data in `flui_platform::data_transfer`, with no lock guard in a public signature. It is
the vocabulary of every transfer, the clipboard's later read included (§7):

- `Mime`: type, subtype and parameter names lowercased, parameter values kept byte for byte
  (RFC 2045).
- `TransferFormat`: `Text`, `Html` (raw; sanitizing is the consumer's), `UriList`,
  `Image { mime }`, `Custom { mime }`.
- `RepresentationDescriptor { format, declared_len }` and `RepresentationIndex`, in the source's
  order of preference.
- `TransferActions` (`COPY`, `MOVE`, `LINK`) and `DropFeedback { accept }`, `None` to refuse.
- `TransferPayload`: `Text`, `Html`, and `Custom { mime, bytes }` for every other type, a URI
  list as the bytes of `text/uri-list` (RFC 2483), an image as the encoded bytes of its container.
- `TransferLimits { max_bytes }` (16 MiB by default, `#[non_exhaustive]`), checked against the
  declared length at the request and the accumulated length during delivery.
- `TransferError`: `StaleOffer`, `UnknownRepresentation`, `TooLarge`, `Decode`, `SourceGone`,
  `Cancelled` (`#[non_exhaustive]`).

### §4. The transport trait

```rust
// flui_platform::data_transfer
pub trait DragTransport: Send + Sync {
    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest;
    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback);
    fn conclude_drop(&self, id: DataTransferId);
}

/// For a host with no drag transport: every request answers `StaleOffer`.
pub struct NullDragTransport;

impl TransferRequest {
    pub fn pending() -> (TransferRequest, TransferCompleter);
    pub fn ready(result: Result<TransferPayload, TransferError>) -> TransferRequest;
}

// flui_native
pub trait Platform {
    /// Required; clones of the platform's one transport.
    fn data_transfer(&self) -> Arc<dyn DragTransport>;
    /* … */
}
```

No method blocks and none is `async fn`. `TransferRequest::pending` makes the pair over one
claim slot, so nothing claims a delivery before the request exists. The request is a `Send`
future polled by the frame-driven async driver; dropping it cancels, which the producer sees as
`TransferCompleter::is_cancelled`. Completing after a cancel is a no-op, and a completer dropped
without completing resolves the request `SourceGone`, so a producer that panics never leaves a
target waiting. `data_transfer()` is required, so the compiler makes every host answer; a host
with no drag transport answers `NullDragTransport`.

### §5. Drag and drop enters as `PlatformInput::DragDrop`

`DragDropEvent` is `Entered { offer, allowed, position }`, `Moved { id, allowed, position }`,
`Dropped { id, action, position }` and `Exited { id }`, for OS sessions only: a drag inside its
window runs on pointer capture (ADR-0211 item 2). It is not a pointer event: during an OS drag the
OS owns the cursor, and gesture arenas and velocity trackers must not run on it. Riding
`PlatformInput` reuses the per-window dispatch and its re-entrancy queue.

A backend dispatches with its table lock released, and no method of `DragTransport` dispatches,
so a target that calls `update_drop_feedback` or `conclude_drop` from its event callback cannot
deadlock (ADR-0287). The Web takes and starts HTML drags (ADR-0211 item 11).

### §6. The clipboard is plain text

The clipboard is a capability of the seam (ADR-0152), not a transport offer. Its surface is the
one ADR-0152 §5 and §6 give, unchanged here:

```rust
// flui_platform::clipboard
pub type ClipboardRead = Result<Option<Arc<str>>, ClipboardError>;
#[non_exhaustive] pub enum ClipboardError { Unavailable, Denied, Timeout, Malformed, TooLarge }
#[non_exhaustive] pub struct ClipboardWrite { /* text, concealed */ }
#[non_exhaustive] pub enum Concealment { Honored, HintOnly, Unavailable }

// flui_interaction, owner-local
impl ClipboardHandle {
    pub fn read_text(&self) -> Completion<ClipboardRead>;
    pub fn write(&self, write: ClipboardWrite);
    pub fn write_text(&self, text: impl Into<String>);
    pub fn concealment(&self) -> Concealment;
}
pub trait ClipboardBackend: 'static {
    fn read_text(&self, answer: Resolver<ClipboardRead>);
    fn write(&self, write: ClipboardWrite);
    fn concealment(&self) -> Concealment;
}

// flui_native, owner-local
pub trait Platform {
    fn clipboard(&self) -> Rc<dyn NativeClipboard>; // required
    fn primary_selection(&self) -> Option<Rc<dyn NativePrimarySelection>>; // X11 and Wayland
    /* … */
}
```

The clipboard belongs to the loop, not to a window or a UI runtime: the runner installs it once
when the loop starts and clears it at teardown, so a live host resource (an X11 or Wayland
connection) never outlives its loop. The primary selection is plain text as well.

### §7. The reserved shape of a richer read

When the first widget reads or writes more than text (a `SelectionArea` copied as HTML, a pasted
image), the clipboard grows by addition, in this shape and no other:

```rust
// flui_platform::clipboard
#[non_exhaustive]
pub enum ClipboardFormat { Text, Html, Media(Mime) } // the cases of `TransferPayload`

/// The formats a read takes, best first, and the most it copies.
pub struct ClipboardAsk { formats: Vec<ClipboardFormat>, limits: TransferLimits }

// flui_interaction
impl ClipboardHandle {
    pub fn read(&self, ask: ClipboardAsk)
        -> Completion<Result<Option<TransferPayload>, ClipboardError>>;
}
impl ClipboardWrite {
    pub fn with(self, payload: TransferPayload) -> Self; // the struct is already non_exhaustive
}
```

- **No offer id.** A read is one request: the host answers the first format of the ask that the
  clipboard holds, `Ok(None)` when it holds none of them. Nothing is minted, nothing is polled,
  and no host needs a change notice.
- **Size before the copy.** A representation whose declared or accumulated size passes
  `limits.max_bytes` answers `ClipboardError::TooLarge` before FLUI copies it, as plain text
  past 64 MiB does now.
- **Sharing is keyed by the ask.** A read joins one in flight only when it asked the same
  formats and limits (ADR-0152 §5); a write still ends the sharing.
- **Existing backends keep compiling.** `ClipboardBackend` and `NativeClipboard` gain the read
  with a default body that answers `Text` from `read_text` and `Ok(None)` for every other format.

Until then no type of §7 exists; `docs/gaps.md` lists the section.

### Consequences

- Good, because a method no host answers leaves every implementation, and a new host or test
  double writes three methods, not four.
- Good, because one record states the drag-and-drop transport and the clipboard; ADR-0038's
  amendments are folded in, and its stale clipboard design is no longer a rule.
- Good, because the later rich read is decided: it is one request that names its formats and
  limit, which every host can answer and which refuses size before copying.
- Bad, because rich clipboard formats stay absent until a widget needs them.
- Neutral, because `DataTransferSource` and `NullDataTransferSource` are renamed
  `DragTransport` and `NullDragTransport`; `Platform::data_transfer()` and the
  `flui_platform::data_transfer` module keep their names, since the vocabulary is shared.

### Confirmation

- The compiler: `DragTransport` has no clipboard method. The `compile_fail,E0599` doctest on
  `DragTransport` calls `clipboard_offer` on a `&dyn DragTransport` and must not compile;
  `data_transfer()` and `clipboard()` are required methods of `Platform`, so a host cannot omit
  either. `a_clipboard_handle_stays_on_its_thread` (`flui-interaction`, E0277) keeps the handle
  owner-local.
- `cargo xtask send-positions` lists `DragTransport`'s `Send` and `Sync` supertraits and fails
  when another appears.
- The transport's tests: `mock_source_drives_all_seven_stages_through_the_async_driver`
  (`flui-app`, §1 and §4), `a_retired_offer_never_aliases_the_next` (`flui-native`'s offer
  table, §2), `a_drop_takes_one_action_the_source_permits` and `headless_drag_contracts`
  (`flui-native`, the headless host's transport), `drop_reaches_only_last_accepting_target` and
  `concurrent_sessions_are_independent` (`flui-interaction`), and
  `drag_and_drop_reaches_the_runtime` (`flui-runtime`, §5), with each host's drag tests
  (`drag::x11::tests`, `wayland_drag`, `ole_drag_and_drop_on_real_windows`).
- The clipboard's tests (§6): `copy_and_paste_round_trip_through_the_headless_clipboard`,
  `reads_asked_while_one_is_in_flight_share_its_answer` and
  `a_concealed_write_reaches_the_host_marked`, in the `capability_seam` table of `flui-testing`.
- §7 has no code, so no test; the change that adds it brings a read of a non-text format through
  `HeadlessHost`, a size refusal before the copy and an ask whose format the clipboard lacks.

## Pros and cons of the options

### 1. Drag and drop alone on the transport; plain-text clipboard; the rich read reserved (chosen)

- Good, because the transport carries what it serves, and the clipboard keeps the surface its
  tests cover.
- Good, because the reserved read needs no change notice and carries its limit.
- Good, because the payload vocabulary stays one, so a payload read from the clipboard and one
  dropped are the same type.
- Bad, because an application that needs rich paste today has none.

### 2. The rich clipboard on the transport now

- Good, because the trait already has the method, and drag and drop's delivery is built.
- Bad, because an offer per clipboard state needs a change notice the Web lacks, Android limits
  and macOS only polls; a source without one holds a long-lived offer whose payload is whatever
  the clipboard holds at fetch time.
- Bad, because the offer table's lifecycle (retire on the next drag, one drop-conclusion
  authority) has no meaning for a clipboard, which nobody drops.

### 3. The rich read on `ClipboardHandle` now

- Good, because the shape of option 1 would exist and could be tried.
- Bad, because no widget reads it: a contract that anticipates its consumers is untested
  (ADR-0151, option 11), and every host would carry readers nobody calls.

### 4. Keep ADR-0038 and `clipboard_offer`

- Good, because nothing changes.
- Bad, because ten implementations keep a method that answers `None`, and the record keeps
  teaching a clipboard design with forms that are gone.

## More information

- `NSPasteboard.availableType(from:)` and `changeCount`:
  <https://developer.apple.com/documentation/appkit/nspasteboard>.
- Android's clipboard read limit from API 29:
  <https://developer.android.com/about/versions/10/privacy/changes#clipboard-data>; a clip's
  types: <https://developer.android.com/reference/android/content/ClipDescription>.
- The Web's `ClipboardItem.types` and `getType`:
  <https://developer.mozilla.org/en-US/docs/Web/API/ClipboardItem>.
- Java's `Transferable.getTransferDataFlavors`:
  <https://docs.oracle.com/en/java/javase/21/docs/api/java.datatransfer/java/awt/datatransfer/Transferable.html>.
- Revisit §4 when `DragTransport` becomes owner-local with `Completion` answers; revisit §7 when
  its first consumer lands, which writes §7's types and moves the section's gap entry out.
