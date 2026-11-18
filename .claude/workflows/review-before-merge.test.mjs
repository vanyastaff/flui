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
  assert.match(result.markdown, /^Read at b{40} \(base a{40}, fix since c{40}\)\./)
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

// 9. A mutation run over this diff turns each missed mutant into a finding the judge sees, even
//    when no reader returned; a run over another diff is left out and says so.
{
  const mutant = { file: 'crates/a/src/lib.rs', line: 7, mutation: 'replace - with + in f' }
  let judgePrompt = ''
  const { result } = await run({ mutants: 'D:/m/mutants.out' }, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label === 'read:mutants') return { diff_matches: true, missed: [mutant], timeout: [], error: '' }
    if (o.label.startsWith('read:')) return null
    judgePrompt = p
    return { confirmed: [{ sources: [1], title: 't', severity: 'important', class: 'c', file: mutant.file, line: 7, scenario: 's', verified: 'v', lenses: [] }], rejected: [], not_kept: [] }
  })
  assert.equal(result.unavailable, undefined)
  assert.deepEqual(result.findings.map((f) => [f.n, f.lens, f.line]), [[1, 'mutants', 7]])
  assert.match(judgePrompt, /No test notices `replace - with \+ in f`/)
  assert.deepEqual(result.confirmed[0].lenses, ['mutants'])
  assert.match(result.markdown, /Mutation run: 1 missed\./)
}
{
  const { result } = await run({ mutants: 'D:/m/mutants.out' }, (p, o) => {
    if (o.label === 'scope') return scope()
    if (o.label === 'read:mutants') return { diff_matches: false, missed: [{ file: 'x.rs', line: 1, mutation: 'm' }], timeout: [], error: '' }
    if (o.label.startsWith('read:')) return { findings: [], tried: ['t'] }
    return { confirmed: [], rejected: [], not_kept: [] }
  })
  assert.equal(result.findings.length, 0)
  assert.match(result.markdown, /made over another diff .*left out/)
  assert.match(result.markdown, /Mutation run: over another diff\./)
}
console.log('all branches behave')
