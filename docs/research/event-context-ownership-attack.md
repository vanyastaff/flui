# Event context: ownership and lifetime attack

This is a source-level adversarial review of the working EventCx migration against
`origin/main`. No builds were run for this review. Line references name the inspected
working-tree snapshot; the parent task owns executable verification. ADR-0086 remains
Proposed and explicitly leaves removal of the Reactive escape hatch and listener
migration outstanding. Planned compute/phase semantics are not implemented guarantees.

## Assessment boundary

The implemented capability proves access to a particular reactive graph. It does not
prove a live widget incarnation, a valid async request, exclusive ownership of a
controller, one physical input dispatch, or transaction success. Treating any of those
as consequences of receiving EventCx would create architectural debt.

The concrete introduced foreign-form partial mutation was reproduced by the parent
task before the ownership preflight fix. The current form code checks the form and
field snapshot before mutation. This review found no further demonstrated introduced
merge blocker. The following limitations still constrain claims about the foundation.

## Graph authority is not phase permission or dispatch identity

Evidence: `crates/flui-view/src/reactive/writer.rs:95,202,215,230`.
`WriterSource::write` constructs a context without admission checks;
`check_context` checks graph equality and the graph's current build marker.
`crates/flui-view/src/reactive/mod.rs:116` has a build marker, not a general
compute/layout/paint phase machine.

Concrete sequence: a widget captures a source during initialization and opens it
inside build. A context exists, but signal mutation refuses. Nested callbacks can
also open another context for the same graph while an outer context is live.
Consequently neither EventCx existence nor its mutable reference proves exclusive
graph mutation or one shared event propagation state. ReadGraph's open implementation
surface does not forge WriteTarget: the latter is sealed (`writer.rs:256`).

Classification: existing pilot limitation, not a newly introduced safety hole.
No current derived-compute production phase was found; a compute bypass is a future
integration hypothesis, not a current defect.

Alternative: keep EventCx explicitly a write-authority facade and carry future event
identity/propagation separately; alternatively require a dispatcher-owned context to
be forwarded across wrappers before adding propagation or batching fields. Making
`write` fallible can reject build entry but still cannot enforce arbitrary interior
mutable application state purity.

Falsification test: nested `source.write` calls must be tested before claiming unique
dispatch identity. The existing build callback regression must continue to demonstrate
runtime refusal, rather than claiming construction is impossible.

## Mounted identity is not a graph identity

Evidence: form source acquisition and clearing at
`crates/flui-widgets/src/form/mod.rs:447,471` and
`crates/flui-widgets/src/form/form_field.rs:647,679`; graph-only check at
`crates/flui-view/src/reactive/writer.rs:215`.

Concrete sequence: launch work holding a form handle and source, unmount the form,
then remount the same handle in the same presentation. An old completion can now
pass the context check and mutate the new mounted form. Detachment rejection only
protects the interval before remount. Signal slot generations protect released
signals, not a reusable controller or a graph-owned signal.

Classification: preexisting reusable-handle/async lifetime debt; the new check does
not solve it. It blocks a claim of automatic stale-completion cancellation.

Alternative: explicitly distinguish stable controller identity from a mounted lease.
A cancellable operation captures a mount generation plus a request revision; ordinary
imperative controller calls can retain stable-handle semantics. Do not put an element
generation on every WriterSource indiscriminately: graph-scoped operations are useful.

Falsification test: capture an operation, unmount/remount in the same BuildOwner,
complete the old operation, and assert the chosen cancellation or retargeting policy.
Also reverse two requests within one mount: a mount generation alone cannot reject
the older response.

## Duplicate mounting violates the source binding invariant

Evidence: `form/mod.rs:437,447,471` and `form/form_field.rs:629,647,679`.
Configuration occurs in create_state before lifecycle source assignment. Assignment
overwrites shared handle state; disposal clears it. Handle documentation now states
one mounted owner as a precondition (`form/mod.rs:105`, `form_field.rs:249`).

Concrete sequence: mount one handle in presentations A and B. B replaces the source
and callback configuration. Disposing A then clears the source used by live B.
The resulting Detached/ForeignPresentation errors are symptoms of aliasing, not a
repair of it. Even rejected callbacks cannot restore the overwritten configuration.

Classification: preexisting unsupported use, newly observable via typed errors.
This is not an enforced ownership invariant and must not be described as one.

Alternative: separate an externally clonable command handle from a unique mounted
binding, or provide fallible attach before configuration. A panic after shared
configuration mutation is not an adequate replacement. This requires a deliberate
lifecycle API decision, not another callback adapter.

Falsification test: two actual mounts followed by disposal of the first, asserting
that the second remains usable or the second attach was refused without effects.
Synthetic interleavings of create_state alone are insufficient.

## Admission success is not transaction success

Evidence: form preflight at `form/mod.rs:195,208`; stored callback reporting at
`crates/flui-view/src/reactive/writer.rs:295,305,318,331`.

Concrete sequence: `form.reset(cx)?` changes native field state and calls a callback;
that callback attempts a released or foreign signal write. The callback adapter logs
the refusal, while the form operation can still return Ok. Similarly a callback can
change external state before its later operation fails. Preflight prevents known
owner mismatch before form mutation, not arbitrary callback rollback.

Classification: explicit nontransactional callback model, not a remaining version of
the fixed foreign-form admission bug. It becomes a contract defect if Result is
documented as guaranteeing completion of every user callback effect.

Alternative: document Result as admission failure. If callers need composition with
observable callback failure, preserve a fallible callback result instead of erasing
it into EventOutcome logging. Atomic domain operations belong in a separate model
transaction, not a generic UI callback rollback mechanism.

Falsification test: admitted reset with an on_reset callback returning a signal error;
assert both the field result and callback/error-observation behavior intentionally.

## Async target is currently a routing key, not cancellation

Evidence: `crates/flui-runtime/src/ui_realm/commands.rs:72,97,426,439` and
`crates/flui-runtime/src/ui_realm/presentations.rs:328`.
SignalWrite documentation explicitly says the slot graph selects the presentation.
The dispatcher matches the graph then invokes apply, without validating slot liveness.

Concrete sequence: enqueue a command, release its target slot while its presentation
remains alive, then drain. Arbitrary effects at the start of apply still run; a later
signal set may reject Released. A closed presentation is different: no matching
graph exists and the closure is discarded.

Classification: existing documented routing semantics, not an introduced stale-slot
mutation bug. Do not advertise it as task cancellation.

Alternative: keep a graph-addressed command and add an explicit live-target operation
for cancellation-sensitive work; or define SignalWrite target as a liveness admission
token and validate before invocation. Neither addresses out-of-order requests on a
still-live signal without a separate request revision.

Falsification test: release target, preserve presentation, drain an apply closure
whose first action increments a separate counter. Pin whether that counter changes.

## Disposal suppressing invocation need not release captures

Evidence: `form/form_field.rs:679` clears binding and rebuild state but retains saved
and reset callbacks and value adapters. `animated/animated_size.rs:233,235,279`
queues a closure with a writer and mounted guard; disposal prevents invocation, but
does not remove that closure from the pending lane.

Concrete sequence: retain an unmounted field handle whose callback captures a large
application object or graph source. Those captures remain alive. Separately, queue
an animation completion then dispose before the lane drains; cancellation of delivery
does not mean immediate reclamation of queued resources.

Classification: field retention predates this migration; post-frame delivery expands
the places where cancellation and reclamation differ. Not an unbounded leak proven
here: queue drain/drop can release the animation closure, and controller retention
can be intentional.

Alternative: define which callbacks belong to a mounted binding and clear those on
detach, dropping outside RefCell borrows; retain deliberate controller-level data.
Use cancellable queue registrations only where immediate reclamation is required.

Falsification test: weak capture probes after disposal, before queue drain and after
drain, separately for retained controller handles and scheduler callbacks.

## Caught writer panic preserves data without invalidation

Evidence: `crates/flui-view/src/reactive/mod.rs:491,576,1027`.
The explicit existing test `a_panicking_closure_returns_the_loaned_value_and_marks_nobody`
pins partial value restoration and no subscriber marking. PANIC-POLICY does not
promise transaction rollback or continued coherent rendering after user panic.

Concrete sequence: update mutates the value then panics; an outer boundary catches
the panic. The value is changed, the slot usable, and subscribers not marked.
This is deliberately tested preexisting behavior, not a newly found migration bug.

Alternative if continued recovery is required: dirty subscribers on unwind (partial
commit semantics), or an explicit clone/transaction-based operation that can roll
back. Generic arbitrary T cannot be rolled back for free. Keeping current semantics
requires recovery documentation not to promise view/model consistency.

Falsification test: existing unit test establishes current behavior. A stronger claim
requires an actual recovering event/scheduler path with an already-rendered subscriber,
then observing whether the UI is refreshed after the caught writer panic.

## Multi-presentation composition limit

Each BuildOwner owns its graph (`flui-view/ARCHITECTURE.md`, Signal reads section).
Sharing one presentation's Signal with another presentation's event callback is
therefore not shared application-state support; ForeignGraph is expected. Rebinding
an EventCx silently to the signal owner would defeat the explicit authority contract.
This topology predates the migration. A shared domain model with per-presentation
projections/commands is a viable alternative; changing to a shared graph requires
multi-owner subscribers, scheduling, disposal and phase semantics to be designed
together. A two-presentation read/write/dispose test must precede such a change.
