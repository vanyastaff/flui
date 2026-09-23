# Safety review: hot-reload plugin macros

Audit date: 2026-09-19. Scope: `crates/flui-hot-reload/src/plugin.rs`,
the canonical `Scene` reexport, and their regression tests. Based on commit
`fbed94084beb5f5e977e159a70b6b3814cee2ef5` plus the current working changes.
This is not a new audit of the loader, worker registry, or compositor.

## Unsafe sites

The original audited denominator was 18 production sites: 14 unsafe symbol
attributes and four ownership reconstruction blocks. Four newly explicit unsafe
function declarations express those blocks' caller obligations, making 22
production syntax sites after the repair. Two existing unit-test calls also now
have explicit unsafe blocks. Line numbers below refer to `src/plugin.rs` in
`crates/flui-hot-reload`.

| Line | Site | Finding and disposition | Runtime evidence |
|---|---|---|---|
| 59 | scene build symbol | Unique symbol family per image documented; factory now typed `Scene` | Scene ownership test |
| 70 | scene version symbol | Unique symbol family per image documented | Existing loader tests |
| 85 | scene drop symbol | Unique symbol family per image documented | Scene ownership test |
| 86 | scene drop unsafe declaration | Caller must own the initialized allocation | Compile rejection and ownership test |
| 91 | scene drop block | P1: safe raw-pointer consumption repaired by unsafe entry point | Miri ownership test |
| 113 | scene free symbol | Unique symbol family per image documented | Scene ownership test |
| 114 | scene free unsafe declaration | Caller must own the emptied allocation after one move | Compile rejection and ownership test |
| 119 | scene free block | P1: safe raw-pointer consumption repaired by unsafe entry point | Miri ownership test |
| 131 | scene ABI token symbol | Unique symbol family per image documented | Existing loader tests |
| 163 | worker init symbol | Unique symbol family per image documented; callback protocol unchanged | Existing worker tests |
| 170 | worker version symbol | Unique symbol family per image documented | Existing worker tests |
| 177 | worker fingerprint symbol | Unique symbol family per image documented | Existing worker tests |
| 185 | worker ABI token symbol | Unique symbol family per image documented | Existing worker tests |
| 385 | app build symbol | Unique symbol family per image documented; pipeline returns canonical Scene | Thread-affinity test |
| 434 | app version symbol | Unique symbol family per image documented | Existing loader tests |
| 449 | app drop symbol | Unique symbol family per image documented | Thread-affinity test |
| 450 | app drop unsafe declaration | Caller must own the initialized allocation | Compile rejection |
| 455 | app drop block | P1: safe raw-pointer consumption repaired by unsafe entry point | Thread-affinity test |
| 472 | app free symbol | Unique symbol family per image documented | Facade macro compilation |
| 473 | app free unsafe declaration | Caller must own the emptied allocation after one move | Compile rejection |
| 478 | app free block | P1: safe raw-pointer consumption repaired by unsafe entry point | Same operation as scene free; not executed under Miri |
| 490 | app ABI token symbol | Unique symbol family per image documented | Existing loader tests |
| 540 | test's first app drop | Just-created scene, not moved or consumed; image is the test executable | Thread-affinity test |
| 564 | test's second app drop | Same ownership argument after foreign-thread refusal | Thread-affinity test |

`tests/scene_ownership.rs` additionally contains four explicit unsafe blocks:
original-scene drop, single `ptr::read`, emptied-allocation free, and null calls.
Each carries its own local safety argument. The negative compile fixtures contain
no executed invalid pointer operations: they require rejection by the compiler.

## Why unsafe is necessary

The existing C entry points pass an opaque raw pointer across an image boundary.
Reconstructing ownership cannot be checked by the Rust borrow checker. A handle
registry or different ABI could change this tradeoff, but is not introduced here.
`unsafe extern "C"` preserves the C calling convention while exposing the real
preconditions to Rust callers. The factory's explicit `Scene` type prevents a
safe factory returning an unrelated allocation layout. `$crate::Scene` resolves
the canonical owner type even when the macro is called through a renamed facade.

## Safety invariants

Every non-null pointer is a live, uniquely owned allocation returned by the
matching build function in the same plugin image. The original-scene drop path
requires its initialized value still be in place, never moved out. The free path
requires that value have been moved out exactly once with `ptr::read`, and
reconstructs `Box<MaybeUninit<Scene>>`: equal size/alignment, no value destructor.
Both paths consume allocation ownership exactly once. The null branch performs
no memory access. None of these functions accepts a dangling pointer merely
because it is aligned.

Keep the image mapped until both the allocation and every plugin-backed payload
have been destroyed, including payloads retained or cloned from a moved scene.
The host does not use `Box::from_raw` to deallocate plugin memory. The ABI-token
handshake is unchanged and does not make arbitrary Rust layouts interoperable.

All 14 exported symbol attributes rely on one expansion of their symbol family
per plugin image, without competing definitions. Each attribute now has a local
safety comment. Each reconstruction block is narrow and documented. The unsafe
entry points publish their preconditions rather than falsely claiming to enforce
the validity of an arbitrary raw pointer.

## How invariants are upheld

The type annotation rejects factories with the wrong payload type. Four separate
external compile tests require E0133 for safe calls to the four teardown functions;
the wrong-factory test requires E0308. Separate consumer packages compile scene
and app macros using only `flui`, then only a renamed `ui` facade dependency.
The initial negative tests intentionally include the layer dependency to isolate
type/safety failures from the independently checked macro hygiene failure.

The executable ownership test uses an annotated layer with a counted destructor.
It checks destruction once on original-scene drop, no destruction when freeing
the moved-out allocation, then exactly one destruction when the moved scene is
dropped. Null calls leave the count unchanged. The test's executable remains
mapped throughout, so it does not pretend to validate dynamic image unloading.

## Verification

RED: `cargo test -p flui --test facade_consumer --offline -- --test-threads=1`
failed the six newly added cases while the six earlier cases passed. In
`/tmp/flui-plugin-red.log`, the wrong factory and each safe teardown call compiled;
facade-only macro expansion failed for inaccessible layer types.

GREEN: the same command passed all 12 tests (`/tmp/flui-plugin-green.log`).

Owner regression command:
`cargo test -p flui-hot-reload --all-features --offline`.
Passed: 11 unit tests, five loader integration tests, and the ownership test;
six existing doctests remain ignored. Log: `/tmp/flui-plugin-owner-tests.log`.
The ownership fixture initially needed an explicit `Layer::AnnotatedRegion`
conversion; this was a test construction error, not a production regression.

`cargo clippy -p flui-hot-reload --all-targets --all-features --offline -- -D warnings`
passed (`/tmp/flui-plugin-clippy.log`). `cargo fmt --all -- --check` and the
focused documentation/test `typos` check also passed.

Miri command:
`cargo +nightly miri test -p flui-hot-reload --test scene_ownership --offline`
(the target name as of that review; since 2026-09-22 the file is a module of the
crate's single integration-test binary, so the same run is
`cargo +nightly miri test -p flui-hot-reload --test hot_reload_it scene_ownership --offline`).
Passed: one test, zero failures, no UB or leak reports in
`/tmp/flui-plugin-miri.log`. Miri emitted its standard warning that the workspace
profile's optimization level is ignored; it reported no code warning. Scope
deliberately excludes dynamic library loading and the intentionally retained
app-pipeline TLS state.

AddressSanitizer/MemorySanitizer were not run: this increment uses Miri for the
bounded allocation protocol and ordinary native tests for loader behavior. No
claim is made about sanitizer coverage or unexecuted foreign-image paths.

## Win32 clipboard: cross-thread sessions

**Defect.** `OpenClipboard(NULL)` does not exclude other threads of the same
process: while one thread holds the clipboard, a second thread's
`OpenClipboard(NULL)` also succeeds (verified directly on Windows 11 with two
threads and a barrier). A writer's `EmptyClipboard` then frees the
`CF_UNICODETEXT` handle a reader on another thread is scanning through
`GlobalLock`, a use-after-free that aborts the process with
`STATUS_HEAP_CORRUPTION` (`0xc0000374`). It reproduced with `WindowsClipboard`
alone, with `arboard` alone (it opens with a `NULL` owner too) and with the two
mixed; writer/writer and reader/reader pairs never crashed. `WindowsClipboard`'s
lock was per instance, so it did not help across instances.

**Fix.** `flui-platform/src/shared/clipboard_lock.rs` holds one process-wide
lock. The Win32 backend can only open the clipboard through
`ClipboardSession`, which takes that lock before `OpenClipboard` and releases
it after `CloseClipboard`. `ArboardClipboard` runs each `arboard` call under
the same lock on Windows.

**Verification.** Three tests in `clipboard_lock.rs` race a reader against a
writer on two threads (Win32/Win32, arboard→Win32, arboard/arboard). With the
lock replaced by a per-call mutex, `cargo test -p flui-platform --all-features
--lib clipboard_lock` aborted with `0xc0000374` in 3 of 3 runs. With the lock,
it passed 3 of 3; the full in-process lib suite (`--skip real_loop_tests`)
passed 20 of 20, and `cargo nextest run -p flui-platform` passed with default
and all features.

**Residual.** `ClipboardSession` opens the clipboard with a message-only owner
window (`HWND_MESSAGE` parent, created once per process on a dedicated thread
that pumps its messages), so Win32 itself refuses a concurrent `OpenClipboard`
from any other owner in the process, third-party `NULL`-owner openers included.
`a_null_owner_open_on_another_thread_fails_while_a_session_is_open` in
`platforms/windows/clipboard.rs` pins this; with the `NULL` owner restored it
fails. `another_opener_can_empty_a_clipboard_flui_owns` pins that the owner
thread pumps: without the pump, another opener's `EmptyClipboard` stalls for
about five seconds on the unanswered `WM_DESTROYCLIPBOARD` and the test fails.
If the owner window cannot be created, sessions fall back to a `NULL` owner and
log an error. What remains open: `arboard` (the winit backend's clipboard)
still opens with a `NULL` owner and exposes no way to pass one, so third-party
`NULL`-owner code on another thread can still race an `ArboardClipboard`
session.

## Independent reviewer sign-off

`cargo clippy -p flui --test facade_consumer --all-features --offline -- -D warnings`
also passed (`/tmp/flui-plugin-consumer-clippy.log`), as did
`just runtime-conformance-check` (`/tmp/flui-plugin-contracts.log`).

Independent unsafe-auditor verdict: **SAFETY-GATE: PASS — COMPLETE.**
The post-fix review covered all 22 production syntax sites, the canonical
`Scene` reexport, and ownership tests. No blocking findings remain within this
scope. Separate Miri execution of app-free is absent and explicitly disclosed;
its identical canonical deallocation operation was reviewed directly.
The verdict excludes the broader loader/driver, dynamic unloading, and retained
app TLS. Full workspace CI remains a separate release requirement.
