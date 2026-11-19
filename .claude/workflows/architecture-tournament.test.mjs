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
// The judge scores every design its schema lists.
const verdict = (p, o) => ({ scores: o.schema.properties.scores.items.properties.stance.enum.map((stance) => ({ stance, score: 80, why: 'w' })), synthesis: { title: 'S', thesis: 't', grafts: 'g', shapes: 's', flow: 'f', prediction: 'p', migration: 'm', risks: 'r' }, experiments: ['e'], debt: ['x.rs:1 debt'], unresolved: [] })
const stances = [{ key: 'a', focus: 'A' }, { key: 'b', focus: 'B' }, { key: 'c', focus: 'C' }]
// The designs an attacker may name are the ones its schema lists.
const targetsOf = (o) => o.schema.properties.attacks.items.properties.target.enum
const happy = (overrides = {}) => (p, o) => {
  if (overrides[o.label] !== undefined) return typeof overrides[o.label] === 'function' ? overrides[o.label](p, o) : overrides[o.label]
  if (o.label.startsWith('research:')) return { facts: [{ claim: 'c', evidence: 'x.rs:1' }], summary: 'sum' }
  if (o.label.startsWith('propose:')) return design(o.label.slice(8))
  if (o.label === 'second-round') return { attacks: [] }
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
  if (o.label === 'judge') return verdict(p, o)
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
    [{ key: 'a', focus: 'A' }, { key: 'a b', focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: 'b-', focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: 7, focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: ['b'], focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: ['red-team'], focus: 'B' }],
    [{ key: 'a', focus: 'A' }, { key: 'b', focus: 7 }],
    Array.from({ length: 7 }, (_, i) => ({ key: `s${i}`, focus: 'F' })),
    'a,b',
  ]
  for (const given of wrong) {
    const { result: refused, calls: none } = await run({ topic: 'T', stances: given }, noAgent)
    assert.match(refused.error, /stances must be/, JSON.stringify(given))
    assert.equal(none.length, 0)
  }
  for (const extra of [{ paths: 'crates/flui-layer' }, { drivers: 'stay under 2 ms' }, { drivers: ['ok', ' '] }, { paths: [7] }, { paths: [] }, { drivers: [] }, { paths: false }, { drivers: 0 }, { paths: '' }]) {
    const { result: refused, calls: none } = await run({ topic: 'T', ...extra }, noAgent)
    assert.match(refused.error, /paths and drivers must/, JSON.stringify(extra))
    assert.equal(none.length, 0)
  }
  // Null means left out, as for the checkout.
  {
    const { result: accepted, prompts } = await run({ topic: 'T', stances, paths: null, drivers: null }, happy())
    assert.equal(accepted.error, undefined)
    assert.match(prompts['research:code'], /Code areas: find them\.\nDrivers a design must satisfy:\n- AGENTS\.md "Design stance", "Code for the next reader" and "Recurring defects"\n/)
  }
  {
    const { prompts } = await run({ topic: 'T', stances, paths: ['crates/flui-layer'], drivers: ['stay under 2 ms'] }, happy())
    assert.match(prompts['research:code'], /Code areas: crates\/flui-layer\.\nDrivers a design must satisfy:\n- stay under 2 ms\n/)
  }
  for (const checkout of ["it's", 'O‘Neil', 'O‛Neil', 'a\nb', 'a\rb', 'a\0b', ' ', 5]) {
    const { result: refused, calls: none } = await run({ topic: 'T', checkout }, noAgent)
    assert.match(refused.error, /checkout must be/, JSON.stringify(checkout))
    assert.equal(none.length, 0)
  }
  // An empty or null checkout means the default, as in review-before-merge.
  for (const checkout of ['', null]) {
    const { result: accepted, prompts } = await run({ topic: 'T', stances, checkout }, happy())
    assert.equal(accepted.error, undefined)
    assert.match(prompts['research:code'], /in the checkout '\.'/)
  }
  for (const n of [2, 6]) {
    const given = Array.from({ length: n }, (_, i) => ({ key: `s${i}-x`, focus: 'F' }))
    const { result: accepted } = await run({ topic: 'T', stances: given }, happy())
    assert.equal(accepted.proposals.length, n)
    assert.deepEqual(accepted.gaps, [])
  }
}

// 2. Every author attacks every rival and a red team attacks all; each revision gets exactly the
//    attacks on its own design and answers them by number; the unanswered ones reach the judge
//    themselves; each role runs on its model.
{
  const { result, calls, labels, prompts } = await run({ topic: 'T', stances }, happy())
  assert.deepEqual(labels.filter((c) => c.startsWith('attack:')).sort(), ['attack:a', 'attack:b', 'attack:c', 'attack:red-team'])
  assert.ok(labels.includes('second-round'))
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
  assert.deepEqual(result.open.map((x) => [x.claim, x.why]).sort(), [['c on b', 'unanswered'], ['red-team on b', 'unanswered']])
  const judgeOpen = prompts.judge.split('Attacks left open (JSON')[1].split('The second round')[0]
  assert.match(judgeOpen, /"claim": "c on b"[\s\S]*"claim": "red-team on b"/)
  assert.doesNotMatch(judgeOpen, /"claim": "a on b"/)
  assert.match(result.markdown, /second round's attacks\n- #\d+ on b by c, unanswered: c on b\n- #\d+ on b by red-team, unanswered: red-team on b\n/)
  // The attack b answered travels with its answer to the second round and the judge.
  for (const label of ['second-round', 'judge']) {
    const revisedB = prompts[label].split('### b: b revised')[1].split('### c:')[0]
    assert.match(revisedB, /"verdict": "fixed"[\s\S]*"claim": "a on b"/, label)
  }
  const models = Object.fromEntries(calls.map((o) => [o.label, `${o.model}/${o.effort}`]))
  for (const r of ['code', 'prior-art', 'failures']) assert.equal(models[`research:${r}`], 'sonnet/high')
  for (const s of ['a', 'b', 'c']) {
    assert.equal(models[`propose:${s}`], 'opus/high')
    assert.equal(models[`attack:${s}`], 'opus/high')
    assert.equal(models[`revise:${s}`], 'opus/high')
  }
  assert.equal(models['attack:red-team'], 'opus/high')
  assert.equal(models['second-round'], 'opus/high')
  assert.equal(models.judge, 'fable/high')
  assert.equal(result.judge, 'fable')
  assert.match(result.markdown, /^# S/)
  assert.match(result.markdown, /x\.rs:1 debt/)
  assert.deepEqual(result.gaps, [])
  // A clean run tells neither the second round nor the judge of failures.
  for (const label of ['second-round', 'judge']) assert.doesNotMatch(prompts[label], /Agents that returned nothing/, label)
}

// 3. Each attack keeps one answer: its first concession, which needs no reason, else its first
//    answer that says how; an answer naming no attack on this design, or a blank `how` (spaces, or
//    a quoted empty string) is not kept. A conceded attack stays open and reaches the judge.
{
  let own = []
  const { result, prompts: misnumbered } = await run({ topic: 'T', stances }, happy({
    'revise:a': (p) => {
      own = [...p.matchAll(/"id": (\d+)/g)].map((m) => Number(m[1]))
      const foreign = [1, 2, 3, 4, 5, 6, 7, 8, 9].find((id) => !own.includes(id))
      return {
        answers: [
          { id: foreign, verdict: 'fixed', how: 'h' },
          { id: own[0], verdict: 'fixed', how: 'h' },
          { id: own[0], verdict: 'conceded', how: 'h2' },
          { id: own[1], verdict: 'refuted', how: '""' },
          { id: own[2], verdict: 'refuted', how: '   ' },
          { id: 999, verdict: 'fixed', how: 'h' },
        ],
        design: design('a revised'),
      }
    },
  }))
  const a = result.proposals.find((p) => p.stance === 'a')
  assert.equal(own.length, 3)
  // own[0] is answered fixed, then conceded: the concession holds and keeps it open.
  assert.deepEqual(a.answers.map((x) => [x.id, x.verdict]), [[own[0], 'conceded']])
  assert.deepEqual(a.unanswered.map((x) => x.id), own.slice(1))
  assert.deepEqual(result.open.filter((x) => x.stance === 'a').map((x) => [x.id, x.why]), [[own[1], 'unanswered'], [own[2], 'unanswered'], [own[0], 'conceded']])
  assert.ok(result.gaps.includes('revision of a: 2 answers named no attack on this design'))
  assert.ok(result.gaps.includes('revision of a: 3 answers not kept (each attack keeps its first concession, else its first answer that says how)'))
  // An author's own misnumbered or empty answers are its fault, not an agent failure the judge excuses.
  for (const label of ['second-round', 'judge']) assert.doesNotMatch(misnumbered[label], /Agents that returned nothing/, label)
  // In the other order the concession holds too, and the first of two concessions is kept.
  const { result: reversed } = await run({ topic: 'T', stances }, happy({
    'revise:a': (p) => {
      const first = Number(p.match(/"id": (\d+)/)[1])
      return { answers: [{ id: first, verdict: 'conceded', how: 'c' }, { id: first, verdict: 'fixed', how: 'h' }, { id: first, verdict: 'conceded', how: 'c2' }], design: design('a revised') }
    },
  }))
  assert.deepEqual(reversed.proposals.find((p) => p.stance === 'a').answers.map((x) => [x.verdict, x.how]), [['conceded', 'c']])
  // A concession that gives no reason still holds against a fixed answer.
  const { result: bare } = await run({ topic: 'T', stances }, happy({
    'revise:a': (p) => {
      const first = Number(p.match(/"id": (\d+)/)[1])
      return { answers: [{ id: first, verdict: 'fixed', how: 'h' }, { id: first, verdict: 'conceded', how: '""' }], design: design('a revised') }
    },
  }))
  assert.deepEqual(bare.proposals.find((p) => p.stance === 'a').answers.map((x) => [x.verdict, x.how]), [['conceded', '']])
  assert.ok(bare.open.some((x) => x.stance === 'a' && x.why === 'conceded'))
  // Without a concession the first answer that says how is kept, not a later one.
  const { result: firstReason } = await run({ topic: 'T', stances }, happy({
    'revise:a': (p) => {
      const first = Number(p.match(/"id": (\d+)/)[1])
      return { answers: [{ id: first, verdict: 'fixed', how: ' ' }, { id: first, verdict: 'fixed', how: 'first' }, { id: first, verdict: 'refuted', how: 'second' }], design: design('a revised') }
    },
  }))
  assert.deepEqual(firstReason.proposals.find((p) => p.stance === 'a').answers.map((x) => [x.verdict, x.how]), [['fixed', 'first']])

  let conceded = 0
  const { result: owned, prompts } = await run({ topic: 'T', stances }, happy({
    'revise:a': (p) => {
      const ids = [...p.matchAll(/"id": (\d+)/g)].map((m) => Number(m[1]))
      conceded = ids[0]
      return { answers: ids.map((id) => ({ id, verdict: id === conceded ? 'conceded' : 'fixed', how: id === conceded ? 'costs a second allocation' : 'h' })), design: design('a revised') }
    },
  }))
  const open = owned.open.find((x) => x.id === conceded)
  assert.equal(open.why, 'conceded')
  assert.equal(open.how, 'costs a second allocation')
  assert.match(prompts.judge.split('Attacks left open (JSON')[1].split('The second round')[0], new RegExp(`"id": ${conceded},[\\s\\S]*"why": "conceded"`))
  assert.match(owned.markdown, new RegExp(`- #${conceded} on a by [a-z-]+, conceded: `))
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
  assert.deepEqual(two.proposals.map((p) => p.stance), ['a'])
}

// 5. A refused revision keeps the first design with its attacks unanswered; a refused attacker or
//    researcher is a named gap; a refused fable judge falls to opus and says so once opus has
//    judged; no judge at all returns the material, not an empty verdict.
{
  const { result, calls, prompts } = await run({ topic: 'T', stances }, happy({ 'revise:a': null, 'attack:c': null, 'research:failures': null, judge: null, 'judge:opus': verdict, 'propose:b': { ...design('b'), debt_removed: Array.from({ length: 13 }, (_, i) => `d${i}`) } }))
  const a = result.proposals.find((p) => p.stance === 'a')
  assert.equal(a.design.title, 'a')
  assert.equal(a.unanswered.length, 2)
  // The failure is named as such, not held against the design's author.
  assert.deepEqual(result.open.filter((x) => x.stance === 'a').map((x) => x.why), ['revision failed', 'revision failed'])
  assert.match(result.markdown, /- #\d+ on a by [a-z-]+, revision failed: /)
  for (const label of ['second-round', 'judge:opus']) {
    assert.match(prompts[label].split('### a: a')[1].split('### b:')[0], /Its revision failed to run/, label)
    assert.match(prompts[label], /Agents that returned nothing in this run, failures rather than faults of any design:\n[\s\S]*- revision of a: no result/, label)
    // A design's own list past its bound is a gap, not an agent failure.
    assert.doesNotMatch(prompts[label].split('Agents that returned nothing')[1], /debt items past the bound/, label)
  }
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
  // A failed agent's error text is cut before it reaches a gap and the later prompts.
  const { result: long } = await run({ topic: 'T', stances }, happy({ 'research:prior-art': () => { throw new Error('x'.repeat(5000)) } }))
  assert.equal(long.gaps.find((g) => g.startsWith('research prior-art: ')).length, 'research prior-art: '.length + 300)
  assert.ok(none.gaps.some((g) => g.startsWith('no judge: fable returned no result')))
  assert.ok(!none.gaps.some((g) => g.startsWith('judged on opus')))
  assert.equal(none.proposals.length, 3)
  assert.deepEqual(none.open.map((x) => [x.claim, x.why]).sort(), [['c on b', 'unanswered'], ['red-team on b', 'unanswered']])
  assert.equal(none.attacks.length, 9)
  assert.deepEqual(none.second, [])
}

// 6. The second round's attacks reach the judge and the result; a refused second round is a gap.
{
  const late = { target: 'b', claim: 'the fix of b leaks', scenario: 'sc', evidence: 'ev', severity: 'fatal' }
  const { result, calls, prompts } = await run({ topic: 'T', stances }, happy({ 'second-round': { attacks: [late] } }))
  assert.deepEqual(targetsOf(calls.find((o) => o.label === 'second-round')), ['a', 'b', 'c'])
  assert.match(prompts.judge.split('The second round')[1], /"claim": "the fix of b leaks"/)
  assert.deepEqual(result.second, [late])
  assert.match(result.markdown, /and 1 in the second/)
  assert.match(result.markdown, /- second round on b \(fatal\): the fix of b leaks/)
  // With every first-round attack answered, the second round's attack is still listed.
  const answerAll = (p) => ({ answers: [...p.matchAll(/"id": (\d+)/g)].map((m) => ({ id: Number(m[1]), verdict: 'fixed', how: 'h' })), design: design('b revised') })
  const { result: closed } = await run({ topic: 'T', stances }, happy({ 'second-round': { attacks: [late] }, 'revise:b': answerAll }))
  assert.deepEqual(closed.open, [])
  assert.match(closed.markdown, /second round's attacks\n- second round on b \(fatal\): the fix of b leaks\n/)
  // The second round keeps the first round's target filter and bound, with their gaps.
  const many = [...Array.from({ length: 15 }, (_, i) => ({ ...late, claim: `late ${i}` })), { ...late, target: 'zzz' }]
  const { result: bounded } = await run({ topic: 'T', stances }, happy({ 'second-round': { attacks: many } }))
  assert.equal(bounded.second.length, 12)
  assert.ok(bounded.gaps.includes('second round: 1 named no design on the table and were left out'))
  assert.ok(bounded.gaps.includes('second round: 3 past the bound of 12 were left out'))
  const { result: refused } = await run({ topic: 'T', stances }, happy({ 'second-round': null }))
  assert.ok(refused.gaps.includes('second round: no result (refused by a usage limit, skipped, or failed after retries)'))
  assert.deepEqual(refused.second, [])
}

// 7. Facts, first-round attacks and a design's debt are bounded with a gap; each design gets one
//    score, the first the judge gave it, in 0..100, and a design scored twice or not at all is a gap.
{
  const many = (n, make) => Array.from({ length: n }, (_, i) => make(i))
  const { result, prompts } = await run({ topic: 'T', stances }, happy({
    'research:code': { facts: many(35, (i) => ({ claim: `fact ${i}`, evidence: 'x.rs:1' })), summary: 'sum' },
    'attack:red-team': { attacks: many(15, (i) => ({ target: 'a', claim: `red ${i}`, scenario: 'sc', evidence: 'ev', severity: 'minor' })) },
    'propose:a': { ...design('a'), debt_removed: many(15, (i) => `debt ${i}`) },
    'revise:b': () => ({ answers: [], design: { ...design('b revised'), debt_removed: many(14, (i) => `rdebt ${i}`) } }),
    judge: (p, o) => ({ ...verdict(p, o), scores: [{ stance: 'a', score: 150, why: 'w' }, { stance: 'a', score: 30, why: 'w2' }, { stance: 'b', score: -5, why: 'w' }, { stance: 'zzz', score: 10, why: 'w' }] }),
  }))
  assert.equal(result.research.find((r) => r.key === 'code').facts.length, 30)
  assert.ok(result.gaps.includes('research code: 5 facts past the bound of 30 were left out'))
  assert.doesNotMatch(prompts['propose:a'], /fact 30/)
  assert.equal(result.attacks.filter((x) => x.by === 'red-team').length, 12)
  assert.ok(result.gaps.includes('attacks by red-team: 3 past the bound of 12 were left out'))
  assert.ok(result.gaps.includes('proposal a: 3 debt items past the bound of 12 were left out'))
  assert.match(prompts['attack:b'], /debt 11/)
  assert.doesNotMatch(prompts['attack:b'], /debt 12/)
  assert.ok(result.gaps.includes('revision of b: 2 debt items past the bound of 12 were left out'))
  assert.match(prompts.judge, /rdebt 11/)
  assert.doesNotMatch(prompts.judge, /rdebt 12/)
  assert.deepEqual(result.scores, [{ stance: 'a', score: 100, why: 'w' }, { stance: 'b', score: 0, why: 'w' }])
  assert.ok(result.gaps.includes('judge: a scored 2 times'))
  assert.ok(result.gaps.includes('judge: c scored 0 times'))
}

// 8. Without stances the four default schools compete.
{
  const { result, labels } = await run({ topic: 'T' }, happy())
  assert.deepEqual(labels.filter((c) => c.startsWith('propose:')).sort(), ['propose:data-oriented', 'propose:incremental', 'propose:lineage', 'propose:minimal'])
  assert.equal(result.proposals.length, 4)
  assert.deepEqual(result.gaps, [])
  // Every attack answered and no second round: nothing is open.
  assert.match(result.markdown, /second round's attacks\nNone\.\n/)
}

// 9. No research at all stops the run with the reasons.
{
  const { result } = await run({ topic: 'T', stances }, (p, o) => (o.label.startsWith('research:') ? null : assert.fail(o.label)))
  assert.match(result.error, /no research returned/)
}
console.log('all branches behave')
