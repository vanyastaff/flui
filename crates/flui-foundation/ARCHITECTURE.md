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

Floating `Point::midpoint` averages through
`FloatUnit::midpoint`: `f32` and `f64` use their native standard-library operation,
so same-sign finite extremes do not overflow and subnormals retain native rounding.
The trait default uses the existing `f64` conversions for custom scalar implementations.
NaN operands and opposite infinities produce NaN in the affected coordinate;
equal infinities keep their sign. Integer geometry does not expose midpoint.
The public family `floating_geometry_midpoints_preserve_the_scalar_range`
checks these boundaries through points.

Point distances convert each coordinate to `f64` before subtraction, matching
their returned scalar. Thus a distance between finite `f32` endpoints can exceed
`f32::MAX` while remaining finite, including the squared result. `f64` differences
and squared results can still exceed their representable range. The public family
`single_precision_geometry_distances_use_double_precision_range` checks point
distances against exact power-of-two results.

`Circle::contains` and `Circle::contains_strict` compare the `hypot` distance
`Point::distance` with the radius rather than squaring both sides, which
overflows or underflows at the ends of the finite range; a distance past
`f64::MAX` is outside every valid circle. The inclusive predicate accepts the
boundary, and a zero-radius circle contains its center but has no strict
interior. `circle_containment_preserves_finite_distance_ranges` pins this.

Vector normalization refuses non-finite components and preserves its existing
`f64::EPSILON` near-zero threshold. When finite components have an overflowing
magnitude, scaling before `hypot` preserves their unit direction instead of
returning zero. `vector_normalization_keeps_finite_directions_and_refuses_invalid_input`
checks ordinary and extreme vectors, fallback admission, and the actual
`Circle::nearest_point` consumer. Endpoint subtraction
and other vector operations retain their own floating-point range limits. `Offset::normalize`
delegates to the same `Vec2` policy, including zero and near-zero refusal and
non-finite components returning zero. Infinite offsets previously produced NaN
components; that invalid normalized output is deliberately refused. The family
also checks `Offset::move_towards` taking a finite unit step toward a target with
an overflowing magnitude. Its subtraction and maximum-distance policies are
unchanged.

Transform decomposition computes column lengths with `hypot`. It keeps a finite,
nonzero direct determinant before dividing by the first scale: prematurely
normalizing an anisotropic column can erase a smaller representable signed scale.
When direct products overflow or underflow to zero, it uses the normalized
column determinant. The existing epsilon threshold still selects zero rotation
and an unsigned second-column length. Public consumer test
`affine_decomposition_retains_finite_extreme_scales` checks ordinary rotation,
reflections, extreme columns, anisotropic shear and determinant underflow.

`Transform::then` applies transformations in declaration order with column-vector
matrices. Horizontal and vertical shear use the same `Matrix4::skew_2d` mapping;
two-axis shear inversion delegates to the existing glam-backed matrix inverse
rather than negating angles. The public family
`affine_composition_and_shear_follow_coordinate_contract` checks actual mapped
coordinates, inverse round trips and flattened composition order.

Matrix inverse admission requires finite input, a finite nonzero computed
determinant and finite computed inverse entries (ADR-0113). The maintained
glam fallible inverse replaces the absolute epsilon cutoff; a finite tiny
scale is admitted. `is_invertible` computes the same inverse, and failed
in-place inversion preserves the original coordinates. Determinant/cofactor
underflow or overflow can still refuse mathematically invertible matrices;
there is no additional conditioning estimate or full-range inversion promise.
The public family `matrix_inverse_requires_a_finite_computed_result` checks
known point coordinates, tiny scaling and each refusal boundary.

Simple `Transform::inverse` variants retain analytical translation, rotation
and scale values. They require finite inputs and finite nonzero scale
reciprocals without an epsilon cutoff. Thus analytical scaling can succeed
where a general matrix determinant would exceed its computed range; complex
variants retain the matrix admission policy above. The existing public family
`affine_composition_and_shear_follow_coordinate_contract` checks tiny scales,
ordinary mapped coordinates and invalid scalar refusal, followed by a healthy
inverse operation (ADR-0113).

Approximate matrix equality and identity require finite components and a finite,
nonnegative tolerance. NaN must not enable the identity optimization that
removes a transform; zero tolerance permits exact equality. The public family
`matrix_tolerance_requires_finite_values` covers these admission boundaries and
checks that conversion preserves a NaN-bearing matrix rather than dropping it.

---

## The signal read contract (`read_scope`)

`flui_foundation::read_scope` holds the *read side* of the
UI runtime's reactive graph (ADR-0085 §2); the graph itself — slot arena, reader registry, writes,
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
`!Send + !Sync` (UI runtime-affine); `SignalSender<T>` is `Send + Sync` and carries only the slot.
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
reads still destroy the callback and result normally. The typed adapter records the
original reader payload outside the graph invocation, then propagates a destructor-free
unit unwind marker through the erased reader. This makes failure visible to the graph
before it finalizes a released loan while preserving the original payload's priority
over graph cleanup and subscription failures. The released-read subprocess rows in
`flui-view`'s `signal_read_and_write_matrix` cover that actual cross-crate bridge.
This exceptional-path leak is the
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

## Diagnostics serialization

Diagnostics nodes expose their structured properties and children and their
human-readable tree formatting. The existing `serde` feature implements value
serialization through serde, including string escaping. There is no separate
JSON string exporter: the removed hand-written exporter had no callers and
could emit unescaped ASCII control characters. Agent transports serialize the
typed protocol tree rather than this diagnostics representation (ADR-0095).

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
| Binding system | Trait composition; every binding-shaped value is constructed and owned per `UiRuntime`/`AppRuntime`, never process-global |

---

## Thread safety

`flui-foundation` is a Layer 0 crate (no FLUI dependencies); concurrency primitives are minimal and live at well-defined seams.

| Site | Primitive | Where | Why |
|------|-----------|-------|-----|
| `GlobalKey` ID counter | `AtomicU64` (static) | `key.rs:140, 462` | Monotonic key allocator. `fetch_add` only, no contention pattern. Off any hot path. |
| `ChangeNotifier::listeners` / `Notifier::listeners` | `Arc<parking_lot::Mutex<HashMap<ListenerId, …Callback>>>` | `notifier.rs` / `notifier_generic.rs` (struct fields) | Listener registry held during register/unregister/notify. Notifier callbacks are invoked outside the lock (clone-then-iterate pattern from [`docs/plans/2026-03-31-core-crates-hardening.md`](https://github.com/vanyastaff/flui/blob/e30ab7194d50ac1c11ffe17c59230958d2fbeecd/docs/plans/2026-03-31-core-crates-hardening.md) Task 3). Not on the render hot path; consumed by the build phase. |
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
- **State-notification surface decided** — `Notifier`/`ChangeNotifier` in this crate is the listener-notification mechanism. The signals crate that the summary table once pointed at (`flui-reactivity`) was removed 2026-07-28. Runtime-scoped signals (ADR-0074) are not a crate: their read contract is this crate's `read_scope` module and their graph lives in `flui-view` (ADR-0085). The `Arc<Mutex<…>>` notifier stays for `Send + Sync` users until the UI callback surface loses `Send` (ADR-0091 §1), when a `Listenable` adapter over a signal replaces it.

---

## Mapping decisions

### Notification channels retain only the surfaces they serve

`Notifier<T>` lends typed arguments; `ChangeNotifier` adapts it to zero-argument
listeners. `ValueNotifier<T>` owns its value without a `Clone` requirement on
reading, mutation or extraction. Its derived `Clone` remains available for
cloneable values, copying the value while sharing the listener channel.
`into_value` disposes that shared channel before extracting the value.
The public `notifier_ownership_and_recovery` family includes a non-Clone owned
value's mutation/extraction sequence and clone compatibility.

`ValueNotifier<T>` implements `Drop` to order and retain its owned parts, so borrowed data inside `T` must
outlive the notifier; this is a drop-check requirement, not a `T: 'static`
bound.

The separate `ListenerRegistry`/`ListenerSubscription` surface is removed.
It had no production consumer; its lazy first/last hooks duplicated notification
ownership and exposed a callback-under-lock transaction. Typed and zero-argument
notification continue through the channels above.

### Claim slots commit outcomes before delivering borrowed wakes

The ADR-0039 claim-slot state machine remains the authority for ownership of a
reply. Executor cloning and displaced-waker destruction run outside the slot's
locks; a clone can synchronously deliver, and the subsequent state check must
observe that outcome. Delivery and owner disconnection publish their terminal
state before notifying blocked or asynchronous requesters. Abandonment retains
the reply for the owner to reclaim even when its wake callback fails, and still
attempts the task wake.

Wake invocation borrows an owning executor envelope held outside catch_unwind.
The first caught wake or retirement failure propagates during ordinary calls;
later failures and failures during an existing unwind are retained. After a
caught failure or during active unwind, shared state and opaque envelopes are
retained so reply or callback captures cannot introduce a competing destructor
failure. Successful ordinary retirement still runs destructors, and an
individual aggregate that double-panics before containment regains control can
abort. This is exceptional-path retention, not a guarantee against arbitrary
destruction inside user callbacks.

The public subprocess family `claim_slot_executor_and_owner_recovery` covers
clone reentry, failed clone and replacement retirement, committed delivery
followed by wake or retirement failure, owner disconnection during unwind,
owner/task failure competition on abandonment, and reclamation of an unclaimed
reply after failure. Each scenario also checks the next request.

### Borrow arguments and retain exceptional notification obligations

Clearing, disposal and final-owner destruction take the listener map out of
its lock, then drop callbacks one at a time in registration order; surviving
notifier clones keep the map alive. `ValueNotifier` drops its value, then its
channel; `into_value` disposes the channel before returning the value. After
the first destructor panic in one of these operations, or when one starts while
the thread is already panicking, the remaining captures and the value are
retained (ADR-0127). A `ValueNotifier`'s channel handle is a shared clone, so
it is released even then; the last owner's storage retains the captures. A failure a caller caught earlier is not visible through
`thread::panicking()`, so that caller retains its own failed value. Pinned by
`notifier_ownership_and_recovery`.

Typed notification callbacks borrow their argument and do not require Clone.
The notifier's owned snapshot prevents a removed callback from disappearing
while it runs. After a caught listener failure, the payload and snapshot remain
retained: opaque capture or panic-payload aggregates can double-panic during
drop before catch_unwind regains control. Later listeners and later notification
rounds still progress. Reporting borrows the original payload behind a separate
unwind boundary; a panicking tracing subscriber cannot interrupt notification,
and its secondary payload is also retained. Normal success retires callback
envelopes one at a time in registration order; a retirement failure propagates
after retaining the remaining envelopes. An individual aggregate double-panic
during ordinary successful-round retirement keeps Rust's abort behavior.

The public subprocess table `notifier_ownership_and_recovery` checks borrowed
non-Clone arguments, hostile payloads and self-removal captures, tracing failure
in competition with a listener failure, chronological retirement competition
and subsequent progress. The common exceptional-payload operation is
`panic::retain_opaque_payload`, used by notifications, signal reads
and the test-table runner. This contract is recorded in
[ADR-0104](../../docs/adr/ADR-0104-borrowed-notification-and-opaque-panic-retention.md).
As refined by [ADR-0119](../../docs/adr/ADR-0119-inert-panic-payload-retirement.md),
the shared operation releases exact `&'static str` and `String` payloads, whose
destruction cannot call user code. Other dynamic payload types remain retained;
callback snapshot retention and recovery ordering are unchanged.


## `Matrix4::lerp` decomposes in `f64` and borrows orientation for a collapsed axis

[ADR-0149](../../docs/adr/ADR-0149-interpolation-contracts.md) item 3.
`Matrix4::lerp` does its own decomposition (`geometry/matrix4_decompose.rs`)
rather than glam's scale/rotation/translation split, which divides by a zero
scale (NaN on every interior frame of a scale-in from zero) and drops skew and
perspective. Gram–Schmidt on the linear block's columns gives scale, shear and
an orthonormal rotation (a quaternion, slerped along the shorter arc); the
bottom row gives perspective. A column whose residual is within `1e-12` of the
longest column is a collapsed axis: that endpoint keeps its scales, translation
and perspective and takes the other endpoint's rotation and skew, so a
scale-in grows in place. This rule is FLUI's own; CSS has no such case. `m33 = 0`, an `m33` whose division overflows,
or a perspective row over a singular block switch at `t = 0.5`. Locked by
`matrix4_lerp_decomposes_like_css_transforms` and the property test
`matrix4_lerp_endpoints_and_finiteness`.

### `Angle` keeps whole turns

[ADR-0149](../../docs/adr/ADR-0149-interpolation-contracts.md) item 4. `Angle`
is not reduced modulo a turn, so its `Lerp` is numeric and a multi-turn rotation
is a value. `nearest_equivalent` reduces the difference to a reference into
`(-½, ½]` turn; the exact half turn goes to the increasing angle. Locked by
`angle_nearest_equivalent_takes_the_shorter_arc`.

### Closed signal graph

Once a presentation closes, its signal graph refuses reads, writes and new
signals with `SignalError::OwnerClosed`
([ADR-0123](../../docs/adr/ADR-0123-exceptional-presentation-close.md)). After an
exceptional close the graph's existing values are retained rather than dropped
([ADR-0127](../../docs/adr/ADR-0127-exceptional-path-retention.md)), but they
cannot be read or written through it.
