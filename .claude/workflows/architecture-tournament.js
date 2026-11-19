export const meta = {
  name: 'architecture-tournament',
  description: 'Competing architectures for one problem: research, rival proposals that tear each other apart, revisions, a red team, and a judge who synthesizes the strongest design',
  whenToUse: 'A scaling problem, a debt or a design question with a measurable target, before a spec design.md is written',
  phases: [
    { title: 'Research', detail: 'the code as it is, prior art pinned to commits, and the failures prior art records' },
    { title: 'Propose', detail: 'one design per stance, each with Rust shapes, predictions and a migration' },
    { title: 'Tear apart', detail: 'every author attacks the others; a red team attacks all' },
    { title: 'Revise', detail: 'each author answers every attack and revises' },
    { title: 'Judge', detail: 'score against the drivers, synthesize, name the experiments and the debt found' },
  ],
}

// Its branches are checked by `node .claude/workflows/architecture-tournament.test.mjs`.
//
// args:
//   topic     the problem: what hurts, the measured facts with their evidence, the target (required)
//   paths     the code areas the problem lives in, as repository paths
//   drivers   the forces a design must satisfy, one sentence each
//             (paths and drivers: left out, null, or a non-empty list of non-empty strings)
//   stances   optional [{ key, focus }], two to six: the schools the proposals come from (default:
//             four); a key is lower-case words joined by dashes, unique, and not "red-team"
//   checkout  the checkout to read (default, also for an empty or null value: the session's working
//             directory)
const opts = args || {}
const gaps = []
// The gaps that are agents which returned nothing, kept apart from the gaps a design's own output
// causes (a misnumbered answer, a list past its bound), so a later prompt can excuse only these.
const failed = []
const agentFailed = (line) => { gaps.push(line); failed.push(line) }
if (typeof opts.topic !== 'string' || !opts.topic.trim()) {
  return { error: 'a topic is required: the problem, its measured facts and the target' }
}
const checkout = opts.checkout === undefined || opts.checkout === null || opts.checkout === '' ? '.' : opts.checkout
// Every agent's brief quotes the checkout as one single-quoted word. QUOTABLE is review-before-merge's
// pattern for the same path: the word ends at a quote (in PowerShell also at U+2018 to U+201B), a
// line break or NUL. This script also refuses a blank path, which names nothing.
const QUOTABLE = /^[^'‘-‛\r\n\0]+$/
if (typeof checkout !== 'string' || !QUOTABLE.test(checkout) || !checkout.trim()) {
  return { error: 'checkout must be a path with no single quote, line break or NUL' }
}
// Paths and drivers a caller names are used as named or refused, like the stances: a default in
// their place would judge the designs against forces nobody named.
const listOfText = (value) => value === undefined || value === null || (Array.isArray(value) && value.length > 0 && value.every((x) => typeof x === 'string' && x.trim()))
if (!listOfText(opts.paths) || !listOfText(opts.drivers)) {
  return { error: 'paths and drivers must each be left out, null, or a non-empty list of non-empty strings' }
}
const paths = opts.paths || []
const drivers = opts.drivers || []

const DEFAULT_STANCES = [
  {
    key: 'data-oriented',
    focus: 'Data-oriented design: arenas and frame-scoped allocators, structure-of-arrays, generational indices, buffers reused across frames, no per-node heap objects on the hot path. Ask what the memory layout should be first and derive the API from it.',
  },
  {
    key: 'incremental',
    focus: 'Incremental computation: memoized derivations keyed by what they read (salsa, adapton, Jetpack Compose\'s slot table, Xilem\'s view diffing), structural sharing and persistent data, so unchanged work is neither redone nor reallocated.',
  },
  {
    key: 'lineage',
    focus: 'The strongest lineage: what Flutter, Compose, SwiftUI, GPUI and Chromium settled on after their own scaling work, read in their source at a pinned commit together with the regressions they had to fix, and adapted to idiomatic Rust rather than transliterated.',
  },
  {
    key: 'minimal',
    focus: 'The smallest change set that meets the target: remove the debt that causes the cost, reuse what FLUI already has, and add no new concept unless the numbers require it. Every new abstraction must pay for itself in the measured target.',
  },
]
// Stances a caller names are used as named or refused: falling back to the defaults would run a
// tournament nobody asked for.
const STANCE_KEY = /^[a-z][a-z0-9]*(-[a-z0-9]+)*$/
let stances = DEFAULT_STANCES
if (opts.stances !== undefined) {
  const given = opts.stances
  const keys = Array.isArray(given) ? given.map((s) => s && s.key) : []
  const wrong = !Array.isArray(given) || given.length < 2 || given.length > 6
    || given.some((s) => !s || typeof s.key !== 'string' || !STANCE_KEY.test(s.key) || s.key === 'red-team' || typeof s.focus !== 'string' || !s.focus.trim())
    || new Set(keys).size !== keys.length
  if (wrong) {
    return { error: 'stances must be two to six { key, focus } entries with unique lower-case dashed keys other than "red-team" and a non-empty focus' }
  }
  stances = given
}
const stanceKeys = stances.map((s) => s.key)

// A list an agent returns that a later prompt re-embeds is bounded, and what is left out is a gap:
// facts, a design's debt, each round's attacks, and answers (one per attack).
const MAX_FACTS = 30
const MAX_ATTACKS = 12
const MAX_DEBT = 12

const dir = `'${checkout}'`
const brief = `The problem, with its measured facts and target:
${opts.topic}

Code areas: ${paths.length ? paths.join(', ') : 'find them'}.
Drivers a design must satisfy:
${drivers.length ? drivers.map((d) => `- ${d}`).join('\n') : '- AGENTS.md "Design stance", "Code for the next reader" and "Recurring defects"'}

The repository is FLUI, a declarative UI framework in Rust; read it in the checkout ${dir}. AGENTS.md is the process and its rules bind every design. Do not edit tracked files and do not build: other agents share this host. Another project's source may be read with \`gh\` or cloned shallowly into the gitignored \`.flutter/\` or \`.gpui/\`.`

// An agent asked for an empty string sometimes writes the quotes themselves (`""`); every string an
// agent returns is read through `blankQuotes`, so a `how` written as `""` is empty: a fixed or
// refuted answer that says nothing is not kept (a concession needs no reason).
function blankQuotes(value) {
  if (typeof value === 'string') return /^\s*(""|'')\s*$/.test(value) ? '' : value
  if (Array.isArray(value)) return value.map(blankQuotes)
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, blankQuotes(v)]))
  return value
}

// An agent refused, skipped or past the token target is a gap the result names, never an empty answer.
async function attempt(prompt, options) {
  try {
    const value = await agent(prompt, options)
    return value ? { value: blankQuotes(value) } : { error: 'no result (refused by a usage limit, skipped, or failed after retries)' }
  } catch (e) {
    return { error: errorText(e) }
  }
}

// An error's text reaches later prompts, so it is cut to a line's worth, by code point so a cut never
// splits a surrogate pair.
function errorText(e) {
  return Array.from(String(e && e.message ? e.message : e)).slice(0, 300).join('')
}

// A phase's own handling of an agent's value can throw on a shape the schema let through. Each
// parallel unit is contained: its throw is named as an agent that returned nothing, and the unit
// yields its fallback, so the run keeps the material the other units produced.
function contained(who, unit, fallback = () => null) {
  return async () => {
    try {
      return await unit()
    } catch (e) {
      agentFailed(`${who}: ${errorText(e)}`)
      return fallback()
    }
  }
}

const RESEARCH_SCHEMA = {
  type: 'object',
  properties: {
    facts: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          claim: { type: 'string' },
          evidence: { type: 'string', description: 'file:line at the checkout, or owner/repo@sha path:line, or an issue URL' },
        },
        required: ['claim', 'evidence'],
      },
    },
    summary: { type: 'string', description: 'what a designer must know from this research, in at most 400 words' },
  },
  required: ['facts', 'summary'],
}

const RESEARCH = [
  {
    key: 'code',
    task: 'Map the code as it is: where the cost or the debt comes from, line by line, with the measurements the repository already records (perf baselines, gaps entries, ADRs). Name every owner of the state involved and every path that allocates or copies. Name the debt you find on the way, with its file:line.',
  },
  {
    key: 'prior-art',
    task: 'Find how at least four other frameworks or engines solve the same problem (Flutter, Compose, SwiftUI or UIKit, GPUI, Xilem/Masonry, egui, Chromium, Servo, Skia, Parley/cosmic-text for text). Read their source pinned to a commit and cite path:line. Say what each one\'s mechanism costs per frame and why it was chosen.',
  },
  {
    key: 'failures',
    task: 'Find the failures those mechanisms are known for: issues, regressions, reverted changes, scaling limits and pathological inputs recorded in the trackers of Flutter, Compose, GPUI, Xilem, Chromium, Parley and their peers for this exact behaviour. A design written from a reference repeats its known bugs unless they are on its checklist.',
  },
]

phase('Research')
const research = await parallel(RESEARCH.map((r) => contained(`research ${r.key}`, async () => {
  const got = await attempt(`${brief}\n\nYour research task: ${r.task}\n\nReturn at most ${MAX_FACTS} facts with their evidence, and a summary a designer can act on.`, {
    phase: 'Research', label: `research:${r.key}`, schema: RESEARCH_SCHEMA, model: 'sonnet', effort: 'high',
  })
  if (got.error) agentFailed(`research ${r.key}: ${got.error}`)
  return got.error ? null : { key: r.key, summary: got.value.summary, facts: got.value.facts.slice(0, MAX_FACTS), dropped: Math.max(0, got.value.facts.length - MAX_FACTS) }
})))
const found = research.filter(Boolean)
if (!found.length) return { error: `no research returned (${gaps.join('; ')})`, gaps }
for (const r of found) if (r.dropped) gaps.push(`research ${r.key}: ${r.dropped} facts past the bound of ${MAX_FACTS} were left out`)
const dossier = found.map((r) => `## ${r.key}\n${r.summary}\n${r.facts.map((f) => `- ${f.claim} (${f.evidence})`).join('\n')}`).join('\n\n')
log(`research: ${found.map((r) => `${r.key} ${r.facts.length} facts`).join(', ')}`)

const DESIGN_SCHEMA = {
  type: 'object',
  properties: {
    title: { type: 'string' },
    thesis: { type: 'string', description: 'the idea in two sentences' },
    shapes: { type: 'string', description: 'the Rust types, traits and ownership it introduces or changes, as code where it helps' },
    flow: { type: 'string', description: 'what happens on the hot path, step by step, and who owns each piece of state' },
    prediction: { type: 'string', description: 'the measured target this design reaches, with the arithmetic that predicts it from the research facts' },
    migration: { type: 'string', description: 'the ordered changes, what each deletes, and how each is tested' },
    risks: { type: 'string', description: 'where it can fail: orders of events, failure paths, scaling limits, the prior-art failures it must avoid' },
    debt_removed: { type: 'array', items: { type: 'string' } },
  },
  required: ['title', 'thesis', 'shapes', 'flow', 'prediction', 'migration', 'risks', 'debt_removed'],
}

// A design's debt list is re-embedded in every later prompt.
function boundDesign(who, design) {
  if (design.debt_removed.length > MAX_DEBT) gaps.push(`${who}: ${design.debt_removed.length - MAX_DEBT} debt items past the bound of ${MAX_DEBT} were left out`)
  return { ...design, debt_removed: design.debt_removed.slice(0, MAX_DEBT) }
}

phase('Propose')
const proposals = (await parallel(stances.map((s) => contained(`proposal ${s.key}`, async () => {
  const got = await attempt(`${brief}\n\nResearch everyone shares:\n${dossier}\n\nYou design from one school: ${s.focus}\n\nPropose the best architecture you can for this problem from that school, aiming higher than the stated target where the numbers allow. Be concrete: Rust shapes, the hot path, the owner of each piece of state, the predicted numbers with their arithmetic, the migration and what it deletes. Other designers will try to tear your design apart; leave them nothing easy.`, {
    phase: 'Propose', label: `propose:${s.key}`, schema: DESIGN_SCHEMA, model: 'opus', effort: 'high',
  })
  if (got.error) agentFailed(`proposal ${s.key}: ${got.error}`)
  return got.error ? null : { stance: s.key, design: boundDesign(`proposal ${s.key}`, got.value) }
})))).filter(Boolean)
if (proposals.length < 2) return { error: `fewer than two proposals returned (${gaps.join('; ')})`, gaps, research: found, proposals }
const proposed = proposals.map((p) => p.stance)
const show = (p) => `### ${p.stance}: ${p.design.title}\nThesis: ${p.design.thesis}\nShapes:\n${p.design.shapes}\nFlow:\n${p.design.flow}\nPrediction: ${p.design.prediction}\nMigration:\n${p.design.migration}\nRisks: ${p.design.risks}\nDebt removed: ${p.design.debt_removed.join('; ')}`

// The target of an attack is one of the designs on the table, so an attack cannot miss its design
// by a spelling.
const attackSchema = (targets) => ({
  type: 'object',
  properties: {
    attacks: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          target: { type: 'string', enum: targets, description: 'the stance of the design attacked' },
          claim: { type: 'string', description: 'what is wrong, in one sentence' },
          scenario: { type: 'string', description: 'the input, order of events or scale that shows it, and the wrong result' },
          evidence: { type: 'string', description: 'the research fact, file:line or prior-art citation behind it' },
          severity: { type: 'string', enum: ['fatal', 'major', 'minor'] },
        },
        required: ['target', 'claim', 'scenario', 'evidence', 'severity'],
      },
    },
  },
  required: ['attacks'],
})

// Both rounds keep an attack only on a design on the table and only up to the bound, and name each
// drop as a gap.
function takeAttacks(who, attacks, targets) {
  const kept = attacks.filter((x) => targets.includes(x.target))
  if (kept.length < attacks.length) gaps.push(`${who}: ${attacks.length - kept.length} named no design on the table and were left out`)
  if (kept.length > MAX_ATTACKS) gaps.push(`${who}: ${kept.length - MAX_ATTACKS} past the bound of ${MAX_ATTACKS} were left out`)
  return kept.slice(0, MAX_ATTACKS)
}

phase('Tear apart')
const attackers = [
  ...proposals.map((p) => ({ key: p.stance, own: p })),
  { key: 'red-team', own: null },
]
const attackRuns = (await parallel(attackers.map((a) => contained(`attacks by ${a.key}`, async () => {
  const targets = proposals.filter((p) => p !== a.own)
  const role = a.own
    ? `You are the author of the "${a.key}" design below. Tear apart every rival design: find where its predicted numbers do not follow from the facts, where its state has two owners or none, where an order of events or a failure breaks it, where it repeats a failure prior art records, what debt it adds, and where your own design is simply better. Be harsh and exact; a vague attack counts for nothing.\n\nYour design:\n${show(a.own)}`
    : 'You are the red team. Attack every design below on what designers miss: the order of events and failures (a panic in code it does not own, a late or duplicated answer, a resource lost mid-frame, an unwind), scaling (10 000 nodes, a deep tree, a long text, many windows), memory held while idle, the migration that leaves two forms alive, and a prediction that does not follow from the facts.'
  const got = await attempt(`${brief}\n\nResearch everyone shares:\n${dossier}\n\n${role}\n\nDesigns to attack:\n${targets.map(show).join('\n\n')}\n\nReturn at most ${MAX_ATTACKS} attacks, the strongest first.`, {
    phase: 'Tear apart', label: `attack:${a.key}`, schema: attackSchema(targets.map((p) => p.stance)), model: 'opus', effort: 'high',
  })
  if (got.error) {
    agentFailed(`attacks by ${a.key}: ${got.error}`)
    return null
  }
  return { by: a.key, attacks: takeAttacks(`attacks by ${a.key}`, got.value.attacks, targets.map((p) => p.stance)) }
})))).filter(Boolean)
// Each attack gets a number its author answers by, so an answer cannot miss its attack by a wording.
const numbered = attackRuns.flatMap((r) => r.attacks.map((x) => ({ ...x, by: r.by }))).map((x, i) => ({ ...x, id: i + 1 }))
const attacksOn = (stance) => numbered.filter((x) => x.target === stance)

const REVISION_SCHEMA = {
  type: 'object',
  properties: {
    answers: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id: { type: 'integer', description: 'the number of the attack answered' },
          verdict: { type: 'string', enum: ['fixed', 'refuted', 'conceded'] },
          how: { type: 'string', description: 'the change that fixes it, the fact that refutes it, or what conceding costs' },
        },
        required: ['id', 'verdict', 'how'],
      },
    },
    design: DESIGN_SCHEMA,
  },
  required: ['answers', 'design'],
}

phase('Revise')
const rivalsOf = (p) => proposals.filter((q) => q !== p).map((q) => `- ${q.stance}: ${q.design.title}. ${q.design.thesis}`).join('\n')
const revised = await parallel(proposals.map((p) => contained(`revision of ${p.stance}`, async () => {
  const against = attacksOn(p.stance)
  const got = await attempt(`${brief}\n\nResearch everyone shares:\n${dossier}\n\nYour design:\n${show(p)}\n\nThe rival designs, in brief:\n${rivalsOf(p)}\n\nEvery attack on your design, each with its number (JSON):\n${JSON.stringify(against, null, 1)}\n\nAnswer each attack by its number: fixed (say how the design changes), refuted (cite the fact or line), or conceded (say what it costs). An unanswered attack counts as conceded. Then return the revised design; take any rival's idea that is better, and say so.`, {
    phase: 'Revise', label: `revise:${p.stance}`, schema: REVISION_SCHEMA, model: 'opus', effort: 'high',
  })
  if (got.error) {
    agentFailed(`revision of ${p.stance}: ${got.error}; its first design stands with its attacks unanswered`)
    return { stance: p.stance, design: p.design, answers: [], unanswered: against, failed: true }
  }
  // Each attack keeps one answer: its first concession, which needs no reason and stays open whatever
  // else the author says, else its first answer that says how. Every other answer is not kept.
  const answers = []
  for (const attack of against) {
    const named = got.value.answers.filter((x) => x.id === attack.id)
    const kept = named.find((x) => x.verdict === 'conceded') || named.find((x) => x.how.trim())
    if (kept) answers.push({ ...kept, attack })
  }
  const unmatched = got.value.answers.filter((x) => !against.some((y) => y.id === x.id)).length
  const dropped = got.value.answers.length - unmatched - answers.length
  if (unmatched) gaps.push(`revision of ${p.stance}: ${unmatched} answers named no attack on this design`)
  if (dropped) gaps.push(`revision of ${p.stance}: ${dropped} answers not kept (each attack keeps its first concession, else its first answer that says how)`)
  return { stance: p.stance, design: boundDesign(`revision of ${p.stance}`, got.value.design), answers, unanswered: against.filter((x) => !answers.some((y) => y.id === x.id)) }
}, () => ({ stance: p.stance, design: p.design, answers: [], unanswered: attacksOn(p.stance), failed: true }))))
// An attack its author leaves unanswered or concedes is open, and so is every attack on a design whose
// revision failed to run, named as such so the failure is not held against the design: the script
// carries all three to the judge and to the result, whatever the judge lists.
const unanswered = revised.flatMap((r) => r.unanswered.map((x) => ({ ...x, stance: r.stance, why: r.failed ? 'revision failed' : 'unanswered' })))
const conceded = revised.flatMap((r) => r.answers.filter((x) => x.verdict === 'conceded').map((x) => ({ ...x.attack, stance: r.stance, how: x.how })))
const open = [...unanswered, ...conceded.map((x) => ({ ...x, why: 'conceded' }))]
// Each answer travels with the attack it answers, so a reader can check it.
const showRevised = (r) => r.failed
  ? `${show(r)}\n(Its revision failed to run: this is its first design, and its attacks went unanswered because of that failure, not by its author.)`
  : `${show(r)}\nAnswers, each with the attack it answers (JSON):\n${JSON.stringify(r.answers, null, 1)}`
// The agents that returned nothing so far, so a reader does not take a missing revision or a missing
// round of attacks for a fault of a design; an author's own misnumbered or empty answers are not here.
const failures = () => (failed.length ? `\n\nAgents that returned nothing in this run, failures rather than faults of any design:\n${failed.map((x) => `- ${x}`).join('\n')}` : '')

// A second round on the revisions: a fix is where the next defect usually is.
const second = await attempt(`${brief}\n\nResearch everyone shares:\n${dossier}\n\nYou are the red team on the revised designs. For each, check whether each "fixed" or "refuted" answer holds and whether the revision broke something it did not break before. Attack only what is still wrong; return at most ${MAX_ATTACKS} attacks.${failures()}\n\n${revised.map(showRevised).join('\n\n')}`, {
  phase: 'Revise', label: 'second-round', schema: attackSchema(proposed), model: 'opus', effort: 'high',
})
if (second.error) agentFailed(`second round: ${second.error}`)
const secondAttacks = second.error ? [] : takeAttacks('second round', second.value.attacks, proposed)

const verdictSchema = {
  type: 'object',
  properties: {
    scores: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          stance: { type: 'string', enum: proposed },
          score: { type: 'integer', minimum: 0, maximum: 100, description: 'against the drivers and the target' },
          why: { type: 'string' },
        },
        required: ['stance', 'score', 'why'],
      },
    },
    synthesis: {
      type: 'object',
      properties: {
        title: { type: 'string' },
        thesis: { type: 'string' },
        grafts: { type: 'string', description: 'which idea came from which design, and why it beat the alternatives' },
        shapes: { type: 'string' },
        flow: { type: 'string' },
        prediction: { type: 'string' },
        migration: { type: 'string' },
        risks: { type: 'string' },
      },
      required: ['title', 'thesis', 'grafts', 'shapes', 'flow', 'prediction', 'migration', 'risks'],
    },
    experiments: { type: 'array', items: { type: 'string' }, description: 'measurements or prototypes that decide what the research could not, each with the number that decides it' },
    debt: { type: 'array', items: { type: 'string' }, description: 'technical debt found on the way, each with its file:line, whether or not the synthesis removes it' },
    unresolved: { type: 'array', items: { type: 'string' }, description: 'the open attacks and the second-round attacks the synthesis does not answer, and what each leaves open' },
  },
  required: ['scores', 'synthesis', 'experiments', 'debt', 'unresolved'],
}

phase('Judge')
const judgePrompt = `${brief}\n\nResearch:\n${dossier}\n\nThe revised designs, each with its answers to the attacks:\n${revised.map(showRevised).join('\n\n')}\n\nAttacks left open (JSON; \`why\` says whether the author left it unanswered or conceded it, or its revision failed to run; a conceded one carries what conceding costs in \`how\`):\n${JSON.stringify(open, null, 1)}\n\nThe second round of attacks on the revisions (JSON):\n${JSON.stringify(secondAttacks, null, 1)}${failures()}\n\nYou judge. Verify the claims that decide the outcome in the code and the cited sources; a prediction whose arithmetic does not follow from the facts scores as no prediction. Score each design against the drivers and the target. Then synthesize the strongest design: start from the best one and graft every better idea from the others, saying where each came from and why. Aim at the architecture that is right, not the one that is easiest to merge, within AGENTS.md's rules. Name the experiments that decide what research could not, the technical debt found on the way with its file:line, and in "unresolved" every open attack above and every second-round attack the synthesis does not answer.`
// The judge runs on the strongest model the account can start; a note is written once the outcome
// it reports is known.
const judgeOptions = { phase: 'Judge', label: 'judge', schema: verdictSchema, effort: 'high' }
let judge = 'fable'
let verdict = await attempt(judgePrompt, { ...judgeOptions, model: 'fable' })
if (verdict.error) {
  const fableError = verdict.error
  verdict = await attempt(judgePrompt, { ...judgeOptions, label: 'judge:opus', model: 'opus' })
  if (verdict.error) {
    gaps.push(`no judge: fable returned ${fableError}; opus returned ${verdict.error}`)
    return { error: `no judge (${verdict.error})`, gaps, research: found, proposals: revised, attacks: numbered, open, second: secondAttacks }
  }
  judge = 'opus'
  gaps.push(`judged on opus after the fable judge returned ${fableError}`)
}

const v = verdict.value
// One score per design: the first the judge gave it, in 0..100; a design scored twice or not at all
// is a gap.
const scores = []
for (const stance of proposed) {
  const given = v.scores.filter((x) => x.stance === stance)
  if (given.length !== 1) gaps.push(`judge: ${stance} scored ${given.length} times`)
  if (given.length) scores.push({ ...given[0], score: Math.max(0, Math.min(100, given[0].score)) })
}
const synthesis = v.synthesis
const markdown = `# ${synthesis.title}

${synthesis.thesis}

## Scores
${scores.map((x) => `- **${x.stance}** ${x.score}: ${x.why}`).join('\n')}

## Where each idea came from
${synthesis.grafts}

## Shapes
${synthesis.shapes}

## The hot path
${synthesis.flow}

## Prediction
${synthesis.prediction}

## Migration
${synthesis.migration}

## Risks
${synthesis.risks}

## Experiments that decide what research could not
${v.experiments.map((x) => `- ${x}`).join('\n')}

## Technical debt found on the way
${v.debt.map((x) => `- ${x}`).join('\n')}

## Attacks the synthesis leaves unresolved, by the judge
${v.unresolved.length ? v.unresolved.map((x) => `- ${x}`).join('\n') : 'None.'}

## Attacks left open (unanswered, conceded, or on a revision that failed), and the second round's attacks
${open.length || secondAttacks.length ? [
  ...open.map((x) => `- #${x.id} on ${x.stance} by ${x.by}, ${x.why}: ${x.claim}`),
  ...secondAttacks.map((x) => `- second round on ${x.target} (${x.severity}): ${x.claim}`),
].join('\n') : 'None.'}
${gaps.length ? `\n## Gaps in this run\n${gaps.map((x) => `- ${x}`).join('\n')}\n` : ''}
Judged on ${judge}; ${proposals.length} designs, ${numbered.length} attacks in the first round and ${secondAttacks.length} in the second.`

return { judge, gaps, scores, synthesis, experiments: v.experiments, debt: v.debt, unresolved: v.unresolved, open, markdown, research: found, proposals: revised, attacks: numbered, second: secondAttacks }
