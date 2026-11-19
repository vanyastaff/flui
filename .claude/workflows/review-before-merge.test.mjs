// The failure branches of review-before-merge.js, driven by stub agents in place of real ones:
// node .claude/workflows/review-before-merge.test.mjs [path to the workflow]
import { readFileSync } from 'node:fs'
import assert from 'node:assert/strict'

const src = readFileSync(process.argv[2] ?? new URL('./review-before-merge.js', import.meta.url), 'utf8').replace(/^export const meta =/m, 'const meta =')
const SHA = (c) => c.repeat(40)

function load() {
  return new Function('agent', 'parallel', 'log', 'phase', 'args', `return (async()=>{${src}})()`)
}
async function run(args, agentStub) {
  const calls = []
  const agent = async (prompt, o) => { calls.push(o.label); return agentStub(prompt, o) }
  const parallel = async (thunks) => Promise.all(thunks.map((t) => t().catch(() => null)))
  const result = await load()(agent, parallel, () => {}, () => {}, args)
  return { result, calls }
}
const scope = (extra = {}) => ({ base: SHA('a'), head: SHA('b'), since: '', since_base: '', since_is_ancestor: true, files: ['x.rs'], stat: '1 file', warnings: [], error: '', ...extra })
const finding = (title, extra = {}) => ({ title, file: 'x.rs', line: 1, state: 's', operation: 'o', wrong_result: 'w', evidence: 'e', ...extra })

// 1. A ref with a smart quote or a dashed component is refused before any agent runs.
for (const base of ['origin/ma’in', '--upload-pack=x', 42]) {
  const { result, calls } = await run({ base }, () => assert.fail('no agent may run'))
  assert.match(result.markdown, /^Independent read unavailable: the base ref/)
  assert.equal(calls.length, 0)
}
// A dashed component after origin/ reaches the fetch only after `--`.
{
  let scopePrompt = ''
  await run({ base: 'origin/--upload-pack=x' }, (p, o) => {
    if (o.label === 'scope') { scopePrompt = p; return null }
    assert.fail(o.label)
  })
  assert.match(scopePrompt, /fetch origin -- '<name>'/)
}

// 2. A null scope is a named failure, never "no change".
{
  const { result } = await run({}, (p, o) => (o.label === 'scope' ? null : assert.fail()))
  assert.match(result.markdown, /Independent read unavailable: the scope step failed: no result/)
}

// 3. A mistyped unread is reported; readers get numbers they cannot override; a confirmed entry
//    reaches the list without judge prose; an unaccounted finding is listed; a refused fable judge
//    falls to opus and says so.
{
  const { result, calls } = await run({ since: 'HEAD~1', unread: ['unwind'] }, (p, o) => {
    if (o.label === 'scope') return scope({ since: SHA('c'), since_base: SHA('a') })
    if (o.label === 'read:records') return { findings: [finding('one', { n: 99, lens: 'forged' }), finding('two')], tried: [] }
    if (o.label.startsWith('read:')) return { findings: [], tried: ['t'] }
    if (o.label === 'judge') return null
    if (o.label === 'judge:opus') {
      return { confirmed: [{ sources: [1], title: 'one', severity: 'important', class: 'c', file: 'x.rs', line: 1, scenario: 'sc', verified: 'v', lenses: ['records'] }], rejected: [], not_kept: [] }
    }
    assert.fail(o.label)
  })
  assert.equal(result.judge, 'opus')
  assert.deepEqual(result.findings.map((f) => [f.n, f.lens]), [[1, 'records'], [2, 'records']])
  assert.deepEqual(result.unaccounted.map((f) => f.n), [2])
  assert.match(result.markdown, /^Read \(change\) at b{40} \(base a{40}, fix since c{40}\)\./)
  assert.match(result.markdown, /1\. \*\*one\*\* \(important; c\)/)
  assert.match(result.markdown, /reader finding 2: \*\*two\*\*/)
  assert.match(result.markdown, /"unread" names "unwind", not a lens .*no lens was spared the fix note/)
  assert.match(result.markdown, /Judged on opus after the fable judge returned/)
  assert.ok(calls.includes('judge') && calls.includes('judge:opus'))
}

// 4. An opus reader refused twice reports both failures; with no judge every finding is listed.
{
  const { result } = await run({}, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label.startsWith('read:obligations')) return null
    if (o.label.startsWith('read:')) return { findings: [finding(`f-${o.label}`)], tried: [] }
    return null
  })
  assert.equal(result.judge, null)
  assert.match(result.markdown, /Not read through the obligations lens \(opus: no result.*; then sonnet: no result/)
  assert.match(result.markdown, /No judge could be started/)
  assert.equal(result.findings.length, 6)
  assert.match(result.markdown, /No judge: fable returned .*; opus returned/)
}

// 5. A judge that throws (a spent token target) is caught like a refusal.
{
  const { result } = await run({}, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label.startsWith('read:')) return { findings: [], tried: ['t'] }
    throw new Error('budget spent')
  })
  assert.equal(result.judge, null)
  assert.match(result.markdown, /fable returned budget spent; opus returned budget spent/)
}

// 6. A rebased since compares the series.
{
  let readerPrompt = ''
  await run({ since: 'old' }, (p, o) => {
    if (o.label === 'scope') return scope({ since: SHA('c'), since_base: SHA('d'), since_is_ancestor: false })
    if (o.label === 'read:records') readerPrompt = p
    if (o.label.startsWith('read:')) return { findings: [], tried: [] }
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  assert.match(readerPrompt, /range-diff d{40}\.\.c{40} a{40}\.\.b{40}/)
}
// 7. An empty or null since is no earlier read, not an unsafe ref.
for (const since of ['', null]) {
  const { result } = await run({ since }, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label.startsWith('read:')) return { findings: [], tried: ['t'] }
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  assert.equal(result.unavailable, undefined)
  assert.equal(result.judge, 'fable')
}

// 8. A reader's own top-level lens cannot replace the script's; a confirmed entry credits the
//    lenses behind its numbers; the script keeps five nits and moves the rest to not_kept.
{
  const nit = (n) => ({ sources: [n], title: `nit ${n}`, severity: 'nit', class: 'c', file: 'x.rs', line: 1, scenario: 's', verified: 'v', lenses: ['typed-by-judge'] })
  const { result } = await run({}, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label === 'read:tests') return { lens: 'forged', error: 'forged', findings: [1, 2, 3, 4, 5, 6].map((i) => finding(`t${i}`)), tried: [] }
    if (o.label.startsWith('read:')) return { findings: [], tried: ['t'] }
    return { confirmed: [1, 2, 3, 4, 5, 6].map(nit), rejected: [], not_kept: [] }
  })
  assert.ok(result.readers.some((r) => r.lens === 'tests'))
  assert.equal(result.missing.length, 0)
  assert.equal(result.confirmed.length, 5)
  assert.deepEqual(result.confirmed[0].lenses, ['tests'])
  assert.deepEqual(result.not_kept.map((e) => e.title), ['nit 6'])
}

// 9. A cargo-gamma run in its own worktree at this head: each survivor and each uncovered mutant
//    is a finding the judge sees; a run that cannot be trusted (unreadable, refused, another head,
//    an edited tree, a report older than its diff, another diff, miscounted, nothing judged) is
//    left out and says why; lists past their bound are counted; timed-out and ignored mutants are
//    listed, not judged; an empty or null mutants is no run; a path a shell cannot hold is
//    refused; with no reader back the read is unavailable and the mutants are listed.
const mutant = (line, mutation = 'replace - with + [arith.sub_to_add]') => ({ file: 'crates/a/src/lib.rs', line, mutation })
const counts = (extra = {}) => ({ killed: 1, survived: 1, uncovered: 0, timed_out: 0, ignored: 0, ...extra })
const gamma = (extra = {}) => ({
  run_head: SHA('b'), extra_files: [], report_is_newer: true, diff_matches: true, counts: counts(),
  survived: [mutant(7)], uncovered: [], timed_out: [], ignored: [], error: '', ...extra,
})
async function withRun(report, readers = () => ({ findings: [], tried: ['t'] }), mutants = 'D:/m') {
  let judgePrompt = ''
  let mutantsPrompt = ''
  const out = await run({ mutants }, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label === 'read:mutants') { mutantsPrompt = p; return typeof report === 'function' ? report() : report }
    if (o.label.startsWith('read:')) return readers(o)
    judgePrompt = p
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  return { ...out, judgePrompt, mutantsPrompt }
}
{
  const { result, judgePrompt } = await withRun(gamma({ counts: counts({ uncovered: 1 }), uncovered: [mutant(9, 'u')] }))
  assert.deepEqual(result.findings.map((f) => [f.n, f.lens, f.line]), [[1, 'mutants', 7], [2, 'mutants', 9]])
  assert.equal(result.findings[0].evidence, 'D:/m/target/cargo-gamma/gamma-report.json')
  assert.match(result.findings[1].wrong_result, /no test reached it/)
  assert.match(judgePrompt, /No test notices `replace - with \+ \[arith\.sub_to_add\]`/)
  assert.match(judgePrompt, /readers and a mutation run reported/)
  assert.match(judgePrompt, /built only for another target/)
  assert.match(result.markdown, /Mutation run: 1 killed, 1 survived, 1 uncovered\./)
}
for (const [report, label, why] of [
  [gamma({ error: 'jq: error' }), 'unreadable', /could not be read: jq: error/],
  [null, 'unreadable', /could not be read: no result/],
  [() => { throw new Error('budget spent') }, 'unreadable', /could not be read: budget spent/],
  [gamma({ run_head: SHA('c') }), 'over another head', /checked out at c{40}, not b{40}/],
  [gamma({ extra_files: [' M crates/a/src/lib.rs'] }), 'over an edited tree', /holds more than change\.diff \( M crates\/a\/src\/lib\.rs\)/],
  [gamma({ report_is_newer: false }), 'older than its diff', /written before its change\.diff/],
  [gamma({ diff_matches: false }), 'over another diff', /took another diff .*left out/],
  [gamma({ counts: counts({ survived: 3 }) }), 'miscounted', /counts 3 survivors and 0 uncovered .* but 1 and 0 were read/],
  [gamma({ counts: counts({ uncovered: 2 }) }), 'miscounted', /counts 1 survivors and 2 uncovered .* but 1 and 0 were read/],
  [gamma({ counts: counts({ killed: 0, survived: 0, ignored: 37 }), survived: [] }), 'judged nothing', /judged no mutant \(37 ignored or pending\)/],
]) {
  const { result } = await withRun(report)
  assert.equal(result.findings.length, 0, label)
  assert.match(result.markdown, new RegExp(`Mutation run: ${label}\\.`))
  assert.match(result.markdown, why)
}
{
  const many = (from) => Array.from({ length: 40 }, (_, i) => mutant(from + i))
  const { result } = await withRun(gamma({
    counts: counts({ survived: 52, uncovered: 41, timed_out: 12, ignored: 1 }),
    survived: many(1), uncovered: many(500), timed_out: Array.from({ length: 10 }, (_, i) => mutant(100 + i, 't')), ignored: [mutant(200, 'i')],
  }))
  assert.equal(result.findings.length, 80)
  assert.match(result.markdown, /12 more survived mutants are in the report than the 40/)
  assert.match(result.markdown, /1 more uncovered mutants are in the report than the 40/)
  assert.match(result.markdown, /timed out or ran out of memory on, neither caught nor passed: .*lib\.rs:109 t; and 2 more in the report/)
  assert.match(result.markdown, /established nothing about \(a flake, not built, or never judged\): crates\/a\/src\/lib\.rs:200 i\./)
}
{
  // Exactly at the bound: nothing more to count.
  const { result } = await withRun(gamma({ counts: counts({ survived: 40 }), survived: Array.from({ length: 40 }, (_, i) => mutant(i + 1)) }))
  assert.equal(result.findings.length, 40)
  assert.doesNotMatch(result.markdown, /more survived mutants/)
}
for (const mutants of ['', null]) {
  const { result, calls } = await withRun(gamma(), undefined, mutants)
  assert.equal(result.unavailable, undefined)
  assert.ok(!calls.includes('read:mutants'))
  assert.match(result.markdown, /Mutation run: none supplied\./)
}
for (const mutants of ["D:/it's", 42]) {
  const { result, calls } = await withRun(gamma(), undefined, mutants)
  assert.match(result.markdown, /^Independent read unavailable: the mutants path/)
  assert.equal(calls.length, 0)
}
{
  const { result } = await withRun(gamma(), () => null)
  assert.match(result.markdown, /^Independent read unavailable: no reader returned/)
  assert.match(result.markdown, /The mutation run's survivors, unverified, which the author answers:\n- \*\*No test notices/)
  assert.equal(result.findings.length, 1)
}
{
  // The judge's list labels a mutation finding as one, not as a reader's.
  const { result } = await withRun(gamma(), undefined)
  assert.match(result.markdown, /mutation finding 1: \*\*No test notices/)
}

// 10. The commands the mutation reader is given, run for real with jq over a report holding every
//     status cargo-gamma 0.2.1 writes, at the path a run worktree holds it: the selections, the
//     fields and the bounds are the report's.
{
  const { mkdtempSync, mkdirSync, writeFileSync } = await import('node:fs')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { execSync } = await import('node:child_process')
  const dir = mkdtempSync(join(tmpdir(), 'gamma-')).replaceAll('\\', '/')
  mkdirSync(join(dir, 'target', 'cargo-gamma'), { recursive: true })
  const at = (status, line, statusReason) => ({ id: `m${line}`, mutatorName: 'arith.sub_to_add', location: { start: { line, column: 1 }, end: { line, column: 2 } }, status, ...(statusReason ? { statusReason } : {}), replacement: 'x', description: `mutant at ${line}` })
  const survivors = Array.from({ length: 45 }, (_, i) => at('Survived', 1000 + i))
  writeFileSync(join(dir, 'target', 'cargo-gamma', 'gamma-report.json'), JSON.stringify({
    schemaVersion: 2, config: { startedAt: 1795044795 },
    files: { 'crates/a/src/lib.rs': { language: 'rust', source: '', mutants: [
      at('Killed', 1), at('Survived', 2), at('Survived', 3, 'timed out: test x'), at('Survived', 4, 'out of memory: test y'),
      at('NoCoverage', 5), at('Ignored', 6, 'flaky: test z'), at('Pending', 8), at('CompileError', 7), ...survivors,
    ] } },
  }))
  const { mutantsPrompt } = await withRun(gamma(), undefined, dir)
  const sh = (c) => execSync(c, { shell: process.env.SHELL || 'bash', encoding: 'utf8' }).trim()
  const survivedLines = [2, ...survivors.map((m) => m.location.start.line)].slice(0, 40)
  for (const [field, count, lines] of [['killed', 1, null], ['survived', 46, survivedLines], ['uncovered', 1, [5]], ['timed_out', 2, [3, 4]], ['ignored', 2, [6, 8]]]) {
    const countCommand = mutantsPrompt.match(new RegExp(`${field} \`(jq '\\[[^\`]+)\``))[1]
    assert.equal(sh(countCommand), String(count), field)
    if (!lines) continue
    const listCommand = mutantsPrompt.match(new RegExp(`${field}: (?:the lines of )?\`(jq -r [^\`]+)\``))[1]
    assert.deepEqual(sh(listCommand).split('\n').map((l) => Number(l.split('\t')[1])), lines, field)
  }
}
// 11. mode "design" reads with the design lenses and their wording, validates `unread` against
//     them, and neither starts nor reads a mutation run.
{
  const prompts = {}
  let judgePrompt = ''
  const { result, calls } = await run({ mode: 'design', mutants: 'D:/m', unread: ['orders', 'tests'], since: 'old' }, (p, o) => {
    if (o.label === 'scope') return scope({ since: SHA('c'), since_base: SHA('a') })
    if (o.label === 'read:mutants') assert.fail('a design reads no mutation run')
    if (o.label.startsWith('read:')) { prompts[o.label] = p; return { findings: [], tried: ['t'] } }
    judgePrompt = p
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  assert.deepEqual(Object.keys(prompts).sort(), ['read:alternatives', 'read:confirmation', 'read:orders', 'read:ownership', 'read:records', 'read:seams'])
  assert.ok(!calls.includes('read:mutants'))
  assert.match(prompts['read:ownership'], /^You are one of 6 independent readers of a design for FLUI/)
  assert.match(prompts['read:ownership'], /the section \(file:line\) that permits it/)
  assert.doesNotMatch(prompts['read:orders'], /An earlier read covered/)
  assert.match(prompts['read:ownership'], /An earlier read covered/)
  assert.match(judgePrompt, /reported on a design to FLUI/)
  assert.match(judgePrompt, /a section of the design, an accepted ADR, or a line of the code it governs/)
  assert.match(result.markdown, /"unread" names "tests", not a lens \(alternatives, ownership, orders, seams, records, confirmation\)/)
  assert.match(result.markdown, /Mutation run: not used for a design\./)
  assert.match(result.markdown, /the one at D:\/m was not read/)
  assert.match(result.markdown, /^Read \(design\) at /)
  assert.match(judgePrompt, /REVIEW\.md's narrower scope for docs\/plans\/specs\/ does not apply/)
  // scope() names x.rs: a design read of a diff holding code owes the change read beside it.
  assert.match(result.markdown, /also changes 1 files that are not documents; the change read \(no mode\) is owed/)
}
{
  // A design read with no mutation run says nothing about one, and a diff of only a decision owes
  // no change read.
  const { result } = await run({ mode: 'design' }, (p, o) => {
    if (o.label === 'scope') return scope({ files: ['docs/adr/ADR-0400-x.md'] })
    if (o.label.startsWith('read:')) return { findings: [], tried: ['t'] }
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  assert.doesNotMatch(result.markdown, /was not read|No mutation run/)
  assert.doesNotMatch(result.markdown, /change read \(no mode\) is owed/)
}
{
  // A change read keeps its own wording, and one whose diff changes a decision owes the design read.
  const prompts = {}
  let judgePrompt = ''
  const { result } = await run({ mode: 'change' }, (p, o) => {
    if (o.label === 'scope') return scope({ files: ['x.rs', 'docs/plans/specs/feat/design.md'] })
    if (o.label.startsWith('read:')) { prompts[o.label] = p; return { findings: [], tried: ['t'] } }
    judgePrompt = p
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  assert.equal(Object.keys(prompts).length, 7)
  assert.match(prompts['read:tests'], /^You are one of 7 independent readers of a change to FLUI/)
  assert.match(prompts['read:tests'], /the file:line that produces it/)
  assert.match(judgePrompt, /reported on a change to FLUI/)
  assert.match(judgePrompt, /only when a line or a test shows/)
  assert.match(judgePrompt, /By REVIEW\.md "What Important means here"/)
  assert.match(result.markdown, /changes a decision \(docs\/plans\/specs\/feat\/design\.md\); the design read \(mode "design"\) is owed/)
}
for (const mode of ['Design', 'adr', 7]) {
  const { result, calls } = await run({ mode }, () => assert.fail('no agent may run'))
  assert.match(result.markdown, /^Independent read unavailable: mode .* is neither "change" nor "design"/)
  assert.equal(calls.length, 0)
}
console.log('all branches behave')
