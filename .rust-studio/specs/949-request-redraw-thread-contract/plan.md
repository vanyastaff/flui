# #949 — state `PlatformWindow`'s thread rule where implementors read it

## What the first draft of this plan got wrong

The first draft proposed making `request_redraw` any-thread-callable with each backend
marshaling internally. Two independent reviews rejected it, and both were right:

- **The direction is already settled, in the opposite direction.** ADR-0045 decision 5:
  "The raster side never calls `PlatformWindow::request_redraw`, **on any backend**. Its
  wake hook pushes onto a channel and pokes a relay; the platform's event-loop waker
  drains that channel **on the owner thread**." Its rejected-alternatives list contains
  the draft's own Decision D almost verbatim: "Let the raster thread call
  `request_redraw` directly on backends where 'it works'. Rejected."
- **Two of the draft's cited defects had already been fixed.** It quoted issue #949's
  body instead of reading the tree. `crates/flui-engine/src/raster_owner.rs` no longer
  endorses the call — it says "**Do not call `PlatformWindow::request_redraw` from it.**
  This doc used to offer that as an example, and it is the one thing the hook must not
  do." And `docs/runtime-contract.toml` already carries `CORRECTION (2026-09-06, issue
  #949)` making exactly the correction the draft listed as owed.
- **The relay is one verb on an existing lane, not a per-backend mechanism.**
  `PlatformProxy`/`ProxyTransport` (`crates/flui-platform/src/traits/owner.rs`) is the
  recorded cross-thread-to-owner lane, with two verbs today (`open_window`,
  `request_quit`) and the discipline "a verb joins when a worker-side consumer exists".

## What survives, and is worth doing

The real finding stands and is untouched by the above: **the trait itself states no
thread rule.**

    /// Request a redraw
    fn request_redraw(&self);

Every other record in the tree has settled this — ADR-0045 decision 5, the corrected
`runtime-contract.toml` entry, the corrected `raster_owner.rs` hook doc, and
`macos/window.rs`'s own SAFETY block. The one place an implementor or caller actually
looks says nothing. That is why two live violators exist and why a third can land
tomorrow without tripping anything.

And it is not one method. Scoped to the trait body — `awk '/^pub trait
PlatformWindow/,/^}/' | grep -c '^    fn '` → **48**; a file-wide count says 51 because it
also counts the test module's mock impl. Of those 48, only `close()` says anything about
threads.
Per-method prose does not scale to 48. Both reviewers independently proposed the same
structural fix, which is what this plan adopts.

## Decisions

**A. One trait-level `## Thread affinity` section**, stating the default once —
owner-thread-only unless a method says otherwise — plus the explicit exception list, and
naming the lane a worker uses instead (`PlatformProxy`). The existing `close()` note and
the callback-registration rule fold into it rather than a fourth paragraph joining two
inconsistent ones. Cites ADR-0045 decision 5 and ADR-0039 §3 so the rule is traceable to
the record that decided it.

**B. `request_redraw` gets the rule explicitly**, since it is the method with live
violators and the one #949 is about.

**C. Record the two live violators**, with citations, as known-open:
- `FrameWakeHandle::wake_frame` (`flui-app/src/app/runtime.rs`) — fired by the scheduler's
  `on_frame_scheduled` hook and by an async task's waker, from whatever thread completed
  the future.
- The AccessKit activation listener (`flui-app/src/app/presentation.rs`) — those callbacks
  run on the adapter's own thread (`platforms/linux/accessibility.rs`). **This one the
  first draft missed entirely**, and it is on a backend CI executes.

**D. ~~Assert on the counter the existing test discards.~~ REPLACED — measured false.**
The premise was that `a_cross_thread_frame_request_reaches_the_realms_platform_wake`
(`flui-app/src/app/ui_realm.rs`) collects redraw evidence and throws it away. Measured:
the counter is **0** there. That test installs a plain counting closure as the realm's
`wake`, not the production `FrameWakeHandle`, so nothing pokes the window on that path;
`_calls` is an unused element of a shared helper's return tuple, not discarded evidence.
Asserting on it would pin something unrelated to #949.

**D′. Pin the violation at the site that actually has it.** `AppRuntime`'s
`frame_wake_callback` — the `Send + Sync` closure production installs — fired from a
worker with a window installed. `TestWindow` gains a record of which thread each
`request_redraw` ran on, because a counter cannot distinguish a conforming call from a
violating one. The test asserts the **violation**, is green because the violation is
present, and documents that it must be re-targeted rather than deleted when the relay
lands.

**E. Update `docs/runtime-contract.toml`'s `thread_affinity`** for the
`PlatformWindow` surface. It records this trait's thread contract and would go stale the
moment the trait doc states one.

## Explicitly NOT in this change

- **The `PlatformProxy` redraw verb** (ALT-1 / ADR-0045 decision 5's actual relay). It is
  the fix, and it needs transports for the lane-less backends — `ClosedTransport` returns
  `Unsupported` for headless, macOS, Win32 and android today. ADR-0045's own table calls
  that "real work on three backends, not a wiring exercise". Scoped to #559/#551, not
  guessed at here.
- **Flipping the call sites to the lane.** Without the transports above, an owner-thread-
  only `request_redraw` turns a working-but-unsound wake into a dead one on every
  lane-less backend — a hang traded for unsoundness.
- **`debug_assert_appkit_main_thread` on macOS's `request_redraw`.** Its own doc already
  says why: the off-thread path is reachable in ordinary production use today, so the
  assert would abort correct-by-current-design programs. It belongs there when the relay
  lands, not before.
- **A parked/deferred headless redraw.** Refuted: its only non-test consumer already
  fail-closes (`realm_dispatch.rs` rejects realm callbacks on a non-owner thread and logs
  it), no backend parks a redraw, and nothing on the `FLUI_HEADLESS=1` path can obtain a
  drive handle. It would replace a loud rejection with a silent park.
- **macOS libdispatch marshaling.** Unverifiable unsafe (manual retain/release across an
  `extern "C"` trampoline, a Rust-panic-into-libdispatch abort boundary) on a backend that
  is type-checked and never linked or executed. Strictly worse failure mode than the
  currently-documented hazard, and unnecessary under the recorded direction.

## Verification

This change is docs plus one assertion, so the evidence is correspondingly modest and
must be reported that way:
- `just ci` green.
- The reactivated assertion fails if the counter is not incremented — i.e. it is a real
  oracle for the thing it names, not a tautology. Demonstrate by inverting it.
- No claim that #949 is closed. Its core defect — the unsound cross-thread call — needs
  the lane work above and stays open with the remainder scoped.

## Risk

The main one is that stating a rule two live call sites violate could read as
"documented, therefore fine". Mitigated by Decision C naming both violators inline and by
Decision D's assertion pinning the violation where a future fix must confront it.
