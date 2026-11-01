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
scope includes current runner migration, Win32 native acceptance and headless acceptance;
every other runner must explicitly retain shared execution and reject dedicated admission
until its own integration and native evidence satisfy this contract. Later host enablement
cannot inherit Win32's pass. No support claim follows from drafting or accepting this spec.

Not part of this change: runtime migration, parallel mutable layout, blanket UI `Send` bounds,
per-window GPU topology, automatic selection of placement, or hard real-time guarantees.

## Observable requirements

| ID | Required behavior | Observable acceptance |
|---|---|---|
| MO1 | One permanent owner constructs, executes and destroys each runtime; multiple presentations stay with it. | Compile-fail transfer cases; captured construction, callback and destructor thread identities match. |
| MO2 | Existing `Isolated`, `Shared`, `Application` and root-taking entry points remain local. Dedicated launch is explicit and rejects unsupported hosts before executing its factory. | Existing consumers compile unchanged; local `Rc` factory succeeds; unsupported request returns a typed refusal and factory execution count is zero. |
| MO3 | A successful dedicated receipt means mounted, routed and redraw requested, not displayed or focused. Every admitted launch settles exactly once. | Startup failure, native rejection, full capacity, cancelled observation and quit races leave no orphan window/runtime/thread. |
| MO4 | A dedicated owner progresses while another owner is inside a synchronous CPU turn. | Deterministic barrier-controlled headless test and actual native two-window release experiment; include shared heavy-first and light-first controls. |
| MO5 | Commands, observations, wakes and completions retain owner incarnation, runtime and presentation authority plus operation-specific version/ticket. | Delay old work through close/reopen, surface recreation, focus change, text replacement and owner replacement; new state never receives it. |
| MO6 | All queues, retained payload bytes, launch slots and owners are bounded. Control obligations cannot be displaced by scenes or telemetry. | Saturation matrix proves typed refusal before acceptance, exactly-once settlement afterwards, bounded high-water marks and continued shutdown progress. |
| MO7 | Native input, focus, pointer capture, IME, clipboard, accessibility and window state preserve their existing semantics across the owner boundary. | OS input and native query/action traces distinguish actual backend delivery from synthetic injection; no owner-wait cycle, stale edit, stuck key/capture or misrouted action. |
| MO8 | Frame demand, scene submission and surface lifetime remain addressed and generation-safe. | Resize, DPI, occlusion, mixed refresh, device recovery and close-in-flight tests; fresh pixels and presentation feedback checked separately from CPU completion. |
| MO9 | Quit, close veto, pending-close holds, persistence flush, services and exit policy operate over all owners. | One owner may veto without partially committing global quit; after committed quit every accepted obligation is settled and every owner acknowledges retirement before loop exit. |
| MO10 | Reentry and panic preserve committed state, capture ownership and first-failure custody. | Callback/wake/drop failures separately, two in competition and next-operation recovery; sibling remains usable unless the accepted terminal policy requires exit. |
| MO11 | All runners use the same ownership/routing contract without importing native or GPU types into `flui-runtime`. | Migration inventory complete; shared-path conformance on available targets; explicit rejection and compile coverage for unavailable hosts; no legacy duplicate dispatch path. |
| MO12 | Measurements and support claims are reproducible and scoped to the executed host/backend. | Commit, executable hash, OS/device/driver, configuration, raw traces, repetitions, failures and negative controls accompany each claim. Parity changes only after required actual runs. |
| MO13 | Shared services have explicit lifetimes; dedicated launch does not duplicate process initialization or transfer local capabilities. | Font/provider revision fan-out, startup refusal, device retry, memory reclaim and teardown tests; resource counts return to baseline. |
| MO14 | Cross-owner document state is explicit and versioned; synchronous shared state remains within one owner. | Two views of one document reject stale replies without lost edits; local shared presentation behavior remains unchanged. No implicit shared `Rc` graph. |

## Native acceptance matrix

These are required scenarios, not completed runs. Every case records host and input origin.
Existing platform deficiencies must be named; no feature is silently disabled to pass the
dedicated-owner suite. A missing essential contract keeps dedicated admission unavailable.

| Case | Required native observation |
|---|---|
| Two-window independence | Heavy dedicated runtime performs real build/layout/paint plus a separate calibrated CPU callback scenario. OS-input counter and timer in the sibling keep progressing; both windows actually present changed content. |
| Placement controls | Shared heavy-first reproduces blocking; shared light-first exposes ordering effects. Heavy native/default-owner work remains an explicit negative control, not an isolation claim. |
| Input and focus | Keys down/up, pointer press/move/release, capture cancellation, focus crossing and close while pressed never reach a replacement incarnation or stay stuck. |
| IME and editing | Compose, commit, cancel, selection, surrounding text and caret queries while moving/resizing/DPI-changing the window; late commit after focus or document revision changes is refused correctly. Win32 includes real TSF composition and a native text-store query. |
| Accessibility | Native client reads changed text/selection/bounds and invokes an action in each owner during load; a screen-reader session checks traversal/focus. Tree snapshots alone do not prove actions. |
| Clipboard and drag/drop | Copy/paste over owners, cancel transfer on close, external drop and cross-window drag with versioned document state; supported native operations keep their thread affinity. |
| Surface and pacing | Resize continuously, minimize/restore/occlude, move across DPI/displays, mixed refresh where devices allow; old scene cannot present on a new surface. Record frame age and feedback availability. |
| GPU failure | Inject recoverable loss, exhaust retry and invoke the existing retry command; each affected presentation receives its own outcome and healthy siblings retain their demands. Real device-loss evidence is distinguished from injection. |
| Close and quit | Close primary while another owner survives, veto/hold/release/stay-open, quit during install, pending persistence and callback panic; no synchronous owner join inside OS callbacks. |
| Services and idle | Preferences/font changes, provider replacement, idle background completion, memory pressure/reclaim, reopen after retirement; no duplicate service startup or leaked owner thread. |

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
