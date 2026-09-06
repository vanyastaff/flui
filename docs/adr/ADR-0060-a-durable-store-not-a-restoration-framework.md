# ADR-0060: The runtime owns a durable store; what goes in it is the application's business

- **Status:** Accepted
- **Date:** 2026-09-06
- **Relates to:** [ADR-0049](ADR-0049-task-worker-service-lifecycles.md) (the
  lifecycle layer this extends), [ADR-0047](ADR-0047-unified-execution-services.md)
  (`ExecutionServices`' IO lane — that number is used twice, see #947),
  [ADR-0027](ADR-0027-owner-affine-ui-realms.md) (concurrent realms, which this
  must not assume away)
- **Supersedes nothing.** Closes the design half of issue #558's last open
  criterion.

## Context

Every other criterion on #558 concerns an *agreed* shutdown. This one does not:
*"forced termination recovers continuously journaled state."* Nothing in the tree
addresses it — `grep -rn journal crates/` returns one comment, in
`app/close_request.rs`, deferring to "the journaled-state slice".

### What the criterion actually asks for

The first draft of this record answered it by porting Flutter's
`RestorationManager` — a bucket tree, restoration IDs, a `BuildContext`
capability — and redirecting its bytes to disk. That was roughly three times the
scope asked for, and the source says so.

The criterion is a **verbatim import from the reference-application section** of
`docs/research/2026-08-01-runtime-architecture-execution-plan.md:353-368`, the
section that becomes issue #564. Its Solution reads:

> Build the editor-shaped proof … shared documents/configuration, independent
> windows, background indexing, embedded realtime wgpu viewport, **restoration**,
> unsaved-close deferral … **Keep product-specific features in the example, not
> framework core.**

and its Acceptance line is *"forced termination recovers journaled state"* —
without the adverb. "Continuously" was added when the line was copied onto #558.
Corroborating, #558's own Design bullet is *"Journal recoverable state before
shutdown"*, and [ADR-0049](ADR-0049-task-worker-service-lifecycles.md) names the
deferred slice "journaled recoverable state". Nothing in any of the four sources
asks for a restoration framework.

So the runtime owes the **mechanism**, not the policy: somewhere durable to put
bytes, a rule for when they reach the disk, and a bounded flush before
termination. What an application journals — a document, a route stack, an index
cursor — is the application's business, and the plan says to keep it in the
example.

## Decision

`DurableStore`: a small, typed, crash-safe key-value store the runtime provides,
written off the frame thread, flushed within a deadline at shutdown. No bucket
tree, no widget tier, no restoration IDs.

### D1 — Two-slot ping-pong with a sequence and a checksum, not atomic replace

The store keeps **two** files per key-space, `state.0` and `state.1`. Each holds
`(seq: u64, crc32: u32, payload)`. A write goes to the slot that does *not* hold
the highest valid `seq`; a read takes the highest `seq` whose CRC validates.

The obvious alternative — write `state.tmp`, fsync, rename over `state` — was the
first draft's answer and is rejected on three counts, one of which was a defect
in that draft:

- **It has an ordering that is easy to get wrong and impossible to test here.**
  The correct sequence is write tmp → fsync tmp → **rename** → **fsync
  directory**; the draft had the directory fsync before the rename, which leaves
  the *rename* undurable and silently rolls back to the old snapshot after a
  power loss. Ping-pong has no directory entry to make durable and no ordering to
  get wrong.
- **It rests on rename semantics this repository cannot verify.** Only two of the
  eight `Platform` backends are exercised on their own OS. Whether
  `std::fs::rename` maps to `MOVEFILE_REPLACE_EXISTING`, what happens when an
  antivirus or the search indexer holds the destination open without
  `FILE_SHARE_DELETE`, whether `File::open(dir).sync_all()` is even expressible
  on Windows (std does not open directories with `FILE_FLAG_BACKUP_SEMANTICS`),
  and how NFS silly-rename and overlayfs copy-up behave — none of it is checkable
  from here. Ping-pong needs none of those answers: two `File::create`s, a write,
  an `fsync`, and a comparison.
- **It leaves a recovery arm with no producer.** An atomic rename cannot produce
  a torn `state`, so "recover from corrupt" would be unreachable except by
  deliberate damage — yet the draft listed it. A CRC gives that arm a real
  producer (a half-written slot after a kill) and turns "parses into the wrong
  state" into "detected".

Cost, stated: one extra file and twelve bytes per key-space.

### D2 — The store is eventually-current, and the bound is one write latency

The frame thread serialises into a buffer and hands it to `ExecutionServices`'
IO lane. It never touches the filesystem. One snapshot is pending at a time;
a newer one supersedes an unwritten older one.

The first draft justified that coalescing by analogy to the `Worker` ring in
`lifecycle.rs`. **The analogy is unsound and is withdrawn.** In the `Worker` a
dropped intermediate input is never observable — the consumer reads only the
newest result, which is computed anyway. In the store the dropped value *is* the
observable: after a kill, disk holds the last *completed* write, so dropping
snapshot N drops the only copy that would have survived a kill during N+1's
write. Coalescing does not merely save work; it widens the loss window.

The honest statement, which the word "continuously" obscures: **the store is
eventually-current with a staleness bound equal to one write latency.** On a slow
device that is tens of milliseconds while a 120 Hz frame produces a snapshot
every ~8 ms — tens of frames, not one. Latest-wins is still the right call for a
snapshot, because an intermediate state nobody will restore to is not worth a
write; but the bound belongs in the contract, not in a footnote.

A payload large enough that one write cannot finish inside the coalescing
interval leaves the store permanently one write behind. Slice 1 therefore
**caps the payload** and rejects an oversized one with a typed error, rather than
carrying a "keep it small" premise nothing enforces. A consumer that outgrows the
cap wants a database, and the cap is where it finds that out.

### D3 — `Platform::data_dir()`, defaulted to "this platform has none"

There is nowhere to write today: `Platform::app_path()` returns the
**executable's** path, which may be read-only and is shared between users.

`Platform` gains `data_dir()` as a **defaulted** method returning
`Err(PlatformError::…)`. The in-tree precedent is
`set_exit_policy_hook`, recorded in `docs/runtime-contract.toml` as "a new
default-no-op trait method, overridden by the winit and headless backends only".

Defaulted, not required, because of what the backends actually answer today.
The first draft claimed "every backend already answers the executable question
with a native call". **That is false, and it is the kind of premise an Accepted
record must not carry:** `windows` and `macos` do; `winit` uses
`std::env::current_exe`; `web` returns a URL; `headless` returns
`/mock/app/path`; `linux` and `ios` are `unimplemented!()`; and `android`
hardcodes `/data/local/tmp` — a shell-scratch directory, world-readable, not
app-private storage. Requiring `data_dir()` would invite four more answers of
that shape, and `scripts/port-check.sh` exempts `platforms/{linux,ios,android}/`
from the `unimplemented!()` trigger, so they would pass every gate in the repo.

A typed `Err` is a better answer than a wrong path. An application on a platform
that returns it gets a store that reports itself unavailable, which is
recoverable; an application that silently journals a document into
`/data/local/tmp` does not.

The draft also rejected the `directories` crate on the grounds that none was in
the workspace. **Also false**: `dirs 6.0.0` is already a workspace dependency
through `flui-cli` (`crates/flui-cli/src/config.rs`), so the supply-chain cost is
paid and already inside `weekly.yml`'s advisory re-check. Backends that want it
may use it; the trait shape is what this record fixes, not the lookup.

**wasm32 answers `Err`.** `flui-platform` and `flui-app` are both inside
`wasm-check`, and the web backend has no filesystem; `ExecutionServices::shutdown`
cannot revoke queued microtasks there either, so there is no flush leg to run.
A durable store on the web is `IndexedDB`, which is a different mechanism with a
different contract — a separate record, not a `data_dir()` that lies.

### D4 — The flush leg is bounded by a deadline, and the deadline wins

`realm_dispatch.rs` already runs `shutdown_lifecycles(SERVICE_SHUTDOWN_DEADLINE)`
then `shutdown_execution(EXECUTION_SHUTDOWN_GRACE)`. The flush goes between them:
it is the last point at which a write can still reach a live IO lane.

It carries its own deadline, and **on expiry teardown proceeds without it.**
`fsync` on a stalled device — a sleeping external volume, a Windows profile on a
network share — blocks uninterruptibly, and #558's own sibling criteria require
that shutdown deadlines be deterministic and that shutdown never block on
optional work. A durability mechanism that can hang teardown fails the issue it
was written for.

`ExecutionServices::drop` calls `shutdown_background()` — non-blocking, no join —
so a panic during teardown abandons the lane with a write in flight. That is the
same loss window D2 already names, and it is why the guarantee is stated as
eventually-current rather than as per-frame durability.

### D5 — Write failures are typed, latched, and reported once

`ENOSPC`, `EROFS`, `EACCES`, `EIO` on fsync, a `data_dir()` that cannot be
created. None may panic (`docs/PANIC-POLICY.md`). None may retry every frame — a
per-frame failing fsync is a per-frame stall. The store latches the failure,
reports it once through the embedder's handler, and stops attempting until the
next explicit save. The distinction that matters to a caller is *transient*
(retry later) versus *terminal* (this store will not work), and the error type
carries it.

## Slice 1, and why it has a caller

**Window geometry.** The store ships with the runtime's own consumer: a window's
size and position, restored across runs. Every desktop toolkit does this; it is
framework core rather than product-specific, it is small enough to sit well
inside the payload cap, and it exercises the whole path — save on change,
coalesce, flush at shutdown, load before the first window opens.

Shipping the store alone was the first draft's plan, and it is the shape this
repository names as its dominant defect class. `close_request.rs`'s own module
doc says it: *"shipping half of that pair is how a seam ends up unreachable."*

What is deliberately NOT here: the restoration framework. If FLUI later wants
Flutter's bucket tree and `RestorationMixin`, that is a separate issue with its
own consumer, and it will consume this store. When it comes, its reference tests
DO transfer — `.flutter/packages/flutter/test/` carries roughly 180 KB of
restoration oracles across ten files — and that corpus is owed then. The
non-transfer claim in this record covers only where the bytes go.

## Verification

- **Forced termination, portably.** An ordinary integration test spawns a
  headless binary with `FLUI_HEADLESS=1` and a tempdir override, waits for a slot
  to appear, calls `Child::kill()` — `SIGKILL` on Unix, `TerminateProcess` on
  Windows — respawns, and asserts the state came back. No window, no pixels, no
  input, no GPU, so it belongs in `crates/flui-app/tests/` rather than beside
  `tools/live-smoke`, which is X11/Linux-only by construction and carries a
  documented ~13% flake whose standing remedy is re-running the job. A durability
  claim hosted in a re-run-on-failure suite is worth less than no claim.
- **Stated limit: no oracle here covers power-loss ordering.** A `SIGKILL` leaves
  the page cache intact and the kernel writes it back afterwards, so the kill test
  passes identically against a correct and an incorrect fsync order. It tests
  application write ordering, not durability ordering. Distinguishing them needs
  `dm-log-writes` or a real power cut. D1's ping-pong is chosen partly so that
  correctness does not depend on an ordering this repository cannot test.
- A half-written slot must be rejected by CRC and the older slot used, asserted by
  writing the damage deliberately.
- Both slots absent, both corrupt, and a `seq` rollover must all start fresh
  rather than fail: a corrupt store must never prevent startup.
- The coalescing rule needs a test that a burst writes the LAST snapshot — not
  merely that a write happened, which is what distinguishes latest-wins from a
  queue.
- Two concurrent realms and two processes sharing a `data_dir()` must not
  interleave. Ping-pong makes a partial write self-identifying rather than
  invisible, but the single-writer rule is a contract slice 1 states and tests,
  not a property it inherits.
