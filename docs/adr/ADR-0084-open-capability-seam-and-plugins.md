# ADR-0084: Platform capabilities are an open, typed set registered by plugins

- **Status:** Proposed
- **Date:** 2026-09-25
- **Supersedes in part (on acceptance):** [ADR-0078](ADR-0078-rules-live-in-types-and-lints.md)
  §1, the sentence "A new capability is a method on `LifecycleContext`, never on
  `BuildContext`" (for platform capabilities; the build/lifecycle split and the sealing stand)
- **Amends (on acceptance):** [ADR-0031](ADR-0031-platform-haptics-capability-and-system-chrome-deferral.md)
  §4 (the widget-facing haptics handle arrives through the registry; this replaces the deferral
  "No widget-facing handle exists yet"); [ADR-0038](ADR-0038-data-transfer-architecture.md) §7
  (`DataTransferHandle` is reached through the registry, not a `LifecycleContext` method)
- **Related:** [ADR-0028](ADR-0028-design-system-decoupling-contract.md) ("Platform-adaptive
  behavior is a capability seam, not a branch"),
  [ADR-0039](ADR-0039-event-loop-affinity-capability.md) §6,
  [ADR-0081](ADR-0081-workspace-tiers-and-reach-facts.md),
  [ADR-0082](ADR-0082-platform-api-contract-crate.md),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md),
  [ADR-0086](ADR-0086-signal-writes-through-event-context.md),
  [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md),
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md)
- **Refs:** decision D3 of the
  [architecture review](../research/2026-09-25-architecture-review/report-architecture.ru.md);
  index in [`design/decisions.md`](../../design/decisions.md)

## Context

ADR-0078 made capability acquisition a type: `ViewState::init_state` and
`did_change_dependencies` receive `&dyn LifecycleContext`, `build` receives
`&dyn BuildContext`, and a capability acquired in `build` does not compile. `LifecycleContext`
is sealed through its supertrait (`crates/flui-view/src/context/build_context.rs:17-19,106,376-377`)
and carries eleven methods, from `rebuild_handle` to `pipeline_owner` (`build_context.rs:392-565`).
The rule that came with it — "A new capability is a method on `LifecycleContext`" — closes the
set: only `flui-view` can add a capability, and only by naming its type, so a capability whose
backend lives in a third-party crate cannot exist. AGENTS.md's "Extending FLUI" row repeats the
rule.

The platform side already has more than the widget side can reach:

- **Haptics.** `PlatformWindow::haptics()` exists (`crates/flui-platform/src/traits/window.rs:342`),
  and `PresentationState::perform_haptic_feedback` resolves it
  (`crates/flui-app/src/app/presentation.rs:893`), but that method and its forwarder
  `UiRealm::perform_haptic_feedback` (`crates/flui-app/src/app/ui_realm/frame_clock.rs:508`) carry
  `expect(dead_code)` with "no production caller yet" (`presentation.rs:883-892`,
  `frame_clock.rs:502-507`). ADR-0031 §4 deferred the widget-facing handle "with the first widget
  consumer, as a lifecycle capability".
- **Clipboard.** `Platform::clipboard()` is a required method
  (`crates/flui-platform/src/traits/platform.rs:423`), resolved once per loop into `AppRuntime`
  (ADR-0038 §9), and the accessor that would hand it out is dead code: "no production caller yet
  -- a Clipboard capability through BuildContext is future wiring"
  (`crates/flui-app/src/app/runtime.rs:1631-1641`).
- **Data transfer.** ADR-0038 §7 names `DataTransferHandle` "a lifecycle capability on
  `LifecycleContext`"; no such method exists in the list above.

Three constraints shape any fix:

- `&dyn LifecycleContext` appears 136 times in `crates`, `src` and `examples`
  (`grep -rn '&dyn LifecycleContext' crates src examples --include=*.rs | wc -l`). A generic
  method `fn capability<C>(&self)` on the trait would make it not object-safe and break every
  one of those sites.
- Rust has no link-time registration without `inventory` or `linkme`, and both carry caveats on
  wasm and static libraries; neither is a dependency today.
- Flutter's federated plugins have no way for an application to replace a plugin's endorsed
  implementation (flutter#80374, cited in the review's
  [ecosystem design](../research/2026-09-25-architecture-review/designs/ecosystem_evolution_first.md)).

## Decision

### 1. Two kinds of capability

- A **framework capability** is implemented by the framework itself: `rebuild_handle`,
  post-frame handles, `focus_manager`, keep-alive, `hit_test_handle`, `text_input_handle` (the
  presentation's IME route, ADR-0037 §5), `pipeline_owner`, and `WriterSource` (ADR-0086). These
  stay methods on `LifecycleContext`, as ADR-0078 decided.
- A **platform capability** is an OS service with a backend outside `flui-view`: clipboard, data
  transfer, haptics, file dialogs, camera, share sheets and the like. These are **never**
  methods on `LifecycleContext`. They form an open set, declared by any crate and reached through
  one object-safe method. Platform capabilities come in two classes, core-required and optional
  (§5); both are reached through the same door, `cx.capability::<C>()`.

### 2. The seam

In `flui-platform-api` (ADR-0082), so that a plugin's interface crate needs nothing else:

```rust
pub trait PlatformCapability: 'static {
    /// The value a widget holds; cheap to clone, owner-thread.
    type Handle: Clone + 'static;
    /// Stable name, for errors, logs and the protocol's capability listing.
    const NAME: &'static str;
}

pub trait CapabilityProvider<C: PlatformCapability>: 'static {
    fn provide(&self, window: &dyn PlatformWindow) -> Result<C::Handle, Unsupported>;
}

#[non_exhaustive]
pub struct Unsupported { pub capability: &'static str, pub reason: UnsupportedReason }

#[non_exhaustive]
pub enum UnsupportedReason { NotRegistered, NotOnThisPlatform, NoWindow }
```

In `flui-view`, one hidden, object-safe method on the sealed trait and a blanket extension:

```rust
pub trait LifecycleContext: BuildContext {
    // … the framework capabilities …
    #[doc(hidden)]
    fn capability_erased(&self, id: TypeId, name: &'static str) -> Result<Rc<dyn Any>, Unsupported>;
}

pub trait LifecycleContextExt: LifecycleContext {
    fn capability<C: PlatformCapability>(&self) -> Result<C::Handle, Unsupported> {
        self.capability_erased(TypeId::of::<C>(), C::NAME).map(|handle| {
            handle
                .downcast_ref::<C::Handle>()
                .expect("BUG: capability registry stored a handle of another type")
                .clone()
        })
    }
}
impl<T: LifecycleContext + ?Sized> LifecycleContextExt for T {}
```

`Unsupported` is defined in `flui-platform-api`, so this adds a new normal edge
`flui-view → flui-platform-api` (tier K to tier C, legal under ADR-0081 §1).

`LifecycleContext` stays sealed, so only `flui-view`'s contexts implement `capability_erased`,
and they forward to the realm. `LifecycleContextExt` is in the prelude. Because the method lives
on `LifecycleContext`, `cx.capability::<Haptics>()` inside `build` is still a compile error, the
same `E0599` ADR-0078 relies on. The 136 `&dyn LifecycleContext` sites are untouched.

### 3. The registry is per realm, filled once

`flui-runtime` (ADR-0083) owns a `CapabilityRegistry` per realm: a map from `TypeId` to an
erased provider. A lookup resolves the provider against the presentation's window, caches the
handle per presentation, and returns `Unsupported { reason: NotRegistered }` when no provider
exists. The registry is filled before the first realm is created and never changes afterwards;
there is no registration at run time and no process-global table (ADR-0097).

### 4. Registration is explicit, one line per plugin

A plugin implements a `Plugin` trait whose `install` receives a registrar
(`registrar.capability::<C>(provider)`); `Plugin` and the registrar are defined in
`flui-runtime` and published to package authors through `flui-sdk` (ADR-0088). An application
lists its plugins on the application builder —
`Application::new(factory).plugin(flui_haptics::Plugin)`, a new method beside today's
`with_config` and `on_ready` on `Application` (`crates/flui-app/src/app/application.rs:53,67`)
— and installs them in call order. The registrar registers capabilities only; other runtime
hooks, such as ADR-0094's reload hook, have their own builder method. A plugin chooses its default provider per target with
`[target.'cfg(..)'.dependencies]` in its own manifest.

Precedence:

1. an application override, `Application::capability::<C>(provider)`, wins regardless of call order;
2. otherwise the one plugin that registered `C`;
3. two plugins registering the same `C` without an application override is a bootstrap error
   (`BootstrapError`) that names both, not a silent last-writer-wins.

No `inventory` or `linkme`.

### 5. Two classes: core-required and optional

The owner decided on 2026-09-25 that platform capabilities fall into two classes behind the one
door of §2. The class decides where the backend obligation lives, not how a widget asks.

**Core-required.** Methods of the backend traits in `flui-platform-api`; a backend that lacks one
does not compile. ADR-0038 §9 made `Platform::clipboard()` required so that no backend can forget
it, and rejected a handle with `None` defaults for exactly that reason; this class generalises
that rule. The class holds:

- **clipboard** and **data transfer**: `Platform::clipboard()` and `Platform::data_transfer()`
  (`crates/flui-platform/src/traits/platform.rs:423,435`), already required;
- **cursor**: `PlatformWindow::set_cursor` (`crates/flui-platform/src/traits/window.rs:467`),
  already required;
- **text input and IME**: `PlatformWindow::text_input()`, today a defaulted method returning
  `None` (`window.rs:333`), becomes required;
- **accessibility**: `PlatformWindow::accessibility()`, today defaulted to `None` (`window.rs:352`),
  becomes required;
- **window chrome basics**: title, size, close request and decorations; the exact method list is
  fixed when the traits move into `flui-platform-api` (ADR-0082).

A backend that cannot serve a core capability yet implements the method explicitly with an inert
object and names the gap in its evidence record (the web backend for text input and accessibility
until H1): the gap becomes a line in that backend, not a trait default nobody sees. The runtime
registers built-in providers for the core capabilities a widget acquires directly (clipboard, data
transfer) before any plugin runs; an application may override them (§4). The core capabilities
the framework consumes itself keep their framework route and are not duplicated as widget
capabilities: text input through the presentation's IME route (`text_input_handle`, ADR-0037 §5),
accessibility through the semantics host, the cursor through mouse regions.

**Optional.** Plugin capabilities registered through the registry (§3, §4); a widget handles a
typed `Unsupported`. Haptics, camera, geolocation, notifications, share sheets and file dialogs
start here. Haptics ships as the first plugin, over the existing `PlatformWindow::haptics()` hook
(`window.rs:342`); `None` from the backend becomes `Unsupported { reason: NotOnThisPlatform }`,
which is ADR-0031's degradation contract made visible: a caller may ignore the error and get
Flutter's silent no-op.

`UnsupportedReason::NotRegistered` can arise only for an optional capability the application did
not install, never for a core one; a core capability can return only `NoWindow`.

**Which class a capability belongs to.** A capability is core-required when both hold:

1. a framework-owned widget or protocol in the Stable surface needs it to meet a contract FLUI
   states for every supported platform (text entry and paste in a text field, an accessibility
   tree, pointer feedback, a window that can be titled and closed); and
2. every supported platform has an OS service for it, so its absence is a backend defect, not a
   platform fact.

Everything else is optional: a service some supported platform legitimately lacks, or one only
applications use.

**How a capability moves between classes.** Each move is a new ADR that amends this section, and
the capability keeps its type `C`, so `cx.capability::<C>()` call sites do not change.

- *Optional to core-required*, when a Stable-surface widget starts to depend on it and every
  supported backend implements it: the method is added to the backend trait (a breaking change for
  third-party backends, so it ships on a train, never in a patch release), the plugin's provider
  becomes the built-in one, and the plugin stays as a no-op for one train.
- *Core-required to optional*, when a supported platform turns out to lack the service: the method
  leaves the backend trait after one train of deprecation, a plugin takes over the provider, and
  callers that relied on the old guarantee must now handle `Unsupported`, which the ADR lists.

## Alternatives considered

- **A generic method on `LifecycleContext`.** Breaks object safety and all 136 `&dyn` sites.
- **Unsealing `LifecycleContext` so plugins add extension traits with their own storage.** Every
  plugin would need somewhere to keep per-realm state, which pushes it toward a process-global
  or a thread-local, and a context implemented outside `flui-view` could hand out capabilities in
  `build`.
- **Link-time auto-registration (`inventory`, `linkme`).** Registration would depend on which
  object files the linker keeps, with known gaps on wasm and static libraries, and an application
  could not see or order what it installed.
- **Keep adding methods, one per capability, in `flui-view`.** Keeps the set closed; every OS
  service needs a change to the spine, and a third-party plugin cannot exist.
- **Capabilities as `InheritedView`s the plugin inserts above the root.** Reads in `build` would
  become possible, which is what ADR-0078 removed, and an override would depend on tree position.
- **Make every capability optional, including clipboard.** Reintroduces the forgettable wiring
  ADR-0038 §9 rejected.
- **Make every capability core-required.** Every backend, including a minimal embedder, would
  have to implement camera, geolocation and the like, and the set would be closed again: a
  third-party capability could not exist without a change to `flui-platform-api`.
- **Two doors: backend methods for core capabilities, the registry for optional ones.** A move
  between classes would change every call site; with one door it changes only the provider.

## Consequences

- ADR-0078 §1's closing sentence is narrowed to framework capabilities; AGENTS.md's "Platform
  capability (a new handle)" row in "Extending FLUI" changes in the implementing change to: an
  interface crate on `flui-platform-api`, a provider per target, registration through a plugin,
  a headless fake, and a test that fails without it.
- The dead clipboard accessor (`runtime.rs:1631-1641`) and the haptics forwarders
  (`presentation.rs:893`, `frame_clock.rs:508`) get production callers or are deleted in favour
  of the built-in clipboard provider and the haptics plugin.
- **Breaks.** `PlatformWindow::text_input()` and `accessibility()` lose their `None` defaults, so
  every backend, including third-party ones, implements them. The web backend's implementations
  are inert until H1 and say so in its evidence record.
- AGENTS.md's "Platform capability" row states the classification rule of §5, so a contributor
  adding a capability knows which class to put it in.
- ADR-0038 §7's `DataTransferHandle` is acquired as `cx.capability::<DataTransfer>()`.
- Registered capabilities are data the realm can list; ADR-0095's protocol can expose that list
  to tools and agents.
- The seam depends on ADR-0082 (for `flui-platform-api`) and ADR-0083 (for the realm's
  registry). Until both land, a prototype can live in `flui-view` and `flui-app`, with the types
  moving in the same change as those crates.
- The runtime's handle cache is per presentation; a presentation that closes drops its handles,
  so a widget that outlives its window sees `Unsupported { reason: NoWindow }` on its next
  acquisition in `did_change_dependencies`.

## Verification

None of these exist yet.

- A `compile_fail` doctest: `cx.capability::<C>()` on `&dyn BuildContext` does not compile.
- `cargo check --workspace --all-targets` stays green with the hidden method added, proving the
  136 `&dyn LifecycleContext` sites are unaffected.
- An out-of-workspace fixture crate (depending only on `flui-platform-api` and `flui-sdk`)
  declares a capability, registers a provider through a plugin, and a test in `flui-testing`
  acquires it in `init_state`; the same test without the plugin receives
  `Unsupported { reason: NotRegistered }`.
- A test that two plugins registering one capability fail bootstrap, and that an application
  override resolves the conflict.
- A headless test that acquires haptics, with the haptics plugin installed, on a window without
  haptics and receives `NotOnThisPlatform`; one on `MockWindow` with `FakeHaptics` that observes
  the call; and one without the plugin that receives `NotRegistered`.
- A `compile_fail` doctest: a `PlatformWindow` implementation without `text_input` or
  `accessibility` does not compile.
- A headless test that acquires the clipboard with no plugin installed and gets a handle, never
  `NotRegistered`.
