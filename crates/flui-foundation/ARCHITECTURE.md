# flui-foundation Architecture

Design notes for the base crate every other FLUI crate builds on: IDs, the shared geometry values, listener notification, the signal read contract and the train guard.

## The geometry values (`geometry`)

`flui_foundation::geometry` holds the geometry values every layer shares (ADR-0098): `Point`,
`Offset`, `Size`, `Rect`, `RRect`/`Radius`, `Edges`/`EdgeInsets`, `Matrix4` (glam inside, no
glam type in a signature), the one `Axis`, and `canonical_bits` for float-keyed caches. A
logical length is a plain `f64`; the types are generic only over their scalar, and the `i32`
instantiations are the device-pixel grid (`DevicePoint`, `DeviceSize`, `DeviceRect`).
`DevicePixelRatio` rejects non-finite, zero and negative ratios. Rounding to the device grid
lives here too: `snap`, `snap_point`, `snap_edges`, `cover`, `device_rect_covering`,
`device_size` and `resolve_stroke_width`; the engine decides where they apply.

---

## The signal read contract (`read_scope`)

`flui_foundation::read_scope` holds the *read side* of the
realm's reactive graph (ADR-0085 §2); the graph itself — slot arena, reader registry, writes,
the build-time guard — lives in `flui-view`, and so does every type that names a reader.

**What it holds.** The handles (`SignalSlot`, `Signal<T>`, `SignalSender<T>`), the error type
(`SignalError`, with `Display`/`Error` written by hand because foundation takes no
`thiserror`), the read-only graph face (`ReadGraph`: `graph_id` and `read_erased`), the
subscription sink (`ReaderSink::subscribe(slot)`), and what a context hands a read
(`ReadScope::scope() -> ScopeRef`). `Signal::get`/`with`/`try_*` take any
`&S where S: ReadScope + ?Sized`; `&S` and `Box<S>` are scopes, so deref shapes keep working.

**What it may not name.** No write, create or release method, no driver hook, and no reader
identity: an `ElementId` appears only in the two build-phase `SignalError` variants, and
`Reader`-like types stay in `flui-view`. `ScopeRef` has private fields and no accessors, so a
scope can be passed to a read and nothing else; a sink is bound to its reader by whoever minted
it, and the sinks that subscribe real elements are private to `flui-view`. A scope built outside
`flui-view` therefore reads but cannot subscribe anyone.

**Invariants a test pins.** A handle forged with the wrong `T` (`Signal::from_slot` and
`SignalSlot::new` are public but `#[doc(hidden)]`, for `flui-view`) is
`SignalError::TypeMismatch`, never a panic; a `ReadGraph` that returns `Ok` without calling the
reader is refused as `Released`; a refused read subscribes nobody. `Signal<T>` is
`!Send + !Sync` (realm-affine); `SignalSender<T>` is `Send + Sync` and carries only the slot.
For a valid read, a panic from the user closure keeps chronological priority over
loan finalization, subscription, returned-value destruction, and panic-payload
destruction. Cleanup is contained before `resume_unwind`; this is required because
`T`, the closure's captures, and the closure's `R` may have arbitrary user-defined
destructors. The public `try_with`/`with`/`peek` reader is therefore `FnMut`, although
it is called at most once. This deliberately rejects an `FnOnce` reader that consumes
a capture: `call_once` would transfer the captures into the caught invocation, where a
panicking capture destructor could abort the process while the reader panic unwinds,
before containment regains control. After a reader or graph panic, the retained opaque
callback envelope and any later opaque result or panic payload are deliberately leaked:
Rust drop glue can destroy a second captured field while the first field's destructor is
unwinding, so no generic `catch_unwind` wrapper can safely retire that aggregate. Normal
reads still destroy the callback and result normally. This exceptional-path leak is the
strongest continuation-safe contract available without constraining public callback and
result types to destructor-free values.

**Why here.** An item added to this module re-checks every crate above foundation, so the module
stays small and changes rarely; the graph, which changes often, stays in `flui-view` (ADR-0085 §6
records the measured re-check sets).

**Every public item here is promised by the facade.** `flui::view` re-exports the module's items
and `BuildContext: ReadScope` reaches all of them (`ScopeRef::new` names `ReadGraph` and
`ReaderSink`), so a change to any public signature, the `#[doc(hidden)]` constructors included,
is a breaking change of `flui` (ADR-0085 Consequences, ADR-0089 §1).

---

## The train guard (`links = "flui_train"`)

This crate's manifest declares `links = "flui_train"`, and `build.rs` exists only because Cargo
requires a build script beside `links`; it prints nothing but its own rerun condition. Cargo
allows one package per `links` value in a dependency graph, so two releases ("trains") of FLUI
cannot meet in one build: an application on one train and a package on another resolve to one
train or fail in the resolver, never later with a type mismatch (ADR-0088 §5).

The guard sits here, not on `flui-sdk`, because this is the crate every train shares: the facade,
`flui-sdk`, `flui-platform-api` and every crate above them except `flui-log`, `flui-assets` and
the `flui-cli` tool have it in their normal dependency closure, while the facade does not depend
on the SDK. A guard on the SDK alone would not separate an application on one
`flui` train from a package on another `flui-sdk` train.

Invariants, checked by `cargo xtask workspace`: this crate declares the guard and no other member
does. `two_trains_refuse_to_resolve` in `tools/xtask` builds two copies of this crate with this
manifest's `links` value into one graph and requires the resolver to refuse it. Dropping the key,
or moving it to a crate that not every train depends on, re-opens the E0308 failure the guard
exists to prevent.

---

## Architecture Decision Summary

| Decision | FLUI |
|----------|------|
| Tree hierarchy | `Debug` trait + custom traits |
| State notification | `Notifier` / `ChangeNotifier` (this crate) |
| Error handling | `thiserror` + `anyhow` |
| Identity | `Key` trait + impls |
| IDs | `Id<T: Marker>` (wgpu-style) |
| Tree storage | `Slab` with typed IDs |
| Platform channels | `bytes` crate |
| Binding system | Trait composition; every binding-shaped value is constructed and owned per `UiRealm`/`AppRuntime`, never process-global |

---

## Thread safety

`flui-foundation` is a Layer 0 crate (no FLUI dependencies); concurrency primitives are minimal and live at well-defined seams.

| Site | Primitive | Where | Why |
|------|-----------|-------|-----|
| `GlobalKey` ID counter | `AtomicU64` (static) | `key.rs:140, 462` | Monotonic key allocator. `fetch_add` only, no contention pattern. Off any hot path. |
| `ChangeNotifier::listeners` / `Notifier::listeners` | `Arc<parking_lot::Mutex<HashMap<ListenerId, …Callback>>>` | `notifier.rs` / `notifier_generic.rs` (struct fields) | Listener registry held during register/unregister/notify. Notifier callbacks are invoked outside the lock (clone-then-iterate pattern from [`docs/plans/2026-03-31-core-crates-hardening.md`](../../docs/plans/2026-03-31-core-crates-hardening.md) Task 3). Not on the render hot path; consumed by the build phase. |
| `ChangeNotifier::next_id` / `Notifier::next_id` | `Arc<AtomicUsize>` | `notifier.rs` / `notifier_generic.rs` (struct fields) | Listener-ID allocator. `fetch_add` only. |

No `RwLock` in `flui-foundation`. No primitive listed here sits inside `perform_layout` / `paint` / `View::build`.

---

## Friction log

No active friction at the time of the graft (2026-05-19). The crate's surface is mature and the established patterns (`Id<T: Marker>` typed IDs, `thiserror` for errors, `parking_lot::Mutex` + clone-then-iterate for `Notifier`) match the strategy clauses without compromise.

Latent question worth tracking — not a violation:

- `Notifier`'s `Mutex<HashMap<ListenerId, ListenerCallback>>` works correctly today, but a re-entrant listener that calls `add_listener` from inside a notify callback would have to take the same lock while the notify path is iterating a clone. The clone-then-iterate pattern prevents deadlock but allows a registration in round N to surface only in round N+1. Documented behaviour; not broken.

---

## Outstanding refactors

Items below are concrete cleanups visible from `flui-foundation` outward. Each is sized for an `/aif-implement` dispatch without out-of-band clarification.

- **`Notifier` re-entrancy semantics — DONE.** Both `ChangeNotifier::notify_listeners` and `Notifier::notify` now document the round-N-vs-round-N+1 behaviour (snapshot-then-fire, mid-notify removals skipped, post-snapshot additions deferred to the next round, `catch_unwind` isolation).
- **State-notification surface decided** — `Notifier`/`ChangeNotifier` in this crate is the listener-notification mechanism. The signals crate that the summary table once pointed at (`flui-reactivity`) was removed 2026-07-28. Realm-scoped signals (ADR-0074) are not a crate: their read contract is this crate's `read_scope` module and their graph lives in `flui-view` (ADR-0085). The `Arc<Mutex<…>>` notifier stays for `Send + Sync` users until the UI callback surface loses `Send` (ADR-0091 §1), when a `Listenable` adapter over a signal replaces it.

---
