# Panic Policy

> When FLUI code is allowed to panic, what a panic message must say, and how
> the `clippy::unwrap_used` gate enforces the boundary. Applies to every crate
> in the workspace. Companion to the error-handling split in `AGENTS.md`
> (`thiserror` in libraries, `anyhow` in applications).

## The rule

A panic is a **bug report**, never a control-flow mechanism.

| Situation | Required handling |
|---|---|
| Caller-triggerable failure (bad input, missing file, lock contention, platform error) | Return `Result` with a `thiserror` error type. Never panic. |
| Internal invariant that the module itself maintains (slab index handed out by this arena, ID minted by this tree, state machine transition guarded upstream) | `expect("BUG: <invariant that was violated>")` |
| Test code, benches, examples | `unwrap()`/`expect()` freely — a panic *is* the failure report there. |
| Convenience twin over a fallible form (`get` over `try_get`, `with` over `try_with`, `signal` over `try_signal`) | Allowed when the failure is a **programming error with a stale or misused handle** — the same class as `RefCell::borrow` over `try_borrow` or slice indexing over `get`. The `# Panics` section documents it, the message **names the `try_` form**, and the `try_` form exists with a typed error. Never for caller-triggerable failures that a `Result` should carry. |
| Compile-time-checkable invariant | Prefer the type system (`NonZeroUsize` IDs, Arity system) over any runtime check. This is the house style — see the ID offset pattern in `AGENTS.md`. |

### The `BUG:` message convention

Every production-path `expect()` documents the invariant it asserts, prefixed
with `BUG:` so a user hitting it knows immediately the fault is FLUI's, not
theirs, and grep can find every invariant assertion in the workspace:

```rust
// YES — states the invariant, identifies the owner of the bug
let node = self.nodes
    .get(index)
    .expect("BUG: RenderId minted by this tree must resolve in its slab");

// NO — restates the operation, blames nobody
let node = self.nodes.get(index).expect("failed to get node");

// NO — caller-triggerable, must be a Result
let file = std::fs::read(path).expect("BUG: asset must exist");
```

`unwrap()` carries no message and is therefore **never** acceptable on a
production path — if the invariant is real, name it with `expect("BUG: …")`;
if you cannot name it, it is not an invariant and the path needs a `Result`.

Exactly one function reads the `BUG:` prefix back out of a caught panic:
`flui_foundation::panic::is_internal_invariant`. One function reads a
caught payload's text: `flui_foundation::panic::payload_text` (the platform
layer's `panic_payload_message` is a fallback-applying wrapper over it). A
new `catch_unwind` boundary that needs either calls these — never a local
`downcast_ref` on the payload, a `Debug` print of the boxed payload (which
renders `Any { .. }` for every panic), or a local `starts_with("BUG:")`
check.

Panics inside `unsafe` contexts deserve extra scrutiny: an `expect()` whose
failure would leave a raw-pointer structure half-updated must either be
hoisted above the unsafe region or the SAFETY comment must cover the
unwind path.

## Destructor failures and unwinding

A container that owns user values withdraws each one and releases its borrows
and locks before the value's destructor runs. After a caught failure in the
same operation it keeps destroying what it withdrew, each destructor under the
operation's containment, so a destructor that panics there is a competing
failure and the first one stays; a caught payload that is not propagated is
destroyed the same way, with `flui_foundation::panic::discard_payload`. A destructor that panics while the thread is
already unwinding aborts the process, as in std; FLUI does not leak values or
panic payloads to prevent it
([ADR-0281](adr/ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md)).
Not implemented everywhere yet: crates still retain such values on failed
paths, as [ADR-0127](adr/ADR-0127-exceptional-path-retention.md) required,
until ADR-0281's migration removes those sites.

## Callback panics and exit codes

A panic in application code is contained where the framework can still go
on, and ends the process only when nothing is left to continue. The rule is
[ADR-0258](adr/ADR-0258-owner-turn-panic-boundary-and-terminal-exit.md),
accepted and not yet implemented: today a caught callback panic is reported
and then resumed, and on Win32, AppKit and UIKit it reaches the OS entry's
boundary, which aborts.

The boundary for application code is `flui-app`'s owner dispatch, not the OS
entry. Every application operation runs under its own catch there, and the
whole body of each `AppHandler` and `WindowHandler` method of `flui-app` runs
under one more, so no application panic reaches `os_entry`. A panic that does
reach an OS entry is a defect of FLUI itself, in the native host's own code
outside any handler call, and the entry keeps the answer it has for any
panic: a failure the OS understands, or, at the Win32 window procedure and
the AppKit and UIKit entries that have none, the end of the process
(ADR-0151 §16).

| Path | Outcome |
|---|---|
| Ordinary close of the last window, including a flush that misses its deadline with the file still whole | exit 0, within `EXIT_BUDGET` (6 s) of the framework's exit work |
| `build` | the subtree becomes an `ErrorView`; the process runs on |
| An input handler, post-frame callback, lifecycle observer, command, or close-request handler | one `CallbackPanic` report; the UI runtime and the process run on |
| Layout or paint | one `frame_failure` report per failure; the frame is retried with backoff, at most one attempt per `FAILURE_RETRY_CAP` (1 s) once the streak is long |
| Closing a UI runtime that is not the last | one `CallbackPanic` report; what was not yet destroyed is retained |
| `dispose` or a `Drop` while the last window closes, or at loop exit | the flush registry is written, services and pools stop, then exit 101 with the panic's text |
| Application code at exit (a destructor, `on_quit`) that blocks until `EXIT_WATCHDOG` (15 s from the start of the exit) | the watchdog writes the flush registry, then ends the process with 124 (`EXIT_CODE_HUNG`), without destructors |
| A `BUG:` invariant inside an application operation | contained like any panic on the paths above, with `internal_invariant` set in its report |
| A panic that reaches an OS entry (`os_entry`): a framework defect, never application code | the entry's answer to the OS; at the Win32 window procedure, the cloak hook and the AppKit and UIKit entries without one, an abort without the exit flush |
| A destructor that panics while the thread is already unwinding | abort, as in std (ADR-0281) |

The first failure is the one reported and resumed; later ones are logged and
their payloads dropped. A terminal failure waits in the host's terminal
slot until the exit order has written what the application persisted; it is
then resumed with `resume_unwind`, which does not run the panic hook again.

A persisted document's codec (`Document::encode`, `Document::decode`) runs
under its own containment
([ADR-0262](adr/ADR-0262-persisted-documents-over-a-byte-storage-capability.md)
§4): a panic in `encode` publishes nothing and leaves the value in memory and
the file on disk untouched; the document's status becomes `Failed` with
`PersistError::Panicked { during: CodecStep::Encode, message }`, and the next
`set` works. A panic in `decode` leaves the file untouched and the document
not loaded, and `load` may run again. The first failure stays in the status
until the next success. Its payload is dropped (ADR-0281). A codec panic
is reported in the document's status, never as a `CallbackPanic`, and never
ends the process.

## Enforcement

`[workspace.lints.clippy]` sets `unwrap_used = "warn"`, and the clippy CI job
runs with `-D warnings`, so an unannotated production `unwrap()` fails CI.
The root `clippy.toml` sets `allow-unwrap-in-tests = true` and
`allow-expect-in-tests = true`, so `#[test]` functions and `#[cfg(test)]`
modules are exempt automatically. No crate opts out.

`expect()` is deliberately **not** linted (`expect_used` stays off): with the
`BUG:` convention it is the sanctioned invariant idiom, and the review bar is
the message, not the call. `panic!()` is held to the same bar: its message
starts with `BUG: ` (an invariant) or names the `try_` twin it is the
convenience form of. Audit them with:

```bash
rg '\.expect\("(?!BUG: )' crates/*/src --pcre2   # expects missing the convention
rg 'panic!\("(?!BUG: )(?!.*try_)' crates/*/src --pcre2   # panics that are neither BUG: nor a twin
rg '\.unwrap\(\)' crates/*/src                   # should be test-only or allow-tracked
```

`unwrap_used` is the only mechanical part. The `BUG:` prefix on `expect()`
and the `BUG:`-or-`try_`-twin bar on `panic!()` are conventions that review
holds, with the searches above as the audit.
