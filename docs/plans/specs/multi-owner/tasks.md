# Multi-owner implementation tasks and evidence

- **Status:** Draft prepared 2026-11-01; awaiting owner review. No implementation task is
  complete and no dedicated-owner platform support is claimed.
- **Base:** `bb618a7f7f1510ff11093109094ba02c937d1970`.
- **Next:** review ADR-0284 and the factory transfer-position exception, then execute MO-T1.
- **Requirements:** [requirements.md](requirements.md). **Design:** [design.md](design.md).

## Work and dependency order

| Task | Dependencies | Requirements | Deliverable and required proof |
|---|---|---|---|
| MO-T1 | Owner acceptance | MO1–MO3, MO11 | Record accepted ADR status and narrow supersession; approve exact factory-bound exception and limits. Inventory every facade consumer and provider field before code changes. No support status changes. |
| MO-T2 | MO-T1 | MO1, MO4, MO12 | Move the external headless experiment into the existing testing/performance facilities; deterministic public-path isolation negative control; immutable raw baseline and source/executable manifests. |
| MO-T3 | MO-T1 | MO5, MO6, MO10 | Typed owner/run identity, local/remote routing and bounded reliable control; saturation, late answers, wake race/reentry and panic matrix. Compiler transfer checks. |
| MO-T4 | MO-T3 | MO3, MO9, MO13 | Extract coordinator/native/local ownership from AppRuntime/InstalledHost; install rollback, consultation/commit quit, persistence and services. Run existing local host conformance before enabling remote routes. |
| MO-T5 | MO-T3, MO-T4 | MO7, MO14 | Implement Win32 native query/edit/publication bridge against TSF and UIA contracts; enumerate mirrored fields and mutation cases; prove no synchronous owner wait. A missing synchronous contract blocks MO-T8. |
| MO-T6 | MO-T4, MO-T5 | MO2, MO3, MO11 | Wire dedicated factory launch to real secondary-window runner; construct local config/providers on destination, preserve existing APIs; SemVer, facade transfer inventory and production example. |
| MO-T7 | MO-T3, MO-T4 | MO8, MO10 | Separate owner CPU frame production from native frame bindings; scene/lifetime/recovery wiring and addressed completion; readback distinguishes stale from correct frames. |
| MO-T8 | MO-T5, MO-T6, MO-T7 | MO4, MO7–MO10, MO12–MO14 | Run full Win32 native two-window matrix, including actual IME/accessibility/input, native thread-affinity, load, quit and resource counts. Preserve failed runs and counterfactuals. |
| MO-T9 | MO-T4, MO-T7 | MO2, MO11 | Migrate AppKit, UIKit, Android, Web, native Linux and winit shared paths; dedicated refusal before factory execution. Run available hosts and record unavailable ones; no blanket native pass from Linux CI. |
| MO-T10 | MO-T6, MO-T8, MO-T9 | MO11, MO12 | Migrate driver/devtools/examples/packages, delete obsolete routing/ownership paths, update architecture and changelog, complete final API and consumer checks. |
| MO-T11 | MO-T8, MO-T9, MO-T10 | MO12 | Review exact-head evidence; enable only qualified host/backend combinations and update gaps/parity with linked run evidence. Others remain explicitly unavailable. |

### Completion checklist

- [ ] MO-T1 — decision and transfer-policy review.
- [ ] MO-T2 — reproducible public-path headless baseline and failing control.
- [ ] MO-T3 — transport and identity contracts.
- [ ] MO-T4 — coordinator, local runner migration and lifecycle.
- [ ] MO-T5 — native query, input and shared-document bridge.
- [ ] MO-T6 — public dedicated entry and real consumer.
- [ ] MO-T7 — native frame and recovery integration.
- [ ] MO-T8 — executed Win32 acceptance and measurements.
- [ ] MO-T9 — all remaining runners migrated with honest capability refusal.
- [ ] MO-T10 — consumer migration, obsolete-path removal and final gates.
- [ ] MO-T11 — evidence-qualified availability and documentation.

## Evidence already available

Blocking design proof before MO-T5 implementation: the current TSF consumer holds an
owner-local `Rc<dyn TextStore>` and executes local read/edit grants. A snapshot-only adapter
is not equivalent. Preserve ADR-0090/0135 semantics, demonstrate the remote edit/lock protocol,
and obtain a narrow text-contract decision if it changes them. If it cannot be demonstrated,
compare Win32 native/UI colocation and amend the initial placement before enabling it.
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

| Host / mode | Implementation acceptance | Native execution | Availability |
|---|---|---|---|
| Headless dedicated production route | Not implemented | Not applicable; historical probe only | Unavailable in production runner |
| Win32 dedicated | Not implemented | Not run | Unavailable |
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

## Validation commands and exit conditions

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
| `native_text_transaction_survives_remote_focus_and_revision_changes` | MO7 | TSF edit/query and delayed completion cannot mutate another field or stale document version. |
| `remote_scene_cannot_present_after_surface_replacement` | MO8 | Distinguishable scene pixels/epochs reject delayed scene after resize/recreate. |
| `global_quit_consults_all_owners_before_retiring_any` | MO9 | One veto or held close prevents partial committed quit; stale round answers are ignored. |
| `owner_failure_preserves_sibling_and_first_failure` | MO10 | Callback failure plus cleanup failure preserves the first and permits the prescribed next operation. |
| `shared_services_survive_one_owner_retirement` | MO13 | Closing an owner does not stop surviving owners' font feed, storage, retry or provider service. |
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
