# Review ledger — issue #1146

The record of stage 5a and stage 5b: which lens returned what, every finding, and how each was
disposed of. The accepted findings are also written up as the brief in `repair-1.md`, which is what
the builder works from. This file exists so the rulings survive, because a ruling that lives only in
a conversation cannot be audited later.

Change under review: the working-tree diff at HEAD `4aa8782b`, 14 modified files plus untracked
`crates/flui-app/src/app/runner/surface_lifecycle.rs`, `1215 insertions(+), 147 deletions(-)`.

## Lens verdicts

| Stage | Lens | Verdict | Findings |
|---|---|---|---|
| 5a | `rust-reviewer` (spec compliance) | ACCEPTABLE | 5 advisory, none blocking |
| 5b | `unsafe-auditor` | NEEDS WORK | 1 blocking, 5 advisory; **SAFETY-GATE withheld** |
| 5b | `api-design-lead` | ACCEPTABLE | 7 advisory, none blocking |
| 5b | `rust-reviewer` (code quality) | NEEDS WORK | 2 blocking, 8 advisory |
| 5b | outside lens (`ollama-lens`) | no blocker; 2 major, 4 minor, 1 lead | see below |

5a reproduced the gates independently: `just ci` EXIT=0 with 10271 tests, the Android check EXIT=0
with all 7 warnings in untouched files, and `cross-typecheck` green on three targets. `api-design-lead`
ran `semver-checks` against HEAD (196 checks, "no semver update required") and found no accidental
`pub`; the new seam is unreachable outside `flui-app`.

## Blocking findings and their rulings

**B1. The re-derived surface format does not reach its consumers.** Code-quality lens. Upheld.
Verified at the mechanism level on four independent links: `Renderer::recreate_surface` commits a
freshly derived `self.config` whose format may differ, and its own doc claims it can; `self.painter`
and `self.offscreen` bake the construction-time format through `PipelineSet::new` and are untouched
by that call, where the sibling `recover` rebuilds both; the per-frame format is read from
`self.config`; and `on_uncaptured_error` only logs, so a validation error sets no `device_lost` flag
and nothing self-heals. Reachability needs a GPU and a surface whose capability list differs, so the
behavioural half is unverified on this host. The repair offers two mutually exclusive fixes because
they rest on opposite claims about whether that format can differ, and requires the builder to say
which it took and on what evidence.

**B2. `RasterLane::note_surface_recreated` misattributes its cause.** Code-quality lens, and the same
finding 5a raised as its strongest advisory. Upheld as blocking. The builder declared this out of
scope as outside the plan's edit-site map, which was accurate about the map and wrong on the merits:
this diff is what gives the function its second production caller, so the log line it leaves behind
asserts a cause that is not the cause on the Android path. The plan's "Explicitly not in this change"
list does not defer it, and the rest of the diff is a sweep correcting docs it made false.

**B3. The SAFETY comment asserts a bound the code does not have.** Unsafe lens, and the only reason it
withheld SAFETY-GATE. Upheld. The comment's first sentence ties the returned handle's validity to the
`&self` borrow, but the same comment establishes that `AndroidWindow` is held across a pause, so the
borrow outlives the pointer, and the comment contradicts itself five lines later. The bound is the
`ANativeWindow` refcount. The lens stated it would sign the gate if re-invoked after the text fix.

## Advisory findings

Accepted and folded into `repair-1.md` as A1 to A9: the `HasWindowHandle` borrow premise in
`traits/window.rs`; the open-at-both-ends `None` span in `platforms/android/window.rs`; the surviving
pre-migration pause-span claim in `error.rs`; the self-contradictory `reconfigure_surface` trait doc;
the lane guard held across an inline realm dispatch; the unobservable second assertion in the lease
test; three ADR-0063 corrections; the missing fourth re-`configure` site in `derive_surface_config`'s
note; and the unnamed `impl_window_callback_setters!` mitigation.

Declined, with the lens that raised it: `Renderer::has_surface` (api; no in-tree consumer, and its doc
states the conflation); any change to the unexecuted Android arms or the outcome-to-action mapping
(api and code-quality; no device on this host, boundary declared in the plan); doctests for the new API
(api; family-wide and pre-existing); the stale implementor count in `docs/runtime-contract.toml` (api;
pre-existing, and that file is a gate registry); widening miri's scope (unsafe; pre-existing, advisory
in CI).

The `runtime-contract.toml` decline has a second reason worth recording, found by reading the plan's
own exclusions: the plan already defers a *different* citation tidy in that same file to its follow-up
list, on the grounds that it is a tidy in a file the change does not otherwise touch. Declining this
one keeps faith with that deferral rather than reopening it. The one line the change did add to that
file is a `[[lock_exemption]]` marker the conformance gate forces onto the new callback slot, which is
a different thing from a citation tidy and which stage 5a judged forced rather than preference.

B2's ruling rests on the plan not deferring it, so that was checked rather than assumed: the plan's
"Explicitly not in this change" list names six items, and the raster-lane log is not among them.

## One finding was declined, then reinstated

5a's advisory 4 proposed a cosmetic addition to `crates/flui-platform/ARCHITECTURE.md`, so that a
reader grepping for the originally-requested `onSurfaceDestroying` spelling would not mistake the
citation for an error. The code-quality lens rejected it on the ground that the landed name,
`SurfaceProducer.onSurfaceCleanup`, appears nowhere in the repository, so adding it would introduce a
symbol the code does not use. That ruling was accepted for pass 1 and the item was declined.

The outside lens then refuted the rejection's premise: the landed name does appear in the repository,
twice, in ADR-0063, which this same change wrote. With the premise gone the original finding stood on
its own merits (the sentence cites only the name that was asked for, never the one that exists), so it
was reinstated as pass 2's R6 and is now in the tree. Recorded in full because three lenses ruled on
one sentence and the file otherwise shows only the outcome; the lesson is that a rejection is only as
good as the fact it rests on, and that fact was checkable with one grep.

## Carried into the repair

Pass 1 covered B1 to B3 and A1 to A9 and is verified. Pass 2 covers the outside lens's findings and
spends the second of the three permitted dispatches.

## The outside lens

It found no blocker and returned 2 major, 4 minor and 1 lead finding. Its three "claims contradicted by
the tree" are artefacts of *when* it read, not defects: it reviewed the tree while pass 1 was still
being applied, so two of the five carry-forwards it ruled on were fixed underneath it, and the diffstat
in its brief was the pre-repair one. Pass 1's evidence is unaffected, because the gates were re-run
after the builder finished. That is a process error of mine and the lesson is recorded separately.

Rulings on its findings, each verified against the tree rather than accepted on the narrative:

- **M1, a failed recreate is never retried.** The mechanism is confirmed: the caller's `Failed` arm
  logs and does nothing else, its own comment says the next `true` re-asks on its own, and the only
  `true` emitters are `Resume` and `InitWindow`. So a non-probe failure leaves the presentation
  released until a lifecycle event. Ruled **advisory, text only**: a retry here means re-arming the
  wake hook and a backoff, which the plan declines and which is a mechanism with its own evidence
  requirements. The device-loss half is covered by the sibling recovery path, which retries under its
  backoff and wakes the loop, so the uncovered residual is the narrower non-device-loss case. The
  repair names the residual and states what statelessness does and does not cover, because the failure
  mode it fixes is a *missed signal*, not a failed action.
- **M2, the release waits under the blocking lane.** Confirmed: the seam's doc says the verb must not
  wait on a raster lane and the caller holds the blocking guard across `ensure_surface`, whose `false`
  branch is the unbounded `vkDeviceWaitIdle` the same doc names as its cost. Ruled **advisory, text
  only**: the blocking lock is deliberate, since it is what makes the release complete before the
  callback returns, and moving the release outside the guard is a design change the plan settled
  against. The wording must stop reading as if the release were lane-free, and the watchdog risk is
  named as unverified since the wait's length is the driver's and no device is available here.
- **m1, the `Failed` doc mandates a level its caller overrides.** Stands; the seam's doc must name
  the `SurfaceTargetUnavailable`-at-`trace` exception rather than be silently overridden.
- **m2, the macro mitigation claims a mechanism.** Stands, and it is against the sentence my own A9
  instruction produced: the macro is `pub(crate)`, so no out-of-crate implementor can use it, and
  `TestWindow` in `flui-app` implements `PlatformWindow` with no `on_*` overrides. A convention must
  not be written as a guarantee.
- **m3, the lease test's name.** Stands; pass 1 made the body and its comment honest and left the name
  overstating, so the rename is what remains.
- **m4, the ARCHITECTURE.md citation.** Stands. This is the item 5a raised and the code-quality lens
  rejected; that rejection rested on the landed name appearing nowhere, which is refuted, since
  ADR-0063 names both spellings. The sentence is an addition of this change, so it is in scope.
- **L1, the cost is per edge, not per cycle.** Confirmed at the emit sites: two `true` emitters, and
  the seam recreates unconditionally by design, so the ordering where `InitWindow` precedes `Resume`
  pays twice while the ADR describes the ordering that pays once. Advisory; correctness is unaffected.
- **L2** is its own list of things it checked and did not find, which needs no action and is useful as
  a negative result: no released-state busy loop, no reachable release-after-handle-death ordering at
  `MainEvent::Destroy`, and `Renderer::surface()` was already `Option` at HEAD so only the meaning of
  `None` widened.


## Repair pass 2, landed and verified

The builder was interrupted by a session restart before it could report, so the tree was inspected
item by item rather than trusted. All six items are present and match the brief:

- R1: the macro sentence now says the mitigation is a convention, scoped to in-crate backends, and
  names `TestWindow` as the out-of-crate implementor it does not cover.
- R2 and R3: `SurfaceLifecycleOutcome::Failed`'s doc names the `SurfaceTargetUnavailable`-at-`trace`
  exception, names the residual, says why no retry, and separates a missed signal from a failed
  action, pointing at the device-loss path for the class that heals itself.
- R4: the lease test is renamed `dropping_a_released_lease_releases_only_the_target`.
- R5: `release_surface`'s doc says the wait runs under the held guard by design, why that is
  accepted, and marks the watchdog risk unverified.
- R6: ADR-0063 books the cost per edge and names the ordering that pays twice; `ARCHITECTURE.md`
  names both the requested and the landed Flutter spelling.

`cargo fmt --all -- --check` EXIT=0. The diff is frozen at `git diff HEAD | sha1sum` =
`aee038fc2603457edc75f08e39545b6f181dd5a0`, 16 files, `1444 insertions(+), 201 deletions(-)`, and
that hash is pinned into `repair-review.md` so the re-review lenses can detect drift. `just ci` is
re-run on this revision alongside them. Two of the three permitted repair dispatches are spent.

## Gates on the frozen final revision

Run by the orchestrator, not reported by an agent, against `aee038fc2603457edc75f08e39545b6f181dd5a0`,
with the hash checked identical before and after each run:

- `just ci`: EXIT=0. `10271 tests run: 10271 passed, 4 skipped`; the 40-test and 289-test suites
  (flui-platform headless, `8 skipped`) both fully passed; zero `FAIL`/`error` lines in the log.
- The Android check (`cargo check -p flui-app --locked --target aarch64-linux-android` with the three
  `cc-rs` env vars): EXIT=0, `flui-app (lib) generated 7 warnings`, all pre-existing dead-code and
  `unfulfilled_lint_expectations` in files this change does not touch.
- `just cross-typecheck`: EXIT=0 on `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` and
  `aarch64-linux-android`, zero `error` lines.

## Re-review: `unsafe-auditor`

**SAFETY-GATE SIGNED. ACCEPTABLE**, three advisory, no blocking. The frozen hash matched before and
after its read. It verified every claim in the rewritten block against the pinned sources (`ndk`
0.9.0's `Clone`/`Drop`, `android-activity` 0.6.1's `native_window()`, `pre_exec_cmd`/`post_exec_cmd`
and `set_window`), confirmed the discharger runs inline before `post_exec_cmd(TermWindow)` on both the
`Pause` and `TerminateWindow` arms, and confirmed the diff adds no `unsafe` (the two `borrow_raw`
lines are unchanged context under a rewritten comment). The signing rests on two named in-tree
invariants, recorded in memory as `raw-window-handle-has-two-contract-levels`. It notes a
`systems-perf-lead` co-sign is the studio's formal clearance for the gate; that co-sign was not sought
because the diff adds no unsafe code and changes only the justification comment of one pre-existing
block, and the verdict says so rather than implying the full gate ran.

Its three advisories, each checked against the tree and upstream:

- **`traits/window.rs` MUST doc under-states upstream.** Confirmed: our sentence cites only
  `HasWindowHandle`'s object-lifetime prose, while `WindowHandle<'a>`'s type doc claims pointer
  validity for `'a`, and that is the claim `borrow_raw` answers to. Accepted; one clause.
- **The Android bullet's "whole span between a `Pause` and the matching `Resume`" is over-broad.**
  Confirmed: a `TerminateWindow` inside that span flips the answer to `Unavailable`, and the bullet's
  own preceding sentence describes that case correctly. A genuine falsehood of the class this change
  removes. Accepted; qualify with the bullet's earlier "absent a `TerminateWindow`" wording.
- **The pre-existing residual**: a safe public `window_handle` whose Android impl can dangle within
  `'a` for a downstream holder. Not introduced by this diff, no fix inside rwh 0.6's API, same shape
  as winit's Android impl. Recorded, not actioned.

The two accepted advisories are held for one final dispatch together with whatever the code-quality
re-review returns, so the third and last repair dispatch is spent once.

## Re-review: `rust-reviewer` (code quality)

**ACCEPTABLE**, eight advisory, no blocking, hash matched. It settled B1's four attacks from the code:
every format-baked GPU object lives in `painter` or `offscreen`, the per-frame offscreen-painter cache
dies with its `Backend`, no static pipeline cache exists, and the only state `recover` resets that
`recreate_surface` skips is device-bound and correctly kept. The guard's `None` arm is unreachable
(the offscreen and shared-services origins already returned `NotInitialized`) and fires in the right
direction anyway; an ordinary resume re-derives the same format and pays nothing; the rebuild neither
submits nor waits. A5 and A6/R4 confirmed correct as landed, with the builder's stated limit on the
lease assertion judged the honest description. Recorded in memory as
`wgpu-renderer-format-consumers-live-in-two-fields`.

Its advisories, every one verified before being accepted:

- **The Vulkan one-surface-per-window rule** reshapes three passages, including the "pays it twice"
  ADR bullet accepted in pass 2. Verified against the Khronos registry page for
  `vkCreateAndroidSurfaceKHR`: only one `VkSurfaceKHR` may exist per `ANativeWindow` at a time
  (`VK_ERROR_NATIVE_WINDOW_IN_USE_KHR`), and the create takes a reference on the window that the
  destroy releases. So a same-window `true` over a held surface is refused at `create_surface`, one
  failed create and a `warn`, not two configures and two mints; `recreate_surface`'s "always builds"
  is "always attempts"; the scripted test pins the seam, not the platform; and the lost-`false`
  residual on Vulkan is a disconnect on a dead producer rather than a use-after-free, leaving only the
  EGL path unverified. Recorded in memory as `android-allows-one-vksurface-per-native-window`.
  Accepted as text; the recreate order and the unconditional recreate are unchanged, since the refusal
  is the price of statelessness and the plan settled it.
- **`ARCHITECTURE.md`'s "they key on `onStop`/`onStart`"** is false against `winit` 0.30.13, which
  maps `InitWindow` → `Resumed` and `TerminateWindow` → `Suspended` with `Start`/`Stop` as TODO
  stubs; verified in the vendored source. The ADR and trait doc in the same diff say the correct thing.
- **The ADR's clear-site census command** finds two files where the claim needs seven, because it
  cannot see the field-call form `callbacks.clear()`; verified by running both regexes. The
  conclusion is true and the proof is not.
- **`raster_lane.rs` under-enumerates the emitters** (names `TerminateWindow`/`InitWindow`, omits
  `Pause`/`Resume`), the census class B2 itself corrected.
- **`traits/window.rs` "destroys the swapchain on suspend"** should be "with the window", per the
  bullet's own preceding clauses.
- **Shape:** the painter-plus-offscreen pair now has two construction sites, the drift hazard the diff
  itself cites for `derive_surface_config`; one private helper called from both. Accepted as a
  no-behaviour extraction, gates as proof.
- **Cost:** one sentence beside the guard naming that a moved format constructs nine pipelines and a
  glyph atlas synchronously under the held lane.

## Repair pass 3

Dispatched as the third and last: `repair-3.md`, nine items (T1 to T9), eight text and one
extraction, folding in the two `unsafe-auditor` doc precisions as T6. Nothing from passes 1 or 2 is
reopened. After it lands the tree is frozen, hashed, and the gates re-run once more before the
verdict. No further repair dispatch is available; anything it leaves is reported, not fixed.

## Repair pass 3, landed and verified

All nine items present; edit set exactly the six named files; nothing under `.rust-studio/specs/`
touched. T9's helper `build_format_consumers` is the only place the two format consumers are
constructed in production, and both `build_windowed_gpu_stack` and `recreate_surface` call it. T1's
ADR bullet books the cost per signal and names what a refusal costs on the shipped path.

**The builder corrected a fact in the brief, and the correction is verified.** `repair-3.md` said a
same-window refusal reaches the seam's `Failed` arm as `SurfaceCreation` and is logged at `warn`.
That is the seam's design, not the shipped path: `wgpu-hal` 30.0.1's `create_surface_android`
(`src/vulkan/instance.rs`, the arm `RawWindowHandle::AndroidNdk` dispatches to) does
`.expect("AndroidSurface failed")`, and the engine selects `Backends::VULKAN` on Android, so
`VK_ERROR_NATIVE_WINDOW_IN_USE_KHR` is a panic on the callback thread. The builder read the hal,
wrote the truth into T1/T2, and reported the discrepancy rather than papering over it. Reachability on
this tree: only via a `false` missed on a window that survived (every arm is mapped and every `false`
dispatches inline at the top of the poll loop), or an `InitWindow`-before-`Resume` ordering the glue
emits on no traced path. So it is a latent hazard behind another bug, not a reachable defect, and it
was found after both re-review lenses returned, so **no lens has ruled on it**. Recorded in memory
under `android-allows-one-vksurface-per-native-window`, with the fix direction (drop-first order in
`recreate_surface`) that the plan's settled build-first decision would need to revisit.

The repair budget is exhausted: three dispatches, all spent. Nothing further is fixed in this
workstream; what remains is reported in the verdict.
