# ADR-0111: Admitted worker images stay mapped until process exit

- **Status:** Accepted
- **Date:** 2026-10-04
- **Supersedes:** [ADR-0094](ADR-0094-hot-reload-through-subsecond.md) worker image lifetime only; its development-only boundary and future replacement remain unchanged.

## Context

A worker build returns Rust views whose descendants can include image-defined
callbacks, state, destructors and borrowed storage. The host retains those values
in its tree, and consumers can clone them out of the tree. Reload previously
unmapped the old worker before reassembling that tree. Registry pruning prevented
a fresh lookup but did not keep already escaped values or addresses valid.
A lease covering only the build call cannot protect those descendants.
The current registration ABI passes a raw function address, and lookup returns
that copyable address. Wrapping dispatch in an `Arc<DynLib>` would protect the
call, but its returned `BoxedView` can contain a `GestureDetector` callback that
is independently cloned out of the tree. That callback, image-defined state and
borrowed storage would not inherit the wrapper's lease. Bounded retirement would
require a lease-bearing ABI and propagation through every such escaping value,
including removing bare-address lifetime escapes; it remains a possible future
design rather than a property of the current worker API.

## Decision

Once a worker passes symbol and ABI-token admission, its loader reference is
retained for the process lifetime. `WorkerPlugin` owns that reference through
`ManuallyDrop<DynLib>` before invoking init. Rejected libraries retain ordinary
loader destruction. Dropping a worker or calling `unload` retires only its matching
registry entries; later registrations with the same fingerprint remain current.
Old addresses, callbacks, vtables and storage stay mapped even after the driver,
configuration or trees are destroyed. Registration and calling conventions still
require the supported matching Rust-ABI development build; mapping retention
does not validate arbitrary signatures or concurrent plugin behavior.

The host must restart to reclaim image memory. On Windows, retained images can
keep old staged files locked: pruning is best effort and cleanup can finish only
after host exit. Content-addressed staging uses distinct paths for distinct image
bytes, under its documented cryptographic collision-resistance assumption.
There is no bounded image-count claim for an indefinitely running reload session.
This conservative policy avoids pervasive descendant lease tracking in the
current development loader; it does not change scene-plugin unload obligations.

## Verification

The `admitted_worker_callbacks_survive_retirement_and_replacement` consumer test
loads actual same-toolchain dynamic fixtures in an isolated process. It retains an
old callback, retires its worker registration, admits a replacement, calls both
callbacks and destroys both, then proves another current build still progresses.
Restoring ordinary worker loader destruction makes the old callback dispatch or
retirement access unmapped image code on loaders that unmap immediately.
On the Windows host, the corrected consumer passed; restoring ordinary loader
destruction caused its isolated child to exit with access violation
`0xC0000005`. This verifies real dynamic loading and callback lifetime on that
host, not the native GUI hot-reload loop on other platforms.
