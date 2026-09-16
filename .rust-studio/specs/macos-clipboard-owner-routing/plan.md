# Plan — issue #1124: prove/revise the macOS clipboard thread-safety contract

Status: Plan approved (user) · architect validation landed (mechanism verified-good; 2 corrections + 6 advisories
folded below) · **Phase 4 build in progress**.

## 1. Problem and resolution

The `clipboard-stays-thread-safe` contract in `docs/runtime-contract.toml` asserts that NSPasteboard
plain-text ops are "documented off-main-safe". Apple's documented position is the opposite: the AppKit
Thread Safety Summary lists no NSPasteboard exception, and an Apple engineer's statement (FB14885505;
undocumented publicly, per Wade Tregaskis 2024-09-12) is that **NSPasteboard is not safe off the main
thread**. The claim is uncitable — and it is now **empirically falsified** on this Mac: the macOS clipboard
unit tests SIGSEGV under parallel (default libtest-threads) execution and pass only under `--test-threads=1`.

The false claim originates in **ADR-0034** ("NSPasteboard access is documented by Apple as safe off the
main thread for reads", `docs/adr/ADR-0034-….md` "What is deferred") and is carried by **ADR-0039 §5/§7**
(whose §5 already sketches the sanctionned fix — "a marshaling `Clipboard` implementation that crosses to
the owner via the lane internally"). Both are amended in this change.

**Resolution (the issue's option 2):** make `MacOSClipboard` a thread-safe proxy that routes every
NSPasteboard operation to the **owner lane** — the AppKit main thread via the main dispatch queue —
replacing the uncitable "off-main-safe" claim with a mechanism that holds without it.

## 2. Design (final, after architect validation)

### 2.1 Dependencies
- Add `dispatch = "0.2"` to `[target.'cfg(target_os = "macos")'.dependencies]` in
  `crates/flui-platform/Cargo.toml` (sibling of the same block's direct `cocoa = "0.26.0"`, `objc = "0.2"`).
  Zero-dependency crate, already in `Cargo.lock` → lockfile unchanged, `--locked` stays green (architect
  independently verified).
- **No `libc`.** Main-thread probe is `+[NSThread isMainThread]` via `msg_send!` (same documented-safe class
  method as `debug_assert_appkit_main_thread`, macos/platform.rs:58).

### 2.2 No `unsafe impl Send/Sync` — raw `id` never crosses threads
Store nothing raw: `pasteboard: Option<String>` (`None` = generalPasteboard, `Some(name)` = named),
immutable after construction; resolve to a live `id` **on the owner lane per operation**. Struct:

```rust
pub struct MacOSClipboard {
    pasteboard: Option<String>,       // None = generalPasteboard, Some(name) = named (tests)
    owner: &'static dispatch::Queue,   // owner lane every operation is routed through
    owner_is_main: bool,              // equivalence gate for the direct-path probe (architect adv. 3)
}
```

Auto `Send + Sync` (architect confirmed sound in kind; trait bound `Clipboard: Send + Sync` intact).
Both `unsafe impl`s deleted — the unsafe surface shrinks to ordinary `msg_send!` blocks.

### 2.3 Why main, and the crash vector, stated precisely (architect adv. 7)
- The current `Mutex<id>` is **per-instance**: production's single instance is already serialized for
  FLUI's own calls. The parallel SIGSEGV in the test suite is **cross-instance** concurrency (each test
  builds its own instance; default libtest threads → concurrent NSPasteboard touches through independent
  locks). Fixing serialization is necessary but not sufficient for production.
- The decisive leg is **alignment**: every other NSPasteboard user in a process — menu commands, other
  frameworks' controls, pasteboard services — runs on the **main thread**. No FLUI-owned mutex can
  coordinate with traffic that does not take FLUI's lock. Main-routing puts FLUI on the one lane all AppKit
  pasteboard traffic already serializes on. Supported by Apple's documented main-thread-only position
  (Thread Safety Summary; FB14885505).
- The serial-`--test-threads=1` pass is *evidence the crash is a concurrency bug*, not a defense of off-main
  usage.

### 2.4 Owner lanes (production main / test shared)
```rust
fn owner_queue() -> &'static dispatch::Queue { static Q: OnceLock<Queue> = …get_or_init(Queue::main); }
#[cfg(test)] fn test_owner_queue() -> &'static dispatch::Queue { static Q: OnceLock<Queue> = …get_or_init(create("flui.clipboard.test-owner", Serial)); }
```
`new()` → `owner_queue()` (owner_is_main = true). Tests construct on `test_owner_queue()` (owner_is_main =
false) — **one process-wide shared serial queue for every test instance** (`cargo test` never pumps main,
so `Queue::main()` would deadlock; per-instance queues would re-create the crash). Every test goes through
the shared lane; no instance straddles lanes.

### 2.5 Routing + direct-path gating
```rust
fn with_pasteboard_on_owner<R: Send>(&self, f: impl FnOnce(id) -> R + Send) -> R {
    let is_main: bool = unsafe { msg_send![class!(NSThread), isMainThread] };
    let name = self.pasteboard.clone();
    if ON_OWNER_QUEUE.with(|f| f.get()) || (self.owner_is_main && is_main) {
        objc::rc::autoreleasepool(|| f(resolve(name)))
    } else {
        // §2.6 dispatch + panic shield
    }
}
```
The fast path is gated on **`owner_is_main && is_main`** — in tests `owner_is_main` is false, so even a
call on the OS main thread dispatches to the test lane (never an off-lane touch while lane traffic runs)
(architect adv. 3). Reentrancy inside an owner-lane block is caught by the RAII marker, which is the other
disjunct.

### 2.6 Panic shield (mandatory)
dispatch's `exec_sync` calls the closure through an `extern "C"` trampoline with **no panic catch**
(verified in vendored source, both here and by the architect) — an uncaught Rust panic unwinding through
libdispatch's C frames is UB. The dispatched body is `catch_unwind`-wrapped and the payload is replayed on
the caller after `exec_sync` returns:

```rust
let r = self.owner.exec_sync(move || {
    let _guard = OnOwnerQueueGuard::new();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        objc::rc::autoreleasepool(|| f(resolve(name)))
    })).map_err(std::boxed::Box::from)            // Result<R, Box<dyn Any + Send>>
});
match r { Ok(v) => v, Err(payload) => std::panic::resume_unwind(payload) }
```
Direct path (already on lane) is all-Rust frames → no shield needed. Bounds `R: Send`, closure `Send`.

### 2.7 RAII-scoped reentrancy marker
GCD reuses pooled worker threads across queues → a sticky flag would route a later call on a reused thread
off-lane and crash. Marker is a guard whose `Drop` clears the thread-local:
```rust
thread_local! { static ON_OWNER_QUEUE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
struct OnOwnerQueueGuard; // new() sets true; Drop sets false
```

### 2.8 Per-op autoreleasepool + owned-NSString leak fix
- Every op runs inside `objc::rc::autoreleasepool` → autoreleased results (`types`, `stringForType:`,
  `pasteboardWithName:create:`) live through the call; named boards are resolved per-op and never stored past
  the pool (no dangling-id UAF).
- `NSString::alloc(nil)` + `init_str(...)` objects are owned (+1) and currently leak per call (`clipboard.rs:99-100,
  155-160, 183-184`). Add explicit `release` after each use (§2.7's pool does not drain owned objects).

### 2.9 Preserve existing non-derived impls (architect adv. 5)
- Keep hand-written `Debug` (`finish_non_exhaustive()`) and `Default` — `dispatch::Queue` has no `Debug`,
  so `#[derive(Debug)]` over the new fields would fail to compile.
- Keep `#[cfg_attr(not(test), expect(dead_code))]` on `change_count` (test-only; dropping it breaks the
  non-test build under `-D warnings`).
- `read_text`/`write_text`/`has_text`/`change_count` become `self.with_pasteboard_on_owner(|pb| …)` with the
  current body minus the `lock()`/nil-log preamble; nil checks stay in-body.

### 2.10 Liveness documentation
Document on `MacOSClipboard` + `with_pasteboard_on_owner`: cross-thread calls **block** on the owner lane
(`exec_sync`). Do not call clipboard from a background thread before `Platform::run`/the event loop is live,
and never wait on a thread blocked in a clipboard call from the main thread — the lane would stall. Verify
no macOS path reaches `clipboard()` before the owner lane is live (desktop.rs wires it via
`owner_platform_installed` from the owner thread's bootstrap).

### 2.11 SAFETY-comment discipline
The literal substrings **`debug_assert_owner`** and **"documented off-main-safe"** must NOT appear anywhere
in `clipboard.rs` (conformance checker substring-scans). SAFETY comments state plain rationale. No issue
numbers, finding IDs, or agent markers in any code (sweep applies to shipped roots; `.rust-studio/specs`
is excluded).

## 3. Runtime-contract.toml changes
`[[contract]] clipboard-stays-thread-safe` — retract the false claim; new statement: `Clipboard` keeps
`Send + Sync`; `MacOSClipboard` routes every NSPasteboard operation to the AppKit main thread via the main
dispatch queue (Apple's main-thread-only guidance; aligns with all other process pasteboard traffic). No
owner-affinity assert: the lane enforces ordering, and an assert would panic the legitimate cross-thread
callers the proxy serves. Documented constraint: a background caller may use clipboard only once the owner
lane is live and must not deadlock it.

`mechanism` — name **only the enforced clause**: "`forbidden_pattern` below (must-not-exist) pins the
no-affinity-assert guarantee on the macOS clipboard implementation" (architect adv. 6 — the checker only
verifies `mechanical` is non-empty; being precise about which clause is enforced keeps the registry claim
honest).

`evidence` unchanged (`symbol` Platform, `source-gate` clipboard.rs `contains = "Clipboard"`).
`[[forbidden_pattern]] debug_assert_owner` → `why` rewritten to the proxy rationale (scope unchanged).

## 4. ADR amendments (architect correction 1)
Amend-in-place with inline "Update" blocks (the repo's established pattern, cf. ADR-0039's own 2026-09-05
update) — no new ADR needed:

- **ADR-0034**, "What is deferred" hazard note: add an update block retracting "documented safe off the main
  thread for reads": the claim is contradicted by Apple's Thread Safety Summary and FB14885505 and was
  **empirically falsified** (parallel SIGSEGV on a real Mac); the implementation now routes to the main
  lane; the suggested future `debug_assert!` is superseded by the lane itself (ADR-0039 §5's marshaling
  design, now landed for plain-text ops).
- **ADR-0039 §5** (and its §7 slice-1 item-2 reference): add an update block noting the off-main-safe basis
  is retracted, the marshaling design §5 already sketched is what landed (issue #1124), and no assert is
  added because the lane enforces ordering and the proxy serves the legitimately cross-thread callers §7
  lists. Keep the §7 parenthetical accurate with a pointer.

## 5. Tests (5 macos `#[test]` fns in clipboard.rs — the file had 4)
**Naming rule:** unique named pasteboard per test + per binary/process: `flui.clipboard.<test>.<pid>`.
**Every NSPasteboard touch in every test goes through the shared lane** (architect correction 2 — a raw
off-lane probe races the hammer's lane traffic and is the same crash class).

1. `test_clipboard_creation` — build on the test lane; route the nil probe **through the lane**:
   `with_pasteboard_on_owner(|pb| pb != nil)` (the public `new()` path uses `owner_queue()`; the probe uses
   the lane, never a raw `resolve`).
2. `test_clipboard_roundtrip` — unique board; write "Hello from FLUI macOS!"; read back; assert eq.
3. `test_has_text` — unique board; write; assert `has_text()`.
4. `test_change_count` — unique board; write; assert count increments.
5. `test_concurrent_access_is_crash_free_and_serialized` (hammer) — ONE shared board
   `flui.clipboard.hammer.{pid}`, N = 8 threads, each its own `MacOSClipboard` on the shared lane:
   (a) main thread writes stable sentinel; (b) spawn 8 threads; each thread
   `write_text("payload-{i}")` then `read_text()`, asserting the read is `Some` and **equals one of the 9
   known strings** (`sentinel` or `payload-0..7`). Deterministic: the lane serializes every op, so every
   write is atomic and every read sees a complete known payload — never `None`, never a torn/garbage
   string; (c) join; main thread writes "final" and reads back exactly "final". The primary gate is
   crash-free completion. (Refined per architect adv. 4: concurrent *writes* through the lane — asserted
   to a deterministic set membership, since with cross-thread interleaving "read your own payload" is not
   guaranteed.)

## 6. Acceptance (observable)
- **Red under HEAD (captured in this spec dir):** `red-state-parallel.log` — parallel
  `cargo test -p flui-platform --lib clipboard --locked` → `signal: 11, SIGSEGV`; `serial-state.log` —
  `--test-threads=1` → `6 passed, 149 filtered`.
- **Green after change:** same parallel command runs all 5 macOS tests + 2 headless crash-free;
  `--test-threads=1` also green; `cargo clippy -p flui-platform --all-targets -- -D warnings` clean;
  `cargo fmt --check` clean; `bash scripts/check-runtime-conformance.sh` green (python3.12 on PATH).
- **Scope note:** macOS tests execute only on a Mac dev box; CI cross-typecheck compiles the backend only.
  Host-validated, matching repo posture.

## 7. Files touched
| File | Change |
|---|---|
| `crates/flui-platform/src/platforms/macos/clipboard.rs` | proxy rewrite, delete unsafe impls, leak fixes, 4 rewritten + 1 new test |
| `crates/flui-platform/Cargo.toml` | +`dispatch = "0.2"` (macos target block) |
| `docs/runtime-contract.toml` | contract statement/mechanism + forbidden_pattern `why` |
| `docs/adr/ADR-0034-clipboard-reachability-without-a-platform-handle.md` | inline update block retracting the false claim |
| `docs/adr/ADR-0039-event-loop-affinity-capability.md` | inline update block on §5 / §7 item 2 |
| `.rust-studio/specs/macos-clipboard-owner-routing/` | this plan + red/green evidence |

## 8. Gates
`cargo test --locked` (clipboard suite, parallel + serial), `cargo clippy -p flui-platform --all-targets
-- -D warnings`, `cargo fmt --check`, `bash scripts/check-runtime-conformance.sh`. `--locked` throughout.

## 9. Review mapping
Folded findings: both adversarial reviewers (mechanism) + chief-architect (plan-level). All resolved as
spelled in §2–§5. Verified-good by the architect: trampoline extern-no-catch (shield mandatory), auto
Send+Sync after deleting unsafe impls, resolve-inside-pool sound, RAII guard correct for GCD reuse, contract
evidence stays valid, manifest placement consistent, lockfile claim accurate.
