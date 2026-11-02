# Multi-owner UI runtime requirements

- **Status:** Draft for owner review; preparation authorized 2026-11-01. Implementation and
  platform support are not approved or claimed by this document.
- **Base:** `bb618a7f7f1510ff11093109094ba02c937d1970`.
- **Decision:** [ADR-0284](../../../adr/ADR-0284-ui-runtimes-may-have-dedicated-owners.md).
- **Design:** [design.md](design.md). **Execution and evidence:** [tasks.md](tasks.md).

## Outcome and scope

An application can request a separately owned independent runtime without making its views,
signals or callbacks transferable. A heavy CPU turn there does not occupy a sibling's UI
owner. Existing shared-owner applications retain their behavior. The complete implementation
scope includes current runner migration, Win32 native acceptance and headless acceptance.
Win32 colocates each dedicated UI runtime with its own native loop, TSF and inline raster.
Every other runner must explicitly retain shared execution and reject dedicated admission
until its own integration and native evidence satisfy this contract. Later host enablement
cannot inherit Win32's pass. No support claim follows from drafting or accepting this spec.

Not part of this change: runtime migration, parallel mutable layout, blanket UI `Send` bounds,
per-window GPU topology, split native/UI execution, remote TSF or frame-result protocols,
new rendered `Shared` presentations, automatic placement, or hard real-time guarantees.

## Observable requirements

| ID | Required behavior | Observable acceptance |
|---|---|---|
| MO1 | One permanent owner constructs, executes and destroys each runtime; multiple presentations stay with it. | Compile-fail transfer cases; captured construction, callback and destructor thread identities match. |
| MO2 | Existing local entries and their `View + Clone` root requirements remain; rendered `Shared` content keeps its current refusal. Dedicated launch is explicit, with `Send` only on its factory, and rejects unsupported hosts before invocation. | Existing consumers compile unchanged; destination constructs/clones an `Rc`-holding root; capturing prebuilt `Rc` fails to compile; unsupported host invokes factory zero times. |
| MO3 | A successful dedicated receipt means mounted, routed and redraw requested, not displayed or focused. Every admitted launch settles exactly once. | Startup failure, native rejection, full capacity, cancelled observation and quit races leave no orphan window/runtime/thread. |
| MO4 | A dedicated owner progresses while another owner is inside a synchronous CPU turn. | Deterministic barrier-controlled headless test and actual native two-window release experiment; include shared heavy-first and light-first controls. |
| MO5 | Commands, observations, wakes and completions retain exact authority/ticket. Stale new requests are refused; already accepted obligations keep their captured original target until settlement. | Delay work through close/reopen, focus change and replacement: it never reaches new state; accepted composition/close work still settles on its original target. |
| MO6 | All queues, retained payload bytes, launch slots and owners are bounded. Control obligations cannot be displaced by scenes or telemetry. | Saturation matrix proves typed refusal before acceptance, exactly-once settlement afterwards, bounded high-water marks and continued shutdown progress. |
| MO7 | Native input, focus, capture, TSF, clipboard, accessibility and frame results remain local to each Win32 owner, preserving synchronous contracts. | Native default prevention, real TSF read/edit grants and actual raster outcomes are observed; no guessed return value, owner-wait cycle, stale edit or misrouted action. |
| MO8 | Frame demand, scene submission and surface lifetime remain addressed and generation-safe. | Resize, DPI, occlusion, mixed refresh, device recovery and close-in-flight tests; fresh pixels and presentation feedback checked separately from CPU completion. |
| MO9 | Quit preserves cancel/defer/consent, tracks membership through concurrent launch and coordinates persistence/services over all owners. | Recorded holds defer; unrecorded veto/panic or stay-open cancels. A newly admitted/published owner cannot bypass consultation; global commit atomically fences admission after revision validation. Each owner retires locally before final app exit. |
| MO10 | Reentry and panic preserve committed state, capture ownership and first-failure custody. | Callback/wake/drop failures separately, two in competition and next-operation recovery; sibling remains usable unless the accepted terminal policy requires exit. |
| MO11 | All runners use the same ownership/routing contract without importing native or GPU types into `flui-runtime`. | Migration inventory complete; shared-path conformance on available targets; explicit rejection and compile coverage for unavailable hosts; no legacy duplicate dispatch path. |
| MO12 | Measurements and support claims are reproducible and scoped to the executed host/backend. | Commit, executable hash, OS/device/driver, configuration, raw traces, repetitions, failures and negative controls accompany each claim. Parity changes only after required actual runs. |
| MO13 | Dedicated launch does not duplicate process services or transfer local registries. Reconstructed app plugins/overrides retain precedence; missing reconstruction refuses launch, optional acquisition refusal remains a value. | Distinguishable app override used by both owners; missing/stale registration manifest refuses before publication; optional Unsupported does not abort launch. Services survive sibling retirement; resource counts return to baseline. |
| MO14 | Cross-owner document state is explicit and versioned; synchronous shared state remains within one owner. | Two views of one document reject stale replies without lost edits; local shared presentation behavior remains unchanged. No implicit shared `Rc` graph. |

## Native acceptance matrix

These are required scenarios, not completed runs. Every case records host and input origin.
Existing platform deficiencies must be named; no feature is silently disabled to pass the
dedicated-owner suite. A missing essential contract keeps dedicated admission unavailable.

| Case | Required native observation |
|---|---|
| Two-window independence | Heavy dedicated runtime performs real build/layout/paint plus a separate calibrated CPU callback scenario. OS-input counter and timer in the sibling keep progressing; both windows actually present changed content. |
| Placement controls | Shared heavy-first reproduces blocking; shared light-first exposes ordering effects. Heavy work stalls its own loop. Different-owner frame progress is checked with the heavy runtime on both the initial owner and a dedicated owner; coordinator-mediated operations may still wait for the coordinator. |
| Input and focus | Keys down/up, allowed and prevented system shortcuts, pointer/capture, focus crossing and close while pressed retain native default handling and exact window identity. Reentrant input keeps its existing special-case result. |
| IME and editing | Real TSF RequestLock/OnLockGranted reads and writes, composition, commit/cancel, selection, formatting, surrounding text and geometry under move/resize/DPI. New stale request is refused; an accepted deferred completion still settles on its captured original store after focus change, detach or shutdown. No redirection to the new field. |
| Accessibility | Native client reads changed text/selection/bounds and invokes an action in each owner during load; a screen-reader session checks traversal/focus. Tree snapshots alone do not prove actions. |
| Clipboard and drag/drop | Copy/paste over owners, cancel transfer on close, external drop and cross-window drag with versioned document state; supported native operations keep their thread affinity. |
| Surface and pacing | Resize continuously, minimize/restore/occlude, move across DPI/displays, mixed refresh where devices allow; old scene cannot present on a new surface. Record frame age and feedback availability. |
| GPU failure | Inject recoverable loss, exhaust retry and invoke the existing retry command; each affected presentation receives its own outcome and healthy siblings retain their demands. Real device-loss evidence is distinguished from injection. |
| Close and quit | Close primary with surviving owner; veto/hold/release/discard/stay-open; admit a new owner during consultation and on the commit boundary; quit during install; persistence/panic. Independent accepted closes may finish during consultation. Session-end obeys OS deadlines; no callback joins an active owner. |
| Services and idle | Preferences/fonts, reconstructed custom provider and precedence, optional refusal, registry-generation race, idle completion, memory reclaim and reopen. No duplicate process startup, COM hold leak or leaked owner thread. |

## Measurement and pass criteria

Functional tests use latches and observable operations, not timing guesses: while owner A is
held inside an admitted turn, owner B must complete an addressed operation before A is
released. A bounded watchdog fails the test and releases A for cleanup. The shared-owner
negative control must remain pending until release. Do not put this latch in a UI production
path or declare the test equivalent to native responsiveness.

The native release harness has unloaded, shared-heavy-first, shared-light-first and dedicated
modes; three or more fresh-process repetitions in rotated order, warmup, at least 1000
sibling samples per mode and real text/layout/paint updates. Record request-to-owner start,
CPU transaction duration, scene submission, native receipt and presentation feedback as
separate timestamps. Never subtract incompatible clocks. Where physical presentation cannot
be observed, report that metric unavailable; no inferred input-to-display number.

For the callback scenario, schedule one calibrated 200 ms CPU turn every 500 ms after
warmup, with no sleep standing in for CPU work; keep actual turn durations and sample
counts. Run real text/layout/paint as a separate workload. Include both semantics-disabled
baseline reproduction and semantics-enabled qualification; label them independently.

Proposed acceptance budget on the recorded reference machine: for a calibrated 200 ms heavy
UI turn, dedicated sibling CPU-response p99 must remain within unloaded p99 plus two measured
display periods (use 16.667 ms only as a declared 60 Hz test setting), with no attributable
stall above 100 ms. Shared-heavy-first must exhibit a distinguishable delay. Record background
OS interference and repeat contaminated runs without discarding their raw data. This budget
is a qualification threshold, not a general API latency guarantee. Presentation regression
is checked against unloaded and shared modes separately; a CPU pass cannot cover a frozen
native loop or missing pixels.

Repeat the two-1000-row CPU-throughput experiment and report effect and variation; the prior
1.91x is a baseline, not a universal minimum speedup. Also measure thread count, peak/live
memory, pending bytes, idle CPU, cold startup and shutdown. Do not enable by default on the
basis of this workload. GPU topology comparisons remain outside this spec.

## Support gate

Headless conformance, native Win32 conformance and native performance evidence are separate
checklists. Linux native Wayland, native X11 and winit are separate hosts; macOS, iOS, Android
and Web need distinct evidence. A feature compile, synthetic test or emulator run must be
labeled as such. The support gate records exact scenarios and remaining limitations rather
than declaring an entire platform ready. No platform is promoted by this documentation change.
