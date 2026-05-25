# Foundation Inline Storage Specification

## Purpose

Document the rationale for the `SmallVec<[ListenerCallback; 4]>` inline-storage
choice in `ChangeNotifier`'s notification snapshot, so future maintainers do not
second-guess the decision or attempt to swap to an incompatible library.

Owner crates: `crates/flui-foundation` (`notifier.rs`).

---

## Requirements

### Requirement: SmallVec selection for notification snapshot MUST be documented (F16)

The `SmallVec<[ListenerCallback; 4]>` in the `notify_listeners` snapshot
(line ~251 of `crates/flui-foundation/src/notifier.rs`) MUST have an inline
source comment that documents the selection rationale and the competing libraries
considered.

The comment MUST record:

| Library | Verdict | Reason |
|---|---|---|
| `tinyvec::TinyVec<[T; N]>` | Rejected | Requires `T: Default`; `ListenerCallback = Arc<dyn Fn() + Send + Sync + 'static>` does NOT implement `Default`. Fails to compile. |
| `tinyvec::ArrayVec<[T; N]>` | Rejected | Same `T: Default` requirement. Fails to compile. |
| `arrayvec::ArrayVec<[T; N]>` | Rejected | Fixed capacity with no heap fallback; would silently drop the 5th+ listener if more than N are registered. Silent data loss is worse than a heap allocation. |
| `Vec::with_capacity(4)` | Rejected | Always heap-allocates on first push; defeats the I-4 optimization goal (zero allocation for common ≤4-listener case). |
| `SmallVec<[T; 4]>` | **Accepted** | Heap fallback on overflow; no `Default` requirement; well-audited (`smallvec 1.x`, ~12 M+ crates.io downloads). The internal `unsafe` (ManuallyDrop / MaybeUninit) is in a reviewed library, not in FLUI code. |

**Why this is a spec requirement (not just a style note):** Without documented
rationale, a future refactor may attempt to replace `SmallVec` with `tinyvec` or
`arrayvec` for "dependency simplification" reasons, only to discover the
incompatibility at compile time or, worse, silently at runtime.  The comment
serves as a durable decision record in the source.

#### Scenario: Source comment is present at the snapshot allocation site

- GIVEN `crates/flui-foundation/src/notifier.rs` at HEAD
- WHEN the line containing `SmallVec<[ListenerCallback; 4]>` (or equivalent
  snapshot allocation) is inspected
- THEN a `// NB:` or `// RATIONALE:` comment in the surrounding lines documents
  at least the `tinyvec` rejection reason (`ListenerCallback: !Default`)

#### Scenario: SmallVec is the chosen inline container

- GIVEN `crates/flui-foundation/src/notifier.rs` at HEAD
- WHEN `grep -n "SmallVec" crates/flui-foundation/src/notifier.rs` is run
- THEN it exits with code 0 (SmallVec is still used for the snapshot)
- AND `grep -n "tinyvec\|arrayvec\|ArrayVec" crates/flui-foundation/src/notifier.rs`
  exits with code 1 (no competing library is present)
