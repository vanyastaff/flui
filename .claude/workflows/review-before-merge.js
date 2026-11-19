export const meta = {
  name: 'review-before-merge',
  description: 'Readers, one lens each, read a change or a design to break it; a judge merges, verifies and ranks what they find',
  whenToUse: 'The independent read AGENTS.md "Before a merge" asks for, on a branch or a checkout, and again on each fix pushed after it; with mode "design", the read of an ADR or a spec design before its code',
  phases: [
    { title: 'Scope', detail: 'merge base, reviewed head and changed files' },
    { title: 'Read', detail: 'one reader per lens, in parallel' },
    { title: 'Judge', detail: 'merge duplicates, verify each finding in the source, rank' },
  ],
}

// Its failure branches are checked by `node .claude/workflows/review-before-merge.test.mjs`.
//
// args (all optional):
//   checkout  path of the checkout whose git repository holds the commits (default: the session's
//             working directory); the readers read the reviewed head, never its working tree
//   base      ref the change is measured against (default: origin/main)
//   head      the reviewed commit (default: HEAD)
//   since     the head an earlier read covered; this read starts from the fix after it
//   unread    lenses the earlier read could not run; they read the whole change, not the fix
//   mode      "design" reads an ADR or a spec's design with the design lenses, before its code;
//             anything else (the default) reads a change
//   mutants   a worktree checked out detached at the reviewed head for a cargo-gamma run over this
//             change (AGENTS.md "Before each push" item 10, docs/testing.md "A change's own
//             lines"): its `change.diff` is the diff the run took with `--in-diff`, and its
//             `target/cargo-gamma/gamma-report.json` the report; each mutant that survived or
//             that no test reached is a finding
const opts = args || {}
const checkout = opts.checkout || '.'
const notes = []
const footer = () => (notes.length ? `\n\n${notes.join('\n')}` : '')

const SCOPE_SCHEMA = {
  type: 'object',
  properties: {
    base: { type: 'string', description: 'full sha of the merge base' },
    head: { type: 'string', description: 'full sha of the reviewed head' },
    since: { type: 'string', description: 'full sha of the earlier reviewed head, or empty' },
    since_base: { type: 'string', description: 'full sha of the merge base of since, or empty' },
    since_is_ancestor: { type: 'boolean' },
    files: { type: 'array', items: { type: 'string' } },
    stat: { type: 'string', description: 'the last line of git diff --stat' },
    warnings: { type: 'array', items: { type: 'string' }, description: 'failures of the fetch or of since, each with its command' },
    error: { type: 'string', description: 'the failed command for head, base or files, and its error, or empty' },
  },
  required: ['base', 'head', 'since', 'since_base', 'since_is_ancestor', 'files', 'stat', 'warnings', 'error'],
}

const FINDINGS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        properties: {
          title: { type: 'string', description: 'the defect in one sentence' },
          file: { type: 'string' },
          line: { type: 'integer' },
          state: { type: 'string', description: 'the input or state before the operation' },
          operation: { type: 'string' },
          wrong_result: { type: 'string', description: 'wrong output, panic, hang, leak, lost work, a false record' },
          evidence: { type: 'string', description: 'the lines read that produce it, with file:line' },
        },
        required: ['title', 'file', 'line', 'state', 'operation', 'wrong_result', 'evidence'],
      },
    },
    tried: {
      type: 'array',
      items: { type: 'string' },
      description: 'scenarios tried that the code rules out, each with the line that rules it out',
    },
  },
  required: ['findings', 'tried'],
}

const SOURCES = { type: 'array', items: { type: 'integer' }, description: 'the numbers of the reported findings this entry accounts for' }

const MUTANT = {
  type: 'object',
  additionalProperties: false,
  properties: {
    file: { type: 'string' },
    line: { type: 'integer' },
    mutation: { type: 'string', description: 'the text after "file:line:col: ", e.g. "replace + with * in SceneSlot::totals"' },
  },
  required: ['file', 'line', 'mutation'],
}

const MUTANTS_SCHEMA = {
  type: 'object',
  properties: {
    run_head: { type: 'string', description: "the run worktree's HEAD" },
    extra_files: { type: 'array', items: { type: 'string' }, description: 'status lines other than the change.diff one' },
    report_is_newer: { type: 'boolean', description: 'whether the report was written after change.diff' },
    diff_matches: { type: 'boolean', description: 'whether the run took exactly this diff' },
    counts: {
      type: 'object',
      properties: {
        killed: { type: 'integer' },
        survived: { type: 'integer' },
        uncovered: { type: 'integer' },
        timed_out: { type: 'integer' },
        ignored: { type: 'integer' },
      },
      required: ['killed', 'survived', 'uncovered', 'timed_out', 'ignored'],
    },
    survived: { type: 'array', items: MUTANT },
    uncovered: { type: 'array', items: MUTANT },
    timed_out: { type: 'array', items: MUTANT },
    ignored: { type: 'array', items: MUTANT },
    error: { type: 'string', description: 'a file or command that failed, and its error, or empty' },
  },
  required: ['run_head', 'extra_files', 'report_is_newer', 'diff_matches', 'counts', 'survived', 'uncovered', 'timed_out', 'ignored', 'error'],
}

// cargo-gamma mutates only the sources of library and binary targets and refuses a diff that
// names any other Rust file, so the diff it takes names only files under a `src/`: an allow-list,
// since tests, benches, examples and build scripts are each another kind of file it skips.
// docs/testing.md gives the same command.
const MUTATION_PATHSPEC = `':(glob)**/src/**/*.rs'`
// The judge verifies each finding in the source, so their number is bounded; every list is counted.
const MAX_MUTANT_FINDINGS = 40
const MAX_MUTANT_NOTES = 10
// cargo-gamma writes a mutant that timed out or ran out of memory as "Survived" and says which in
// `statusReason`; flakes, mutants the build never compiled and ones never judged are "Ignored" or
// "Pending" (cargo-gamma-lib 0.2.1, `elements::report::status_of`).
const GAMMA_SELECTIONS = {
  killed: '.status == "Killed"',
  survived: '.status == "Survived" and ((.statusReason // "") | test("^(timed out|out of memory): ") | not)',
  timed_out: '.status == "Survived" and ((.statusReason // "") | test("^(timed out|out of memory): "))',
  uncovered: '.status == "NoCoverage"',
  ignored: '.status == "Ignored" or .status == "Pending"',
}

const VERDICT_SCHEMA = {
  type: 'object',
  properties: {
    confirmed: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          sources: SOURCES,
          title: { type: 'string' },
          severity: { type: 'string', enum: ['important', 'nit'] },
          class: { type: 'string', description: 'the AGENTS.md "Recurring defects" item, or a short name for another kind' },
          file: { type: 'string' },
          line: { type: 'integer' },
          scenario: { type: 'string', description: 'state, operation, wrong result' },
          verified: { type: 'string', description: 'what the judge read to confirm it, file:line' },
          lenses: { type: 'array', items: { type: 'string' }, description: 'readers that reported it' },
        },
        required: ['sources', 'title', 'severity', 'class', 'file', 'line', 'scenario', 'verified', 'lenses'],
      },
    },
    rejected: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          sources: SOURCES,
          title: { type: 'string' },
          reason: { type: 'string', description: 'the line or test that shows the scenario cannot happen' },
        },
        required: ['sources', 'title', 'reason'],
      },
    },
    not_kept: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          sources: SOURCES,
          title: { type: 'string' },
        },
        required: ['sources', 'title'],
      },
      description: 'verified nits beyond the five kept',
    },
  },
  required: ['confirmed', 'rejected', 'not_kept'],
}

const LENSES = [
  {
    key: 'records',
    model: 'sonnet',
    focus: `Records against the code.
- Every claim the diff adds, or keeps beside code it changes, in an ADR, an ARCHITECTURE.md, a doc
  comment, a code comment, a changelog fragment, docs/gaps.md, docs/platform-parity.md, a spec row
  or the book: does the code at the reviewed head do it? Check behaviour, counts and numbers, and
  every citation (the cited section, test, line or upstream source says what is claimed).
- The replaced form: name what the diff replaces or reverses, then search the head for the old
  name and the old rule across crates/, packages/, docs/, each ARCHITECTURE.md and AGENTS.md. A
  caller or a document that still uses or teaches it is a finding.
- A consumer-visible change with no changelog.d fragment, or a fragment that files a change
  under the wrong heading or reports a fix for behaviour that already held.
- Every expectation AGENTS.md "Before each push" item 14 names that the change edits or leaves
  stale: find each in the diff and judge it on the code alone. A new value the code does not
  justify, moved so a check turns green, is the defect the check was there to find.
- A new pub item no production path reaches, a variant or error nothing builds any more.`,
  },
  {
    key: 'tests',
    model: 'sonnet',
    focus: `Tests against the fix.
- For each test the diff adds or changes: which line of the non-test change does its failing
  assertion depend on? Trace the test with that line reverted. If it still passes, the test does
  not pin the fix: a finding.
- Does each test reach the path its name claims, or does a stand-in, an early return, a host that
  skips the step, a warm cache or an assumed environment (a GPU, a display size, a fixed order)
  decide it?
- New behaviour, a new branch or a new guard with no test through the public API under tests/.
- A perf or bench row whose bound a regression could not cross, or whose warm-up measures nothing.
- What a mutation run cannot see, since it changes one line at a time: a scenario no test builds,
  an assertion that holds on an all-zero or empty result, a check whose bound is too loose to fail.`,
  },
  {
    key: 'obligations',
    model: 'opus',
    fallback: 'sonnet',
    focus: `Obligations and state.
- Every request, waker, obligation and hold the diff accepts: delivered or refused with a typed
  error on every exit (success, panic, cancel, owner retirement, a full queue)? An older wake
  clearing a newer obligation? A wake or deadline reported where nothing serves it, so a loop
  spins or a retry never ends?
- State spread over several flags or records (a mark, a retry, a refusal, a base): write down the
  orders of events the diff allows and find one where the rules disagree.
- A mark, a failure or an answer attributed to the wrong frame, window, request or owner; an
  answer matched to "the current one" instead of by ticket or generation.`,
  },
  {
    key: 'unwinding',
    model: 'opus',
    fallback: 'sonnet',
    focus: `User code and unwinding.
- List every point where the diff runs code it does not own: a callback, an observer, a waker's
  clone or drop, the Drop of a replaced, captured or dismissed value (a hold, a ticket, a guard,
  a completion), Into, Display, a tracing event, a backend or provider trait method. At each
  one, are borrows and guards released and the state committed first (ADR-0287 commit, Outbox)?
- Make each of those points panic in turn. What state is left: a window still marked busy, a
  slot still held, a flag still set, a resolver never answered? Does the next operation work?
- Make two of them panic in one operation (a Drop that panics while an earlier panic is being
  carried): is the first failure kept (FirstPanic) and the second discarded, or does the second
  win, resume twice, or abort outside an unwind that was already running?
- A guard built after the call it should guard, so an unwind inside the call skips it; cleanup
  that reads thread::panicking() after a catch, where it is false.`,
  },
  {
    key: 'boundaries',
    model: 'sonnet',
    focus: `Inputs, identity and arithmetic.
- Untrusted input (a file, a path, a URI, the clipboard, IPC, a recording, a wire message, an
  environment value): checked by a deny-list instead of an allow-list, sized after it is decoded
  or copied, or checked before the thing it names is opened. A queue, cache, walk or recursion
  without a bound.
- A validator or check that covers only the forms its author wrote (it stops at the first
  refusal, reads one syntax, handles the obvious case), so a later or unusual form passes; or one
  that refuses a valid input.
- Identity: a cache or coalescing key missing something the result depends on; a label compared
  where a typed ID belongs; a stale generation; a counter that can wrap.
- Arithmetic: overflow in an intermediate value, precision lost (an f32 cast of a large magnitude,
  a rounding at 0 or 1.0), non-finite geometry published, mixed units, clocks or 0- and 1-based
  indexes.`,
  },
  {
    key: 'seams',
    model: 'sonnet',
    focus: `Seams and copies.
- A rule (a limit, a normalization, a refusal, a version floor) held by one host or caller
  instead of at the seam: check every host the rule concerns, headless and the scripted doubles
  included. A rule written twice whose copies disagree.
- A copy of state (a mirror, cache or snapshot): list every field of its source and every
  operation that changes the source (reorder, removal, hiding, a failed read, a change of owner);
  is the copy refreshed on each? Does one failed read drop the whole copy?
- Two steps in the wrong order: an effect committed before the fallible call, a restore before a
  rebuild, a value taken by something that does not own it.
- A workspace-wide change that misses a feature, an example, a shader, tooling, or a cfg target
  the author did not build (wasm32, Linux, macOS, Android, iOS): read the cfg gates.`,
  },
  {
    key: 'rules',
    model: 'sonnet',
    focus: `The rules nothing else checks. Before the first release no program holds them (ADR-0371, ADR-0375), so
the readers of this review are their only check. Read each of these AGENTS.md sections in full,
as the reviewed head has it, and check the change against every item in it, not a sample:
"Rules a reader holds", "Code for the next reader", "Current forms" (a new line that uses a form
from its "Not" column), "Review guidelines" and "Definition of Done".`,
  },
]

// A design (an ADR, a spec's design.md) is read before the code it decides is written: the defects
// that cost the most review rounds are ones a design allows, an order of events it never names or
// state it spreads over several records, and a reader of the document can find them there.
const DESIGN_LENSES = [
  {
    key: 'alternatives',
    model: 'sonnet',
    focus: `Options and prior art.
- Is each considered option real and judged against the stated drivers, with its pros and cons? Is
  there an option missing that meets the drivers better, or a driver the chosen option fails?
- How do other frameworks and platform toolkits solve the same problem (Flutter, Compose, SwiftUI,
  GPUI, Xilem, egui, Chromium, the platform SDKs)? Read their source or issue trackers with \`gh\`,
  pinned to a commit, and list the defects they record for this behaviour: a design written from a
  reference repeats its known bugs unless they are on its checklist.
- A choice justified by "it is so already" or by matching another framework, not by a driver.`,
  },
  {
    key: 'ownership',
    model: 'opus',
    fallback: 'sonnet',
    focus: `Owners of state.
- List every piece of state the design introduces or touches (a flag, a mark, a counter, a queue, a
  cache, a handle, a retry, a refusal) and who owns it. State whose rules live in several records,
  or that two owners write, is the design defect each review round of its code will find another
  order for: name the one owner it should have.
- Every copy or mirror of state: which operation of its source can leave it stale?
- The thread and owner model: the frame path stays synchronous, state only the owner thread touches
  is not behind a lock, and a value crosses a thread only where the compiler can prove it may.`,
  },
  {
    key: 'orders',
    model: 'opus',
    fallback: 'sonnet',
    focus: `Orders of events and failures.
- For each request, answer, frame or obligation the design defines: how is an answer matched to
  what asked for it (a ticket, a stamp, a generation)? Walk an answer that is late, duplicated, or
  arrives after a write, a cancel, a close or the owner's retirement.
- Walk each failure the design must survive: a panic in code it does not own and two at once, a lost
  device or resource, a peer that is slow, wedged or dead, a full queue, a refused request, an
  unwind in the middle of the operation. What happens to work already accepted on each exit, and is
  it delivered or refused with a typed error?
- Production against consumption: what bounds the work in flight, and what wakes a waiter so that
  nothing spins and nothing waits forever?`,
  },
  {
    key: 'seams',
    model: 'sonnet',
    focus: `Seams and hosts.
- Where does each policy the design states live (a limit, a normalization, a refusal, a version
  floor)? At the seam, or in one host or caller? Check every host it concerns: Windows, macOS,
  Linux, Android, iOS, the web, headless and the scripted doubles.
- A capability a host lacks: is its absence typed, or left to a default that pretends to work?
- A layer boundary the design crosses: does it keep the tiers (AGENTS.md "Rules a reader holds")
  and keep host and OS types out of the crates that must not name them?`,
  },
  {
    key: 'records',
    model: 'sonnet',
    focus: `Records against records.
- Every accepted ADR, spec and ARCHITECTURE.md the design touches: does it contradict one without
  naming it under Supersedes, and does its reversal reach every record that states the old rule?
- AGENTS.md "Current forms", "Rules a reader holds" and docs/gaps.md: does the design use a form on
  its way out, or leave a gap entry stating something it changes?
- The shape docs/adr/README.md asks for: drivers, every option with pros and cons, the decision as
  a Rust shape, and how compliance is confirmed. Counts, citations and claims about the current code
  checked against the code at the reviewed head.`,
  },
  {
    key: 'confirmation',
    model: 'sonnet',
    focus: `How each decision is held and how the old form goes.
- For each decision: which rung holds it (the compiler, a clippy lint, a test of the owning crate),
  or is it only text? Name the test through the public API from tests/ that fails when the decision
  is broken, or say none can be written as designed.
- A decision that replaces a form (a primitive, an API, a rule): does the design move every user of
  the old form and delete it in the same change, or name its deprecation and gap entry?
- A behaviour the design asks for that no headless test can observe, and the native run it is then
  owed.`,
  },
]

// `mode: 'design'` reads a design; any other value reads a change.
// A value that names neither read is refused rather than read as a change, which would record a
// design read that never happened.
const mode = opts.mode === undefined || opts.mode === null || opts.mode === '' ? 'change' : opts.mode
if (mode !== 'change' && mode !== 'design') {
  return unavailable(`mode ${JSON.stringify(opts.mode)} is neither "change" nor "design"`)
}
const lenses = mode === 'design' ? DESIGN_LENSES : LENSES

// An agent that a usage limit or a skip refuses resolves to null, and one started past the turn's
// token target throws; the runtime does not say which. Each is a failure the result names, and
// none may read as an empty answer.
// An agent asked for an empty string sometimes writes the quotes themselves (`""`); every string an
// agent returns is read through `blankQuotes`, so such a field is empty wherever the script tests it.
function blankQuotes(value) {
  if (typeof value === 'string') return /^\s*(""|'')\s*$/.test(value) ? '' : value
  if (Array.isArray(value)) return value.map(blankQuotes)
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, blankQuotes(v)]))
  return value
}

async function attempt(prompt, options) {
  try {
    const value = await agent(prompt, options)
    return value ? { value: blankQuotes(value) } : { error: 'no result (refused by a usage limit, skipped, or failed after retries)' }
  } catch (e) {
    return { error: String(e && e.message ? e.message : e) }
  }
}

// A lens the earlier read could not run reads the whole change; a name that is not a lens is
// reported, not dropped.
const unread = []
const unknownUnread = []
if (opts.unread !== undefined) {
  const given = Array.isArray(opts.unread) ? opts.unread : [opts.unread]
  for (const key of given) {
    if (lenses.some((l) => l.key === key)) unread.push(key)
    else unknownUnread.push(key)
  }
}

function unavailable(reason, extra = {}) {
  log(`Independent read unavailable: ${reason}`)
  const { markdownTail = '', ...rest } = extra
  return {
    ...rest,
    unavailable: reason,
    confirmed: [],
    rejected: [],
    markdown: `Independent read unavailable: ${reason}${markdownTail}${footer()}`,
  }
}

const SHA = /^[0-9a-f]{40}$/
const baseRef = opts.base || 'origin/main'
const headRef = opts.head || 'HEAD'
// An empty or null since means no earlier read, as an empty base or head means the default.
const sinceArg = opts.since === undefined || opts.since === null || opts.since === '' ? '' : opts.since
// Refs and the checkout path go into shell commands an agent runs, single-quoted. A single-quoted
// word ends at a quote (in PowerShell also at U+2018 to U+201B) or a line break, and a word that
// starts with a dash reads as an option; the fetch passes its name after `--`.
const QUOTABLE = /^[^'‘-‛\r\n\0]+$/
for (const [name, value] of [['base', baseRef], ['head', headRef], ['since', sinceArg || 'HEAD']]) {
  if (typeof value !== 'string' || !QUOTABLE.test(value) || value.startsWith('-')) {
    return unavailable(`the ${name} ref ${JSON.stringify(value)} cannot be passed to git safely (not a string, a quote, a line break, or a leading dash)`)
  }
}
// An empty or null mutants means no run, as an empty since means no earlier read.
const mutantsDir = opts.mutants === undefined || opts.mutants === null || opts.mutants === '' ? '' : opts.mutants
if (mutantsDir && (typeof mutantsDir !== 'string' || !QUOTABLE.test(mutantsDir))) {
  return unavailable(`the mutants path ${JSON.stringify(mutantsDir)} is not a string or holds a quote or a line break`)
}
if (typeof checkout !== 'string' || !QUOTABLE.test(checkout)) {
  return unavailable(`the checkout path ${JSON.stringify(checkout)} is not a string or holds a quote or a line break`)
}
const dir = `'${checkout}'`

phase('Scope')
const scoped = await attempt(
  `Work out the range of a change for review. Run only these git commands, each exactly as written, in the checkout ${dir}.
1. If '${baseRef}' is a remote-tracking ref (origin/<name>), run \`git -C ${dir} fetch origin -- '<name>'\` first, so the merge base does not come from a stale ref. If the fetch fails, add the command and its error to "warnings" and go on with the local ref.
2. head: \`git -C ${dir} rev-parse '${headRef}'\`
3. base: \`git -C ${dir} merge-base <head> '${baseRef}'\`
4. ${sinceArg
    ? `since: \`git -C ${dir} rev-parse '${sinceArg}'\`. since_is_ancestor: run \`git -C ${dir} merge-base --is-ancestor <since> <head>\`; exit 0 is true and exit 1 is false (an answer, not a failure). since_base: \`git -C ${dir} merge-base <since> '${baseRef}'\`. When one of these fails, leave its field empty and add the command and its error to "warnings".`
    : 'since and since_base: the empty string; since_is_ancestor: true.'}
5. files: \`git -C ${dir} diff --name-only <base> <head>\`
6. stat: the last line of \`git -C ${dir} diff --stat <base> <head>\`
Return full 40-character shas. When step 2, 3 or 5 fails, put the command and its error in "error"; never return an empty file list in place of a failed command.`,
  { phase: 'Scope', label: 'scope', schema: SCOPE_SCHEMA, model: 'haiku', effort: 'low' },
)
if (scoped.error) return unavailable(`the scope step failed: ${scoped.error}`)
const scope = scoped.value
notes.push(...scope.warnings.map((w) => `Scope: ${w}.`))
if (scope.error) return unavailable(`the scope step failed: ${scope.error}`)
if (!SHA.test(scope.head) || !SHA.test(scope.base)) {
  return unavailable(`the scope step returned no full sha for the base or the head ("${scope.base}", "${scope.head}")`)
}
// A since that cannot be placed loses the pointer to the fix, not the review: the readers read
// the whole change, and the scope warnings above say why.
const since = sinceArg && SHA.test(scope.since) && SHA.test(scope.since_base) ? scope.since : ''
if (sinceArg && !since) notes.push(`The earlier head ${sinceArg} could not be placed against ${baseRef}; the readers read the whole change.`)
const header = `Read (${mode}) at ${scope.head} (base ${scope.base}${since ? `, fix since ${since}` : ''}).`
// A decision and code in one diff owe both reads: each read names the other when the diff asks
// for it, so neither is taken for the whole.
const decisionFiles = scope.files.filter((f) => /^docs\/adr\/ADR-\d+[^/]*\.md$|^docs\/plans\/specs\/[^/]+\/design\.md$/.test(f))
const codeFiles = scope.files.filter((f) => !/\.md$/.test(f))
if (mode === 'design' && codeFiles.length) {
  notes.push(`The diff also changes ${codeFiles.length} files that are not documents; the change read (no mode) is owed beside this one (AGENTS.md "Before a merge").`)
}
if (mode === 'change' && decisionFiles.length) {
  notes.push(`The diff changes a decision (${decisionFiles.join(', ')}); the design read (mode "design") is owed beside this one (AGENTS.md "Before a merge").`)
}
if (!scope.files.length) {
  log('The merge base and the head have the same tree.')
  return { base: scope.base, head: scope.head, confirmed: [], rejected: [], markdown: `${header} No change to review: the merge base and the head have the same tree.${footer()}` }
}
log(`Reviewing ${scope.base.slice(0, 9)}..${scope.head.slice(0, 9)}: ${scope.stat}`)

const range = `\`git -C ${dir} diff ${scope.base} ${scope.head}\``
// `since..head` is the fix alone only when the branch kept its base. After a rebase, or a merge of
// main into the branch, it also holds what main gained in between; comparing the two series
// shows the fix alone.
const fixRange = !since
  ? ''
  : scope.since_is_ancestor && scope.since_base === scope.base
    ? `\`git -C ${dir} diff ${since} ${scope.head}\``
    : `\`git -C ${dir} range-diff ${scope.since_base}..${since} ${scope.base}..${scope.head}\` (the branch took main's commits after the earlier read)`
if (unknownUnread.length) {
  notes.push(`"unread" names ${unknownUnread.map((k) => JSON.stringify(k)).join(', ')}, not a lens (${lenses.map((l) => l.key).join(', ')}); ${fixRange ? 'no lens was spared the fix note for it.' : 'there was no fix note to spare it from.'}`)
}
// The checkout's working tree may move while the readers run (the author keeps working), so
// everyone reads the head from git.
const whereToRead = `Read the reviewed head from git, never from the working tree, which may hold other edits: \`git -C ${dir} show ${scope.head}:<path>\`, \`git -C ${dir} grep <pattern> ${scope.head} -- <paths>\`, \`git -C ${dir} ls-tree -r --name-only ${scope.head} <dir>\`.`

function readerPrompt(lens) {
  const fixNote = fixRange && !unread.includes(lens.key)
    ? `An earlier read covered up to ${since}. Start from ${fixRange}: the fix is where the next defect usually is. Judge the whole change, not only the fix.`
    : ''
  const preamble = mode === 'design'
    ? `You are one of ${lenses.length} independent readers of a design for FLUI (an ADR, a spec's design) before the code it decides is written or changed. You have seen none of the author's conversation, plan or pull-request body; do not look for them.

The design: ${range} (${scope.files.length} files; the reviewed head is ${scope.head}). Read each design document in full as that head has it, and the code it governs as it stands there. ${whereToRead} ${fixNote}

Assume the design allows a defect of the kinds your lens names and find the order of events, the state or the input it permits, from the documents and from the code they govern. Other readers cover the other kinds; stay in your lens.`
    : `You are one of ${lenses.length} independent readers of a change to FLUI before it merges. You have seen none of the author's conversation, plan or pull-request body; do not look for them.

The change: ${range} (${scope.files.length} files; the reviewed head is ${scope.head}). ${whereToRead} ${fixNote}

Assume the change has a defect of the kinds your lens names and find the input, the order of events or the state that shows it, in the code around the diff as much as in the changed lines. Other readers cover the other kinds; stay in your lens.`
  const findingRule = mode === 'design'
    ? 'A finding is a failure scenario the design permits: the state, the operation, the wrong result, and the section (file:line) that permits it or that should have ruled it out, with the code it governs where it exists. When you find nothing, list in "tried" the scenarios you tried and the section or line that rules each out.'
    : 'A finding is a failure scenario: the state, the operation, the wrong result, and the file:line that produces it, read in the source rather than inferred from a name. When you find nothing, list in "tried" the scenarios you tried and the line that rules each out.'
  return `${preamble}

Do not edit anything, and do not build or run tests: the other readers share this host, and a build would queue them all. Do not fix, and do not sum up what is good.

${findingRule}

Your lens: ${lens.focus}`
}

phase('Read')
// The mutation run is read beside the readers. It ran in a worktree of its own, detached at the
// commit it read, so the worktree itself says which head the report belongs to: its HEAD, a tree
// holding nothing but change.diff, and a report written after that diff.
const runTree = `'${mutantsDir}'`
const gammaReport = `'${mutantsDir}/target/cargo-gamma/gamma-report.json'`
const gammaList = (key, limit) => `jq -r '.files | to_entries[] | .key as $f | .value.mutants[] | select(${GAMMA_SELECTIONS[key]}) | "\\($f)\\t\\(.location.start.line)\\t\\(.description) [\\(.mutatorName)]"' ${gammaReport} | head -n ${limit}`
const gammaCount = (key) => `jq '[.files[].mutants[] | select(${GAMMA_SELECTIONS[key]})] | length' ${gammaReport}`
const mutantsRead = mutantsDir && mode === 'change'
  ? attempt(
      `Read a cargo-gamma run made in its own worktree. Run only these commands, each exactly as written, and copy their output; do not transcribe by hand.
1. run_head: \`git -C ${runTree} rev-parse HEAD\`
2. extra_files: the lines of \`git -C ${runTree} status --porcelain --untracked-files=all\` other than "?? change.diff".
3. report_is_newer: whether \`test ${gammaReport} -nt '${mutantsDir}/change.diff'\` exits 0.
4. diff_matches: whether \`git -C ${dir} diff ${scope.base} ${scope.head} -- ${MUTATION_PATHSPEC} | cmp -s - '${mutantsDir}/change.diff'\` exits 0 (1 is false, an answer; any other exit is an error).
5. counts: killed \`${gammaCount('killed')}\`, survived \`${gammaCount('survived')}\`, uncovered \`${gammaCount('uncovered')}\`, timed_out \`${gammaCount('timed_out')}\`, ignored \`${gammaCount('ignored')}\`.
6. survived: the lines of \`${gammaList('survived', MAX_MUTANT_FINDINGS)}\`; uncovered: \`${gammaList('uncovered', MAX_MUTANT_FINDINGS)}\`; timed_out: \`${gammaList('timed_out', MAX_MUTANT_NOTES)}\`; ignored: \`${gammaList('ignored', MAX_MUTANT_NOTES)}\`. Each line splits at its two tabs into file, line and mutation.
When a file cannot be read or a command fails, put the command and its error in "error".`,
      { phase: 'Read', label: 'read:mutants', schema: MUTANTS_SCHEMA, model: 'haiku', effort: 'low' },
    )
  : null
const reads = await parallel(
  lenses.map((lens) => async () => {
    const options = { phase: 'Read', label: `read:${lens.key}`, schema: FINDINGS_SCHEMA, model: lens.model, effort: 'high' }
    let read = await attempt(readerPrompt(lens), options)
    let model = lens.model
    // The first failure is kept beside the second: a lens that fails twice reports both.
    let first = ''
    if (read.error && lens.fallback) {
      first = `${lens.model}: ${read.error}`
      log(`read:${lens.key} on ${first}; ${lens.fallback} reads instead`)
      model = lens.fallback
      read = await attempt(readerPrompt(lens), { ...options, label: `read:${lens.key}:${model}`, model })
    }
    return read.error
      ? { lens: lens.key, model, first, error: read.error }
      // Only the two fields the schema asks for are taken, so nothing else a reader returns can
      // stand in for the script's lens, model or failure.
      : { lens: lens.key, model, first, findings: read.value.findings, tried: read.value.tried }
  }),
)
const done = reads.filter((r) => r && !r.error)
const failed = lenses.filter((l) => !done.some((r) => r.lens === l.key)).map((l) => {
  const r = reads.find((x) => x && x.lens === l.key)
  if (!r) return { lens: l.key, error: 'the reader threw' }
  return { lens: l.key, error: r.first ? `${r.first}; then ${r.model}: ${r.error}` : `${r.model}: ${r.error}` }
})
notes.push(
  ...failed.map((f) => `Not read through the ${f.lens} lens (${f.error}). The author reads the diff through it (AGENTS.md, "Before a merge"), and the next read passes it in "unread".`),
  ...done.filter((r) => r.first).map((r) => `The ${r.lens} lens was read again on ${r.model} after ${r.first}.`),
)
// A mutant that survived is a changed line whose behaviour every test reaching it is blind to,
// and one no test reaches is a line nothing pins; both go to the judge, who rejects one that
// cannot change behaviour or sits in code built only for another target.
let mutation = 'none supplied'
const mutantFindings = []
const listed = (xs, count) => `${xs.map((x) => `${x.file}:${x.line} ${x.mutation}`).join('; ')}${count > xs.length ? `; and ${count - xs.length} more in the report` : ''}`
if (mode === 'design') {
  // A design has no lines of Rust to mutate; a run passed beside it is not read.
  mutation = 'not used for a design'
  if (mutantsDir) notes.push(`A design is read without a mutation run; the one at ${mutantsDir} was not read.`)
} else if (!mutantsDir) {
  notes.push('No mutation run was supplied (AGENTS.md "Before each push" item 10).')
} else {
  const m = await mutantsRead
  const v = m.value
  const judged = v ? v.counts.killed + v.counts.survived + v.counts.uncovered + v.counts.timed_out : 0
  if (m.error || v.error) {
    mutation = 'unreadable'
    notes.push(`The mutation run at ${mutantsDir} could not be read: ${m.error || v.error}.`)
  } else if (v.run_head !== scope.head) {
    mutation = 'over another head'
    notes.push(`The mutation run at ${mutantsDir} is checked out at ${v.run_head || 'no commit'}, not ${scope.head}; its results were left out.`)
  } else if (v.extra_files.length) {
    mutation = 'over an edited tree'
    notes.push(`The mutation run's worktree at ${mutantsDir} holds more than change.diff (${v.extra_files.join('; ')}); its results were left out.`)
  } else if (!v.report_is_newer) {
    mutation = 'older than its diff'
    notes.push(`The report in ${mutantsDir} was written before its change.diff, so it is not the run over that diff; its results were left out.`)
  } else if (!v.diff_matches) {
    mutation = 'over another diff'
    notes.push(`The mutation run at ${mutantsDir} took another diff than ${scope.base.slice(0, 9)}..${scope.head.slice(0, 9)} over ${MUTATION_PATHSPEC}; its results were left out.`)
  } else if (
    v.survived.length !== Math.min(v.counts.survived, MAX_MUTANT_FINDINGS)
    || v.uncovered.length !== Math.min(v.counts.uncovered, MAX_MUTANT_FINDINGS)
  ) {
    mutation = 'miscounted'
    notes.push(`The mutation report counts ${v.counts.survived} survivors and ${v.counts.uncovered} uncovered mutants, but ${v.survived.length} and ${v.uncovered.length} were read; its results were left out, and the author reads the report.`)
  } else if (!judged) {
    // A run whose build failed still writes a report, with every mutant ignored: it judged nothing.
    mutation = 'judged nothing'
    notes.push(`The mutation run at ${mutantsDir} judged no mutant (${v.counts.ignored} ignored or pending); a run that judges them is still owed.`)
  } else {
    mutation = `${v.counts.killed} killed, ${v.counts.survived} survived, ${v.counts.uncovered} uncovered`
    const finding = (x, wrong) => ({
      title: `No test notices \`${x.mutation}\``,
      file: x.file,
      line: x.line,
      state: `the change's line ${x.file}:${x.line} mutated: ${x.mutation}`,
      operation: 'the test packages of the mutation run ran against it',
      wrong_result: wrong,
      evidence: `${mutantsDir}/target/cargo-gamma/gamma-report.json`,
    })
    for (const x of v.survived) mutantFindings.push(finding(x, 'every test that reached it passed (cargo-gamma: survived)'))
    for (const x of v.uncovered) mutantFindings.push(finding(x, 'no test reached it (cargo-gamma: no coverage)'))
    for (const [key, count] of [['survived', v.counts.survived], ['uncovered', v.counts.uncovered]]) {
      if (count > MAX_MUTANT_FINDINGS) notes.push(`${count - MAX_MUTANT_FINDINGS} more ${key} mutants are in the report than the ${MAX_MUTANT_FINDINGS} the judge was given; the author answers them from it.`)
    }
    if (v.counts.timed_out) notes.push(`Mutants the tests timed out or ran out of memory on, neither caught nor passed: ${listed(v.timed_out, v.counts.timed_out)}.`)
    if (v.counts.ignored) notes.push(`Mutants the run established nothing about (a flake, not built, or never judged): ${listed(v.ignored, v.counts.ignored)}.`)
  }
}

const unverifiedMutants = () => (mutantFindings.length
  ? `\n\nThe mutation run's survivors, unverified, which the author answers:\n${mutantFindings.map((f) => `- **${f.title}** \`${f.file}:${f.line}\``).join('\n')}`
  : '')
// No reader back is no independent read, whatever the mutation run found; its survivors are
// still listed for the author.
if (!done.length) {
  return unavailable(`no reader returned (${failed.map((f) => `${f.lens}: ${f.error}`).join('; ')})`, { missing: failed, findings: mutantFindings, markdownTail: unverifiedMutants() })
}
// The script's number and lens come last, so nothing a reader returns can replace them.
const reported = [
  ...done.flatMap((r) => r.findings.map((f) => ({ ...f, lens: r.lens }))),
  ...mutantFindings.map((f) => ({ ...f, lens: 'mutants' })),
].map((f, i) => ({ ...f, n: i + 1 }))
log(`${reported.length} findings from ${done.length} of ${lenses.length} readers and the mutation run (${mutation})`)

phase('Judge')
const judgePrompt =
  `You judge the findings ${done.length} independent readers${mutantFindings.length ? ' and a mutation run' : ''} reported on a ${mode} to FLUI: ${range}, reviewed head ${scope.head}. ${whereToRead} You have not seen the author's account of the change and need none.

For each finding:
1. Merge it with the others that share its root cause; keep the clearest scenario and list every lens that reported it.
2. Verify it at the reviewed head: open the cited lines and follow the scenario. It is rejected only when ${mode === 'design' ? 'a section of the design, an accepted ADR, or a line of the code it governs' : 'a line or a test'} shows the scenario cannot happen; name it. That it seems unlikely is not a rejection.
3. Rank it: important, or nit. ${mode === 'design' ? 'For a design, important is a scenario the design permits that a user or the code built from it would hit (any AGENTS.md "Recurring defects" class), a decision no test or rung can hold, or a contradiction of an accepted ADR; REVIEW.md\'s narrower scope for docs/plans/specs/ does not apply to this read.' : 'By REVIEW.md "What Important means here".'} Keep at most five nits, each with the better shape; put the other verified nits in "not_kept".

A finding from the "mutants" lens is a change cargo-gamma made to one line of the diff that either every test reaching it passed with (survived) or that no test reached (no coverage); its "wrong_result" says which. Verify it like the others: confirm it as a missing test, or reject it only when the mutation cannot change what the code does (an equivalent mutant) or, for one no test reached, when the line is built only for another target than the run's; say why from the code.

Every finding is numbered ("n"). Each entry of "confirmed", "rejected" and "not_kept" lists in "sources" the numbers it accounts for, and every number appears in one of them.

Do not edit or build. Do not add findings of your own unless verifying one shows a second defect beside it.

Findings (JSON):
${JSON.stringify(reported, null, 1)}

What each reader tried and ruled out (JSON):
${JSON.stringify(done.map((r) => ({ lens: r.lens, tried: r.tried })), null, 1)}`

// The judge decides what the author must answer, so it runs on the strongest model the account
// can start; when that one fails, the next one judges. A note is written once the outcome it
// reports is known.
const judgeOptions = { phase: 'Judge', label: 'judge', schema: VERDICT_SCHEMA, effort: 'high' }
let judge = 'fable'
let judged = await attempt(judgePrompt, { ...judgeOptions, model: 'fable' })
if (judged.error) {
  const fableError = judged.error
  log(`The fable judge: ${fableError}; opus judges.`)
  judged = await attempt(judgePrompt, { ...judgeOptions, label: 'judge:opus', model: 'opus' })
  if (judged.error) {
    judge = null
    notes.push(`No judge: fable returned ${fableError}; opus returned ${judged.error}.`)
  } else {
    judge = 'opus'
    notes.push(`Judged on opus after the fable judge returned ${fableError}.`)
  }
}

const lensLine = `Lenses read: ${done.map((r) => `${r.lens} (${r.model})`).join(', ')}${failed.length ? `; not read: ${failed.map((f) => f.lens).join(', ')}` : ''}. Mutation run: ${mutation}. Judge: ${judge || 'none'}.`
// Reader findings keep the readers' numbers, labelled so they never read as the judge's list.
const readerFindingLine = (f) => `- ${f.lens === 'mutants' ? 'mutation' : 'reader'} finding ${f.n}: **${f.title}** — ${f.state}; ${f.operation}; ${f.wrong_result}. \`${f.file}:${f.line}\` (${f.lens})`
const nothingFound = done
  .filter((r) => !r.findings.length)
  .map((r) => `The ${r.lens} lens found nothing; it tried: ${r.tried.join(' | ')}`)
const triedBlock = nothingFound.length ? `\n\n${nothingFound.join('\n')}` : ''

if (!judge) {
  return {
    base: scope.base,
    head: scope.head,
    judge,
    missing: failed,
    findings: reported,
    confirmed: [],
    rejected: [],
    markdown: `${header} ${lensLine}\n\nNo judge could be started, so the readers' findings below are unverified: the author answers each one.\n\n${reported.map(readerFindingLine).join('\n')}${triedBlock}${footer()}`,
  }
}

// The list the author answers is built from the verdict's entries, not from prose the judge
// writes, so a confirmed entry cannot drop out of it; a finding the judge did not account for is
// listed beside it rather than lost.
const verdict = judged.value
// The lenses an entry credits come from the readers behind its source numbers, and REVIEW.md's
// five-nit cap is held here, whatever the judge returned.
const lensesOf = (sources) => [...new Set(sources.map((n) => reported.find((f) => f.n === n)).filter(Boolean).map((f) => f.lens))]
const important = verdict.confirmed.filter((c) => c.severity === 'important')
const nits = verdict.confirmed.filter((c) => c.severity !== 'important')
const confirmed = [...important, ...nits.slice(0, 5)].map((c) => ({ ...c, lenses: lensesOf(c.sources) }))
const notKept = [...verdict.not_kept, ...nits.slice(5).map((c) => ({ sources: c.sources, title: c.title }))]
const confirmedList = confirmed.length
  ? confirmed
      .map((c, i) => `${i + 1}. **${c.title}** (${c.severity}; ${c.class}) — ${c.scenario} \`${c.file}:${c.line}\`; reported by ${c.lenses.join(', ') || 'no reader the numbers name'}`)
      .join('\n')
  : 'The judge confirmed no finding.'
const rejectedBlock = verdict.rejected.length
  ? `\n\nRejected by the judge:\n${verdict.rejected.map((r) => `- ${r.title} — ${r.reason}`).join('\n')}`
  : ''
const accounted = new Set([...verdict.confirmed, ...verdict.rejected, ...verdict.not_kept].flatMap((e) => e.sources))
const unaccounted = reported.filter((f) => !accounted.has(f.n))
if (unaccounted.length) {
  notes.push(`The judge did not account for these reader findings; the author answers each one:\n${unaccounted.map(readerFindingLine).join('\n')}`)
}
if (notKept.length) {
  notes.push(`Verified nits beyond the five kept: ${notKept.map((e) => e.title).join('; ')}.`)
}

return {
  base: scope.base,
  head: scope.head,
  judge,
  readers: done.map((r) => ({ lens: r.lens, model: r.model })),
  missing: failed,
  findings: reported,
  confirmed,
  rejected: verdict.rejected,
  not_kept: notKept,
  unaccounted,
  markdown: `${header} ${lensLine}\n\n${confirmedList}${rejectedBlock}${triedBlock}${footer()}`,
}
