# Event context: alternative ownership foundations

Research date: 2026-09-27. Read-only investigation of FLUI checkout
`e330fc413a4b40a758ca8c2fd52f55d0929c1c10` and its working changes. No compilation,
benchmark, competitor execution, or production edit was performed. This report
does not supersede ADR-0086; recommendations below require an explicit decision.

## Verdict

The present design is a reasonable **presentation-local mutation capability**.
It is not yet evidence for a complete application-state, event-delivery, or
mounted-lifetime foundation. Three viable alternatives expose materially
different ownership boundaries, not merely different callback punctuation.
Do not freeze the current design as the universal application model until a
shared-document/two-presentation example and a disposable async component have
been designed and tested. This is not a recommendation to rewrite the renderer.

## What the current implementation actually establishes

Read together: [ADR-0086](../adr/ADR-0086-signal-writes-through-event-context.md),
[the current design challenge](2026-09-27-event-context-design-review.md),
[owner decisions](../../design/decisions.md),
[writer implementation](../../crates/flui-view/src/reactive/writer.rs), and
[reactive implementation](../../crates/flui-view/src/reactive/mod.rs).

- `WriterSource` owns a cloned `Reactive` and can open arbitrarily many short
  `EventCx` values. Acquisition is lifecycle/render-registration scoped; opening
  a context is not restricted to a physical input dispatch.
- `EventCx` and `Writer` provide current untracked reads and graph-checked writes.
  `WriterSource::check_context` checks graph identity and active build phase,
  not attachment, cancellation, event origin, or operation atomicity.
- A graph is presentation-local today. A writer for A refuses B's signal even
  when the two presentations show one conceptual document. That is isolation,
  not an implemented shared-document model.
- A captured source can outlive the widget that acquired it. Widget adapters
  must enforce their own mounted lifetime. Releasing element-owned signal slots
  may reject stale signal writes, but cannot prevent arbitrary callback effects.
- Each signal mutation is immediate; the graph marks readers afterward. There
  is no rollback or multi-signal transaction. A mutable reference to a short-lived
  writer is not an exclusive borrow of the graph: another cloned source can open
  another context. The runtime loan/reentrancy guards remain necessary.
- The transitional `Reactive: WriteTarget` still exists. Even after removal,
  captured sources leave phase enforcement partly dynamic. The compiler benefit
  is narrower than a universal prohibition of mutation while constructing UI.

The current report already acknowledges most of these limits. The architectural
question is whether the missing boundaries are intentionally separate layers or
will be repeatedly reimplemented by every catalog component.

## Alternative 1: typed messages, reducer, explicit effects

**Verified precedent.** Iced 0.14.0 separates state, messages, update and view;
update can return `Task<Message>`, including async work, and subscriptions describe
ongoing inputs. Tasks support composition and cancellation. Its daemon constructor
supports an application lifetime independent of open windows and requires
`Message: Send + 'static`.
[Iced API](https://docs.rs/iced/0.14.0/iced/),
[daemon contract](https://docs.rs/iced/0.14.0/iced/fn.daemon.html).

**Proposed FLUI adaptation, not an Iced guarantee.** Widgets emit typed domain
messages; a realm/application-owned reducer mutates document state. UI-local
state can remain in retained widgets or local signals. Async completion carries
an explicit document/component generation rather than a captured writer.
Window commands name a presentation address. The host executes effects at an
allowed boundary. This need not enqueue every pointer sample for a later frame:
the reducer can run synchronously after input routing when the owner is idle.

**Wins:** an editor has two views of one document, an undo stack, and a background
save. A typed `RenameDocument`/`SaveFinished` vocabulary centralizes ownership and
lets the application reject obsolete save results. Deterministic reducer tests
need no mounted tree. A replay log is possible if messages/effects are designed
to be serializable and nondeterminism is recorded; replay is not automatic.

**Loses:** a generic third-party slider wants immediate local drag feedback and
should not require the host application to add a message variant. An obligatory
central reducer would couple independent packages and invite a large root enum.
Component message mapping and local transient state are necessary. A `Send`
message policy copied verbatim would conflict with FLUI's owner-local values.

**Adoption cost:** medium as an optional application-model layer; high as the
only widget protocol. It can coexist with `EventCx` if callbacks dispatch domain
messages rather than silently crossing presentation graphs. It does not justify
turning `EventOutcome` into a generic effect interpreter.

## Alternative 2: mutable state supplied by routed view actions

**Verified precedent.** Xilem 0.4.0's button callback receives `&mut State` and
returns an action. The runtime builder also accepts mutable-state app logic;
multi-window configurations use `Xilem::new`. Xilem Core distinguishes action,
rebuild request, no-op and stale-target outcomes. Masonry's widget contract names
an action type, and its event context submits actions to the application.
[button](https://docs.rs/xilem/latest/xilem/view/fn.button.html),
[runtime builder](https://docs.rs/xilem/latest/xilem/struct.Xilem.html),
[message result](https://docs.rs/xilem_core/0.4.0/xilem_core/enum.MessageResult.html),
[Masonry widget](https://docs.rs/masonry/latest/masonry/core/trait.Widget.html),
[Masonry event context](https://docs.rs/masonry/latest/masonry/core/struct.EventCtx.html).

**Correction to prior research.** The archived
[q7 report](2026-09-25-architecture-review/decisions/q7_callback_writer.md)
says Xilem makes writes impossible in view construction. The published runtime
signature gives its logic `&mut State`, so that statement is not supported.
Xilem is evidence for explicit state borrowing and routed actions, not for
FLUI's build-purity guarantee.

**Proposed FLUI adaptation.** Store application state independently of mounted
elements. Route a widget action to its current component/state projection and
lend the selected state to its handler. A stable route/generation distinguishes
a removed component from a newly inserted component with similar structure.
Keep the existing rendering and gesture substrate underneath this state layer.

**Wins:** a reusable checkout editor receives just `&mut Address` rather than
capturing a set of signal handles and a writer source. Removing it invalidates
its routed action target centrally. A shared document can be represented once,
with separate views borrowing the appropriate state projection.

**Loses:** deeply independent widgets and dynamic heterogeneous plugin state
need projection, routing and type-erasure rules. Coarse reconstruction may need
memoization; neither its cost nor an advantage over FLUI's reader-targeted
invalidations is established here. Passing `&mut AppState` everywhere also
grants more authority than a selected field requires unless projections narrow it.

**Adoption cost:** high if replacing catalog callbacks; medium as an opt-in
stateful composition layer. An especially useful separable lesson is explicit
stale-target handling. A graph ID alone cannot distinguish two attachments
within the same presentation.

## Alternative 3: entity-owned state, leases and weak callback adapters

**Verified precedent.** GPUI 0.2.2 uses entity-specific `Context<T>`, observation
and event subscriptions. `spawn_in` supplies a weak entity and an async window
context. Entity updates lease state from an application-owned map; nested access
to an already leased entity can panic. Weak handles avoid owning the target.
The inspected map's context identity check is a debug assertion, so its precise
failure policy must not be copied as FLUI's release-mode isolation policy.
[Context](https://docs.rs/gpui/0.2.2/gpui/struct.Context.html),
[entity-map source](https://docs.rs/gpui/latest/src/gpui/app/entity_map.rs.html).

**Proposed FLUI adaptation.** Give component or application models an owning
entity arena and generational handles. Widget listeners hold weak attachment
references; dispatch upgrades the target, validates its presentation policy,
and lends its state/context. A detached target has an explicit outcome before
user code executes. Document models and mounted widget attachments are distinct
entities: keeping a document alive must not revive a dead widget callback.

**Wins:** a document outline starts an asynchronous search, is removed, then the
result arrives. A weak attachment adapter rejects delivery without each widget
inventing a boolean mounted flag. A document entity can outlive either window;
presentation views observe it without pretending its storage belongs to A's
reactive graph. Listener replacement can be centralized around attachment identity.

**Loses:** cycles of strong entities still need design discipline. A wrong weak
boundary can drop useful background document work merely because one view closes.
Entity-level notification can be coarser than per-signal readers. Lease reentrancy
still needs a defined failure policy; the abstraction is not a transaction.

**Adoption cost:** high as a new universal model, moderate if extracting only a
reusable weak mounted-callback registration. FLUI should not import GPUI's broad
application capabilities into a write-only context merely because both APIs use
the spelling `cx`.

## Comparison for FLUI, not an external benchmark

| Required behavior | Current graph capability | Reducer/effects | Routed state borrow | Entity/attachment lease |
|---|---|---|---|---|
| Fine-grained local signal updates | Direct, existing | Needs local state or reducer projection | Needs rebuild/projection strategy | Needs granular notification strategy |
| One document in two presentations | Requires another model boundary | Natural central owner | Shared root with view projections | Shared model entity, separate attachments |
| Reject delivery after widget removal | Adapter-owned today | Generation in message/envelope | Stale route outcome | Weak attachment upgrade |
| Record/replay domain operations | Opaque closures insufficient | Explicit vocabulary fits | Requires recording routed actions | Requires typed commands over entities |
| Synchronous return-valued routing query | Existing separate query contract | Must remain outside deferred effects | Can remain synchronous | Can remain synchronous |
| Rollback arbitrary callback effects | Not provided | Not automatic | Not provided | Not provided |

## Falsification plan before a stable contract

These are proposed experiments, not executed results or promised performance.

1. **Two presentations, one document.** Edit in A and B before either rebuilds;
   undo from B; close A during a pending save. Both surviving views must converge
   without a global current writer, raw `Reactive` escape, or silent rebinding of
   a foreign event context. If the only implementation duplicates document state
   or switches writer provenance invisibly, reject the graph capability as the
   application-model foundation. It may still remain the local UI capability.
2. **Attachment churn.** Queue a result, replace the handler, remove the widget,
   then mount a different widget using the same external handle. Count callback
   delivery and captured-object drops. No old event may reach a new attachment.
   If several unrelated widgets need bespoke mounted flags and generations,
   prefer a shared attachment registration over more individual patches.
3. **Plugin-owned state.** Build a reusable component without access to the
   application's concrete root type. It must compose synchronous queries,
   local edits and typed async completion. If a reducer/root-state alternative
   requires editing the application's enum for every internal component detail,
   reject that alternative as the mandatory catalog protocol.
4. **Capability value.** After removing transitional graph access, deliberately
   invoke a captured source during build, nest two sources for the same graph,
   and try cross-presentation operations. Separate compiler refusals from runtime
   refusals. If the type-level benefit cannot be demonstrated outside the five
   pilot callbacks, reconsider the cost of imposing the context on every event.
5. **Overload and release.** Deliver rapid progress plus one terminal completion
   while rendering is paused; then dispose the target. Measure retained captures,
   delivery latency and queue growth. A source/context pair alone cannot pass a
   bounded-delivery requirement; the delivery channel needs an explicit lossless
   versus replaceable-event policy under any of the alternatives.

## Recommendation and uncertainty

Keep the narrow `EventCx` contract provisional, not expansive. First demonstrate
a separate shared application-model owner and a reusable attachment-lifetime
boundary. A small message/reducer adapter is the least disruptive way to test the
first; an entity-inspired weak registration is the least disruptive way to test
the second. Choose them because the scenarios require them, not to combine three
frameworks into one architecture.

Do not add event identity, global application access or async ownership to
`EventCx` until its independently opened sources have a coherent propagation
contract. Such additions would otherwise require changing the ownership model
behind already familiar signatures. Do not equate the existing pilot's ergonomic
success with scalability evidence: its counter/todo scope cannot settle document
sharing, attachment replacement, replay, or bounded delivery.

Primary documentation was fetched on the research date. Observed published
versions were Iced 0.14.0, Xilem/Masonry/Xilem Core 0.4.0, and GPUI 0.2.2; `latest`
links above identified those versions during inspection and may change later.
The Xilem package page dates 0.4.0 to 2025-10-29:
[release metadata](https://docs.rs/crate/xilem/latest).
No claim is made that these releases equal repository main or that they are more
performant than FLUI. The Masonry internal pass-system page could not be retrieved
in this run, so no deferred-mutation lifetime guarantee is inferred from it.
No secondary GPUI guide, discussion thread, or speculative future release is used
as authority. The architectural adaptations and scenario rankings are this
review's inferences, not capabilities claimed by those projects.
