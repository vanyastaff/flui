# flui-foundation Architecture

Design notes for the base crate every other FLUI crate builds on: IDs, geometry,
listener notification, the signal read contract and the train guard.

## Admission

A public item here needs a production caller or a named follow-up; a rustdoc or
its own test is not a consumer. A general-purpose foundation primitive belongs
here only when at least two crates in different tiers use it. A value serving a
single subsystem lives with that subsystem instead.

## Monotonic identities

`InputSequenceId` is a nonzero, presentation-local identity issued by an
owner-local `IdMint<InputSequenceId>` (ADR-0207 §4). It is distinct from reusable
device pointer IDs and from arena-local member generations. Exhaustion refuses
permanently; earlier identities remain valid labels for accepted contacts and
their motion (`an_exhausted_sequence_mint_refuses_permanently`). Binding-driven
gesture tests cover actual admission and pointer reuse
(`binding_admits_distinct_drag_sequences_when_pointer_ids_are_reused`). This
supplies the identity contract required by
[ADR-0209](../../docs/adr/ADR-0209-nested-scroll-connection.md), without making
process-global state or serialization a second identity issuer.

`FrameCallbackId` uses the same owner-local mint in its scheduler. Listener
registries use `IdMint<ListenerId>`; clones of `Notifier` share one mint through
`Rc`, while independent owners keep independent sequences. A `ListenerId` is
either a minted registration or a typed inert admission result. The inert form
has no numeric identity and cannot withdraw a callback from another source.
`TaskId` uses `AtomicIdMint`, which shares the sealed `MintedId` construction and
permanent-exhaustion contract but permits concurrent standalone task creation.
These identity types have private numeric constructors. The public sequence
tests cover repeated refusal and a race for the last identity; neither mint has
a reset operation.

## The mint family and the ids they issue

`flui_foundation::sequence` is the workspace's only issuer of identities.
`IdMint` is the owner-local mint; `AtomicIdMint` is its cross-thread twin for
an id domain more than one thread mints (the scheduler's task and callback
identities). Both share the sealed `MintedId` contract, whose `MINT_CEILING`
keeps every raw value convertible to the identity type's own representation,
and the same refusal rule: an issued id is never issued again, and an
exhausted mint refuses, typed (`IdExhausted`), forever
(`the_atomic_mint_issues_the_same_sequence_the_owner_local_mint_does`,
`an_exhausted_atomic_mint_refuses_permanently`,
`an_atomic_mint_admits_exactly_one_of_concurrent_requests_for_the_last_identity`,
`a_pointer_width_bounded_id_refuses_past_its_type_ceiling`,
`scheduler_and_listener_ids_are_minted_by_the_family`). The counter ids with
no slab behind them (`ListenerId`, `TaskId`, `FrameCallbackId`) are minted by
the family; the slab-backed ids stay the slot plus one.

## Slot generations (ADR-0313)

A packed generational key (`ElementId`, `RenderId`, `UiRuntimeId`) names one
slot with a `generation::Generation`: 1-based, checked, never wrapped, minted
only by the slot arena's `generation::GenSlots` ledger. A release preflights
its step with `GenSlots::peek` before user code runs and commits it with
`GenSlots::commit` after; a caught panic between the two steps nothing. A
slot whose generation space is exhausted closes at its release and never
mints again: the signal graph re-lists nothing, the element tree refuses the
removal with its occupant kept, and the render slab marks the slot `Retired`
and keeps it off the free list. The render layout lease checks both the
ledger generation and `Occupied`, because a retired entry retains its last
generation.

## The mint family and the ids they issue

`flui_foundation::sequence` is the workspace's only issuer of identities.
`IdMint` is the owner-local mint; `AtomicIdMint` is its cross-thread twin for
an id domain more than one thread mints (the scheduler's task and callback
identities). Both share the sealed `MintedId` contract, whose `MINT_CEILING`
keeps every raw value convertible to the identity type's own representation,
and the same refusal rule: an issued id is never issued again, and an
exhausted mint refuses, typed (`IdExhausted`), forever
(`the_atomic_mint_issues_the_same_sequence_the_owner_local_mint_does`,
`an_exhausted_atomic_mint_refuses_permanently`,
`an_atomic_mint_admits_exactly_one_of_concurrent_requests_for_the_last_identity`,
`a_pointer_width_bounded_id_refuses_past_its_type_ceiling`,
`scheduler_and_listener_ids_are_minted_by_the_family`). The counter ids with
no slab behind them (`ListenerId`, `TaskId`, `FrameCallbackId`) are minted by
the family; the slab-backed ids stay the slot plus one.

## Listener subscriptions (the owned listener)

A listener paired by hand — an id held apart from its source, removed later
through a remove call — is wrong-channel by construction:
ids are per-channel counters that start at one, so the channel they name is
not in the value. The owned form is the channel's `listen`
(`ChangeNotifier::listen`, `ValueNotifier::listen`, `Notifier::listen`),
which hands back a `Subscription`: cancel-on-drop, `detach` for an early
exit, and a detach that knows its channel, so it removes exactly its own
registration (`a_subscription_detaches_only_its_own_channel`,
`a_subscription_detached_inside_a_round_is_skipped_and_retired_at_its_tail`).
No id-based registration or withdrawal is a `Listenable` method
([ADR-0109](../../docs/adr/ADR-0109-notification-channel-surface.md) decision
5): the id machinery is the channel level's own
(`take_listener`, `take_listeners`, `add_listener`, `add_with_recovery`),
reached through concrete types, not through the trait.

## Slot generations (ADR-0313)

A packed generational key (`ElementId`, `RenderId`, `UiRuntimeId`) names one
slot with a `generation::Generation`: 1-based, checked, never wrapped, minted
only by the slot arena's `generation::GenSlots` ledger. A release preflights
its step with `GenSlots::peek` before user code runs and commits it with
`GenSlots::commit` after; a caught panic between the two steps nothing. A
slot whose generation space is exhausted closes at its release and never
mints again; the signal graph re-lists nothing there, the element tree
refuses the removal with the occupant kept. The render slab keeps its
hand-rolled ledger until #2241 (gaps entry 43).

## The geometry values (`geometry`)

`Matrix4::project_plane_point` checks the forward projection of the local
`z = 0` plane. It refuses the camera horizon, hidden points, non-finite inputs
and unrepresentable output. Its homogeneous quotient ignores irrelevant depth
coefficients and preserves positive common matrix scales. An affine quotient
can remain representable when its intermediate product overflows; divide
coefficients by the positive homogeneous weight before retrying that coordinate.
The public `matrix_tolerance_requires_finite_values` family pins this alongside
tilted planes, tiny/huge common scales and finite extremes. Inverse planar
queries share this arithmetic, while general 3D unprojection keeps the ray-plane
intersection contract.

The `f64` implementation of `Lerp` preserves authored endpoint bits. A fused
multiply-add recovers a representable extrapolation after product overflow;
a weighted sum handles an overflowing difference. Opposite finite extremes interpolate to
representable intermediate values; extrapolation and invalid intermediate inputs
remain visible rather than being clamped. The `Offset`, `Size`, `Rect`, edge and
radius implementations share this scalar path. The public family
`scalar_and_composite_lerp_preserve_representable_values` checks finite extreme
interpolation, signed zero and NaN endpoint bits, extrapolation product overflow
and NaN propagation.

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
non-finite components returning zero. Infinite offsets therefore produce zero
rather than NaN components. The family also checks `Offset::move_towards` taking
a finite unit step toward a target with an overflowing magnitude. Subtraction
and maximum-distance limiting retain their own floating-point range limits.

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
(`SignalError`, whose `Display` and `Error` implementations are written explicitly), the
read-only graph face (`ReadGraph`: `graph_id` and `read_erased`), the subscription sink
(`ReaderSink::subscribe(slot)`), and what a context hands a read
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
before containment regains control. After a reader or graph panic, the callback envelope,
any later result and any later panic payload are destroyed under containment, and a panic
from their destructors is discarded so the first failure propagates (ADR-0281). The typed
adapter records the
original reader payload outside the graph invocation, then propagates a destructor-free
unit unwind marker through the erased reader. This makes failure visible to the graph
before it finalizes a released loan while preserving the original payload's priority
over graph cleanup and subscription failures. The released-read subprocess rows in
`flui-view`'s `signal_read_and_write_matrix` cover that actual cross-crate bridge.

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

The guard sits here, not on `flui-sdk`, because the facade, SDK and platform/framework
layers converge on foundation through normal dependencies, while the facade does not depend
on the SDK. A guard on the SDK alone would not separate an application on one `flui` train
from a package on another `flui-sdk` train.

Invariants, checked by `cargo xtask workspace`: this crate declares the guard and no other member
does. `two_trains_refuse_to_resolve` in `tools/xtask` builds two copies of this crate with this
manifest's `links` value into one graph and requires the resolver to refuse it. Dropping the key,
or moving it to a crate that not every train depends on, re-opens the E0308 failure the guard
exists to prevent.

---

## Typed inspection (`inspect`)

`inspect` is ADR-0242's typed description model. `Inspect` describes an object's own fields;
`InspectTree` supplies identity and structure. Invariants:

- **Describing allocates only in the sink.** `Inspect::inspect` writes borrowed `Field`s into
  one `&mut dyn Sink`; `Value` is `Copy` and owns nothing. `inspect_allocation` (its own test
  target, because it installs a counting allocator) walks every value kind, a sequence and a
  `Debug` field with a counting sink and requires zero allocations; the snapshot sink is its
  positive control.
- **A description says what the object holds, infinity and NaN included.** An unconstrained
  maximum is infinity, which is meaningful, and a NaN is usually a bug worth seeing, so `Fields`
  never turns either into an absent value. The wire carries them as a typed value
  (`flui_protocol::InspectValue::NonFinite`), a composite holding one as its `Debug` text.
  `a_non_finite_number_is_described_as_itself` pins it.
- **The value set is closed.** A consumer that handles every `Value` handles every description,
  which is what lets the owned snapshot be a wire contract (`flui_protocol::InspectNode`). The
  colour type lives in `flui-painting`, so a description carries `Rgba` channels.
- **Values compare by value; `Value::Debug` equals nothing,** itself included, because its type
  is unknown. A snapshot counts `Debug` fields per node so a ratchet can drive them down.
- **Identity and structure belong to the tree.** An object never describes its children or its
  id: `InspectTree` yields `InspectId`s over the generational ids, children, `Links` and the
  tree's own facts. A reused slot prints with its generation (`r12.2`), so a stale id never reads
  as the node that replaced it.
- **One pinned text form.** `Display` for `Size` (`360x48`), `Offset` (`@8,8`), `Rect`
  (`8,8 344x32`), `EdgeInsets` (`8` or `top,right,bottom,left`) and every `Value` prints numbers
  in Rust's shortest round-trip form, and `geometry_text_parses_back_to_the_same_values` parses
  them back bit for bit. `Debug` is not an output format.
- **The diff's tolerance is the unit's.** Pixels for geometry and `logical_px`/`device_px`
  fields, and an exact comparison for a field with no unit.

---

## Inspection, diagnostics and serialization

Typed inspection and structured diagnostics are separate surfaces.
`inspect::snapshot` builds an owned `SnapshotNode` with tree facts, object fields,
links and children; `inspect::render` and `SnapshotNode`'s `Display` produce its
human-readable outline. The optional `serde` feature implements serialization for
foundation value types, including keys, IDs, geometry and text-sizing values. It
does not define `Serialize` or `Deserialize` for `inspect::Value`, `OwnedValue`
or `SnapshotNode`. The `diagnostics` module owns the structured `tracing` wire vocabulary, not a
serializable node tree. `FrameSpan` gives every framework frame, presentation,
phase and animation span one typed `(target, name)` identity: producers open
spans through `target()`/`name()`, and consumers recover the same identity with
`FrameSpan::of`. `FrameSpan::ALL` and the public diagnostics contract test pin
the complete table, its uniqueness and negative matches. `RUNTIME`,
`PRESENTATION` and `PERIOD_US` name the frame-partition and display-period
fields shared by producers and capture; `PRESENTATION_ID` and `REDACTED_VALUE`
retain their existing correlation and privacy roles. `LayoutRoot` is diagnostic
detail rather than a profiler phase, so devtools admits its captured subset
explicitly instead of treating every variant as profile data.

---

## Thread safety

`flui-foundation` is tier V and has a normal dependency on `flui-macros` to
re-export `#[derive(Inspect)]`. Its concurrency primitives live at these seams:

| Site | Primitive | Where | Rule |
|------|-----------|-------|------|
| `Key::new` | `KEY_COUNTER: AtomicU64` | `key.rs` | Process-wide, 1-based key sequence. `try_update` installs a sticky zero exhaustion sentinel instead of wrapping and reissuing keys. |
| `UniqueKey::new` | `UNIQUE_KEY_COUNTER: AtomicU64` | `key.rs` | Independent sequence with the same sticky-exhaustion rule as `Key::new`. |
| `event_epoch` | `EVENT_EPOCH: LazyLock<Instant>` | `clock.rs` | Initialized once and fixed for the process, so native input timestamps and `SystemClock::epoch` share an epoch. |
| `GpuResourceGeneration::mint` | Function-local `NEXT: AtomicU64` | `epoch.rs` | Mints process-wide GPU-resource generations starting at 1 with `fetch_add`. |
| `ClaimSlot` / `ClaimHandle` | `parking_lot::Mutex` for state and the registered waker; `parking_lot::Condvar` for blocked requesters | `claim_slot.rs` | State transitions are linearized under private locks. Executor and wake code runs after the corresponding lock is released. |
| `ManualClock::elapsed` | `Arc<parking_lot::Mutex<Duration>>` | `clock.rs` | Clones share one virtual timeline; `advance`, `now` and `elapsed` synchronize through the mutex. |
| `Notifier::listeners` | `Rc<ListenerStorage>` with `RefCell<HashMap<ListenerId, …>>` | `notifier_generic.rs` | Owner-local membership. Delivery snapshots registration order and releases every borrow before invocation or retirement. `ChangeNotifier` adapts this same storage. |
| `Notifier::next_id` | `Rc<IdMint<ListenerId>>` | `notifier_generic.rs` | Clones share monotonic owner-local identities. Exhaustion refuses permanently before an identity can be reissued. |

No `RwLock` in `flui-foundation`. No primitive listed here sits inside `perform_layout` / `paint` / `View::build`.

---

## Mapping decisions

### Notification channels retain only the surfaces they serve

`Notifier<T>` lends typed arguments; `ChangeNotifier` adapts it to zero-argument
listeners. These owner-local channels carry state notification (ADR-0178).
Runtime-scoped signals retain their separate graph and dependency tracking in
`flui-view`; their read contract lives in `read_scope` (ADR-0085).

Notification state is owner-local. A round owns a registration-ordered snapshot
while reentrant registration changes live membership: additions join the next
round, and removal or disposal silences the remaining selection. The framework's
pre-delivery observation hook runs after that snapshot and its capture custody
are established, with no state borrow held. Disposal and removal from the hook
silence the remaining selection; additions wait for the next round.
`animation_observation_contract` exercises this seam through real animation
profiling subscribers at span creation and entry. Framework relays borrow
enclosing failure custody rather than create another notification path. Worker
replies and wakes retain their separate Send edges.

`ValueNotifier<T>` owns its value without a `Clone` requirement on
reading, mutation or extraction. Its derived `Clone` remains available for
cloneable values, copying the value while sharing the listener channel.
`into_value` disposes that shared channel before extracting the value.
The public `notifier_ownership_and_recovery` family includes a non-Clone owned
value's mutation/extraction sequence and clone compatibility.

`ValueNotifier<T>` implements `Drop` to order its owned parts, so borrowed data inside `T` must
outlive the notifier; this is a drop-check requirement, not a `T: 'static`
bound.

Notification ownership stays in `Notifier`/`ChangeNotifier`; there is no separate
`ListenerRegistry`/`ListenerSubscription` API. Lazy first/last-listener hooks are
not a second notification path: they would duplicate channel ownership and
require a callback-under-lock transaction. Typed and zero-argument notification
use the channels above.

### Claim slots commit outcomes before delivering borrowed wakes

The ADR-0039 claim-slot state machine remains the authority for ownership of a
reply. Executor cloning and displaced-waker destruction run outside the slot's
locks; a clone can synchronously deliver, and the subsequent state check must
observe that outcome. Delivery and owner disconnection publish their terminal
state before notifying blocked or asynchronous requesters. Abandonment keeps
the reply for the owner to reclaim even when its wake callback fails, and still
attempts the task wake.

Wake invocation borrows the withdrawn executor envelope, then destroys it; both
run contained. The first caught wake or destructor failure propagates during
ordinary calls; later failures, and every failure during an existing unwind,
are discarded with `panic::discard_payload`. Shared state and envelopes are
destroyed normally on every path: a destructor that panics while the thread is
already unwinding aborts, as in std (ADR-0281).

The public subprocess family `claim_slot_executor_and_owner_recovery` covers
clone reentry, failed clone and replacement retirement, committed delivery
followed by wake or retirement failure, owner disconnection during unwind,
owner/task failure competition on abandonment, and reclamation of an unclaimed
reply after failure. Each scenario counts the destroyed captures and checks the
next request.

### Borrow arguments and destroy withdrawn notification obligations

Clearing, disposal and final-owner destruction take the listener map out of
its borrow, then drop callbacks one at a time in registration order; surviving
notifier clones keep the map alive. `ValueNotifier` drops its value, then its
channel; `into_value` disposes the channel before returning the value. If a
destructor panics, the remaining captures are still dropped and the first panic
propagates; a destructor that panics while the thread is already unwinding
aborts, as in std (ADR-0281). Pinned by `notifier_ownership_and_recovery`.

Typed notification callbacks borrow their argument and do not require Clone.
The first callback failure resumes after the healthy tail; removing a listener
or disposing the channel immediately silences its remaining snapshot entries.
Active delivery defers outgoing callbacks until the outermost round retires
them. Framework relays borrow `panic::RecoveryScope` from an owning
`PanicRecovery`; only the owner can complete the enclosing delivery. Reentrant owner cleanup
inherits active channel failure custody without an ambient registry (ADR-0178).
The borrowed recovery marks all active channel signals as soon as a nested relay
catches a failure, including before returning to its parent. Channel depth owns
signal reset; nested completion cannot clear an enclosing failed delivery.
Signal links borrow their enclosing stack frames and channel-local atomics;
wrapper depth does not grow a heap collection or retain channel storage.
The recovery decisions also keep independent scopes: destruction of caught
payloads and withdrawn values is
[ADR-0281](../../docs/adr/ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md),
and live notification membership plus borrowed first-failure custody is
[ADR-0178](../../docs/adr/ADR-0178-notification-first-failure-custody.md). An enclosing
operation owns recovery; this map does not join independent runtime or platform
operations into one global failure boundary.

The notifier's owned snapshot prevents a removed callback from disappearing
while it runs. After a caught listener failure, the recovery owns the original
payload until completing delivery, and outgoing callback envelopes retire one
at a time under its containment (ADR-0281 §3). Healthy live listeners and later
notification rounds still progress. Reporting borrows the original payload behind a separate
unwind boundary; a panicking tracing subscriber cannot interrupt notification,
and its secondary payload is discarded under containment. Normal success retires callback
envelopes one at a time in registration order; the first retirement failure
propagates after the remaining envelopes are retired. A capture aggregate that
panics again while one of its own fields unwinds aborts, as in std.

The public subprocess table `notifier_ownership_and_recovery` checks borrowed
non-Clone arguments, hostile payloads and self-removal captures, tracing failure
in competition with a listener failure, chronological retirement competition
and subsequent progress, including recursive relay retirement through the same
channel. This contract is recorded in
[ADR-0104](../../docs/adr/ADR-0104-borrowed-notification-and-opaque-panic-retention.md),
with current delivery/propagation custody under ADR-0178. Caught payloads that
are not propagated are dropped where the decision is made
([ADR-0281](../../docs/adr/ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md)):
a payload or capture whose destructor panics while the thread unwinds aborts,
as in std.


### Owners commit state before running user code

[ADR-0287](../../docs/adr/ADR-0287-owner-state-is-committed-before-user-code-runs.md).
`panic::commit` borrows an owner's `RefCell` for one mutation and hands it the
state and an `Outbox`, never the cell; `Outbox` has no public constructor. User
code the change owes is queued there (`call`, `wake`, `release`) and runs after
the borrow ends, in order, each item contained, the first panic resumed once
every item ran. A call is an `FnMut` called at most once, so its captures are
destroyed after it, contained, and never by its own unwind (ADR-0281 §3). A
mutation that panics runs none of its calls but still wakes
its wakers. `commit_into` does the same inside an operation's `PanicRecovery`.
`panic::Obligation` claims an owed flag and sets it again unless the delivery
discharges it. `TicketSource<K>` issues opaque `Ticket<K>`s from an `IdMint`
and accepts an answer only for the current ticket, once. Pinned by
`commit_then_call_contract`.

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
exceptional close the graph's existing values are not released element by element; they stay
in the graph, which cannot read or write them, until it is destroyed with its presentation under
the close's containment
([ADR-0281](../../docs/adr/ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md)
§3).
