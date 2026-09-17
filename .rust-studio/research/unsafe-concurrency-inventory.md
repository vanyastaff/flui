QUESTION: Where are flui's current unsafe/concurrency ownership hotspots, and did this pass confirm a new nonduplicate problem?

ANSWER: No new confirmed nonduplicate issue was filed from this inventory pass. The main manual `unsafe impl` hotspots are platform/window/raw-handle wrappers, renderer raw handles, hot-reload dynamic library/build pointers, `ObjectKey`, and allocation-counting test harnesses. Several high-risk items are already covered by existing issues (#1043 raw renderer/surface lifetime, #949 platform wake/thread affinity as referenced in source).

VERSIONS: flui workspace current checkout, inspected 2026-09-13.

SOURCES:
- `rg -n "unsafe impl" crates -g '*.rs'`: manual unsafe impl inventory included `RawHandles`, platform window/platform/accessibility wrappers, web wrapper types, hot-reload `DynLib`/`BuildPtr`, `ObjectKey`, and test-only `GlobalAlloc` counters.
- `crates/flui-view/src/key/object_key.rs`: `ObjectKey` stores a raw `*const ()` plus `Arc<dyn Any + Send + Sync>`; the raw pointer is used only for address equality/hash/debug and is never dereferenced.
- `crates/flui-foundation/src/key.rs`: `ViewKey: Send + Sync + 'static`, so `ObjectKey::new<T>` requires `T: Send + Sync + 'static`; this bounds the stored holder to match the trait object's public contract.
- `crates/flui-engine/src/wgpu/renderer.rs`: source comments explicitly tie `RawHandles` unsafe impl to raw-window-handle/surface lifetime; this area is already represented by #1043 rather than a new issue from this pass.
- `crates/flui-scheduler/src/scheduler.rs`: `set_on_frame_scheduled` documents the cross-thread platform wake hazard and points to #949; this pass did not execute macOS/backend code.

EXECUTED:
- `fd -e rs . crates | wc -l`: 1570 Rust files.
- `rg -n "unsafe" crates -g '*.rs' | wc -l`: 629 unsafe mentions, including comments/tests/generated code.
- `rg -n "unsafe impl" crates -g '*.rs'`: listed the manual impl sites used for this triage.
- Issue search for `ObjectKey object key unsafe Send Sync pointer identity key`: no existing flui issue was returned; no issue filed because the inspected code has a documented invariant and no reproducer/caller-visible failure was established.

OPEN:
- This is an inventory, not a full unsafe audit. Platform wrappers need per-backend proof on the OSes they target, especially macOS/Windows objects whose safety comments depend on thread affinity and native lifetime.
- `ObjectKey` could potentially be redesigned to avoid a raw pointer field by storing a plain address value plus the holder, but this pass did not establish a correctness or soundness defect.
- Hot-reload dynamic-library/pointer ownership was only identified as a hotspot; no source-level audit was completed here.
