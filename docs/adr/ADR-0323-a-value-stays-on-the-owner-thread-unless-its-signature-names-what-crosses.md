# ADR-0323: A value stays on the owner thread unless its signature names what crosses

- **Status:** Accepted (2026-11-12)
- **Date:** 2026-11-12
- **Supersedes:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md) §2 (the thread-affinity table) and
  §9's items on the public API's thread affinity and the cross-thread vocabulary;
  [ADR-0136](ADR-0136-owner-local-ui-surfaces.md) §3 (the two gates and the kinds of value that
  cross); [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) §4 (where parallel work
  leaves the owner thread)
- **Related:** [ADR-0266](ADR-0266-one-owner-thread-ui-runtimes-isolate.md) §1, §2 and §5 (one
  owner thread per process, which this record builds on), [ADR-0136](ADR-0136-owner-local-ui-surfaces.md)
  §2 (owner-local tasks and `Weak` handles), [ADR-0091](ADR-0091-one-owner-thread-isolated-realms-raster-thread.md)
  §2 and §4 (serial layout, one raster thread), [ADR-0305](ADR-0305-one-gpu-context-serves-every-surface-through-a-raster-executor.md)
  (the raster executor a scene crosses to), [ADR-0287](ADR-0287-owner-state-is-committed-before-user-code-runs.md)
  (owner state is committed before user code runs), [ADR-0089](ADR-0089-upstream-types-in-stable-signatures.md)
  §6 (the rustdoc JSON the positions are read from), [ADR-0303](ADR-0303-a-rule-takes-the-first-rung-that-states-it-and-an-xtask-gate-is-the-last.md)
  (why the gates are `xtask` gates), [ADR-0311](ADR-0311-development-reload-is-a-hook-the-application-installs.md)
  (the development hooks' factories)

## Context and problem statement

Every UI runtime of a process runs on one owner thread (ADR-0266 §1). What that means for a
signature was written in four records: ADR-0027 §2 and §9 said which types are `!Send`, ADR-0136
§3 added the two gates and four kinds of value that may cross a thread, ADR-0266 §4 listed the
handles, and ADR-0091 kept layout serial. No record says, for a given `Send` in the public API,
whether it stays. The measured state on `main` at `e13b7ce`:

| Fact | Evidence |
|---|---|
| The public API of `flui` and `flui-sdk` has 240 listed `Send`/`Sync` positions on about 140 items; 15 name the kind of value that crosses | `tools/xtask/allowlists/send-positions.toml` |
| The four kinds do not cover work handed to a worker, a committed scene, or an error's source, although such positions are listed and are right | `flui_view::background::Background::compute`, `flui_layer`'s `AnnotationValue`, `flui_app::app::application::AppRunError` |
| Widget signatures ask for `Send + Sync` on values no other thread reads | `Localizations::of<R: Send + Sync>() -> Arc<R>` (`crates/flui-widgets/src/localization/localizations.rs`), `RouteSettings::arguments<T: Any + Send + Sync>` (`navigator/route.rs`), `StreamBuilder<K, T, E>` (`crates/flui-view/src/element/stream_builder.rs`), `AnimatedList<K: Send + Sync>` |
| Owner-thread state sits behind locks | `Mutex`/`RwLock` outside tests in `flui-widgets/src/navigator/navigator.rs`, `flui-view/src/element/future_builder.rs`, `flui-scheduler/src/scheduler.rs`, `flui-interaction/src/pointer/resampler.rs`, `flui-view/src/owner/build_owner.rs`, `flui-interaction/src/gesture/arena/mod.rs` |

A bound an application author meets in a widget signature is a promise the framework has to keep
after publication: a `Send + Sync` can be dropped later, an `Arc<R>` in a return type cannot become
an `Rc<R>`. Which `Send`, `Sync` and `Arc` does a signature keep, and what replaces the rest?

## Decision drivers

- The frame path is synchronous and owner-local (`AGENTS.md`): a lock or an atomic on state one
  thread touches costs time on every access and hides re-entrancy as a deadlock.
- A widget author holds `Rc`, `Cell` and `RefCell` state. A `Send + Sync` bound on a route
  argument or a localization resource refuses that state for no reader on another thread.
- The compiler is the first rung (ADR-0303): a value crosses a thread because its type proves it
  may, never because a wrapper asserts it.
- The rule has to be decidable for one position by its reviewer, without a call-graph survey.
- Flutter is the reference for how an author writes widgets. A Dart isolate shares no objects:
  every widget, state and listener is local to its isolate, and what goes to another isolate is
  what `compute` and `SendPort.send` accept. FLUI's owner thread is that isolate, and `Send` is the
  compile-time form of "may be sent".
- Nothing published may close a later thread per runtime (ADR-0266 §5).

## Considered options

1. Owner-local by default with a closed set of crossing kinds, each listed position naming its
   kind.
2. Keep `Send + Sync` on widget-facing values so that a later thread per runtime needs no
   signature change.
3. Keep the list as it is and decide each position when a change touches it.

## Decision

Chosen option: 1, because it gives each position one answer a reviewer can check, it is the
only option under which an author's `Rc` state fits every widget API, and a thread per runtime
keeps a runtime's values on that runtime's thread, so option 2 buys nothing for it.

### §1. A value the owner thread alone touches has an owner-local shape

A value that is created, read, called and dropped on the owner thread carries no `Send` or `Sync`:
not as a bound, a supertrait, a `dyn … + Send` or a `where` clause. This covers `View`,
`BuildContext`, `ViewState`, the element and render traits, `ParentData`, notifications, every
listener, animation-status, post-frame, gesture and event callback, a controller, a key, a route's
arguments, a localization resource and a delegate.

Its shapes are fixed:

| Need | Shape | Not |
|---|---|---|
| Shared ownership | `Rc<T>` | `Arc<T>` |
| Mutation | `Cell<T>`, `RefCell<T>`; a re-entry that finds it borrowed gets a typed outcome | `Mutex`, `RwLock`, an atomic, `try_lock` as a re-entrancy detector |
| A stored callback | `Rc<dyn Fn(…)>`, `Box<dyn FnMut(…)>`, called after the owner's state is committed (ADR-0287) | `Arc<dyn Fn(…) + Send + Sync>` |
| A type-erased value | `Rc<dyn Any>` | `Arc<dyn Any + Send + Sync>` |
| A generic parameter | the bounds its use needs (`'static`, `Clone`, `PartialEq`, `Hash`) | `Send + Sync` added to them |
| A marker trait of zero-sized types | no thread supertrait | `: Send + Sync` so that containers inherit it |

`Arc` appears in an owner-local signature only for a value that also crosses under §2: bytes and
paths a decode or IO job reads (`Arc<[u8]>`, `Arc<str>`), a host service, a handle.

### §2. Seven kinds of value cross a thread

A `Send` or `Sync` in the public API is right only at a position that carries one of these, and
the compiler proves each: nothing is made `Send` by assertion.

| `crosses` | What crosses | Shape | Where |
|---|---|---|---|
| `panic-payload` | a caught panic's payload | `Box<dyn Any + Send>`, the type `std` gives it | `flui_foundation::panic`, a failure a turn hands on |
| `waker` | a wake, and what it carries | `Waker`, `Fn() + Send + Sync`: it only requests, it never touches owner state | `FrameWaker`, `RebuildHandle`, `ReloadWake`, `RedrawRequester`, a scheduler's frame hook |
| `job` | work handed to another thread, with its captures | `FnOnce() -> T + Send + 'static`, `Future + Send + 'static` | `Background::compute` and `io`, `TaskSpawner`, `ComputeJob`, `IoFuture`, `DecodeJob`, a service's future, `OwnerPoster::post` towards the owner |
| `worker-result` | what a job returns or publishes | `T: Send + 'static` on the job's output only | the same items, `ServicePublisher<E>`, `Resolver<T>` |
| `handle` | a capability another thread holds or calls | a `Clone + Send + Sync` type, or a trait whose implementations are called off the owner thread | `SignalSender`, `AppHandle`, `Executor`, the host pools, `HttpClient`, `Storage`, `SurfaceTarget`, `DisplayConnection`, a factory inside `AppConfig`, a tracing sink |
| `snapshot` | a committed scene, which moves by value (`SceneSnapshot`, `Scene`, `LayerTree`), and an immutable value it carries to the raster executor | `Send` on the scene; `Send + Sync` on a carried value, shared by `Arc` | `flui-layer`'s `AnnotationValue`, and with it `AnnotatedRegion<T>` |
| `error` | an error's source | `Box<dyn Error + Send + Sync + 'static>` | every public error enum's `source` |

Rules that follow:

- **A job owns what it reads.** A job borrows nothing of the owner: it captures owned, `Send`
  values and returns one. The result lands in an owner-local value (`BackgroundTask<T>`, a
  `Completion<T>`) that the owner thread reads. `T: Send` is asked on the job's output, not on the
  widget that shows it.
- **A wake carries no state.** A `waker` position is `Fn()` or `Waker`. A closure that needs owner
  state is not a waker: it is a `job` posted to the owner.
- **A handle is a type or a host contract, not a callback list.** A widget-layer trait is a
  `handle` only when the framework calls it from another thread. A window capability reached
  through `PlatformWindow` (drag source, haptics, system chrome, text input) is called on the
  owner thread and is owner-local.
- **Every public error is `Send + Sync + 'static`**, so an application's `?` into `anyhow::Error`
  compiles. An error enum holds no `Rc`; a failure that names an owner-local value names its typed
  id.
- **No public executor of arbitrary closures on the owner thread.** `OwnerPoster` is the host's
  crate-level seam; applications come back to the owner through `SignalSender`, `RebuildHandle`,
  `AppHandle` and a job's result. Raw channels do not appear in public signatures.
- **A value both sides need is split.** Where a worker produces what the owner then shares
  (localization resources loaded from a file, a decoded image), the loader's output is the
  `worker-result` and the owner wraps it in `Rc`. The read API returns the `Rc`.

### §3. The gates

- **`cargo xtask send-impls`** lists every `unsafe impl Send` and `unsafe impl Sync` of the
  workspace in `tools/xtask/allowlists/send-impls.toml`. An unlisted impl fails, and the list only
  shrinks. `deny.toml` bans `send_wrapper` and `fragile`, which turn the compiler's refusal into a
  panic on the wrong thread.
- **`cargo xtask send-positions`** lists every `Send` and `Sync` position an author wrote in the
  public API of `flui` and `flui-sdk` in `tools/xtask/allowlists/send-positions.toml`, read from
  the nightly rustdoc JSON of ADR-0089 §6 in CI's `api-contracts` job. An unlisted position fails,
  and so does an entry the code no longer has.
- **Every entry names its kind.** `crosses` is one of §2's seven values and the gate refuses any
  other. An entry without `crosses` is a position §4 removes. When none is left, `crosses` becomes
  required and the gate fails an entry without it.
- A change that writes a new position lists it with its kind in the same change.

### §4. The listed positions

**Stay, with their kind.** Each is an item of the list at the commit above; an item with several
positions keeps all of them unless a marker is named.

| Kind | Items |
|---|---|
| `panic-payload` | `flui_foundation::panic::{FirstPanic::{held, fold, record, into_payload}, discard_payload, payload_text}`, `flui_view::view::error::FrameworkError::from_panic`, `flui_platform::capability::owner::PendingCompletions::retire`, `…::runtime_local::RuntimeLocals::retire`, `flui_platform::text_store::lock::CommitGate::{defer_failure, take_failure}`, `…::owner_calls::OwnerCalls::{into_failure, keep}` |
| `waker` | `flui_foundation::claim_slot::claim_slot`, `flui_foundation::testing::CountingWaker::{calling, retiring_waker}`, `flui_platform::surface_target::RedrawRequester` and its blanket impl, `flui_platform::capability::owner::OwnerPoster::new`, `flui_scheduler::scheduler::UpdateScheduler::set_on_frame_scheduled`, `flui_view::dev_reload::ReloadWake::new`, `flui_view::owner::build_owner::BuildOwner::set_on_build_scheduled`, `flui_rendering::pipeline::owner::PipelineOwner::set_on_need_visual_update`, `flui_hot_reload::dispatch::register_request_rebuild` |
| `job` and `worker-result` | `flui_view::background::Background::{compute, io}`, `flui_app::app::lifecycle::{TaskSpawner::{spawn_compute, spawn_io, spawn_worker}, WorkerHandle, ServiceFuture, ServiceDefinition::new, CancellationSignal::cancelled, ServicePublisher, service_events}`, `flui_platform::executor::{ComputeJob, IoFuture, RoomFuture::new}`, `flui_platform::http::{HttpFuture, BodySource}`, `flui_platform::storage::StorageFuture`, `flui_platform::capability::owner::OwnerPoster::post`, `flui_platform::capability::completion::{CompletionSource::pending, Resolver}`, `flui_widgets::image::provider::{DecodeJob::{decode, read_then_decode}, ImageProvider::resolve_async}` and its two implementations |
| `handle` | `flui_platform::executor::Executor`, `flui_runtime::execution::{HostComputePool, HostIoPool}`, `flui_platform::http::{HttpClient, HttpAuthority::of}`, `flui_platform::storage::Storage`, `flui_platform::surface_target::{SurfaceTarget, DisplayConnection}`, `flui_runtime::frame_failure::FrameFailureHandlerFactory::new`, `flui_runtime::input_tap::InputTapFactory::new`, `flui_view::dev_reload::DevReloadHook` and `flui_view::dev_agent::DevAgentHook` (`Send` only, until ADR-0311's factories carry it), `flui_app::app::config::AppConfig::memory_reclaim` (`Send` only), `flui_devtools::capture::Capture::{on_frame, on_frame_open}` (a tracing layer is shared by every thread that emits), `flui_hot_reload::dynlib::DynLib` |
| `snapshot` | `flui_widgets::paint::annotated_region::AnnotatedRegion` and its `View` and `RenderView` impls |
| `error` | `flui_app::app::application::AppRunError::{Platform, Service}::source`, `flui_app::app::application_control::AppWindowError::{Mount, Native, Renderer, Show}::source`, `flui_app::app::launch::LaunchError::Platform::source` |

**Go, with what replaces them.**

| Item | Becomes |
|---|---|
| `flui_widgets::localization::localizations::Localizations::{of, maybe_of}` | `of<R: 'static>(ctx) -> Rc<R>`, `maybe_of<R: 'static>(ctx) -> Option<Rc<R>>` |
| `LocalizationsDelegate::Resources`, `BoxedLocalizationsDelegate::{new, From}`, `WidgetsLocalizations` | `type Resources: 'static`; a delegate is held as `Rc<dyn …>`; no thread supertrait. A delegate that loads off the owner thread returns its resources through a `job`, whose output is the `worker-result` |
| `flui_widgets::navigator::route::{RouteArguments, RouteSettings::{argument, with_arguments}}`, `named_route::{RouteKey::with_arguments, RouteRequest::argument}` | `RouteArguments = Rc<dyn Any>`, `with_arguments<T: Any>`, `argument<T: Any>() -> Option<&T>` |
| `flui_view::element::stream_builder::{BoxedResultStream, StreamBuilder, StreamBuilderState}` and their trait impls | the bounds of `FutureBuilder` (ADR-0136 §2): `K: Clone + PartialEq + Debug + 'static`, `T, E: 'static`, a stream without `Send`, polled by the owner-local driver; the state's slot is a `RefCell`. A source that produces off the owner thread (a socket, a file watcher, a worker) stays a `job`: it runs through `Background::io` or a service and its items reach the owner as `worker-result`s, which the stream handed to `StreamBuilder` yields on the owner thread. The bound that goes is the builder's, not the source's |
| `flui_widgets::scroll::animated_list::{sliver::{SliverAnimatedList, SliverAnimatedListState}, views::{AnimatedList, AnimatedGrid}}` and their trait impls | `K: Clone + Eq + Hash + Debug + 'static` |
| `flui_view::key::object_key::ObjectKey::new` | `new<T: 'static>(object: &Rc<T>)`; the key holds the `Rc`, so the address it compares cannot be reused |
| `flui_widgets::image::provider::ImageProvider` (supertrait), `flui_widgets::image::cache_key::ImageCacheKey::custom` | no thread supertrait; `Image` holds `Rc<dyn ImageProvider>`; `K: Hash + Eq + Debug + 'static`. Reading and decoding stay off the owner thread and stay `job`s: `ImageProvider::resolve_async` keeps its `Future + Send + 'static` and `DecodeJob` its `Send` closures (both in the table above), and each owns the bytes, path or client it reads instead of borrowing the provider. What goes is the bound on the provider object, which the owner thread alone holds and calls. If a lane is found to hold the provider or a cache key itself, that position stays as a `handle` |
| `flui_view::binding::WidgetsBindingObserver::{did_pop_route, did_push_route_information}` | futures without `Send`, polled on the owner thread |
| `flui_rendering::binding::RendererBinding::{add, remove}_semantics_enabled_listener` and the `flui_runtime` impls, `PipelineOwner::{set_on_semantics_owner_created, set_on_semantics_owner_disposed}` | `Rc<dyn Fn(bool)>`, `impl Fn() + 'static` |
| `PipelineOwner::hit_test`, `flui_rendering::testing::{inspect::Probe, queries::BoxQueryRun}` (`Phase: Sync`) | no bound on the phase parameter |
| `flui_rendering::protocol::protocol::Protocol`, `flui_foundation::arity::Arity` | sealed marker traits without `Send + Sync` |
| `flui_foundation::clock::MonotonicClock`, `flui_foundation::observe::TreeObserver` | no thread supertrait; held as `Rc<dyn …>` |
| `flui_foundation::wasm::WasmNotSendSync` and its blanket impl | deleted: no production caller |
| `flui_platform::{data_transfer::DragTransport, drag::PlatformDragSource, haptics::PlatformHaptics, system_chrome::PlatformSystemChrome, text_input::PlatformTextInput}` | no thread supertrait: each is reached through a `PlatformWindow` on the owner thread |
| `flui_app::app::close_request::CloseRequestHandler::new` | `impl Fn(&CloseRequest) -> CloseResponse + 'static`, held as `Rc`; the wrong-thread branch of its call goes with the bound |
| `flui_app::app::direct::run_direct` | `impl FnMut(&mut SceneBuilder, f64, f64) + 'static` |
| `flui_app::app::config::AppConfig::memory_reclaim` (`Sync`) | `impl ReclaimMemory + Send + 'static` |
| `flui_hot_reload::pipeline::PluginPipeline::mount` | `V: View + StatelessView + Clone + 'static` |
| `flui_painting::typography::text_spans::InlineSpan::as_trait` | `&dyn InlineSpanTrait` |

The verdict of a row rests on where the item's contract says it is called. The change that removes
a bound is its proof: the workspace compiles without it, or the compiler names the thread the
value reaches. In the second case the entry stays under that kind and the change corrects this
table.

### §5. Owner-thread state holds no lock

State behind a `Mutex`, an `RwLock` or an atomic that only the owner thread reaches takes §1's
shapes. A lock stays where two threads meet: the cross-thread half of a waker (a ready queue, a
frame latch), a host pool's queue, a `handle`'s shared state. The six files of the context table
are owed this, `build_owner.rs` last, since two other lines of work edit it.

### Consequences

- Good, because an author's `Rc`, `Cell` and `RefCell` state fits route arguments, localization
  resources, list keys and stream payloads.
- Good, because each listed position has one of seven reasons, and a position without one is a
  removal with its replacement written down.
- Good, because a re-entry into owner state is a typed outcome or a `RefCell` panic at the site,
  not a deadlock.
- Bad, because `Localizations::of`, `RouteSettings::arguments`, `ObjectKey::new` and the
  platform capability traits change their signatures, and every caller with them.
- Bad, because an application that kept one value for both a widget and a worker now holds two:
  the `Rc` the widget reads and the owned copy the job takes.
- Neutral, because a thread per runtime (ADR-0266 §5) would keep each runtime's values on its own
  thread: no row of §4 would return.

### Confirmation

- The compiler: the fixtures in `tests/ui/thread_boundary/` and each crate's `compile_fail`
  cases (`recognizer_stays_on_its_thread`, `a_clipboard_handle_stays_on_its_thread`,
  `async_driver_stays_on_its_thread`) refuse an owner-local value on another thread.
- `cargo xtask send-impls` in `cargo xtask checks` and `cargo xtask send-positions` in CI's
  `api-contracts` job, each with its `--self-test`; xtask's
  `send_positions_list_entry_names_one_of_the_kinds_that_cross` refuses a `crosses` value §2 does
  not name.
- `cargo xtask deps` (cargo-deny over `deny.toml`) holds the crate bans.
- For §4's removals, the proof is the change itself, as §4 says, and the entry leaving the list.
- For §5, clippy's `disallowed-types` for `std::sync::Mutex`, `std::sync::RwLock` and
  `parking_lot`'s in the crates whose state is owner-local, with `#[expect(…, reason)]` at a
  site where two threads meet.

## Pros and cons of the options

### 1. Owner-local by default, a closed set of crossing kinds

- Good, because the reviewer of one position asks one question: which of seven values crosses
  here.
- Good, because it matches how the widget layer is written everywhere else: no lock, no `Arc`.
- Good, because the compiler confirms each removal.
- Bad, because the removals are breaking changes to public signatures, which have to land before
  publication.

### 2. Keep `Send + Sync` on widget-facing values

- Good, because no signature changes now.
- Bad, because a view, its state and its callbacks are `!Send` already, so a `Send + Sync`
  argument is held by a value that cannot leave the thread: the bound proves nothing.
- Bad, because it refuses `Rc` state in exactly the APIs an author fills with their own types.
- Bad, because `Arc<R>` in a published return type stays forever.

### 3. Decide each position when a change touches it

- Good, because it costs nothing today.
- Bad, because 225 positions carry no reason, and the next signature copies its neighbour's
  bound.
- Bad, because publication freezes whichever positions nobody touched.

## More information

- Dart isolates share no mutable objects, and widgets, state and listeners belong to the
  isolate that made them: <https://docs.flutter.dev/perf/isolates>. `compute` runs a callback on
  another isolate and returns its result: <https://api.flutter.dev/flutter/foundation/compute.html>.
  What a `SendPort` accepts: <https://api.dart.dev/dart-isolate/SendPort/send.html>.
  `Localizations.of<T>` returns the resources object itself:
  <https://api.flutter.dev/flutter/widgets/Localizations/of.html>.
- `std::panic::catch_unwind` returns `Box<dyn Any + Send + 'static>`, and `anyhow::Error` is built
  from `E: std::error::Error + Send + Sync + 'static`: the two kinds whose bound is another
  crate's.
- Revisit §2's `snapshot` kind when the retained scene (ADR-0304) changes what the owner thread
  hands the raster executor, and §4's `handle` row for the development hooks when ADR-0311's
  factories land.
