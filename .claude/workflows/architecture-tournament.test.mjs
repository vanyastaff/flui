// The branches of architecture-tournament.js, driven by stub agents in place of real ones:
// node .claude/workflows/architecture-tournament.test.mjs [path to the workflow]
import { readFileSync } from 'node:fs'
import assert from 'node:assert/strict'

const src = readFileSync(process.argv[2] ?? new URL('./architecture-tournament.js', import.meta.url), 'utf8')
  .replace(/^export const meta =/m, 'const meta =')

async function run(args, stub) {
  const calls = []
  const prompts = {}
  const agent = async (prompt, o) => { calls.push(o); prompts[o.label] = prompt; return stub(prompt, o) }
  const parallel = async (thunks) => Promise.all(thunks.map((t) => t().catch(() => null)))
  const fn = new Function('agent', 'parallel', 'log', 'phase', 'args', `return (async()=>{${src}})()`)
  const result = await fn(agent, parallel, () => {}, () => {}, args)
  return { result, calls, labels: calls.map((o) => o.label), prompts }
}
const design = (title) => ({ title, thesis: 't', shapes: 's', flow: 'f', prediction: 'p', migration: 'm', risks: 'r', debt_removed: ['d'] })
const verdict = { scores: [{ stance: 'a', score: 80, why: 'w' }], synthesis: { title: 'S', thesis: 't', grafts: 'g', shapes: 's', flow: 'f', prediction: 'p', migration: 'm', risks: 'r' }, experiments: ['e'], debt: ['x.rs:1 debt'], unresolved: [] }
const stances = [{ key: 'a', focus: 'A' }, { key: 'b', focus: 'B' }, { key: 'c', focus: 'C' }]
// The designs an attacker may name are the ones its schema lists.
const targetsOf = (o) => o.schema.properties.attacks.items.properties.target.enum
const happy = (overrides = {}) => (p, o) => {
  if (overrides[o.label] !== undefined) return typeof overrides[o.label] === 'function' ? overrides[o.label](p, o) : overrides[o.label]
  if (o.label.startsWith('research:')) return { facts: [{ claim: 'c', evidence: 'x.rs:1' }], summary: 'sum' }
  if (o.label.startsWith('propose:')) return design(o.label.slice(8))
  if (o.label === 'attack:second-round') return { attacks: [] }
  if (o.label.startsWith('attack:')) {
    const by = o.label.slice(7)
    return { attacks: targetsOf(o).map((target) => ({ target, claim: `${by} on ${target}`, scenario: 'sc', evidence: 'ev', severity: 'major' })) }
  }
  if (o.label.startsWith('revise:')) {
    const stance = o.label.slice(7)
    // b answers only the first of its attacks, by number; the others count as unanswered.
    const ids = [...p.matchAll(/"id": (\d+)/g)].map((m) => Number(m[1]))
    const answered = stance === 'b' ? ids.slice(0, 1) : ids
    return { answers: answered.map((id) => ({ id, verdict: 'fixed', how: 'h' })), design: design(`${stance} revised`) }
  }
  if (o.label === 'judge') return verdict
  return null
}
const noAgent = () => assert.fail('no agent may run')

// 1. A topic is required, and stances a caller names are used as named or refused.
{
  const { result, calls } = await run({}, noAgent)
  assert.match(result.error, /a topic is required/)
  assert.equal(calls.length, 0)
  const wrong = [
    [{ key: 'a', focus: 'A' }],
    [{ key: 'a', focus: 'A' }, { key: 'a', focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: 'red-team', focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: 'B b', focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: 'b', focus: ' ' }],
    [{ key: 'a', focus: 'A' }, null],
    Array.from({ length: 7 }, (_, i) => ({ key: `s${i}`, focus: 'F' })),
    'a,b',
  ]
  for (const given of wrong) {
    const { result: refused, calls: none } = await run({ topic: 'T', stances: given }, noAgent)
    assert.match(refused.error, /stances must be/, JSON.stringify(given))
    assert.equal(none.length, 0)
  }
}

// 2. Every author attacks every rival and a red team attacks all; each revision gets exactly the
//    attacks on its own design and answers them by number; the unanswered ones reach the judge
//    themselves; each role runs on its model.
{
  const { result, calls, labels, prompts } = await run({ topic: 'T', stances }, happy())
  assert.deepEqual(labels.filter((c) => c.startsWith('attack:')).sort(), ['attack:a', 'attack:b', 'attack:c', 'attack:red-team', 'attack:second-round'])
  const targetsOfA = prompts['attack:a'].split('Designs to attack:')[1]
  assert.doesNotMatch(targetsOfA, /### a:/)
  assert.match(targetsOfA, /### b:[\s\S]*### c:/)
  assert.match(prompts['attack:a'].split('Designs to attack:')[0], /Your design:\n### a:/)
  assert.deepEqual(targetsOf(calls.find((o) => o.label === 'attack:a')), ['b', 'c'])
  assert.deepEqual(targetsOf(calls.find((o) => o.label === 'attack:red-team')), ['a', 'b', 'c'])
  assert.match(prompts['revise:b'], /"claim": "a on b"/)
  assert.match(prompts['revise:b'], /"claim": "c on b"/)
  assert.doesNotMatch(prompts['revise:b'], /"claim": "a on c"/)
  assert.match(prompts['revise:b'], /- a: a\. t\n- c: c\. t/)
  // a, c and the red team attack b; it answers one.
  assert.match(prompts['revise:b'], /"by": "red-team"/)
  const b = result.proposals.find((p) => p.stance === 'b')
  assert.deepEqual(b.unanswered.map((x) => x.claim).sort(), ['c on b', 'red-team on b'])
  assert.deepEqual(result.unanswered.map((x) => x.claim).sort(), ['c on b', 'red-team on b'])
  const judgeUnanswered = prompts.judge.split('left unanswered')[1].split('The second round')[0]
  assert.match(judgeUnanswered, /"claim": "c on b"[\s\S]*"claim": "red-team on b"/)
  assert.doesNotMatch(judgeUnanswered, /"claim": "a on b"/)
  assert.match(result.markdown, /Unanswered by their authors \(2\): #\d+ on b by c: c on b; #\d+ on b by red-team: red-team on b/)
  const models = Object.fromEntries(calls.map((o) => [o.label, `${o.model}/${o.effort}`]))
  for (const r of ['code', 'prior-art', 'failures']) assert.equal(models[`research:${r}`], 'sonnet/high')
  for (const s of ['a', 'b', 'c']) {
    assert.equal(models[`propose:${s}`], 'opus/high')
    assert.equal(models[`attack:${s}`], 'opus/high')
    assert.equal(models[`revise:${s}`], 'opus/high')
  }
  assert.equal(models['attack:red-team'], 'opus/high')
  assert.equal(models['attack:second-round'], 'opus/high')
  assert.equal(models.judge, 'fable/high')
  assert.equal(result.judge, 'fable')
  assert.match(result.markdown, /^# S/)
  assert.match(result.markdown, /x\.rs:1 debt/)
  assert.deepEqual(result.gaps, [])
}

// 3. An answer by a number that is not one of the design's attacks answers nothing.
{
  const { result } = await run({ topic: 'T', stances }, happy({
    'revise:a': (p) => ({ answers: [{ id: 999, verdict: 'fixed', how: 'h' }], design: design('a revised') }),
  }))
  const a = result.proposals.find((p) => p.stance === 'a')
  assert.deepEqual(a.answers, [])
  assert.equal(a.unanswered.length, 3)
}

// 4. A refused proposal is a named gap, an attack on it is left out with a gap, and fewer than two
//    proposals stop the run.
{
  const onAll = (by) => () => ({ attacks: stances.filter((s) => s.key !== by).map((s) => ({ target: s.key, claim: `${by} on ${s.key}`, scenario: 'sc', evidence: 'ev', severity: 'major' })) })
  const { result } = await run({ topic: 'T', stances }, happy({ 'propose:c': null, 'attack:a': onAll('a') }))
  assert.ok(result.gaps.includes('proposal c: no result (refused by a usage limit, skipped, or failed after retries)'))
  assert.ok(result.gaps.includes('attacks by a: 1 named no design on the table and were left out'))
  assert.equal(result.proposals.length, 2)
  assert.ok(result.attacks.every((x) => x.target !== 'c'))
  const { result: two } = await run({ topic: 'T', stances }, happy({ 'propose:b': null, 'propose:c': null }))
  assert.match(two.error, /fewer than two proposals/)
}

// 5. A refused revision keeps the first design with its attacks unanswered; a refused attacker or
//    researcher is a named gap; a refused fable judge falls to opus and says so once opus has
//    judged; no judge at all returns the material, not an empty verdict.
{
  const { result, calls } = await run({ topic: 'T', stances }, happy({ 'revise:a': null, 'attack:c': null, 'research:failures': null, judge: null, 'judge:opus': verdict }))
  const a = result.proposals.find((p) => p.stance === 'a')
  assert.equal(a.design.title, 'a')
  assert.equal(a.unanswered.length, 2)
  assert.ok(result.gaps.includes('revision of a: no result (refused by a usage limit, skipped, or failed after retries); its first design stands with its attacks unanswered'))
  assert.ok(result.gaps.includes('attacks by c: no result (refused by a usage limit, skipped, or failed after retries)'))
  assert.ok(result.gaps.includes('research failures: no result (refused by a usage limit, skipped, or failed after retries)'))
  assert.ok(result.gaps.includes('judged on opus after the fable judge returned no result (refused by a usage limit, skipped, or failed after retries)'))
  assert.equal(calls.find((o) => o.label === 'judge:opus').model, 'opus')
  assert.equal(result.judge, 'opus')
  assert.match(result.markdown, /Gaps in this run/)
  assert.match(result.markdown, /Judged on opus/)
  const { result: none } = await run({ topic: 'T', stances }, happy({ judge: null, 'judge:opus': () => { throw new Error('budget spent') } }))
  assert.match(none.error, /no judge \(budget spent\)/)
  assert.ok(none.gaps.some((g) => g.startsWith('no judge: fable returned no result')))
  assert.ok(!none.gaps.some((g) => g.startsWith('judged on opus')))
  assert.equal(none.proposals.length, 3)
}

// 6. The second round's attacks reach the judge and the result; a refused second round is a gap.
{
  const late = { target: 'b', claim: 'the fix of b leaks', scenario: 'sc', evidence: 'ev', severity: 'fatal' }
  const { result, calls, prompts } = await run({ topic: 'T', stances }, happy({ 'attack:second-round': { attacks: [late] } }))
  assert.deepEqual(targetsOf(calls.find((o) => o.label === 'attack:second-round')), ['a', 'b', 'c'])
  assert.match(prompts.judge.split('The second round')[1], /"claim": "the fix of b leaks"/)
  assert.deepEqual(result.second, [late])
  assert.match(result.markdown, /and 1 in the second/)
  const { result: refused } = await run({ topic: 'T', stances }, happy({ 'attack:second-round': null }))
  assert.ok(refused.gaps.includes('second round: no result (refused by a usage limit, skipped, or failed after retries)'))
  assert.deepEqual(refused.second, [])
}

// 7. Every list an agent returns is bounded, and scores name only designs on the table, in 0..100.
{
  const many = (n, make) => Array.from({ length: n }, (_, i) => make(i))
  const { result, prompts } = await run({ topic: 'T', stances }, happy({
    'research:code': { facts: many(35, (i) => ({ claim: `fact ${i}`, evidence: 'x.rs:1' })), summary: 'sum' },
    'attack:red-team': { attacks: many(15, (i) => ({ target: 'a', claim: `red ${i}`, scenario: 'sc', evidence: 'ev', severity: 'minor' })) },
    judge: { ...verdict, scores: [{ stance: 'a', score: 150, why: 'w' }, { stance: 'zzz', score: 10, why: 'w' }] },
  }))
  assert.equal(result.research.find((r) => r.key === 'code').facts.length, 30)
  assert.ok(result.gaps.includes('research code: 5 facts past the bound of 30 were left out'))
  assert.doesNotMatch(prompts['propose:a'], /fact 30/)
  assert.equal(result.attacks.filter((x) => x.by === 'red-team').length, 12)
  assert.ok(result.gaps.includes('attacks by red-team: 3 past the bound of 12 were left out'))
  assert.deepEqual(result.scores, [{ stance: 'a', score: 100, why: 'w' }])
}

// 8. Without stances the four default schools compete.
{
  const { result, labels } = await run({ topic: 'T' }, happy())
  assert.deepEqual(labels.filter((c) => c.startsWith('propose:')).sort(), ['propose:data-oriented', 'propose:incremental', 'propose:lineage', 'propose:minimal'])
  assert.equal(result.proposals.length, 4)
  assert.deepEqual(result.gaps, [])
}

// 9. No research at all stops the run with the reasons.
{
  const { result } = await run({ topic: 'T', stances }, (p, o) => (o.label.startsWith('research:') ? null : assert.fail(o.label)))
  assert.match(result.error, /no research returned/)
}
console.log('all branches behave')
