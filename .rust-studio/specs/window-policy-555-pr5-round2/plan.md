# #555 PR-5 round-2 review fixes (window-policy branch)

Branch: `window-policy`, worktree `/mnt/data/dev/flui/.claude/worktrees/agent-ab9bd22d8afc9c672`.
PR #609 CI green, codex found a P1 product gap + marker violations. This spec tracks
the fix-up work so it survives context compaction.

## Findings (from coordinator)

1. **P1 — runner.rs `open_secondary_window` can't work on real winit backend.**
   `OwnerPlatform::open_window` returns `WindowOpen::Pending` for any call after
   `on_ready` on the winit owner lane; `open_secondary_window_impl` only handled
   `Ready`. Must handle `Pending`: complete the install when the pending window
   resolves, using the `PendingWindow` completion seam. Add a probe that exercises
   Pending on a backend (extend `HeadlessPlatform` with a deferred-open mode since
   it's normally synchronous-Ready). Probe must drive Pending -> resolved ->
   both-window close/exit end-to-end. If full winit-lane completion needs
   follow-up machinery, implement the honest subset (Pending arm wired through
   the existing completion seam + headless-deferred probe) and name the residual
   in the registry entry — no silent Ready-only requirement may remain.
2. **P1 — runner.rs:~3945 "Review finding" marker** — private review history banned
   by AGENTS.md. FIXED: reworded doc comment, removed "Review finding: " prefix.
3. **runtime-contract.toml "PR-4/#608" / "PR-5" slice markers** in contract prose —
   rephrase in plain language, keep content, drop process IDs. FIXED: three
   occurrences of "(issue #555's PR-5)" / "PR-4/#608" reworded to "(issue #555's
   final slice)" / "(#608)" at lines ~430, ~1162, ~1281. Line 2026 (#553-owned)
   and lines 2316-2428 (the forbidden_pattern gate's own PR-0..PR-6 documentation
   entries) are explicitly OUT OF SCOPE — do not touch.
4. **headless/platform.rs `notify_closed` TOCTOU** — computes `windows_empty`,
   drops lock, re-locks for hook/quit; a window opened in between makes the
   check stale. FIXED: recheck `state.windows.is_empty()` under the SAME lock
   that takes the exit-policy hook; hook/quit still invoked outside the lock.
5. **Copilot's `#[non_exhaustive]` objection on `PlatformHandlers`** — DECLINED,
   coordinator replies on thread. No code change.

## Status as of this file's creation

- Finding 2: DONE (runner.rs marker removed, whole-file sweep clean).
- Finding 3: DONE (all 3 occurrences fixed; verified via grep only line 2026
  (#553, out of scope) and the gate's own `pattern = "PR-N"` entries remain).
- Finding 4: DONE (headless notify_closed rewritten, single-lock recheck),
  verified: `cargo check -p flui-platform --all-features` clean,
  `FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --all-features`
  175/175 passed, 8 skipped.
- Finding 5: no code change, coordinator handles thread reply.
- Finding 1: DONE. `HeadlessPlatform::enable_deferred_window_open` +
  `HeadlessDeferredWindowOpens::resolve_next` (flui-platform) exercise the
  Pending arm without winit; `open_secondary_window_impl` now matches
  `WindowOpen::{Ready, Pending}`, extracting `finish_open_secondary_window`
  (shared install/wiring path) and adding
  `spawn_pending_secondary_window_completion` (drives completion via the
  first hosted realm's `AsyncDriver::spawn_local`, never a busy-loop).
  New tests: `deferred_window_open_resolves_via_the_pending_arm_like_the_real_
  winit_owner_lane` (flui-platform) and
  `open_secondary_window_completes_through_the_pending_arm_like_the_real_
  winit_owner_lane` (flui-app, full Pending -> resolved -> both-window
  close/exit). Both mutant-verified red before the fix. Full gate ladder green:
  416 tests (flui-app + flui-platform), workspace clippy, conformance,
  port-check, taplo, typos, fmt-check, inventory-check, cargo doc (both with
  and without --document-private-items), doc-tests all pass.

## Finding 1 design (worked out, not yet coded)

### flui-platform: `crates/flui-platform/src/platforms/headless/platform.rs`

Add a deferred-window-open test mode so Pending can be exercised without winit:

- `HeadlessState` gets a `deferred_window_open: bool` flag and a
  `pending_opens: Vec<(ClaimSlot<Result<Arc<dyn PlatformWindow>, OpenWindowError>>, WindowOptions)>`
  queue (exact field names flexible, keep private).
- Extract the existing synchronous mock-window construction in
  `Platform::open_window` into a shared free function (e.g.
  `create_mock_window(state: &Arc<Mutex<HeadlessState>>, options: WindowOptions) -> Arc<dyn PlatformWindow>`)
  reused by both the sync path and the new deferred-resolve path.
- New `HeadlessDeferredOwnerHooks` implementing the (pub(crate)) `OwnerHooks`
  trait: `open_owner_window` constructs a `claim_slot` pair via
  `flui_foundation::claim_slot::claim_slot`, stashes `(slot, options)` in
  `pending_opens`, and returns
  `WindowOpen::Pending(PendingWindow::new(handle, owner_thread))`.
- `HeadlessPlatform::run()`: capture `state_handle` (the `Arc<Mutex<HeadlessState>>`)
  before `*self` is consumed/moved, and when `deferred_window_open` is set,
  construct `HeadlessDeferredOwnerHooks` instead of `DirectOwnerHooks` for the
  `OwnerPlatform` it builds.
- Public surface: `HeadlessPlatform::enable_deferred_window_open(&self) ->
  HeadlessDeferredWindowOpens` sets the flag and returns a handle; public
  struct `HeadlessDeferredWindowOpens { state: Weak<Mutex<HeadlessState>> }`
  with `pub fn resolve_next(&self) -> bool` — pops the oldest pending open,
  builds the mock window via `create_mock_window`, and delivers it through the
  `ClaimSlot` (waking the `ClaimHandle`'s waker, which the `AsyncDriver`
  Waker-wiring turns into a fresh frame request). Returns `false` if nothing
  pending.
- Re-export `HeadlessDeferredWindowOpens` (and confirm
  `enable_deferred_window_open` is reachable) from flui-platform's public
  surface (probably `crates/flui-platform/src/lib.rs` or the headless module's
  existing re-export block) so flui-app's tests can use it.

### flui-app: `crates/flui-app/src/app/runner.rs`

- Extract everything after `window: Arc<dyn PlatformWindow>` is obtained in
  today's `open_secondary_window_impl` (~line 6384) — the per-policy
  realm/presentation install, `window.on_input`/`on_resize`/`on_close`/
  `on_should_close`/`on_active_status_change`/`on_visibility_status_change`
  wiring, initial `Resumed` lifecycle dispatch — into a new function
  `finish_open_secondary_window(config: &AppConfig, policy: WindowPolicy,
  window: Arc<dyn PlatformWindow>) -> anyhow::Result<(RealmDispatcher,
  Arc<dyn PlatformWindow>)>`.
- `open_secondary_window_impl` new signature:
  `anyhow::Result<Option<(RealmDispatcher, Arc<dyn PlatformWindow>)>>`.
  - `Ready` arm: call `finish_open_secondary_window` immediately, wrap in `Some`.
  - `Pending` arm: `None` (request accepted, completes async). Look up a driver
    realm via the same `state.realms.iter().next()` pattern already used for
    `WindowPolicy::SharedRealm`'s `shared_with` lookup — a generic "first
    hosted realm" independent of which policy governs the new window. Error
    honestly (`anyhow::bail!` or similar, traced) if no realm is hosted on this
    thread to drive completion.
    Build a `flui_scheduler::BoxedTask` async block that `.await`s the
    `pending: PendingWindow`; on success calls `finish_open_secondary_window`
    (trace `error!` on its own failure, don't panic); on failure (pending
    resolves to an error) trace `error!`. Dispatch a `RealmTask::Frame` to the
    driver realm that does
    `realm.scheduler().async_driver().spawn_local(future)` and pushes the
    returned `TaskToken` into a new thread-local
    `PENDING_SECONDARY_WINDOW_OPENS: RefCell<Vec<flui_scheduler::TaskToken>>`
    (append-only; accepted as an honest, documented simplification — no
    natural per-request owner, and `TaskToken::drop` cancels on realm/thread
    teardown anyway).
- `open_secondary_window` (public wrapper) becomes `.map(|_| ())` regardless of
  arm — unaffected signature-wise.
- Update the 3 existing test call sites of `open_secondary_window_impl`
  (previously ~3966, ~4020, ~4092) for the `Option` wrapper — add
  `.expect("Ready path always returns Some")` (or equivalent) after the
  existing `.expect(...)` on the outer `Result`.
- New end-to-end Pending-path test: use `HeadlessPlatform::enable_deferred_window_open`,
  drive `open_secondary_window` through Pending, manually call `resolve_next()`,
  pump the driver realm's frame (drive its `AsyncDriver`/scheduler tick),
  assert realm/presentation now installed, then close both windows and assert
  clean exit. This is the literal requirement from the coordinator: "the probe
  must drive open_secondary_window end-to-end through Pending → resolved →
  both-window close/exit."
- Update doc comments on `open_secondary_window` / `open_secondary_window_impl`
  to honestly describe Pending handling and name any residual (e.g., if a full
  winit-lane completion test genuinely needs follow-up machinery, say so in
  the doc comment AND in the runtime-contract.toml entry — do not claim more
  than what's tested).

### docs/runtime-contract.toml

- Update the `single-native-window-map-authority` / `app-runtime-composition-host`
  entries (the ones just de-slice-marker'd) to describe the Pending-arm fix
  honestly, naming any winit-specific residual per codex's escape hatch.

## Gate ladder (run after finding 1 is coded)

```
FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-app -p flui-platform --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash scripts/check-runtime-conformance.sh   # or `just runtime-conformance-check`
bash scripts/port-check.sh                  # or `just port-check`
taplo fmt --check
typos
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p flui-app -p flui-platform --all-features
```

## After gate green

- Commit (message covering all 4 code findings + the declined finding 5 note),
  push to `window-policy`.
- Append Round-2 delta section to
  `/tmp/claude-1000/-mnt-data-dev-flui/4215df67-ab97-411c-849e-805d6dba3a03/scratchpad/pr-notes-555-5.md`
  documenting all 5 findings (fixed/declined) with evidence.
- Report new HEAD sha + verdict to coordinator.
