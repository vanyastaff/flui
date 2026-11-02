# Multi-owner implementation tasks and evidence

- **Status:** In progress, approved by the owner (2026-11-01). Transport, coordinator and
  destination factory are implemented and exercised through the headless kit and explicit
  Win32 qualification entry. Shared runners use the revised lifecycle; ordinary production
  dedicated admission remains unavailable. No platform support is claimed.
- **Base:** `bb618a7f7f1510ff11093109094ba02c937d1970`.
- **Next:** MO-T7 and the integrated MO-T8 matrix, then finish actual-host MO-T9 verification
  and MO-T10 development-hook attachment. MO-T11 remains blocked on that evidence.
- **Requirements:** [requirements.md](requirements.md). **Design:** [design.md](design.md).

## Work and dependency order

| Task | Dependencies | Requirements | Deliverable and required proof |
|---|---|---|---|
| MO-T1 | Owner acceptance | MO1–MO3, MO11 | Accept independent-owner policy and proposed Win32 native/UI colocation, not a split native bridge; review exact factory-bound exception, Clone compatibility, provider reconstruction and limits. Inventory consumers; no support status change. |
| MO-T2 | MO-T1 | MO1, MO4, MO12 | Move the external headless experiment into the existing testing/performance facilities; deterministic public-path isolation negative control; immutable raw baseline and source/executable manifests. |
| MO-T3 | MO-T1 | MO5, MO6, MO10 | Typed owner/run identity, local/remote routing and bounded reliable control; saturation, late answers, wake race/reentry and panic matrix. Compiler transfer checks. |
| MO-T4 | MO-T3 | MO3, MO9, MO13 | Extract run coordinator from owner-local AppRuntime/InstalledHost; install rollback, membership-versioned quit, deferred close holds, persistence and shared services. Existing local conformance remains green. |
| MO-T5 | MO-T1, MO-T2 | MO1, MO7, MO13 | Qualify two actual same-host Win32 loops on their creating STA threads through existing native testing facilities: HWND/input defaults, TSF grants, UIA, native/COM release. Inventory process startup so a worker cannot run full Application bootstrap. A failure revises the design, not a silent fallback to remote TSF. |
| MO-T6 | MO-T4, MO-T5 | MO2, MO3, MO11 | Wire dedicated factory launch to real secondary-window runner; construct local config/providers on destination, preserve existing APIs; SemVer, facade transfer inventory and production example. |
| MO-T7 | MO-T4, MO-T5 | MO8, MO10 | Wire existing synchronous native/frame resources on each owning loop; coordinated retry/recovery, correct input epochs/haptics and generation-safe readback. No queued-success sink variant. |
| MO-T8 | MO-T5, MO-T6, MO-T7 | MO4, MO7–MO10, MO12–MO14 | Run full Win32 native two-window matrix, including actual IME/accessibility/input, native thread-affinity, load, quit and resource counts. Preserve failed runs and counterfactuals. |
| MO-T9 | MO-T4, MO-T7 | MO2, MO11 | Migrate AppKit, UIKit, Android, Web, native Linux and winit shared paths; dedicated refusal before factory execution. Run available hosts and record unavailable ones; no blanket native pass from Linux CI. |
| MO-T10 | MO-T6, MO-T8, MO-T9 | MO11, MO12 | Migrate driver/devtools/examples/packages, delete obsolete routing/ownership paths, update architecture and changelog, complete final API and consumer checks. |
| MO-T11 | MO-T8, MO-T9, MO-T10 | MO12 | Review exact-head evidence; enable only qualified host/backend combinations and update gaps/parity with linked run evidence. Others remain explicitly unavailable. |

### Completion checklist

- [x] MO-T1 — owner accepted the corrected ADR/spec, factory exception, consumer/configuration
  inventory and bounded-work design on 2026-11-01; the implemented factory has exactly one
  new named `Send` bound, verified by the transfer gate.
- [x] MO-T2 — checked-in public-path headless test/example, raw baseline and failing control.
- [x] MO-T3 — typed run/owner identity, bounded document and main-window receipt admission,
  independent quit/retirement control, stale edit preservation and compiler transfer checks.
- [x] MO-T4 — membership-versioned coordinator, publication rollback, owner-local close
  routing, initial-owner service custody and joins outside native callbacks; headless and
  existing local conformance passed. Full native lifecycle acceptance is still MO-T8.
- [x] MO-T5 — two actual Win32 loops: local input defaults, real TSF grants, UIA and STA teardown;
  process-startup inventory in design.md. This is a prerequisite, not MO-T8 production acceptance.
- [x] MO-T6 — destination factory, private local configuration and provider reconstruction;
  existing secondary installer, document example, transfer/API/SemVer and consumer checks.
- [ ] MO-T7 — native frame and recovery integration.
- [ ] MO-T8 — executed Win32 acceptance and measurements.
- [ ] MO-T9 — common desktop and portable single-owner paths adapted, with refusal before
  factory invocation. Windows execution and Linux/Android/wasm type checks passed; remaining
  actual-host regressions and Apple compilation are still required.
- [ ] MO-T10 — consumer migration, obsolete-path removal and final gates.
- [ ] MO-T11 — evidence-qualified availability and documentation.

## Evidence already available

Code review fixes the initial placement at Win32 native/UI colocation: native input needs
its synchronous default-handling result, TSF holds a local store/grant, and FrameSink returns
actual raster completion. None may be replaced by a queue acknowledgement. MO-T5 qualifies
the actual multi-loop/STA lifetime before runner enablement. A split-native/UI architecture
would need a separate decision for all three contracts and is outside this implementation.
No IME fallback or missing editor support is an acceptable shortcut to the native pass.

Historical research on 2026-11-01, not acceptance of the proposed runners:

| Evidence | Result | Limit |
|---|---|---|
| FLUI base above, Rust 1.99 release, 15 processes, 600 measured rounds | Heavy-first sibling 39.811 ms shared / 0.165 ms separate; light-first control 0.137 ms; two-heavy round 76.264 / 39.910 ms. Exactly one scene per updated panel and final text/paragraph checks passed. | Headless real runtime transactions; semantics disabled; no native host, GPU or proposed transport. |
| FLUI probe release build, release clippy with warnings denied, formatting | Passed before the measurement series; archived source and binary hashes. | Checks apply to the experiment, not to code implementing this specification. |
| WPF installed .NET Framework 4.8.10008.0, nine native processes | Sibling heartbeat p99 209.24 ms shared / 30.69 ms separate / 32.01 ms unloaded. | Synthetic CPU callback; not FLUI, not GPU-present latency. |
| Raven pinned source and 45 CPU / 24 Vulkan processes | Large-scene parallel paint 1.87x / 1.72x; render-thread result inconclusive across tested modes; final static captures equal. | Different workload; no direct FLUI-versus-Raven speed comparison. |

Evidence bundle: external `threading-research` directory delivered with the research report,
dated 2026-11-01. Report SHA256:
`ddd3f40a71e1dc32832ad256547a03cf197a18f98caec3e442abf021ca402d0e`.
FLUI probe executable SHA256:
`d4c84609d0269b7606785116f8f619da4f4d3c5468741ea759d2a3280c6ca403`.
The bundle contains source, locked dependencies, coordinators, raw CSV, aggregation and
environment metadata. Archive it with implementation run artifacts before relying on the
numbers in a release decision; it is not a portable checked-in test suite yet.

## Native evidence ledger

### Executed prerequisites (2026-11-01, Windows 11 build 26300, Rust 1.99)

- MO-T2: `flui-testing::multi_owner::independent_runtime_frames_progress_while_a_sibling_callback_is_held`
  uses real mounted runtime transactions; both sibling roots are mounted before the frame
  request. Shared-owner control cannot produce while the callback is held. Separate-owner
  output includes changed text and exactly one scene before release.
- The checked-in `flui-testing/examples/multi_owner.rs` replaces the external probe.
  Final release build `r8358`, bundled fonts, 15 fresh processes, 40 measured rounds after
  5 warmups each (600 measured rounds). Median light-frame latency: **36.0541 ms** shared,
  **0.1484 ms** separate; reversed-order shared control **0.1189 ms**. Two 1,000-text-row
  runtimes: median pair **71.86055 ms** shared / **36.9413 ms** separate (about **1.95x**).
  Every request produced one scene; final text and paragraph counts matched in every process.
  Semantics disabled; these are CPU transactions, not native/GPU presentation latencies.
- MO-T5: `text::windows::tsf::multi_owner::two_win32_loops_keep_tsf_grants_and_retirement_on_their_owners`
  enters two real `WindowsPlatform::run` loops of the production host type. Each creates its
  HWND, STA and TSF connection locally. Actual `ITfContext::RequestEditSession` edits Unicode
  text through the production ACP store; a shut commit gate refuses a synchronous edit
  without changing text, and the next allowed edit succeeds. While A's callback is held,
  B's TSF edit and actual UI Automation name query complete. B also works after A retires.
  Prevented F11 leaves native `WindowMode` unchanged on both loops; allowed F11 toggles it
  and toggles it back. HWND destruction, TSF shutdown inside STA and final STA release are
  asserted on each owner. No foreground change or system-wide keyboard injection is used.
- The full local Windows suite (`r8354`) passed **626 tests**, with two opt-in tests skipped.
  It first exposed an obsolete TSF test expecting a retained payload (`r8351`, reproduced
  alone in `r8352`). ADR-0281 requires contained destruction; the corrected assertion counts
  one destruction and verifies the next healthy focus. Production cleanup behavior did not change.
- Three temporary negative controls (`r8355`) all failed as intended: forced shared routing
  timed out waiting for the independent frame; ignored `default_prevented` made prevented F11
  change native mode; retaining the TSF payload yielded zero destructions instead of one.
  Exact source bytes were restored and checked. Final targeted run `r8357`: **3/3 passed**.
- `cargo heli clippy --locked -p flui-native -p flui-testing --all-targets -- -D warnings`
  passed (`r8356`); formatting and diff whitespace checks passed. Builds used an isolated
  checkout target and `CARGO_INCREMENTAL=0`.
- `cargo heli xtask checks` passed (`r8359`, exit 0): source/documentation, module, panic,
  transfer, test-kit and repository consistency gates. This does not replace native acceptance.

Artifact bundle: `multi-owner-implementation-2026-11-01`, archived outside the public source
tree. It contains the original historical source/raw baseline, both new measurement series,
source/lockfile and executable copies, machine/toolchain metadata, negative-control details,
restoration hashes and complete success/failure logs. Final measured executable SHA256:
`bdd07165da3778cd66f95b8a93be08c87746d406276d112530f3d8ff64b363ce`.
Reproduction commands and argument meanings are in `crates/flui-testing/ARCHITECTURE.md`.
The native command is `cargo heli nextest run --locked -p flui-native --lib -E 'test(two_win32_loops_keep_tsf)'`.

Those prerequisite runs use the reference text store, not a mounted editor, and do not
exercise foreground IME composition/candidates or the integrated runner. The subsequent
implementation runs below add factory/coordinator coverage, without completing MO-T8.
MO-T7–MO-T11 remain required; no support cell changes.

| Host / mode | Implementation acceptance | Native execution | Availability |
|---|---|---|---|
| Headless dedicated route | Factory, coordinator and transport exercised through testing kit | Public runner tests and document example passed; earlier CPU benchmark remains a separate baseline | Unavailable in production runner |
| Win32 dedicated | Existing native/frame installer wired on destination owners under testing qualification | Low-level TSF prerequisite and real two-owner factory/GPU-window smoke passed; full native matrix not run | Unavailable |
| Native Wayland dedicated | Not implemented | Not run | Unavailable |
| Native X11 dedicated | Not implemented | Not run | Unavailable |
| winit Wayland/X11 dedicated | Not implemented | Not run | Unavailable |
| AppKit dedicated | Not implemented | Not run | Unavailable |
| UIKit dedicated | Not implemented | Not run | Unavailable |
| Android dedicated | Not implemented | Not run | Unavailable |
| Web dedicated | Not implemented | Not run | Unavailable |

For each future run attach commit/dirty diff, binary hash, OS/host/backend/device/driver,
display configuration, feature flags, commands, scenario/test names, raw samples and traces,
negative control, pixel predicates, resource counts and outcome. Separate CPU completion,
native input acknowledgement and physical presentation. Mark unavailable metrics explicitly.
No cell may be promoted solely from another host's result or from accepting ADR-0284.

### Transport, coordinator and factory execution (2026-11-02 UTC)

Implementation base: `8dbe70827f63a7cb2bec4978e599d01fdded1639`, with the exact implementation
diff, new files, lockfile and executable hashes retained in the external
`multi-owner-runtime-2026-11-02` evidence bundle. Windows 11 build 26300, Rust 1.99,
i9-13900K; RTX 3070 Ti driver 32.0.15.6094, 3440x1440 at 100 Hz. These are behavioral
checks, not a new throughput or compositor-latency measurement.

| Contract | Executed observation |
|---|---|
| Construction and rollback | `dedicated_factory_constructs_and_retires_local_captures_on_its_owner` observes factory, build and capture destruction on the destination. Factory failure settles and permits another owner. Spawn reservation has an unwind rollback guard. |
| Bounded reliable admission | `saturated_owner_admission_does_not_execute_rejected_factory_or_block_quit`, `document_payload_budget_covers_unobserved_completions_and_quit`, and both `completed_launch_receipts_keep_their_admission_budget_until_observed` / `completed_main_receipt_keeps_the_shared_command_budget_until_observed` retain capacity through unread completions while reserved quit still progresses. |
| Identity and asynchronous data | `cross_owner_document_rejects_stale_reply_without_losing_edit` returns both the winning snapshot and losing edit. `failed_owner_ports_never_retarget_a_survivor_or_a_new_application` refuses retired ports and a different application incarnation. |
| Reentry and failures | `document_completion_reentry_and_panicking_wake_preserve_all_accepted_work`, `factory_panic_payload_retires_on_its_owner_for_both_launch_paths`, `published_owner_failure_is_retained_until_application_result` and the strengthened secondary-cancellation table retain accepted obligations, destroy captures and report the retained failure. |
| Global quit and lifetime | `global_quit_waits_for_starting_owner_and_consults_its_window` includes an abandoned receipt's pending launch. `global_quit_revalidates_windows_admitted_during_consultation` rejects an obsolete vote. Existing close/hold/persistence tests pass. `last_owner_retirement_rechecks_windowless_application_exit_policy` proves the coordinator does not strand an empty application. |
| Shared services and local policy | `registered_fonts_are_shared_across_owner_retirement` covers a surviving owner and a fresh subsequent application. `dedicated_provider_policy_is_explicit_and_optional_refusal_is_preserved` checks reconstruction without eager acquisition or loss of optional refusal. |
| Real consumer | `flui-app/examples/multi_owner_document.rs` creates two local provider instances and exchanges versioned document messages. The headless run observes a conflict without losing either value; ordinary native admission reports unavailable. |
| Compiler boundary | `dedicated_factory_builds_local_state.rs` compiles with a destination-local `Rc`. `dedicated_factory_cannot_capture_local_state.rs` fails with E0277 for a captured `Rc<String>`. |

Executed gates:

- `r8405`: strict clippy, `flui-app`, `flui-runtime`, facade; all targets and all features.
- `r8406`: **214 app/runtime tests passed**, one opt-in native case skipped; `r8402` also
  ran all **15 dedicated headless cases** after the main receipt budget repair.
- `r8407`: explicit `native_dedicated_factory_builds_on_two_win32_owners_and_retires`,
  **1 passed**. Two real GPU-backed Win32 windows, two root-owner thread IDs, coordinated
  retirement. No pixel readback, IME candidate/composition, assistive-technology client,
  multi-owner GPU recovery or complete session-end matrix was measured by this smoke.
- `r8408`: actual Windows `flui-native` suite, **546 passed**, two opt-in tests skipped;
  includes the two-loop TSF/input/UIA prerequisite.
- `r8409`: **96 facade/testing consumer tests passed**, including external consumer projects.
- `r8410`: complete facade trybuild suite passed without snapshot overwrite. The one added
  stderr fixture records the intentional new factory boundary; no existing snapshot changed.
- `r8411`: app/runtime doctests, **11 passed, 4 ignored**.
- `r8412`: send-position gate passed, **276 positions across 26 crates**. One new factory
  bound is authorized by ADR-0284; eight existing error-source positions changed module path.
- `r8413`: facade API closure passed. `r8415`: explicit patch-level facade SemVer check
  against the implementation base, **229 checks passed, 31 skipped**. The initial `r8414`
  skipped all checks for the prerelease version and is not treated as compatibility proof.
- `r8416` / `r8417`: strict app/facade library clippy passed for wasm32 and Android arm64
  (installed NDK 29, API 21 C compiler). `r8420`: Linux GNU app/facade library check passed.
  These compile target code; they do not execute browser, Android or Linux native loops.
- `r8418`: repository macOS cross-typecheck could not start because `zig` is absent.
  `r8419`: repository iOS check explicitly skipped without a genuine Apple SDK. Neither
  result is an Apple pass; actual-host checks remain owed.
- After the retirement corrections below: `r8442` strict all-target/all-feature clippy
  passed again; `r8443` ran **217 app/runtime tests, all passed**, one opt-in native case
  skipped. `r8434` separately ran all **17 headless multi-owner cases**, all passed.
  `r8444` repeated the actual two-window Win32 factory smoke, **1 passed** (0.879 seconds).
  `r8421` repository source/document checks passed before these final retirement corrections;
  final results are retained with the external evidence bundle.
- `r8445`: final headless document example preserved the committed value and rejected edit.
  `r8446`: Linux GNU app/facade library check passed again after the join-helper changes.
  `r8447`: the actual two-loop Win32 TSF/input/UIA prerequisite passed again (0.308 seconds).
- `r8448`: final repository checks passed, including formatting, source/document consistency,
  globals, panic boundaries, transfer implementation inventory and testing-kit placement.
  `git diff --check` passed. No unsafe transfer implementation was added.

Failing evidence is retained, not replaced by green logs: obsolete quit membership (`r8360`),
missing final-owner exit recheck (`r8381`), retained initial-factory panic payload (`r8394`),
stale document replacement (`r8397`), launch receipt/cancelled capture counterfactuals (`r8399`),
and unread main receipt capacity (`r8401`). Two initial counterfactuals falsely passed in
`r8397`; the tests were strengthened with an observed completion wake and propagation of
the platform boundary's held assertion, then both failed as intended in `r8399`. Production
sources were restored and SHA256-checked before green verification. No failed native run
or negative-control result is omitted from the external bundle.

Retirement review added two public regressions. `r8422` showed admission blocking on a
worker whose `is_finished` was true while its TLS destructor still waited: the test's
10-second release channel expired. A non-UI join helper now retains membership until a
full join, transfers the result through reserved control state, and runs no application
wakers, logging or panic-payload destruction itself. Failed-launch settlement follows
owner-local cleanup and admission fencing. `owner_capacity_includes_thread_local_retirement`
now refuses a second owner immediately while that destructor is held.
`r8429` initially passed the quit regression without an independent main-window close;
that was insufficient evidence. The strengthened
`global_quit_keeps_initial_loop_until_full_thread_retirement` failed in `r8433`: the initial
loop had already stopped. Auto-exit now retains the coordinator through the full join.
Both cases pass in `r8434` and the final app/runtime suite.

Runner/caller reconciliation:

| Path | Result and remaining acceptance |
|---|---|
| `Application`, `main_window`, desktop legacy entry, main show requests | One initial coordinator, shared process startup, receipt budget and membership-aware exit. Existing public entry bounds remain local. |
| `secondary_window`, `installed_host`, `window_routes`, native bindings | Local publication reserves membership through success/rollback; native callbacks remain local. Rendered `Shared` policy still refuses. |
| Dedicated headless / Win32 qualification | Factory builds local config/providers; existing synchronous desktop installer owns native/frame resources. No worker calls `Application::run`. |
| `frame_driver`, `owner_dispatch`, fonts, storage/execution inputs | Close outcomes carry the source owner; retry wakes every registered renderer. Worker teardown releases local objects without shutting down application pools/feed; initial teardown joins workers first. Full frame/recovery proof remains MO-T7/MO-T8. |
| AppKit and Linux winit desktop path | Reuse the common initial-owner runner; production backend selection leaves dedicated unavailable. Linux typechecked; AppKit actual-host/compiler validation pending. Native Wayland/X11 remain unavailable implementations. |
| UIKit sessions, Android activity, Web direct raster | Preserve single-owner OS lifecycle and synchronous local dispatch. Desktop coordinator fields are target-gated; portable dedicated entry refuses before invocation. Android/wasm typechecked; actual-host regressions pending. |
| Facade, SDK, examples/packages | Facade exports the factory and opaque receipt/configuration; SDK gains no app/native dependency. New document example exercises the API; facade/package feature consumers compile. |
| Driver/devtools/reload attachments | Still MO-T10: the testing dedicated route explicitly refuses applications with development hooks before invoking the factory. Existing single-owner attachments remain. No second app endpoint or silently missing attachment is accepted. |

The unavailable admission gate must stay in place until MO-T7–MO-T10 and the integrated
native acceptance matrix are satisfied. The existing CPU benchmark numbers do not establish
the performance or physical presentation latency of this new application route.

## Validation commands and exit conditions

### Integrated Win32 progress (2026-11-01 America/Chicago)

MO-T7/MO-T8 remain incomplete; production dedicated admission remains unavailable.
Artifacts are in the external `multi-owner-native-2026-11-02` bundle (UTC naming).

- `post_frame_device_recovery_repaints_the_recreated_surface_without_new_input`
  reproduced a missing fresh scene after post-frame recovery (r8450), then passed (r8451).
  The actual-GPU `native_multi_owner_frames_survive_recovery_and_resize` passes (r8497);
  removing the repaint fix makes it fail waiting for the new surface capture (r8501),
  with bounded cleanup. The earlier failing control r8458 hung during shutdown and is
  retained separately. Fixed source restoration was SHA256-checked.
- `native_multi_owner_recovery_exhaustion_and_retry` observes actual backoff exhaustion
  on each owner, its own failure report, no autonomous retry after abandonment, recovery
  after `AppHandle::retry_render_device`, and fresh pixels while the healthy sibling runs.
  r8484 and the three-case r8497 passed. Device loss is injected; GPU rebuilding is real.
- `native_multi_owner_idle_quit_retires_both_loops` exposed an exit-notifier platform pin
  surviving until TLS destruction (r8470/r8471), then a retained STA caused by live HWNDs
  after loop exit (r8474/r8475). Teardown now releases the notifier outside the runtime
  borrow and closes the native window set. The strengthened test passes (r8481/r8497).
  `owner_quit_closes_live_windows_and_refuses_reentrant_admission` passes (r8507); omitting
  the native close step fails with live windows after return (r8509), after test cleanup.
- `native_multi_owner_minimize_restore_preserves_geometry_and_pixels` passed (r8504):
  both owners restore their actual screen/client rectangles and fresh surface pixels,
  with sibling progress during minimization. r8502 incorrectly requested restoration from
  suspended build work; r8503 sampled a transitional iconic rectangle too early. Both
  are retained as fixture failures, not evidence of a product regression.
- `native_multi_owner_dispose_failure_retires_both_loops_and_reports_failure` passed
  (r8508): a dedicated `ViewState::dispose` panic returns `DedicatedOwner`, both renderers
  retire and the initial STA is released. r8505/r8506 instead injected a raw native-handler
  panic, which reaches the aborting OS boundary; they do not test application containment.
- Interactive session 04 exercised mounted EditableText on both owners: Japanese Hiragana,
  visible candidate suggestions, conversion, commit/cancel, A-to-B composition completion,
  native selection/copy/paste across owners, close while composing and surviving-owner IME.
  The reference-store TSF probe also passed with its explicit keystroke preprocessor disabled
  (r8483), so a missing preprocessor is not an established cause of the earlier failed input.
- Session 05's independent MTA UIA client observed current values and correct A/B keyboard
  focus on both owners. The automation tool's stale `focused_element` did not reproduce in
  native UIA or in `two_fields_publish_the_current_keyboard_focus`, including its folded
  published adapter tree (r8498/r8499). No production focus fix was made.
- r8496 strict all-target/all-feature clippy passed for app/engine/native before the later
  qualification additions. Final checks are still required. No native latency or throughput
  result is claimed from these functional run durations. The complete external acceptance
  and measurement ledgers retain every unmet obligation.
- The full Windows native suite (r8512) and isolated OLE drag/drop case (r8513) aborted when
  COM retirement pumped a UIA property request after AccessKit's string scratch TLS was
  destroyed. The standalone vendor regression reproduced both late acquisition and late
  lease return (r8530). Optional-cache access now tolerates destroyed TLS: all five cache
  tests pass (r8531), and `ole_drag_and_drop_on_real_windows` passes (r8532).
  The standalone dependency lock was generated offline and retained with the external run
  artifacts; these vendor unit tests do not replace the actual native case.
  The subsequent full Windows native suite passed: r8534, 547 passed, 2 skipped,
  70.982 seconds. This is a native crate regression gate, not the complete multi-owner matrix.
- All five native GPU cases passed together in r8537 (30.233 seconds). Source/document
  checks passed in r8539; strict all-target/all-feature clippy for app/engine/native/widgets
  passed in r8540. The broader all-feature test run r8542 stopped at an image-filter GPU
  oracle: `filtered image matrix range` returned red 0 instead of 64 for a finite maximum
  coefficient and offset 0.25. Restoring the three changed engine sources to HEAD reproduced
  the same failure in r8546; the working versions were then restored with hash verification.
  A complete non-trybuild continuation, r8547, ran 333 tests: 332 passed and this one failed,
  with 9 skipped (the 3 trybuild cases had already passed in r8542).
- The image-matrix regression now covers 150 coefficient/source/offset combinations,
  including cancellation, positive/negative saturation and finite maximum offsets. The
  shader normalizes through two bounded divisors and bounds both weights and offset before
  their sum. r8579 passed all combinations; restoring the original shader made the expanded
  test fail in r8580. Earlier incomplete variants are retained as diagnostics, not fixes.
  The fixed shader was restored with SHA256 verification. The full engine library rerun
  r8583 passed all 96 tests, with no skips and `FLUI_REQUIRE_GPU=1` (140.955 seconds).
  Readback logs identify Intel UHD Graphics 770, Vulkan, driver 101.7088; this records the
  executed device/backend, not a claim about unexecuted combinations.
- `native_multi_owner_production_admission_remains_closed` passed through ordinary
  `Application::run` in r8581. It checks typed refusal before any factory execution and STA
  retirement. Temporarily admitting qualification owners through that production entry
  makes the test fail after cleanup (r8582). Production admission was restored to unavailable
  and checked against its original source bytes.
- r8584 passed the combined app/engine/widgets all-feature run including opt-in native
  cases: 340 passed, 3 trybuild cases excluded, 232.994 seconds, GPU required. The previously
  failing image row now passes in the combined run. The three trybuild cases passed in r8542.
- The external checker was executed, not inferred from the source inventory. Its first run
  exposed Windows shell quoting errors in the CHECK commands; the commands were repaired
  without changing test names, criteria or the required nonzero count. Reverify recorded in
  `checker-reverify-0653.log` confirms P1, P2, F1, N10 and N17: 5 met, 16 unmet of 21.
  All 9 measurement criteria remain unmet. Missing scenario tests produce a zero-test
  failure; the measurement harness/verifier is still absent. Historical manual IME evidence
  is retained separately and is not a substitute for the complete editing/geometry matrix.
- Final strict clippy passed in r8604 (app/engine/native/widgets, all features and targets,
  warnings denied). The full source/document gate passed in r8605. The vendored patch is
  checked for reverse applicability with ordinary `git apply --check`, as well as whitespace.

### Native placement, transport and measurements (2026-11-02)

- `native_multi_owner_placement_controls_preserve_independent_progress` passed in r8610:
  six actual Win32/GPU cases cover shared heavy-first, shared light-first and dedicated
  placement, holding either initial or sibling owner. Changed surface pixels distinguish
  independent progress from a completed CPU callback or a frozen previous image.
- `native_multi_owner_resource_retirement_returns_to_baseline` passed in r8641 and r8646:
  three fresh application runs check both HWNDs, renderer release, STA custody and completion
  of a deliberately held dedicated TLS destructor before `Application::run` returns.
  Replacing `join` with an `is_finished` wait fails that exact return-order assertion in
  r8644; the original implementation was restored and its SHA256 checked. An earlier
  premature detach control, r8642, instead timed out before retirement and is retained
  separately, not presented as the decisive counterfactual.
- `native_multi_owner_saturation_preserves_control_and_settlement` passed in r8655:
  command-count and retained-byte limits are exercised separately on two native owners.
  Unread completions retain reservations, replacement wakers receive completion, stale
  writes preserve the newer revision and rejected bytes, and quit settles accepted edits
  with ordinary queues full. Removing the admission bounds fails in r8656; bounds were
  restored with SHA256 verification. Strict app/runtime/native clippy passed in r8657.
- The `native_multi_owner_measure` qualification example uses actual Win32 windows, native
  frame callbacks, renderer submission/present-return stamps and ticket-specific surface
  readback. Semantics-on and semantics-off populations are explicitly controlled and checked.
  The off switch exists only in testing; normal accessibility demand is unchanged.
- Historical release acquisition `measurement-release-0752` completed 60 fresh processes,
  but was subsequently withdrawn as qualification of two visible windows: its launcher
  supplied hidden startup, hiding the first HWND. The raw data and following historical
  numbers remain preserved; they do not satisfy the native presentation gate:
  48 response runs (four placements, two workloads, two semantics configurations, three
  repetitions) and 12 native 1,000-row pair runs. Response runs contain 30 warmups and 350
  measured light requests each. The executable SHA256 is
  `24fcaa806751a996b5be4212dc3c41720abf4f3742f5becbde4d7d95825a61ff`;
  the complete source-manifest SHA256 is
  `9652d522b4fa6aab9f64fada88c987e6feb0bec255c533bd14bd51adddeb04ac`.
  The actual adapter was NVIDIA GeForce RTX 3070 Ti, DX12, with a 10 ms native display period.
- Callback request-to-scene p99, semantics off/on respectively: unloaded 0.9019/0.8768 ms,
  dedicated 0.7465/0.7604 ms, shared heavy-first 202.2216/202.9705 ms. Dedicated maxima were
  1.7913/1.0827 ms. These are wall response times before GPU work, not pure CPU durations.
  All 222 measured heavy callbacks charged 203.125 ms thread CPU; observed accounting
  granularity was 15.625 ms. Scheduling timestamps and calibration tolerance are preserved.
- Separate CPU-only throughput acquisition `cpu-throughput-0809` has six fresh processes,
  ten warmups and fifty measured pairs each. Both runtimes layout and paint 1,000 text rows;
  semantics-disabled state, paragraph counts and submitted scenes are asserted. Pairwise
  median speedups were 1.8381, 1.8083 and 1.7650; geometric mean 1.8035. Native pair results
  include GPU/readback cost and are not relabeled as this CPU experiment.
- Raw resource counters preserve startup/shutdown, sampled peak/live threads and memory,
  retained document payload bytes and a bracketed two-second idle interval. Residual process
  threads are not automatically classified as FLUI leaks: a separate native start-address
  observation found surviving workers in system DLLs after FLUI owner/join/font threads ended.
  The full-join test above separately verifies owner resource custody.
- Eight corrupted acquisition controls fail the verifier: missing run, missing measured
  request, wrong placement, stale readback pixels, missing idle counters, changed executable,
  duplicate callback schedule and over-budget response. Original acquisition files remain
  immutable. The checker must reverify the complete ledger before updating its verdict.
- One bounded WPR CPU recording of the frozen release executable completed with 1,000
  measured unloaded semantics-on requests. The profiler is stopped and the process exited.
  Profiling results are diagnostic, not replacement unprofiled performance measurements.
  Startup/GPU stacks and out-of-range nearest-symbol names cannot establish a hot function
  in the submillisecond request-to-scene interval. No optimization or library replacement
  is claimed from the aggregate profile.
- A second bounded WPR recording used full matching PDBs and 2,000 measured unloaded
  semantics-on requests. The diagnostic executable is
  `8c683f0fbe356446c5211b6340aa335c5d1c14ae36e82c5b705b4dac258e0550`.
  Among 1,060 transaction-to-scene CPU stack samples, enclosing build/layout/semantics/paint
  phases accounted for 41.7/24.3/14.6/5.8 percent; the remainder was other frame work.
  These are sampled CPU distributions, not fractions of wall-time p99. No out-of-range
  native symbol or broken stack occurred in that interval. Six alternating unprofiled
  runs showed greater variability in this diagnostic build; it does not replace the
  qualification baseline. No single dominant library, replacement benefit or optimization
  was established. Both WPR sessions ended and their raw ETL/PDB evidence is preserved.
- Corrected acquisition `measurement-visible-0942` recorded 60 fresh processes with
  actual HWND visibility observations and acquisition-time archives of 1,559 artifacts,
  including all eight generated compiler inputs. Executable SHA256:
  `f7ab7b59801c839c630425e1034bbcbfbaaaa394dcd44b415e40152644f44de2`;
  source-manifest SHA256:
  `7b351b32e0ecd96739d8c3f92573ecfeaf3dbca2a16cf1c25fb630a0104117a7`.
  A latency-independent review flagged 51 runs for external CPU interference. All 51 were
  repeated in `measurement-visible-repeat-1011` with that identical executable; 33 repeats
  still exceeded the conservative one-average-core external-load trigger. Original and
  repeated raw data remain separate. Window enumeration now has its own begin/end timestamps
  to distinguish an observation spanning window closure from the measured interval.
- `checker-visible-repeat-1023.log` recorded 8 met and 1 unmet measurement gates before
  subsequent testing-kit changes. Presentation observations passed; response qualification
  remains unmet because the affected cases lack below-trigger repetitions. This is not a
  current-source performance pass after further changes. Final acquisition/reverification
  remains required in a suitable host-load window. CPU-only throughput retains its explicit
  source-compatibility evidence; its raw acquisition was not relabeled or replaced.
- `native_multi_owner_accessibility_reads_and_actions` passed in r8721 and, after fixture
  cleanup and lint fixes, r8730. An independent native UIA client changes a mounted
  `EditableText`, selects and reads its first five characters, observes finite nonempty
  selection bounds and requests focus while the other owner is held in charged CPU work.
  Both owners take both roles; the held scene remains unpresented until release, the active
  owner presents, and the other field and sibling document retain their values. Actual HWND
  visibility is checked. Routing the action to the other field fails the independent
  addressed-field assertion (r8722); after removing a secondary fixture cleanup panic,
  r8725 retains only the intended failure. The correct target was restored. Strict
  app/native all-target/all-feature clippy passed in r8729. This covers native UIA actions,
  not the separate real screen-reader or full IME matrix.

Artifacts above are in the external `multi-owner-native-2026-11-02` evidence bundle.
- `native_multi_owner_mounted_editor_tsf_grants_wait_for_idle` and the existing native UIA
  case passed together in r8788 (2 tests). The mounted `EditableText` receives an actual
  TSF read/write grant on each owner while its sibling is held in charged CPU work. A
  synchronous request during build specifically receives `TS_E_SYNCHRONOUS`; an accepted
  asynchronous request runs after the commit gate opens. Full Unicode text, native text
  geometry, owner identity and independent UIA readback agree. Omitting `SetText` fails
  the exact resulting-text assertion (r8785); the helper was restored byte-for-byte.
  This is a native edit-session client, not an installed IME candidate-window test.
- The earlier focus-change controls r8775/r8778 timed out, while the no-focus-change
  control r8780 passed. Closing a TSF document discards its ordinary waiting grants;
  the accepted composition-completion custody requirement is a distinct operation.
  These observations do not establish a production defect or satisfy that requirement.
- Two loaded Narrator attempts (`reader-1043`, `reader-1051`) did not establish spoken
  labels or traversal. Tool input/window errors prevented speech-recap observation;
  one fixture hit its watchdog, the other was terminated after the failed attempt.
  They are retained as unsuccessful attempts, not clean-retirement evidence. Computer
  Use was released, and the user confirmed Narrator was stopped. Screen-reader acceptance
  remains open. The last complete matrix checker remains 9 met / 12 unmet of 21
  (`checker-native-1034.log`); these partial editing observations do not promote a gate.
- Before publishing the native qualification increment, strict all-feature/all-target
  clippy passed for app/native/runtime/engine/widgets (r8794). The full Windows native
  suite passed again: r8796, 547 passed, 2 skipped, 57.693 seconds. Repository checks
  passed in r8792. These gates do not replace the remaining integrated native matrix.

Physical request-to-scanout latency remains unavailable. These results do not complete the
remaining IME, accessibility, native input, surface, lifecycle and failure matrix; ordinary
production dedicated admission remains closed.

Planned public-path acceptance names below are contracts for future tests, not claims that
those tests already exist. Place them in the appropriate existing integration-test modules
and testing kits; the native cases additionally require recorded actual-host runs.

| Planned case | Requirements | Failure distinguished from success |
|---|---|---|
| `dedicated_owner_progresses_while_sibling_turn_is_held` | MO1, MO4 | Force routes local: sibling cannot complete until heavy turn releases. |
| `dedicated_factory_constructs_and_retires_local_captures_on_its_owner` | MO1, MO3 | Creation, callbacks and destruction have the same destination identity, including failed install. |
| `unsupported_owner_placement_does_not_execute_factory` | MO2 | Unavailable host refuses before factory side effects; no fallback to shared execution. |
| `owner_route_rejects_replaced_incarnations` | MO5 | Held old input/wake/completion never reaches a reopened window or runtime. |
| `saturated_owner_still_settles_control_and_retirement` | MO6, MO9 | Full ordinary lane cannot drop receipt settlement, terminal fence or retirement acknowledgement. |
| `owner_wake_replacement_preserves_newer_work` | MO6, MO10 | Older wake completion cannot clear a newer pending obligation. |
| `native_input_default_observes_each_owners_resolved_result` | MO7 | Prevented shortcut suppresses native default; allowed shortcut reaches it, on both actual loops. |
| `accepted_composition_completion_keeps_its_original_store` | MO5, MO7 | Focus change/detach/shutdown cannot cancel an accepted completion; a new stale request is refused separately. |
| `owner_scene_cannot_present_after_surface_replacement` | MO8 | Distinguishable scene pixels/epochs reject stale work after resize/recreate. |
| `queued_work_is_not_a_presented_frame` | MO8 | Admission alone never consumes input epochs or fires frame-presented haptics; actual inline verdict controls them. |
| `global_quit_revalidates_membership_and_preserves_recorded_holds` | MO9 | New launch joins consultation; holds defer, unrecorded refusal/stay-open cancels; no coordinator retirement before commit. Independent closes still finish. |
| `owner_failure_preserves_sibling_and_first_failure` | MO10 | Callback failure plus cleanup failure preserves the first and permits the prescribed next operation. |
| `shared_services_survive_one_owner_retirement` | MO13 | Closing an owner does not stop surviving owners' font feed, storage, retry or provider service. |
| `dedicated_provider_policy_is_explicit_and_optional_refusal_is_preserved` | MO13 | App override cannot silently become a built-in; missing reconstruction refuses launch, while context-dependent optional Unsupported does not. |
| `cross_owner_document_rejects_stale_reply_without_losing_edit` | MO14 | Delayed versioned reply cannot overwrite a later accepted edit in either view. |

For this documentation change: `cargo heli xtask checks` with a checkout-local target.
Record its actual outcome below; this command is not native validation.

During implementation, use locked Cargo commands through heli: formatting, targeted clippy
with `--all-targets -- -D warnings`, nextest for changed crates and public-API dependents,
doctests, facade SemVer/API closure and send-position gates on the required toolchains.
Native changes require `cargo heli nextest run --locked -p flui-native` on the actual target;
this is necessary but insufficient for the native matrix. GPU readback uses one test thread.
CI Linux/wasm coverage does not replace Windows, Apple, Android or browser execution.

Before completion, search all existing direct native/TLS dispatch callers, record each one's
new route or justified local-only use, and remove unused alternatives. Verify optional
features, examples, package consumers and tooling. On feature integration into main, move
durable contracts into ADR/architecture and remove this spec per repository policy; retain
run evidence through the project's artifact publication mechanism.

### Documentation validation

- 2026-11-01: `cargo heli xtask checks`, heli run `r8339`, exit 0 (197.3 seconds),
  checkout-local target and `CARGO_INCREMENTAL=0`. Formatting, typos, TOML, links/paths,
  workspace/layer gates, globals, panic boundaries and remaining source checks passed.
  This validates documentation/source consistency only; no native multi-owner run occurred.
- 2026-11-01: corrected specification passed `cargo heli xtask checks`, heli run `r8341`,
  exit 0 (89.0 seconds), with the same isolated target and incremental setting. Targeted
  spelling, all eight spec links and `git diff --check` also passed. Independent standards
  and requirements reviews found their reported contradictions resolved after correction.
  No native qualification or production implementation is included in this validation.
