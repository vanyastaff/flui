# FLUI Rust Style and Engineering Standard

> Normative code-quality rules for FLUI. This document targets Rust 1.97,
> Edition 2024, and the architecture described by the repository's ADRs.

## 1. Purpose and precedence

This is not a formatting guide disguised as an engineering standard. `rustfmt`
owns formatting. This document defines how FLUI code remains safe, predictable,
reviewable, and maintainable as the framework grows.

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are
normative.

When rules appear to conflict, use this order:

1. Rust soundness and observable correctness.
2. Root and crate-local `AGENTS.md` architecture contracts.
3. Accepted ADRs and the Flutter-port behavior contract.
4. This document.
5. Local convention in the module being changed.

Do not use "style" to justify changing Flutter-compatible core behavior. The
three-tree model, lifecycle, reconciliation, layout, paint, hit testing, and
semantics remain behavior-first ports. Rust-native improvements belong in
representation, ownership, diagnostics, and explicitly sanctioned leapfrog
zones.

### 1.1 Adoption and exceptions

This standard applies immediately to new and modified code. Existing code may
carry documented technical debt; touching a module does not require an
unrelated rewrite, but a change MUST NOT add new violations or rely on existing
debt as precedent.

An exception is acceptable only when all of the following are true:

- the general rule would make this specific code less correct or less clear;
- the exception is scoped to the smallest item or module;
- the reason is written next to the suppression or recorded in an ADR;
- safety, observable behavior, and test coverage are not weakened;
- a removal condition or owning issue exists when the exception is temporary.

Repository-wide exceptions belong in workspace configuration or an ADR, never
in a growing collection of copy-pasted `allow` attributes.

## 2. Toolchain and automated baseline

- Code MUST compile on the workspace MSRV, Rust 1.97.
- Code MUST use stable Rust. Nightly features require an accepted ADR and a
  separate compatibility strategy.
- Every crate MUST inherit workspace lints with `[lints] workspace = true`.
- `cargo fmt` is authoritative for formatting. Do not hand-align code against
  rustfmt output.
- Clippy warnings are errors in CI. A lint suppression MUST be narrow and carry
  a reason that explains why the code is clearer or more correct as written.
- Do not add crate-wide `allow` attributes to make a new warning disappear.
- New code MUST NOT depend on APIs stabilized after the declared MSRV.
- `cfg` branches not compiled on Linux MUST still pass the repository's
  cross-target checks.

Required pre-PR gate:

```bash
just ci
taplo fmt --check
typos
```

Run the narrower crate gate while iterating, but never present it as a
substitute for the workspace gate.

## 3. Formatting and source organization

### 3.1 Formatting

- Accept rustfmt output without local exceptions.
- Keep normal source lines within the configured 100-column width. Long URLs,
  generated tables, and compiler-required strings are reasonable exceptions.
- Prefer trailing commas in multiline expressions so rustfmt produces stable
  diffs.
- Group code by responsibility, not by visibility or item kind alone.
- Keep the public entry points near the top of a module and implementation
  details below them when that improves navigation.

### 3.2 Modules and files

- A module MUST have one coherent responsibility and a name that describes it.
- Split a file when independent invariants are being maintained, not because it
  crossed an arbitrary line count.
- Avoid `mod.rs` re-export mazes. A public item should have an unsurprising
  canonical path.
- Re-export intentionally. Wildcard re-exports are reserved for curated
  preludes and tightly controlled facade modules.
- Visibility MUST be the narrowest that serves the current consumer:
  private, `pub(super)`, `pub(crate)`, then `pub`.
- Do not publish speculative extension points. Extract an abstraction after it
  has two real consumers or one consumer with materially different backends.

### 3.3 Imports

- Let rustfmt order imports.
- Import the item used, not an entire module namespace, unless the namespace
  communicates essential context.
- Avoid glob imports outside preludes, generated code, and tests where the
  imported vocabulary is deliberately the test DSL.
- Prefer explicit qualification when two domains use similar names.

## 4. Naming

Names MUST communicate role, ownership, and units without requiring the reader
to inspect the implementation.

- Types and traits use `UpperCamelCase`; functions, methods, modules, and local
  variables use `snake_case`; constants use `SCREAMING_SNAKE_CASE`.
- Avoid project-specific abbreviations unless they are established domain terms
  such as GPU, UI, ID, DPI, or FFI.
- Do not encode types in names: prefer `users`, not `user_vec`.
- Include units when the type system does not: `timeout_ms`, `byte_len`,
  `frame_index`.
- Prefer unit newtypes when mixing coordinate spaces, durations, identifiers,
  or generations would be a correctness bug.
- Boolean names describe the true state: `is_visible`, `has_focus`,
  `should_rebuild`. Avoid negated names such as `disable_cache`.
- Predicates read as questions. Mutating operations use verbs.
- Getters do not use a `get_` prefix unless they perform a lookup or follow an
  established protocol.
- Conversions follow Rust conventions:
  - `as_` for cheap borrowed views;
  - `to_` for potentially allocating conversions;
  - `into_` for ownership-consuming conversions.
- Iterator methods use `iter`, `iter_mut`, and `into_iter`.
- A type named `Builder`, `Guard`, `Handle`, `Token`, `Snapshot`, or `Owner`
  MUST actually provide the lifetime or ownership semantics its name implies.

Do not place private planning markers, review IDs, or agent history in source
names and comments. State the durable invariant instead.

## 5. Public API design

### 5.1 Make invalid states unrepresentable

Prefer, in order:

1. A type that cannot represent invalid input.
2. A checked constructor returning `Result`.
3. Validation at the public boundary.
4. A documented internal invariant assertion.

Do not accept a primitive and document a hidden domain when a small type can
enforce it. FLUI's unit wrappers, arity markers, typed IDs, and lifecycle
tokens are the model.

Public boolean parameters SHOULD become enums when the call site would
otherwise be ambiguous:

```rust
// Avoid: the meaning of `true` is invisible at the call site.
window.set_mode(true);

// Prefer.
window.set_mode(WindowMode::Fullscreen);
```

Configuration structs may contain booleans when field names remain visible.

### 5.2 Constructors and builders

- Use `new` for the primary unsurprising constructor.
- Implement `Default` only when there is a meaningful, valid default.
- Do not use `Default` to create a partially initialized value.
- Builders are appropriate for optional, evolving, or validation-heavy
  configuration. Required values belong in the constructor or typestate.
- A builder's `build` method MUST validate cross-field invariants.
- Consuming builders are preferred for immutable configuration objects.
- Fallible construction returns a typed error; it does not log and continue
  with silently altered semantics.

### 5.3 Traits

- A trait represents a stable behavioral contract, not a way to share three
  lines of implementation.
- Keep required methods minimal. Provide default methods only when their
  semantics are valid for every implementation.
- Document object-safety expectations when dynamic dispatch is intended.
- Seal traits whose downstream implementations would prevent future evolution
  or violate framework invariants.
- Do not add `Send + Sync` bounds "for flexibility". Add them only when the
  value is semantically allowed to cross or be shared between threads.
- Associated types are preferred when one implementation has one natural type;
  generic parameters are preferred when callers choose the type per use.

### 5.4 Evolution

- Public struct fields SHOULD be private unless direct construction is the
  deliberate long-term contract.
- Public enums that are expected to grow SHOULD be `#[non_exhaustive]`.
- Public error enums SHOULD be `#[non_exhaustive]` unless exhaustive matching
  is an intentional promise.
- Implement applicable common traits eagerly: at minimum consider `Debug`,
  `Clone`, `Copy`, `Default`, `Eq`, `Ord`, and `Hash`.
- `Display` is user-facing and stable; `Debug` is diagnostic and MUST be useful
  but need not be stable.
- Use `#[must_use]` when silently discarding a value almost certainly loses an
  effect, guard, transaction, or prepared operation.
- Breaking changes are currently permitted, but they still require migration
  clarity and a reason better than aesthetic preference.

### 5.5 Standard parameter shapes

- Borrowed text is `&str`; owned text is `String`.
- Borrowed filesystem paths are `&Path`; owned paths are `PathBuf`.
- Borrowed contiguous data is `&[T]` or `&mut [T]`, not `&Vec<T>`.
- Accept `impl Into<T>` mainly at ownership-taking construction boundaries.
  Do not make every method generic when accepting `T` or `&T` is clearer.
- Accept `impl AsRef<Path>` only when supporting several path-like caller types
  is valuable enough to justify the generic surface.
- Return iterators when streaming or laziness is part of the contract; return a
  collection when ownership and a completed snapshot are the contract.
- Use `Option` for absence, not sentinel integers, empty strings, null handles,
  or magic coordinates.

### 5.6 Static and dynamic dispatch

- Prefer concrete types when there is one implementation.
- Prefer generics when the caller selects behavior and monomorphization cost is
  bounded.
- Use trait objects at genuine runtime substitution or ownership boundaries,
  not to avoid naming an enum or associated type.
- New `dyn` storage MUST comply with the sanctioned-boundary policy enforced by
  port-check.
- Hot per-node dispatch requires measurement before introducing a trait object.
- Do not expose a public generic parameter that callers cannot use meaningfully.

## 6. Ownership, borrowing, and lifetimes

- Ownership should match the domain owner, not the easiest compiler fix.
- Borrow inputs when the callee only observes them and ownership transfer adds
  no value.
- Consume immutable UI configuration when consumption makes lifecycle and
  identity unambiguous.
- Return owned data when a borrow would expose internal storage or couple the
  caller to a lock guard.
- Do not clone to bypass a borrow-design problem. First identify the actual
  owner and required lifetime.
- A clone in a hot path SHOULD have an explicit cost argument or measurement.
- Use `Arc` only for genuinely shared ownership across threads or independent
  lifetimes. Do not use it merely to satisfy `Send`.
- Owner-thread state SHOULD use owner-affine types. It MUST NOT gain unsafe
  `Send` or `Sync` implementations to satisfy an inconvenient API.
- Weak references are appropriate for non-owning back-references. Their upgrade
  failure is a lifecycle state and must be handled deliberately.
- Avoid self-referential structures. Prefer stable IDs, arenas, owned buffers,
  or pinned abstractions with a reviewed invariant.
- Lifetimes communicate real relationships. Do not add a lifetime parameter
  solely to avoid a small, intentional owned value.

## 7. IDs, indices, and arenas

FLUI public tree IDs are one-based `NonZeroUsize` values over zero-based arena
indices.

- Insertion converts `slab_index + 1` to the public ID.
- Lookup converts `id.get() - 1` to the arena index.
- Conversion logic belongs in one reviewed boundary, not duplicated across
  callers.
- Never interchange `ViewId`, `ElementId`, `RenderId`, `LayerId`, or
  `SemanticsId`, even if their representation matches.
- Stale-ID behavior MUST be defined. If reuse is possible, generation handling
  belongs inside the arena accessors.
- Integer overflow policy MUST be explicit for indices and generations.
- An ID is not proof that an object is alive; APIs MUST model lookup failure or
  prove the lifetime through ownership.

## 8. Error handling and panics

The detailed policy is in `docs/PANIC-POLICY.md`.

- Caller-triggerable failures return `Result`.
- Library crates use typed `thiserror` errors.
- Application and CLI composition code may use `anyhow` for contextual
  aggregation, but public library APIs MUST NOT expose `anyhow::Error`.
- Errors describe the failed operation and retain the source error where one
  exists.
- Do not log an error and return it at the same abstraction level. The owner
  that decides recovery or presentation owns the log.
- `unwrap()` is forbidden on production paths.
- `expect` is reserved for internal invariants and uses
  `expect("BUG: <violated invariant>")`.
- A panic is not input validation, feature detection, or platform dispatch.
- `catch_unwind` is permitted only at an explicit user-code or FFI boundary
  where the enclosing transaction remains valid after unwinding.
- Never catch a panic after partially mutating a tree unless rollback or whole
  subtree replacement is the documented invariant.
- Destructors MUST NOT panic. Cleanup failure is reported before `Drop` or sent
  to diagnostics when no caller remains.

Error types SHOULD include actionable structured data rather than only a
formatted sentence.

## 9. Unsafe Rust and FFI

Safe Rust is the default. Unsafe code is allowed only when it is required for
FFI, a proven data-structure invariant, or measured performance that safe Rust
cannot provide.

### 9.1 Unsafe blocks

Every unsafe block MUST have a nearby `SAFETY:` comment that proves the exact
obligations used by that block. A valid proof addresses the relevant subset of:

- pointer provenance and non-nullness;
- alignment;
- initialized bytes and valid bit patterns;
- bounds and allocation identity;
- aliasing and exclusive access;
- lifetime and drop order;
- thread ownership and synchronization;
- callback re-entry;
- unwind behavior.

The comment must describe facts established by code, types, or a caller
contract. "The pointer is valid" is not a proof.

- Keep unsafe blocks as small as possible.
- An `unsafe fn` MUST document a `# Safety` section for its caller obligations.
- Unsafe operations inside `unsafe fn` still require explicit unsafe blocks.
- Every `unsafe impl Send` or `unsafe impl Sync` MUST explain why all reachable
  state satisfies the trait contract, including mutation and destruction.
- Do not use `transmute` when a checked conversion, pointer cast, `MaybeUninit`,
  or dedicated API expresses the operation.
- Do not create references to uninitialized, misaligned, aliased-mutable, or
  foreign-owned data even temporarily.

### 9.2 FFI

- Keep raw ABI declarations and conversions in a narrow platform module.
- Use `unsafe extern` and Edition 2024 unsafe attributes as required.
- Validate null pointers, lengths, alignment, integer narrowing, enum values,
  ownership, and string encoding at the boundary.
- Rust panics MUST NOT unwind across an FFI boundary.
- Document who allocates, who frees, which thread calls back, and how long
  borrowed data remains valid.
- Wrap foreign handles in RAII types with an explicit invalid state only when
  the foreign API actually has one.
- Platform callback state MUST define re-entry and shutdown behavior.

Unsafe changes require targeted tests and SHOULD run under Miri when the
supported subset permits it.

## 10. Concurrency and synchronization

Concurrency is an ownership design problem before it is a locking problem.

- Mutable View, Element, and Render state remains owner-affine unless an ADR
  explicitly changes that contract.
- Cross-thread work communicates through immutable snapshots, typed commands,
  bounded channels, or narrowly owned shared state.
- Channels MUST be bounded on paths where producers can outrun consumers.
  Backpressure and overflow behavior must be defined.
- Do not hold a lock while calling user code, sending events, performing I/O,
  awaiting, or invoking another subsystem.
- Public APIs MUST NOT expose lock guards.
- Lock ordering MUST be documented when more than one lock can be held.
- Prefer moving ownership over sharing mutation.
- Atomics require a comment explaining the state machine and why each ordering
  is sufficient. `SeqCst` is not a substitute for that explanation.
- Cancellation MUST be observable and race-safe. Use generation or ownership
  tokens where a stale completion could affect a newer operation.
- Background workers require a shutdown protocol. Dropping the last handle
  should not leak a thread or block indefinitely.
- Process-global state requires an explicit ownership policy and repeat-init
  semantics.
- Tests that mutate a process-global singleton MUST use the repository's
  serialization guard or process isolation.

Never add unsafe `Send` or `Sync` merely because a platform callback requires
it. Adapt the callback boundary and dispatch back to the owner.

## 11. Async code

- Async is for waiting on I/O, platform services, timers, workers, and build
  pipelines. It is not part of build, layout, paint, hit-test, composite, or
  render hot paths.
- An async API MUST define cancellation behavior.
- Futures MUST NOT borrow a lock guard across `.await`.
- Do not perform blocking filesystem, process, GPU, or network operations on an
  async executor thread.
- Spawning is an ownership decision. The spawning layer owns error observation,
  cancellation, and shutdown.
- Detached tasks are forbidden unless their process-lifetime semantics are
  explicit and tested.
- Prefer structured task ownership over global executors.
- A returned future MUST make progress requirements clear when it depends on a
  frame pump, event loop, or external wake source.
- WASM and native implementations must preserve the same observable contract
  even when their executors differ.

## 12. Lifecycle and resource management

- Initialization, activation, suspension, shutdown, and destruction are
  distinct states when the platform observes them differently.
- Lifecycle transitions SHOULD be represented by a state machine or typed
  capability rather than loosely coupled booleans.
- Registration APIs return a token or guard when deregistration is required.
- Cleanup MUST be idempotent when platform callbacks can repeat.
- `Drop` MUST NOT perform unbounded blocking work.
- Offer an explicit `shutdown`, `close`, or `flush` operation when cleanup can
  fail or wait.
- A graceful shutdown has a documented deadline and escalation path; the OS
  remains free to force-kill the process.
- Never call user callbacks after the scope that owns them has begun
  destruction. State the ownership property, not the name of whichever type
  currently implements it.

## 13. Rendering and frame-pipeline code

- Frame hot paths remain synchronous.
- Build, layout, paint, hit testing, and semantics must preserve Flutter's
  observable ordering and edge behavior unless a documented divergence exists.
- Dirty-state changes MUST identify the owner responsible for scheduling work.
- A frame phase MUST NOT acquire lifecycle-only capabilities that can schedule
  unbounded re-entry.
- Layout and paint code must not perform I/O, wait on channels, initialize
  process globals, or acquire contended application locks.
- Tree mutation must be transactional enough that an error or caught user panic
  cannot expose a half-committed tree.
- Parent data belongs to the protocol that interprets it. Do not downcast by
  convention when a typed relationship can enforce the contract.
- Hit testing, paint order, semantics order, and child visitation order are
  observable behavior and require tests when changed.
- Defaults such as `Size::ZERO`, an empty list, or a missing baseline are not
  acceptable substitutes for unimplemented behavior.

## 14. Performance and allocation

Correctness comes first, followed by measurement, then optimization.

- Identify whether code is setup-time, per-frame, per-node, per-event, or
  per-pixel before choosing a data structure.
- Avoid allocation in inner frame loops when ownership permits reuse.
- Do not add caching without an invalidation proof and a memory bound.
- Cache keys MUST include every input that affects the result.
- Prefer contiguous storage for hot iteration; prefer maps when lookup behavior
  actually requires them.
- `SmallVec`, interning, arenas, lock-free structures, and custom allocators
  require a measured workload or a strong structural reason.
- Do not trade soundness or deterministic ordering for an unmeasured speedup.
- Benchmark changes to algorithms, allocation patterns, synchronization, text,
  tessellation, scene construction, or tree traversal.
- A benchmark must protect against the compiler removing the work and must
  describe the representative workload.
- Performance claims in PRs include the command, input, before/after values,
  and relevant build profile.

## 15. Numeric, geometry, and time code

- Preserve unit types across API boundaries. Do not introduce generic
  `From<f32>` escape hatches for coordinate wrappers.
- State the coordinate space for transforms, rectangles, offsets, and clips.
- Validate NaN, infinity, negative zero, overflow, and degenerate geometry where
  they affect observable behavior.
- Float equality is acceptable when the algorithm or oracle requires exact
  values; otherwise compare with an error model appropriate to the operation.
- Do not use one global epsilon for unrelated geometry algorithms.
- Integer casts at buffer, GPU, FFI, and allocation boundaries MUST be checked
  when truncation or sign changes are possible.
- Durations use `Duration` or a unit newtype, not an unqualified integer.
- Runtime behavior uses an injected monotonic clock where deterministic tests
  or platform portability require it.
- Wall-clock time is reserved for calendar timestamps and external protocols.

## 16. Collections and iteration

- Choose collections by required semantics: ordering, lookup, mutation,
  locality, and memory bound.
- Do not rely on hash iteration order.
- Stable observable order requires an ordered collection or explicit sorting.
- Avoid repeated linear scans in per-node or per-frame loops when a maintained
  index has a simpler invariant.
- Iterator pipelines SHOULD remain readable. Use an explicit loop when state,
  early exits, diagnostics, or error propagation become clearer.
- Avoid intermediate collections unless they simplify ownership or materially
  reduce repeated work.
- Collection mutation while dispatching callbacks requires a snapshot or a
  documented re-entry-safe algorithm.

## 17. Logging, diagnostics, and observability

- Shipped code uses `tracing`; never `println!`, `eprintln!`, or `dbg!`.
- Library crates emit events and spans but do not install subscribers.
- Subscriber ownership belongs to composition roots.
- Separate process-global facilities require separate ownership policies;
  permission to install a tracing subscriber does not imply permission to claim
  the `log` facade, a panic hook, or an exporter.
- Use structured fields instead of interpolating machine-readable values into
  the message.
- Event targets and field names consumed by devtools are contracts and require
  migration when changed.
- Choose levels consistently:
  - `error`: an operation failed and needs owner attention;
  - `warn`: degraded behavior or recoverable abnormal state;
  - `info`: low-volume lifecycle milestones;
  - `debug`: developer-facing state transitions;
  - `trace`: high-volume per-event or per-frame detail.
- Do not log secrets, clipboard contents, text input, file contents, access
  tokens, or precise location by default.
- On device sinks (logcat, Apple unified logging) fields are private by
  default: `flui-log` redacts dynamic values — strings, `Debug`/`Display`
  renderings, errors — to `<private>` unless the field name ends in `.public`;
  a name ending in `.private` redacts a scalar (for example a precise
  coordinate). Scalars and the message publish by default, so a
  machine-readable or user-provided value belongs in a field — interpolating it
  into the message bypasses classification. See `flui_log::backend::privacy`
  for the full contract.
- Native logging backends MUST document privacy and field mapping.
- Correlation fields name the real owner or operation an event belongs to, and
  MUST NOT encode the runtime's internal topology. A field name is read by log
  sinks, devtools, exporters, crash reports, and hand-written filters, so it
  becomes a consumer-visible format long before the type it was named after is
  stable; naming a field for an internal construct turns that construct's
  removal into a format migration for everyone downstream.
- One spelling per concept. A field name shared across crates is defined once as
  a constant in `flui_foundation::diagnostics` and referenced, so `presentation`
  and `presentation_id` cannot diverge without a compile error. A constant is
  added when the concept is durable AND something already emits it — a name with
  no emitter is a guess about a design that has not been made.
- Stable identifier fields use their canonical numeric representation, not
  `Debug`. Debug formatting is for people and may include type names or change
  without a schema migration; collectors need the same primitive value on every
  event and platform.
- Filters are scoped to the sink they govern. A native-console filter MUST NOT
  sit below the whole subscriber registry and accidentally clip timeline,
  metrics, crash, or test-capture layers with independent retention policies.
- A recoverable error is not both logged and returned unless the log represents
  a distinct decision made at that layer.

## 18. Documentation and comments

### 18.1 Rustdoc

- Public modules and types explain purpose, ownership, and lifecycle, not merely
  restate their names.
- Public fallible functions document `# Errors`.
- Public panicking functions document `# Panics`.
- Unsafe APIs document `# Safety`.
- Examples demonstrate why and use `?` rather than teaching `unwrap`.
- Intra-doc links are preferred for Rust items and are compiled by doctests.
- Public contracts must document thread affinity, cancellation, ordering, and
  callback re-entry when relevant.

### 18.2 Comments

- Comments explain why the code has this shape, the invariant being maintained,
  or the external behavior being matched.
- Do not narrate syntax.
- A workaround names the external constraint and its removal condition.
- A TODO requires an owning issue when leaving it unresolved could affect
  correctness, safety, compatibility, or public API.
- Do not cite private review cycles, agent names, or temporary planning IDs.
- Flutter reference comments identify the behavioral contract, not line numbers
  likely to drift, unless the exact source location is essential evidence.

## 19. Testing

Tests prove behavior, not implementation trivia.

- Every bug fix starts with or includes a test that fails without the fix.
- Test names describe the scenario and expected behavior.
- Cover the happy path, boundary conditions, invalid input, lifecycle teardown,
  and relevant concurrency interleavings.
- Prefer deterministic clocks, seeded input, headless rendering, and explicit
  synchronization over sleeps.
- A sleep-based test requires a reason and a generous platform-independent
  deadline; it must not be the primary correctness assertion.
- Property tests are preferred for geometry identities, parser invariants,
  state-machine sequences, and round trips.
- Snapshot tests are appropriate for rich stable output, but snapshots must be
  reviewed rather than blindly accepted.
- Render behavior requires the render-object harness and, when pixels matter,
  an offscreen screenshot/readback check.
- Flutter parity changes cite and verify the corresponding oracle behavior.
- Tests must not special-case the harness input in production code.
- Global state tests use process isolation or the repository's explicit lock.
- Concurrency tests assert ownership, ordering, cancellation, and shutdown, not
  only eventual completion.
- Platform code distinguishes "type-checked" from "executed on the target" in
  completion reports.

## 20. Dependencies, features, and conditional compilation

- Add a dependency only when it removes more risk or complexity than it adds.
- Prefer the standard library and existing workspace dependencies when they
  meet the requirement without reimplementing a domain engine.
- Shared dependency versions belong in `[workspace.dependencies]`.
- Internal normal dependencies MUST follow `docs/workspace-layers.toml`.
- Library crates use `thiserror`; composition binaries may use `anyhow`.
- Disable unnecessary default features, especially for networking, TLS, image,
  async, and platform crates.
- Feature flags SHOULD be additive and independently compilable.
- A feature MUST NOT silently change safety or ownership guarantees.
- Optional dependencies use `dep:` wiring and are checked by the feature-matrix
  CI job.
- Keep target-specific dependencies under the narrowest correct target `cfg`.
- Prefer a target module with one selected implementation over scattered `cfg`
  expressions throughout domain logic.
- Unsupported targets fail with a clear compile-time or typed runtime error;
  they do not silently select an incorrect fallback.
- New dependencies require license, advisory, source, MSRV, maintenance, and
  target-support review.

## 21. Security and untrusted input

- Treat files, network data, clipboard data, drag-and-drop payloads, fonts,
  images, shader source, plugin messages, and platform callbacks as untrusted.
- Parse with bounded allocation, bounded recursion, and validated lengths.
- Check integer arithmetic before allocating or slicing from external lengths.
- Do not construct paths through string concatenation. Normalize according to
  the security boundary and reject traversal when confinement is promised.
- Avoid shell invocation. When launching tools, pass arguments without an
  intermediate shell.
- Never place secrets or user content in diagnostics by default.
- Dynamic libraries and hot-reload plugins require ABI, version, symbol,
  lifetime, and unload validation.
- Shader and GPU inputs require bounds and alignment validation before upload.
- Denial-of-service risk is part of parser and cache review even when memory
  safety is guaranteed by Rust.

## 22. Macros and generated code

- Prefer functions, traits, and derives over macros when they express the same
  contract.
- A public macro accepts normal Rust syntax fragments and supports visibility
  and attributes where item macros are expected to.
- Use `$crate` for paths emitted by exported declarative macros.
- Proc macros emit errors at the relevant input span and do not panic on user
  input.
- Generated code includes a generated-file marker and is reproducible from a
  checked-in source or build step.
- Never edit generated output to fix a defect; fix the generator and regenerate.
- Generated unsafe code follows the same safety-proof standard as handwritten
  code.

## 23. Review discipline

Every change should let a reviewer answer these questions:

1. What observable behavior changes?
2. Which owner is responsible for the new state?
3. Can invalid input reach this path?
4. What happens during cancellation, shutdown, panic, or partial failure?
5. Are thread-affinity and `Send`/`Sync` claims true?
6. Does an unsafe block prove every obligation it uses?
7. Does the dependency direction match the workspace policy?
8. Is the hot-path cost known and proportionate?
9. Which test fails without this change?
10. Was Flutter behavior checked where the Prime Directive requires it?

Reject changes that are merely green but achieve that result with a stub,
default value, narrowed test, ignored error, or undocumented divergence.

## 24. Focused checklist for new code

Before requesting review:

- [ ] The API makes invalid states difficult or impossible to construct.
- [ ] Ownership, thread affinity, lifecycle, and cancellation are explicit.
- [ ] No production `unwrap`, uncontrolled panic, or swallowed `Result` exists.
- [ ] Unsafe and FFI obligations are documented and tested.
- [ ] Locks are private, ordered, and not held across callbacks or awaits.
- [ ] Hot paths avoid I/O, async suspension, and unexplained allocation.
- [ ] Public API has useful rustdoc and applicable common trait implementations.
- [ ] Logs use structured `tracing` fields and contain no sensitive data.
- [ ] Tests cover behavior, failure, boundaries, and teardown.
- [ ] Cargo features, targets, and dependency layers were checked independently.
- [ ] The full repository gate passes.

## References

This standard adapts the following upstream guidance to FLUI's architecture:

- [The Rust Style Guide](https://doc.rust-lang.org/style-guide/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rust Reference: unsafe](https://doc.rust-lang.org/reference/unsafe-keyword.html)
- [Rust 2024 Edition Guide: unsafe operations in unsafe functions](https://doc.rust-lang.org/edition-guide/rust-2024/unsafe-op-in-unsafe-fn.html)
- [Clippy usage and lint configuration](https://doc.rust-lang.org/clippy/usage.html)

Repository-specific architecture and behavior rules remain authoritative where
they are stricter than these upstream guidelines.
