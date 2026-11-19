// The branches of architecture-tournament.js, driven by stub agents in place of real ones:
// node .claude/workflows/architecture-tournament.test.mjs [path to the workflow]
import { readFileSync } from 'node:fs'
import assert from 'node:assert/strict'

const src = readFileSync(process.argv[2] ?? new URL('./architecture-tournament.js', import.meta.url), 'utf8')
  .replace(/^export const meta =/m, 'const meta =')

async function run(args, stub) {
  const calls = []
  const prompts = {}
  const agent = async (prompt, o) => { calls.push(o.label); prompts[o.label] = prompt; return stub(prompt, o) }
  const parallel = async (thunks) => Promise.all(thunks.map((t) => t().catch(() => null)))
  const fn = new Function('agent', 'parallel', 'log', 'phase', 'args', `return (async()=>{${src}})()`)
  const result = await fn(agent, parallel, () => {}, () => {}, args)
  return { result, calls, prompts }
}
const design = (title) => ({ title, thesis: 't', shapes: 's', flow: 'f', prediction: 'p', migration: 'm', risks: 'r', debt_removed: ['d'] })
const verdict = { scores: [{ stance: 'a', score: 80, why: 'w' }], synthesis: { title: 'S', thesis: 't', grafts: 'g', shapes: 's', flow: 'f', prediction: 'p', migration: 'm', risks: 'r' }, experiments: ['e'], debt: ['x.rs:1 debt'], unresolved: [] }
const stances = [{ key: 'a', focus: 'A' }, { key: 'b', focus: 'B' }, { key: 'c', focus: 'C' }]
const happy = (overrides = {}) => (p, o) => {
  if (overrides[o.label] !== undefined) return typeof overrides[o.label] === 'function' ? overrides[o.label](p, o) : overrides[o.label]
  if (o.label.startsWith('research:')) return { facts: [{ claim: 'c', evidence: 'x.rs:1' }], summary: 'sum' }
  if (o.label.startsWith('propose:')) return design(o.label.slice(8))
  if (o.label === 'attack:second-round') return { attacks: [] }
  if (o.label.startsWith('attack:')) {
    const by = o.label.slice(7)
    return { attacks: stances.filter((s) => s.key !== by).map((s) => ({ target: s.key, claim: `${by} on ${s.key}`, scenario: 'sc', evidence: 'ev', severity: 'major' })) }
  }
  if (o.label.startsWith('revise:')) {
    const stance = o.label.slice(7)
    // b answers only one of its attacks; the others count as unanswered.
    const claims = p.match(/"claim": "[^"]+"/g).map((c) => c.slice(10, -1))
    const answered = stance === 'b' ? claims.slice(0, 1) : claims
    return { answers: answered.map((attack) => ({ attack, verdict: 'fixed', how: 'h' })), design: design(`${stance} revised`) }
  }
  if (o.label === 'judge') return verdict
  return null
}

// 1. A topic is required.
{
  const { result, calls } = await run({}, () => assert.fail('no agent may run'))
  assert.match(result.error, /a topic is required/)
  assert.equal(calls.length, 0)
}

// 2. Every author attacks every rival and a red team attacks all; each revision gets exactly the
//    attacks on its own design; an unanswered attack is counted; the judge synthesizes.
{
  const { result, calls, prompts } = await run({ topic: 'T', stances }, happy())
  assert.deepEqual(calls.filter((c) => c.startsWith('attack:')).sort(), ['attack:a', 'attack:b', 'attack:c', 'attack:red-team', 'attack:second-round'])
  const targetsOfA = prompts['attack:a'].split('Designs to attack:')[1]
  assert.doesNotMatch(targetsOfA, /### a:/)
  assert.match(targetsOfA, /### b:[\s\S]*### c:/)
  assert.match(prompts['attack:a'].split('Designs to attack:')[0], /Your design:\n### a:/)
  assert.match(prompts['revise:b'], /"claim": "a on b"/)
  assert.match(prompts['revise:b'], /"claim": "c on b"/)
  assert.doesNotMatch(prompts['revise:b'], /"claim": "a on c"/)
  // a, c and the red team attack b; it answers one.
  assert.match(prompts['revise:b'], /"by": "red-team"/)
  assert.equal(result.proposals.find((p) => p.stance === 'b').unanswered, 2)
  assert.equal(result.judge, 'fable')
  assert.match(result.markdown, /^# S/)
  assert.match(result.markdown, /x\.rs:1 debt/)
  assert.deepEqual(result.gaps, [])
}

// 3. A refused proposal is a named gap, and fewer than two proposals stop the run.
{
  const { result } = await run({ topic: 'T', stances }, happy({ 'propose:c': null }))
  assert.ok(result.gaps.some((g) => g.startsWith('proposal c:')))
  assert.equal(result.proposals.length, 2)
  const { result: two } = await run({ topic: 'T', stances }, happy({ 'propose:b': null, 'propose:c': null }))
  assert.match(two.error, /fewer than two proposals/)
}

// 4. A refused revision keeps the first design with its attacks unanswered; a refused fable judge
//    falls to opus and says so; no judge at all returns the material, not an empty verdict.
{
  const { result } = await run({ topic: 'T', stances }, happy({ 'revise:a': null, judge: null, 'judge:opus': verdict }))
  const a = result.proposals.find((p) => p.stance === 'a')
  assert.equal(a.design.title, 'a')
  assert.equal(a.unanswered, 3)
  assert.equal(result.judge, 'opus')
  assert.ok(result.gaps.some((g) => g.startsWith('fable judge:')))
  assert.match(result.markdown, /Gaps in this run/)
  const { result: none } = await run({ topic: 'T', stances }, happy({ judge: null, 'judge:opus': () => { throw new Error('budget spent') } }))
  assert.match(none.error, /no judge \(budget spent\)/)
  assert.equal(none.proposals.length, 3)
}

// 5. No research at all stops the run with the reasons.
{
  const { result } = await run({ topic: 'T', stances }, (p, o) => (o.label.startsWith('research:') ? null : assert.fail(o.label)))
  assert.match(result.error, /no research returned/)
}
console.log('all branches behave')
