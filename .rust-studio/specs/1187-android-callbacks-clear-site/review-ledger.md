# Review ledger — issue #1187

## Plan review (stage 2.5)

| Lens | Verdict | Findings |
|---|---|---|
| outside lens (`glm-5.3`, agentic) | ACCEPTABLE | 1 blocking (text), 7 advisory |
| `harsh-critic` | ACCEPTABLE with required changes | 3 required (1 shared with the outside lens), 5 advisory |

### Outside lens — rulings, each verified by the orchestrator before acceptance

- **Blocking (text): a fourth route, panic-unwind.** Verified in `android-activity` 0.6.1
  `glue.rs:976–993`: `catch_unwind(|| android_main(app)).unwrap_or_else(log_panic)` then
  `ANativeActivity_finish`. A panic anywhere inside `run` unwinds past the exit region, the process
  survives, and the next activity's `android_main` runs beside a cycle nobody will ever break. The
  plan's D1 ("exactly one exit … three routes") and the proposed comment ("reached by every route out
  of it") are false as written. **Ruling: fix the text, do not add a `Drop` guard.** A guard would
  run `callbacks().clear()` during unwind, which drops the renderer and its configured
  `wgpu::Surface`, which reaches `vkDeviceWaitIdle`; any panic inside that drop during an unwind is
  an abort (the class #1165 tracks), so the guard trades a leaked window per panicked run for a
  possible abort. winit's `finish_shutdown` has the identical boundary. The comment names the panic
  route as uncovered, and the plan's risks carry it.
- **Guard, D3.** Two specified mutants pass the guard as written: a `clear()` on a different
  receiver inside the region, and both statements inside `if false`. The second is inherent to a
  textual guard whose target is nested in an `if let`; the first is closed by tying the clear's
  receiver identifier to the take's binding. Accepted: adopt the identifier tie.
- **Capture-release test.** Discriminating: removing the `on_request_frame` take from `clear_now`
  fails the frame probe, and no existing test covers that mutant (the one `is_none()` assertion on
  that slot is vacuous, the slot is never registered there). The surface mutant is already red
  under `surface_status_change_cleared_from_inside_is_not_resurrected` through a different path.
  Accepted: keep both probes, and the plan says which one is the first real pin.
- **Third holders.** None: `RasterResizeHook` holds a `RasterHandle` + `Arc<LaneStamp>`, no lane;
  `install_pre_present_hook` is desktop-only; a queued `RealmTask::Frame`'s lane clone dies with the
  realm at teardown. The orchestrator's own trace agrees (three `Arc<RasterLane>` holders: the
  bootstrap local and the two slots). The quit-route aftermath is clean: the glue sets
  `thread_state = Stopped` after `android_main` returns, so the JVM thread's `set_window(None)` park
  falls through; no ANR from the unread pipe.
- **Locking.** Clean: the take is its own `let`; `clear_now` releases all eleven guards before
  dropping payloads; no `Drop` on the window, callbacks, lane or renderer touches a platform mutex;
  the quit hook never reads the field; `active_window()` post-exit answers `None` and has no caller
  outside `flui-platform`.
- **Re-registration.** D4 holds. One unstated assumption to name in D2: a second
  `OwnerPlatform::open_window` during the loop overwrites the field without clearing the displaced
  window's callbacks, so the exit path clears only the latest window. Pre-existing and not worsened;
  the plan states it as an assumption rather than an invariant.
- **`on_close` on the quit route.** Honest as written; the change makes an already-unfired callback
  explicitly unfireable, matching winit's quit route.
- **Text sites.** Inventory complete, plus one adjacent site the plan must name and keep true:
  `platforms/android/window.rs:168–171`'s SAFETY comment says "nothing clears that field on a
  termination". It stays true (the site clears at loop exit, outside the pause/termination span the
  comment reasons about), and the plan says so.

### `harsh-critic` — converged on the panic route; found the guard's own defects

Finding 1 is the outside lens's finding 1, independently. Its own additions, verified by the
orchestrator: `run()` has ten comment lines with apostrophes, so a char-literal masker run before
comment stripping mis-locates the loop close (R2); test 2 of the guard is green on the
guard-held-across-clear shape the plan forbids (R3); `RasterOwner` has a `Drop` at
`raster_owner.rs:1616` that D5's grep missed, conclusion intact because `set_wake_hook` has no
non-test caller (R4); the surface-slot mutant is already red under an existing test (R4). ALT-1 (a
`Drop` guard) and ALT-2 (a presentation-owned lane, the ADR-0045/#559 direction) recorded; neither
adopted here. The orchestrator corrected `acceptance.md`'s A1/A4 (F7) directly.

Reshape 1 dispatched to the same lead: `reshape-1.md`. Design unchanged; text, guard spec and
evidence rewritten. Re-review of the reshape to follow on a narrow remit.

### Re-review of the reshape: `harsh-critic`, ACCEPTABLE

D1 overclaims nowhere ("every route" survives only in the sentences banning it). The guard spec was
simulated with a script against the real file: each anchor resolves to one site on HEAD (`fn run`
at 268, `loop {` at 292, close at 492, `invoke_quit()` at 495; region 493–494, zero non-blank lines),
HEAD goes red on the shape assertion rather than a walker panic, the five guard mutants and the
three handlers mutants are each red as listed, and an attempted brace inversion via a `"}"` string
literal does not shift the close. Three advisories, all folded into the plan by the orchestrator:
assert the clear count before extracting the binding (so HEAD's red is the intended message); add
`return` to the inversion-refusal list (an early `return Err` between take and clear would pass and
lose a route; the real tail's `return` is outside the region); the char-literal masker must be
strict, because the file's code lines carry lifetimes and no char literals.

Plan review is closed. The plan goes to approval.

## Diff review (stage 3)

Two builders, split so that no file had two writers: plan items 1–3 (the site, the `handlers.rs`
doc and capture-release test, the source guard and its registration) and items 4–6 (the runner
comment, the window SAFETY comment, the ADR passages). Both hand-backs carried the evidence the
plan's `## Gate order` and D3 ask for, and the items 1–3 builder ran the eight-mutant matrix itself.

### Orchestrator edits after both builders finished

Both are claim-accuracy fixes inside sentences this change already rewrites, so they are part of
the diff, not outside it:

- `shared/handlers.rs`, `clear()`'s doc: the enumeration the sentence calls a completeness claim
  ("finds every call site") omitted `WinitApp::release_open_window_callbacks`
  (`platforms/winit/platform.rs`) — seven code hits, six named, false on HEAD before this change.
  Added.
- `ADR-0063`, decision 5's census bullet: the amended sentence placed Android on the wrong side of
  the accessor-vs-field split. `platforms/android/mod.rs` calls `window.callbacks().clear()` (the
  accessor); the field form (`self.callbacks.clear()`) is macOS, Windows, headless and
  `WinitWindow::drop`. Rewritten true of the census output.

### `rust-reviewer`: NEEDS WORK — one blocker, doc-only; the code half COMPLETE

The blocker: the newly written ADR sentence "winit's `finish_shutdown` has the identical boundary —
it runs only after `run_app` returns" is false. Verified independently by the orchestrator:
`WinitApp::exiting` (`platforms/winit/platform.rs`, winit's own loop-exit callback, taking an
`&ActiveEventLoop` that cannot exist after `run_app` returns) calls `finish_shutdown`; so does
`request_exit`, reachable from `process_control` inside the loop; and once more from the
post-`run_app` tail. The plan's D1 carried the same false supporting clause, so the error entered
through the plan. The analogy's conclusion survives — none of those sites is on the unwind path,
so a panic out of a callback skips winit's clear too — and the sentence was rewritten to say the
routes rather than the boundary. One copy existed, in the ADR; fixed there.

Everything else cleared, with the reviewer's own evidence:

- The exit region read as written, not through the guard: own `let`, no `.lock()` on the clear
  line, after the loop's close and before `invoke_quit()`; the only `break` in `run` is the
  top-of-loop check, fed by the three named routes.
- The guard re-executed independently (copies of the guard and the module compiled under
  `rustc --test`, worktree untouched): final revision green, HEAD red on the intended assertion,
  mutants c–g all red on their named assertions, and adversarial probes of the honesty header —
  a `//` inside a string literal in the loop body fails loudly, a `}` inside one does not shift
  the walk, the documented conditional-wrapper pass is not new, clear-before-take is red.
- Revision accounting: no mutant left in the tree (md5 against the builder's backup), the reported
  green measured against the final sources by binary timestamps.
- Claim-family sweep clean outside the ADR's own dated amendment notes; the `android-activity`
  0.6.1 and AOSP claims re-read from the crate source rather than taken from the plan.
- Independently re-run: `cargo fmt --all -- --check`, the Android-target clippy line,
  `FLUI_HEADLESS=1 cargo nextest run -p flui-platform --all-features` (292 passed), the NDK-free
  `flui-app` Android check (the same 7 pre-existing warnings, none added).

### Outside lens (`glm-5.3:cloud`, agentic) — one confirmed gap in the oracle, folded

The lens wrapper's own read-only pass found it, and it was **verified by experiment before the guard
was touched**: *identifier shadowing between the take and the clear*. Inserting
`let window = Arc::new(AndroidWindow::new(platform.app.clone()));` after the take keeps the
`let `-prefix, keeps the clear's receiver equal to the take's binding, and keeps the order, so all
three shape assertions passed while the real window's slots stayed full and the cycle stayed closed.
Reproduced in a copy outside the worktree (`rustc --edition 2024 --test`): unmutated green, shadowed
green. It is a distinct class from the conditional wrapper the guard's header already admits — that
one wraps, this one rebinds — and D3's own mutant (g) shows identifier-level evasions were in scope.

Fixed in `crates/flui-platform/tests/android_exit_path.rs` by a **rebinding refusal** in
`window_take_binding`: no other region line may `let`-bind the take's identifier. The new mutant (h)
is red with its named message.

The same report surfaced a plan-vs-disk deviation, also fixed: D3 specified bounding `run`'s body by
its matching close, and the shipped `exit_region` walked from the signature to end of file instead.
That was a live fragility, not a theoretical one — with the old bound, adding an unrelated sibling
function after `run` that contains a top-level `loop {` failed the guard with `could not locate
`run`'s `loop`: … found 2`. Both versions were run against that mutation: old red, new green.

Two of the wrapper's items needed no action: the census enumeration it could not run is the one the
orchestrator had already re-derived (7 code sites, all named), and the Weak-probe test it read
discriminates `mem::forget` as claimed. It stated its own limits honestly (no `rg`, no cargo, the
diff itself unsettled in its window), which is why its finding came with a settling experiment
rather than a verdict — and the experiment is what decided it.

The peer lens itself (`glm-5.3:cloud`) reported afterwards, from a session whose permission mode
denied every file-mutating command, so its mutant claims were analysis. It confirmed the exit-path
release as written and the drop-order trace, verified the `android-activity` 0.6.1 claims against
the registry source, independently confirmed both orchestrator claim-accuracy edits against the
census output, and swept the claim family clean. Its findings:

- **F1 (Low, confirmed by experiment here):** the inversion refusal banned `return` but not `?`,
  which is the same early exit spelled the way `run`'s `Result` return type makes idiomatic — a `?`
  between the take and the clear was GREEN. The early-exit ban is now its own refusal
  (`assert_exit_region_has_no_early_exit`) over `return`, `?`, `panic!` and `process::exit`, whose
  message is about the route losing the clear; `return` moved there from the inversion list, whose
  message had called it a mis-located walk.
- **F2 (Info, confirmed by experiment):** the depth/line indexing holds only while `run`'s `{` stays
  on its signature line. Not a green hole — a reformat fails loudly — so it is now a note on
  `line_start_depths` rather than a change.
- **F3 (Unverified):** the AOSP half of the ADR's `Destroy`-route claim is read at `refs/heads/main`
  rather than a pinned tag. The ADR now says so beside the claim, and says the Rust half is read from
  `android-activity` 0.6.1's source.
- **F4:** the `runtime-contract.toml` staleness already recorded below, out of scope, unfiled.

### Third round — the rebinding refusal, re-decided over the binding

`rust-reviewer` re-ran against the first revision of the shadowing refusal and found it decided one
syntactic form rather than the claim: keyed on the line prefix `let `, it left three green evasions
of the class its doc named — an `if let Some(window) = stolen_window()`, a destructuring
`let (window, _other)`, and a `let` that does not start its line. All three reproduced here.

The refusal is now stated over the binding: the take, the clear, and a line trimmed exactly to
`if let Some(<b>) = <b> {` are the only lines that may name `<b>`, matched as a whole identifier so
`window` does not match `windows` and a literal's text cannot count (literals are masked). The clear
line must additionally be exactly `<b>.callbacks().clear();`. It is deliberately conservative —
`let window = window;` reaches the taken window and is refused — and the refusal's doc says that,
rather than claiming the clear reaches another window.

Battery on that revision, all in copies outside the worktree: pristine GREEN; HEAD RED; the
plan's c–g RED; the shadowing form and the reviewer's A (one-line and multi-line), B, C and a
`while let` variant RED; `let window = window;` RED; `?`, `return` and `panic!` between the take and
the clear RED; the sibling-`loop {` and `}`-in-a-string probes GREEN; `//`-in-a-string RED and loud.

### Fourth round — the value tie, and the residual named

Round three asked whether the binding rule could still be defeated, and it could, three ways, every
one reproduced here before it was closed:

- **`break 'tail;` out of a labelled block** skipped the clear on an ordinary route while naming
  neither the binding nor any banned token. `break` is now in the early-exit ban, whose doc records
  that a labelled block needs no loop.
- **The take line's expression was unpinned** — the tie was on the name, so `.filter(|_| false)`
  (which empties the field, drops the taken `Arc`, and leaves the clear never running),
  `.map(|w| other_window(w))` and a take off a second platform all stayed green. The take line must
  now equal exactly `let {binding} = platform.window.lock().take();`, which makes the tie one of
  value as well as name.
- **The conservatism was under-documented.** The refusal's doc now says it reaches past a harmless
  rebinding to a pure read of the name, and that the clear-statement rule refuses a rename of the
  unwrap's own inner binding; the message names the shape expected instead of claiming the clear
  reaches another window.

The module header now carries the residual class where a reader meets it: a conditional wrapper, or
an early exit spelled in a way the token list cannot see (a macro expanding to one, an imported
`exit`). That is the boundary the guard states about itself — the scan decides presence and shape,
never that a route reaches them — and the reason further rounds chase spellings rather than
guarantees.

25 probes on the final revision: every mutant above RED, the sibling-`loop {` and `}`-in-a-string
probes GREEN, and a new control GREEN as well — `tracing::debug!("retry? return to sender")` between
the take and the clear proves the token bans are not substring traps.

### Fifth round — the close condition, and a correction to the lens's own record

`rust-reviewer` re-attacked a fourth time and returned **COMPLETE for the guard**, with the answer
asked for: nothing plausibly reachable is left outside the residual the header states. 38 probes;
pristine GREEN, HEAD RED, each round's evasions RED, and the plausibly-reachable-but-correct edits
(collapsing the statements into a fused `if let`, the tail extracted to a helper, the tail moved
after `invoke_quit()`) all RED and loud. What remains falls in two buckets, neither a gap in the
claim: an instance of the stated residual (a conditional wrapper, an early exit spelled invisibly),
or a change outside the exit region, which the scan never claimed to police.

Two items it raised and the disposition of each, so neither is left implied:

- **A cosmetic asymmetry it named**: `panic!` is banned in the region while `expect(`/`assert!`/
  `debug_assert!` are not. It also showed the asymmetry has no reachable consequence — any placement
  that actually touches the pinned value (`window.as_ref().expect("BUG: …")`, `assert!(window.is_some(), …)`)
  is refused by the naming rule first, and the remaining placements are exactly the residual the
  header states ("an early exit spelled in a way the token list cannot see" — an assertion macro
  expanding to a panic is that example verbatim), whose consequence on a returning route is the
  panic route ADR-0063 decision 5 already names and accepts. **Not fixed, by decision:** the rule's
  own doc says it is a list of spellings, not a syntactic analysis, and this is a spelling inside
  the class that sentence names. Recorded here rather than traded for a sixth round.
- **"The ADR finding from round one is still open" — that is wrong, and the fault is the
  orchestrator's brief.** It was fixed before the second round; the brief for the narrow re-reviews
  said "the ADR and the text sites are untouched", meaning untouched *by that round*, and the lens
  reasonably read it as untouched on disk and carried the finding forward through three rounds.
  Verified on disk: the false clause is absent and the passage now reads "winit's clear is reached
  only on a returning route too: `WinitApp::finish_shutdown` runs from `exiting()` …". Lesson for
  the next dispatch: scope a re-review's remit without describing the rest of the tree as frozen
  when it is not.

### Pre-publication verification of the outward artefacts

Before the commit was made, the PR body and the commit message — the two things that get published —
were audited by three independent lenses (their factual claims against the tree and the logs, the
staged file set against the plan and the repository's diff rules, and the commit/issue conventions)
plus a completeness critic. Eight findings, every one of them reproduced here before it was accepted,
and all of them applied to the drafts:

- **The headline universal was false.** "every other windowed backend already has [the site]" — the
  census printed in the same paragraph shows no hit under `web/`, and `platforms/web/window.rs` owns
  an `Arc<WindowCallbacks>` whose `close()` dispatches close without clearing, with
  `runner/web.rs` building the same window → slot → closure → renderer → cycle. The runner opens one
  window and never closes it, so nothing is stranded in the shipped runner, but the universal
  over-claimed. The sentence now names the four backends that destroy windows, and the web backend
  is a named observation in its own right.
- **"four review rounds, each found an evasion"** — false of the first (a doc-only blocker) and the
  last (nothing left); the wording now says what each round actually did.
- **`just adr-citations` "exit 0, citations all resolve"** — exit 0 is that script's default and it
  reports 5 provably-stale citations repo-wide; the row now says ADR-0063 is not among them and that
  the command is a measurement, not a gate.
- **"the same 7 pre-existing dead-code warnings as on `main`"** — one of the seven is an unfulfilled
  lint expectation, not dead code, and no `main`-revision log exists on disk. The row now names the
  split and rests on the verifiable fact instead: the `flui-app` diff is comment-only, so the warning
  set cannot differ.
- **The commit message contradicted the ADR** on the `quit()` route ("returns from `android_main`
  before `Destroy` is ever written" — `Destroy` *is* written, into a pipe the loop no longer reads).
  Reworded to the ADR's sentence.
- **The double-panic reasoning was attributed to winit's shutdown path**, which does not state it —
  and winit's last-resort `WinitWindow::drop` *does* clear on unwind, which is the route this change
  rejects. Now attributed to `OwnerHostClearGuard` alone, with the winit difference stated rather
  than glossed.
- **The critic's own two:** neither artifact said that #1187 stays open, why, or what would close it —
  which a maintainer cannot otherwise answer from the PR. Both now carry it: the body devotes the end
  of the divergence section to it, and the commit body has a clause beside `Refs`.
- **And: the evidence table did not name the revision it was measured on**, which turned out to be
  three commits behind `origin/main` (one a `flui-platform` change). No file overlap with this
  change, so the fix is a rebase onto `origin/main` followed by re-running the whole gate order on
  the rebased tree — the same discipline the guard's own review rounds were held to. The table now
  names the revision, and "frozen tree" (internal review vocabulary) is gone from it.

The critic also noted that `acceptance.md`'s checker-written `EVIDENCE:` line has no precedent in the
repository's other tracked spec directories. It stays — it is the ledger, and definition-bound
evidence is the format's point — but it now carries a legend under the Gates heading so a reader
meeting `def=…` for the first time is not left to guess at it.

### Gate, on the frozen tree

`just ci` exit 0 (port-check 23/23, workspace clippy `-D warnings`, doc-strict, typos + taplo,
nextest workspace; flui-platform 292 passed / 8 skipped, both `android_exit_path::*` pass).
`just cross-typecheck` exit 0 across the Windows, macOS and Android targets under `-D warnings`;
it is not part of `just ci`, so it was run as its own step. `just adr-citations` exit 0 after the
ADR fix. Acceptance ledger `--reverify`: `ALL MET — 5 met, 0 unmet, 0 stale, 0 abandoned of 5`.

The whole order was re-run after each change to the guard — once for the body bound and the first
rebinding refusal, once for the binding-keyed refusal and the early-exit split — and every verdict
repeated each time: `just ci` exit 0 with the same three tests passing and port-check clean,
`just cross-typecheck` exit 0 (its `--all-targets` line is what compiles the guard for Android),
`just fmt-check` 0, clippy `-D warnings` 0, `nextest -p flui-platform` 292 passed / 8 skipped,
`just text-check` 0 without skips, `just adr-citations` 0, the mutant battery green in every
direction above, and the acceptance `--reverify` `ALL MET`. The oracle changed twice after
`rust-reviewer` had verified it, so its verdict was re-measured rather than carried over — which is
what produced the third round rather than a rubber stamp.

### Named, not absorbed

The panic that unwinds out of `run` skips the exit region, by decision, with the double-panic
argument against a `Drop` guard recorded in the ADR. Two pre-existing hazards in `run` stay
unfixed (the quit hook runs under the `handlers` lock; every arm dispatches under the `window`
lock). `docs/runtime-contract.toml`'s `run_app_android` entry still says `on_ready` fires at the
first `Resume` where it fires at the first `InitWindow` since #1186 — stale, found independently
by the plan and by the reviewer, out of scope here, and **not** claimed as filed.
