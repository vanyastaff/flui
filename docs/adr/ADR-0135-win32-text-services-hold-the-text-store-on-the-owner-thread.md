# ADR-0135: Win32 text services hold the text store on the owner thread

- **Status:** Accepted (2026-10-06). §1–§3 and the owner's queue in §4 are implemented: every
  Win32 window activates its text services as it is created and offers them as its host
  (`WindowsWindow::text_store_host`); the other production backends answer `None` (a headless
  window offers one only when a test asks).
- **Date:** 2026-10-06
- **Implements part of:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) §4 (owner-thread
  storage, step 1; the owner-proof shape of step 2, for one capability)
- **Related:** [ADR-0090](ADR-0090-ime-pull-text-store-contract.md) (the store contract),
  [ADR-0142](ADR-0142-text-store-commit-gate-and-owner-code-containment.md) (resolving a
  composition, the pull host and owner-code containment: items 4, 7 and 8),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no new global), [ADR-0127](ADR-0127-exceptional-path-retention.md) (retirement on failure)

## Context

ADR-0090 made a text field's document a store the platform pulls from: an `Rc<dyn TextStore>`
that lives on the owner thread. Windows text services (TSF) are the pull platform, but no backend
could hold a store: the per-window contract, `PlatformWindow`, and its push capability,
`PlatformTextInput`, are `Send + Sync`, so a method on either that hands back or accepts an `Rc`
would be a rule in a doc comment, not in a type. The presentation reached the store only through
`TextInputOwner::active_store`, a hidden accessor nothing in production called.

ADR-0082 §4 step 2 plans an owner-minted window registrar (`OwnerPlatform::window(..) ->
OwnerWindow`) that proves the thread for every window capability. It does not exist yet. The text
services spike showed what the connection must survive: a call from FLUI into TSF can come back
into the store and from there into application code that moves focus or ends a composition
again, and `TerminateComposition` fails while the frame transaction refuses locks, so the
composition must still end somewhere.

## Decision

1. **The pull side is `TextStoreHost`** (`flui_platform_api::text_store`): `focus_store(Option<Rc<dyn
   TextStore>>)` tells the window which field's store takes input, and
   `complete_composition(&Rc<dyn TextStore>)` ends the platform's composition in that store. The
   host compares it by identity (`Rc::ptr_eq`) with the store it was last told to focus, queued
   focus changes included, and answers `CompositionEnd::Committed`, `Abandoned` (the platform
   could not), `Deferred` (the request arrived inside a platform call into a store; the host
   keeps the store, ends the composition when that call returns, and commits it in place itself
   if the platform then cannot or has shut down), `TextStoreHostError::NotFocused` for any other
   store, or `TextStoreHostError::Unavailable` once the window's text services are gone. On
   `Abandoned` and either error the caller commits in place:
   `text_store::commit_composition_in_place` clears the composing range under an asynchronous
   lock and keeps the text, the one function every caller uses; it returns the lock's outcome,
   and a `Deferred` commit is accepted work its caller owes a later run. The host is shared as
   `Rc<dyn TextStoreHost>` and is not `Send`.
2. **Only owner-thread proof reaches it.** `OwnerPlatform::text_store_host(&Arc<dyn HostWindow>)`
   reads the window's host through `HostWindow::text_store_host`, whose argument, an
   `OwnerThreadToken`, only `OwnerPlatform` can build: its type is public in a private module, so
   the method cannot be called outside `flui-platform`. The token proves the platform's owner
   thread, not the window's: the Win32 window reads its host through `with_window_context`, which
   refuses a thread other than the one that created the HWND. This is the interim home until ADR-0082
   §4 step 2's owner window exists, when the method moves there. `PlatformWindow` and
   `PlatformTextInput` do not change. The runner reads the host once, beside the accessibility
   bridge (`runner::presentation_window`), and `PresentationWindow` carries it, so a
   `PresentationWindow` is not `Send`.
3. **Win32 keeps the text services in the window.** The `ITfThreadMgr`, the empty document and
   the focused field's document (one state: nothing, a field's document, or shut down) live in
   the window's `WindowContext`, on the owner thread, with
   no `static` or `thread_local!`; no `windows::*` type leaves `flui-platform`. They are
   activated as the window is created, once its `WindowContext` is installed, and the window
   offers them as its host from then on; a window whose activation failed answers `None`, and
   its text input takes the `WM_CHAR` path. `WM_DESTROY` deactivates them after the window's
   close callbacks (the presentation's close, which unfocused its field) and before the context
   retires; inside a TSF call into a store the deactivation waits for that call to return. The
   teardown's application-code failures are logged, contained, since the window procedure must
   not unwind. A host the presentation still holds then answers `Unavailable`.
4. **The presentation's owner orders host calls and never nests them.**
   `TextInputOwner::new(TextInputBackend)` takes `Push(Arc<dyn PlatformTextInput>)`,
   `Pull(Rc<dyn TextStoreHost>)` or `Unsupported`, held in one `RefCell`; the presentation picks
   `Pull` when the window has a host. An attach queues the store's focus, the active detach and
   close queue `None`, and `complete_composition` (the owner's, and the handle's for its own
   token) queues a completion that captures its store. The queue drains when no owner call on
   the host is running and the frame transaction is closed, and at the anchor before any deferred
   grant, so a call that reaches the owner again from inside the host waits for the outer one, and
   a completion asked for in a frame reaches its field even after a detach. The host's calls are
   platform code that reaches application code, so each goes through `OwnerCalls` like any owner
   code ([ADR-0142](ADR-0142-text-store-commit-gate-and-owner-code-containment.md) item 8): the
   operation, its store and the host clone are taken from the queue before the call, and the
   first failure in time is authoritative (a failure parked before the call, then one a grant
   settled inside it parked, then the call's own panic, then one its unwind's cleanup parked;
   a close orders its host calls the same way). On the owner's turn (a completion, the
   anchor) the presentation's gate is taken before the call and after it; an attach or a detach
   leaves a failure the gate already held for that turn, and an attach parks what its host call
   raised there too, behind it, and returns the token (ADR-0142 item 8). A host operation that
   panics releases its values and the queue goes on: the completed store is retained once the
   scope has failed, the host clone (framework-owned) is released, contained, even then; the
   first failure propagates once the rest (and, at the anchor, the deferred grants) have run. An
   `Abandoned` answer or an error commits the composition in place; a push or storeless owner does that directly. Close applies the
   queued operations in order (a focus change queued before a completion moves the host first,
   so the completion reaches its own store), then tells a host left focused `None`; after a
   failure it retires the rest and still sends the `None`. An owner dropped without a close only
   does the latter. The host queues what reaches it inside a
   platform call that did not come from the owner (the Win32 text services count those entries,
   `entry_depth`), and answers `Deferred`.
5. **One TSF document per focused field**, associated with `AssociateFocus` and focused with an
   explicit `SetFocus`; a document is never closed under its own COM entry. Each COM entry catches
   panics; a `BUG:` panic inside the adapter poisons the document, and after three the window
   falls back to `WM_CHAR`.
6. **Geometry and lock translation are pure functions** in `flui_platform::shared`, with no
   `windows` type, tested on every host: `text_geometry::range_rect_to_screen` with outward
   rounding, and, once written, the ACP flag and error translation beside it.
7. **No IMM32.**

## Alternatives considered

- **A method on `PlatformWindow` answering `None` off the owner thread.** The runtime would wire
  itself, but a `Send + Sync` trait handing out an `Rc` makes the thread rule a comment.
- **A method on `PlatformTextInput`.** Against ADR-0090's decision that the push capability is
  not the contract.
- **The host alone queues reentry.** Correct for Win32, but every other host (and every test
  double) would have to repeat it, and a completion asked for inside a frame would reach TSF while
  locks are refused and come back `Abandoned` every time. The owner queues what it can see; the
  host queues only what the owner cannot.
- **Two variants, no `Deferred`.** A host asked from inside its own store call cannot answer
  before the call returns; an answer of `Committed` would be a guess, and `Abandoned` would make
  the owner clear a composition the platform is still about to commit.

## Consequences

- `TextInputOwner::new` takes a `TextInputBackend` (a breaking change, `changelog.d/`), and
  `TextInputOwner::active_store` and the UI runtime's test-only `active_text_store` are gone. Tests reach
  the focused store through the host the headless window offers:
  `HeadlessWindow::with_text_store_host` and `flui_testing::RecordingTextStoreHost`;
  `widgets::harness::mount_with_ime` is pull-model and `mount_with_push_ime` keeps the push
  recorder. A presentation without input-method support passes `TextInputBackend::Unsupported`.
- A platform-level test makes a headless window pull-model with
  `HeadlessPlatform::with_text_store_host`, which is how the runner's
  `runner::presentation_window` is tested.
- `TextInputHandle::complete_composition` is called by `EditableText` on blur, on a pointer-down
  on the field and on a paste (ADR-0142 item 4). `TextInputOwner::complete_composition` has no
  production caller yet: the runtime's pointer-down and close hooks call it.
- When ADR-0082 §4 step 2 lands, `text_store_host` moves to the owner window and the token goes.

## Verification

- `flui-platform` `traits::owner::tests::the_owner_platform_reads_a_window_s_text_store_host`
  and the `compile_fail` examples on `HostWindow::text_store_host` (§2).
- `flui-interaction` `the_owner_drives_its_text_store_host` (focus follows attach, frame and
  reentry queueing, the answers, a completion that outlives its detach, close, a close that
  applies a queued focus change before its completion, push and no backend),
  `two_panicking_host_operations_in_a_close_still_unfocus` (a close keeps its ADR-0123
  containment) and
  `text_input_retirement_allows_reentry_and_preserves_recovery`'s pull-host row (issue #1052's
  reentrant retirement on a pull owner) (§1, §4).
- `flui-widgets` `owner_code_is_contained_at_every_point`'s `host:` rows, one child process
  each: a panicking focus change with the queue behind it, a focus change and a completion both
  panicking at the anchor, a host call after a parked failure, an attach's host call that parks a
  failure and then panics (the token is returned, the next turn reports the parked failure
  first), one that detaches and then panics (its store's destruction panics), and one
  that closes the owner and then panics (the host's own destruction panics), a completion, at its
  turn and at a close, whose unwind parks a failure (the host's panic is raised), each followed
  by the next operation (§4).
- `flui-runtime` `a_window_with_a_text_store_host_takes_input_through_it`, `flui-app`
  `runner_bootstrap_matrix`'s
  `presentation_window_hands_a_pull_window_s_host_to_its_presentation` and `flui-widgets`
  `focus_gain_and_loss_reach_the_store_host` (§2, §4).
- `flui-platform` `text_services::tests::the_text_services_answer_a_completion_for_its_store`,
  Windows only, against the real TSF in a hidden window: a completion queued behind a COM entry
  commits its store in place after a shutdown or without a document, and a store the host does
  not serve, or any store after shutdown, is refused (§1, §3); a teardown whose observer
  retirement and diagnostic both panic stays inside the COM entry, a completion TSF refuses
  reports its teardown's failure ahead of its in-place recovery's, a teardown whose `Pop`
  diagnostic panics after its observer retirement did raises the retirement's failure, and a
  protection change reaches TSF as a new context, ending a composition TSF holds in the old one
  first (committing it in place when TSF refuses) (§4, §5); a window destroyed inside a TSF
  call shuts its text services down when that call returns (§3).
- `flui-platform` `text_services::tests::a_window_offers_its_text_services_as_its_host`,
  Windows only: a real `WindowsWindow` offers its own text services as its host, a focused store
  gets a TSF document of the window, and after `WM_DESTROY` the host answers `Unavailable` (§3).
- The Win32 text services' opt-in probe,
  `cargo test -p flui-platform --lib text_services -- --ignored --nocapture`, run 2026-10-06 with
  Microsoft IME ja-JP at 100 %: activation, `toukyou` → 東京, `TS_S_ASYNC` behind a shut gate,
  candidate placement with both negative controls, `TS_E_NOLAYOUT` retries, and `Committed` /
  `Abandoned` with the gate open and shut (§3, §5, §6). Run again 2026-10-06 at 100 % through
  the host the window itself offers: the same results, and the host answers `Unavailable` after
  the window's `WM_DESTROY` (§3).
