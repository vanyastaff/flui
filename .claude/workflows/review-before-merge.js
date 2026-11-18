export const meta = {
  name: 'review-before-merge',
  description: 'Seven readers, one lens each, read a change to break it; a judge merges, verifies and ranks what they find',
  whenToUse: 'The independent read AGENTS.md "Before a merge" asks for, on a branch or a checkout, and again on each fix pushed after it',
  phases: [
    { title: 'Scope', detail: 'merge base, reviewed head and changed files' },
    { title: 'Read', detail: 'records, tests, obligations, unwinding, boundaries, seams, rules in parallel' },
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
- A perf or bench row whose bound a regression could not cross, or whose warm-up measures nothing.`,
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

// An agent that a usage limit or a skip refuses resolves to null, and one started past the turn's
// token target throws; the runtime does not say which. Each is a failure the result names, and
// none may read as an empty answer.
async function attempt(prompt, options) {
  try {
    const value = await agent(prompt, options)
    return value ? { value } : { error: 'no result (refused by a usage limit, skipped, or failed after retries)' }
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
    if (LENSES.some((l) => l.key === key)) unread.push(key)
    else unknownUnread.push(key)
  }
}

function unavailable(reason, extra) {
  log(`Independent read unavailable: ${reason}`)
  return {
    unavailable: reason,
    confirmed: [],
    rejected: [],
    markdown: `Independent read unavailable: ${reason}${footer()}`,
    ...extra,
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
const header = `Read at ${scope.head} (base ${scope.base}${since ? `, fix since ${since}` : ''}).`
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
  notes.push(`"unread" names ${unknownUnread.map((k) => JSON.stringify(k)).join(', ')}, not a lens (${LENSES.map((l) => l.key).join(', ')}); ${fixRange ? 'no lens was spared the fix note for it.' : 'there was no fix note to spare it from.'}`)
}
// The checkout's working tree may move while the readers run (the author keeps working), so
// everyone reads the head from git.
const whereToRead = `Read the reviewed head from git, never from the working tree, which may hold other edits: \`git -C ${dir} show ${scope.head}:<path>\`, \`git -C ${dir} grep <pattern> ${scope.head} -- <paths>\`, \`git -C ${dir} ls-tree -r --name-only ${scope.head} <dir>\`.`

function readerPrompt(lens) {
  const fixNote = fixRange && !unread.includes(lens.key)
    ? `An earlier read covered up to ${since}. Start from ${fixRange}: the fix is where the next defect usually is. Judge the whole change, not only the fix.`
    : ''
  return `You are one of ${LENSES.length} independent readers of a change to FLUI before it merges. You have seen none of the author's conversation, plan or pull-request body; do not look for them.

The change: ${range} (${scope.files.length} files; the reviewed head is ${scope.head}). ${whereToRead} ${fixNote}

Assume the change has a defect of the kinds your lens names and find the input, the order of events or the state that shows it, in the code around the diff as much as in the changed lines. Other readers cover the other kinds; stay in your lens.

Do not edit anything, and do not build or run tests: the other readers share this host, and a build would queue them all. Do not fix, and do not sum up what is good.

A finding is a failure scenario: the state, the operation, the wrong result, and the file:line that produces it, read in the source rather than inferred from a name. When you find nothing, list in "tried" the scenarios you tried and the line that rules each out.

Your lens: ${lens.focus}`
}

phase('Read')
const reads = await parallel(
  LENSES.map((lens) => async () => {
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
const failed = LENSES.filter((l) => !done.some((r) => r.lens === l.key)).map((l) => {
  const r = reads.find((x) => x && x.lens === l.key)
  if (!r) return { lens: l.key, error: 'the reader threw' }
  return { lens: l.key, error: r.first ? `${r.first}; then ${r.model}: ${r.error}` : `${r.model}: ${r.error}` }
})
notes.push(
  ...failed.map((f) => `Not read through the ${f.lens} lens (${f.error}). The author reads the diff through it (AGENTS.md, "Before a merge"), and the next read passes it in "unread".`),
  ...done.filter((r) => r.first).map((r) => `The ${r.lens} lens was read again on ${r.model} after ${r.first}.`),
)
if (!done.length) {
  return unavailable(`no reader returned (${failed.map((f) => `${f.lens}: ${f.error}`).join('; ')})`, { missing: failed })
}
// The script's number and lens come last, so nothing a reader returns can replace them.
const reported = done.flatMap((r) => r.findings.map((f) => ({ ...f, lens: r.lens })))
  .map((f, i) => ({ ...f, n: i + 1 }))
log(`${reported.length} findings from ${done.length} of ${LENSES.length} readers`)

phase('Judge')
const judgePrompt =
  `You judge the findings ${done.length} independent readers reported on a change to FLUI: ${range}, reviewed head ${scope.head}. ${whereToRead} You have not seen the author's account of the change and need none.

For each finding:
1. Merge it with the others that share its root cause; keep the clearest scenario and list every lens that reported it.
2. Verify it in the source at the reviewed head: open the cited lines and follow the scenario. It is rejected only when a line or a test shows the scenario cannot happen; name that line. That it seems unlikely is not a rejection.
3. Rank it by REVIEW.md "What Important means here": important, or nit. Keep at most five nits, each with the better shape; put the other verified nits in "not_kept".

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

const lensLine = `Lenses read: ${done.map((r) => `${r.lens} (${r.model})`).join(', ')}${failed.length ? `; not read: ${failed.map((f) => f.lens).join(', ')}` : ''}. Judge: ${judge || 'none'}.`
// Reader findings keep the readers' numbers, labelled so they never read as the judge's list.
const readerFindingLine = (f) => `- reader finding ${f.n}: **${f.title}** — ${f.state}; ${f.operation}; ${f.wrong_result}. \`${f.file}:${f.line}\` (${f.lens})`
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
