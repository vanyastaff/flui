# Safety Review: Win32 HWND userdata context lifetime

**Audit date:** 2026-08-10

**Scope:** `flui-platform`'s `WindowContext` ownership at the
`HWND`/`GWLP_USERDATA` boundary only

**Base revision:** `030f89f520b1384f88b0e3ebc12d6263795013f1`

---

## Unsafe sites

| Site | Operation | Status | Severity | Miri |
|---|---|---|---|---|
| `crates/flui-platform/src/platforms/windows/platform.rs:101` (unsafe groups at lines 111–115, 131–135, 145) — `install_window_context` | Transfer one `Arc` strong reference into `GWLP_USERDATA` | Documented | Boundary | Blocked |
| `crates/flui-platform/src/platforms/windows/platform.rs:162` (unsafe expressions at lines 172, 181–184) — `acquire_window_context` | Increment and reconstruct an invocation-local `Arc` | Documented | Boundary | Blocked |
| `crates/flui-platform/src/platforms/windows/platform.rs:195` (unsafe expressions at lines 205–209, 215) — `take_window_context` | Clear userdata and consume its strong reference | Documented | Boundary | Blocked |
| `crates/flui-platform/src/platforms/windows/platform.rs:489` (narrow unsafe expressions at lines 501, 529, 537, 581, 604, 631, 859–860, 895) — `WindowsPlatform::window_proc` | Win32 WNDPROC dispatch and reentrant teardown | Documented | Boundary | Blocked |

This review does not claim to enumerate or approve unrelated unsafe sites in the
Win32 backend.

---

## Why unsafe is necessary

Win32 exposes `GWLP_USERDATA` as an untyped pointer-sized integer. Rust cannot
express ownership in that OS-managed slot directly, so the boundary must convert
one `Arc<WindowContext>` strong reference to a raw pointer and reconstruct it.
The rest of the backend uses safe `Arc` and `parking_lot::Mutex` ownership; raw
Arc operations are isolated to three helpers beside WNDPROC.

The conversion deliberately uses strict-provenance APIs: installation calls
`expose_provenance` before storing the address, and acquisition/teardown call
`with_exposed_provenance` before any Arc operation. There is no plain
integer-to-pointer cast at this boundary.

---

## Safety invariants

### `install_window_context` — raw strong-reference transfer

- **Invariant:** the stored pointer was produced by `Arc::into_raw` for exactly
  one `Arc<WindowContext>` strong reference.
- **Holds because:** this is the only installation helper, called once after a
  FLUI HWND is created. It verifies the slot is empty before converting the Arc
  to raw, so an unexpected foreign value is rejected without being overwritten.
  A successful write transfers ownership to the slot; a detected write failure
  reconstructs and drops the reference. If the owner-serialized preflight
  invariant is externally violated between calls, a successful replacement is
  left slot-owned for caller-driven HWND destruction rather than creating a
  dangling pointer.

### `acquire_window_context` — invocation pin

- **Invariant:** a non-null slot value still owns a live strong reference while
  `Arc::increment_strong_count` executes.
- **Holds because:** Win32 serializes WNDPROC dispatch for an HWND on its owner
  thread, and only WNDPROC clears the slot. The increment happens before any
  callback or nested dispatch. The reconstructed local `Arc` therefore remains
  live if a callback synchronously causes `WM_DESTROY`.

### `take_window_context` — slot teardown

- **Invariant:** a nonzero value returned while clearing the slot is the exact
  raw Arc reference previously transferred to it, and is consumed once.
- **Holds because:** installation and teardown are private to the same module;
  `WM_DESTROY` is the sole clearing path. A failed ambiguous zero return is
  reported without reconstructing a pointer, preferring a leak to a guessed
  double decrement.

### `WindowsPlatform::window_proc` — reentrant callback lifetime

- **Invariant:** every callback observes a live `WindowContext`, and no state or
  callback-storage lock is held across callback invocation or nested Win32
  dispatch.
- **Holds because:** WNDPROC acquires its local Arc before dispatch. Mutable
  window data is copied or updated under short `WindowState` locks that are
  dropped before callbacks. `WM_DESTROY` marks the cache destroyed, dispatches
  close notifications, removes the weak-backed registry entry, then clears the
  slot; the invocation-local Arc outlives all of those operations.

---

## How invariants are upheld

- All raw userdata reads, writes, and Arc reconstruction live in
  `platform.rs`'s three private helpers.
- Raw pointer addresses cross Win32's integer slot only through
  `expose_provenance` and `with_exposed_provenance`.
- `WindowContext` contains no `Cell`; its mutable data shares the synchronized
  `WindowState` used by `WindowsWindow`.
- A compile-time assertion requires `WindowContext: Send + Sync`.
- Public window methods never access userdata. Fullscreen, cursor application,
  and close use a private, closed owner-command vocabulary; foreign-thread
  calls use `PostMessageW` and trace failures.
- `WindowContext` holds only a `Weak` registry reference, so the platform map
  does not participate in an Arc cycle.
- `WindowsWindow` has no by-value `Clone` implementation; shared ownership is
  exclusively `Arc<WindowsWindow>`, so each Rust window value has one Drop path.
- Cached destroyed state makes post-close queries deterministic and prevents
  recursive native destruction.
- Constructor and WNDPROC unsafe operations are scoped to the individual FFI or
  raw-Arc operation; safe state transitions and callback dispatch no longer sit
  inside function-sized unsafe blocks.

---

## Verification

**Miri**

Miri was attempted for `flui-platform`, but execution is blocked by the
unsupported `OpenClipboard` foreign-function call. This is not a clean Miri
run and provides no runtime validation of the Win32-only path.

**Windows cross-target evidence**

```text
$ cargo clippy -p flui-platform --locked --all-targets \
  --target x86_64-pc-windows-msvc -- -D warnings
    Blocking waiting for file lock on build directory
   Compiling flui-platform v0.2.0 (/mnt/data/worktrees/win32-userdata-lifetime/crates/flui-platform)
    Finished `dev` profile [optimized + debuginfo] target(s) in 4.13s
```

This cross-target check compiles and lints the backend but does not link or
execute it on Windows.

**Structural evidence**

- `rg 'GWLP_USERDATA|Arc::(into_raw|from_raw|increment_strong_count)'` confirms
  the boundary is centralized in `platform.rs`.
- `rg 'as \*const WindowContext|as \*mut WindowContext|\.addr\(\)'` confirms
  no plain integer-to-pointer reconstruction or non-exposed address storage
  remains at the userdata boundary.
- `rg 'Cell<'` confirms `WindowContext` has no unsynchronized mutable fields.
- The compile-time `Send + Sync` assertion is built by the Windows cross-target
  command.

No sanitizer run or Windows runtime test was performed.

---

## Reviewer sign-off — SAFETY-GATE: partial

| Check | Result |
|---|---|
| All unsafe sites in the scoped userdata boundary listed and justified | Complete |
| Invariants sufficient to rule out userdata use-after-free | Complete — final auditor confirmed all nine blockers resolved |
| Invariants enforced by the module boundary | Complete |
| Miri run clean | No — blocked by unsupported `OpenClipboard` FFI |
| Sanitizer run clean | Not run |
| Native Windows teardown exercised | Not run |

**SAFETY-GATE:** `PARTIAL`

This document does not grant a whole-backend SAFETY-GATE pass. Native Windows
runtime validation remains required.
