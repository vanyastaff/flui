# ADR-0336: The Web's accessibility tree is a DOM mirror the host keeps itself

- **Status:** Accepted (2026-11-15)
- **Date:** 2026-11-15
- **Supersedes:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) decision 18 in part (that the Web's
  `PlatformAccessibility` consumes `TreeUpdate` through `accesskit_consumer`); the parallel ARIA DOM
  in the semantics host, its one element per node and its lazy placeholder stand
- **Related:** [ADR-0255](ADR-0255-mobile-and-web-hosts.md) decisions 16 and 17 (the host's DOM root
  and the textarea the semantics field doubles as), [ADR-0240](ADR-0240-semantics-identifier.md)
  (the identifier the mirror writes as `data-flui-id`), [ADR-0214](ADR-0214-selection-area-text-runs.md)
  (the text runs static-text selection maps to), [ADR-0204](ADR-0204-synthetic-semantics-nodes.md)
  (the announcer nodes, which reach the page as live regions)

## Context and problem statement

ADR-0255 decision 18 builds the Web's accessibility as a parallel ARIA DOM, and says the
`PlatformAccessibility` implementation consumes each `TreeUpdate` through `accesskit_consumer`.
The code does not: `flui-native`'s `accessibility::web` keeps the tree itself.
`mirror::Mirror::apply` turns each update into DOM operations, refusing one that names a node it
never received, `aria::describe` maps a node to its element, and `dom` runs the operations,
writing only what changed between two element specs. `accesskit_consumer` is not a dependency of
`flui-native` on wasm32. The record and the code disagree, which `AGENTS.md` ("ADR Policy") calls
a defect in one of them.

The mirror does several things per node that a consumer's tree does not give: it chooses the
element by what the browser must act on (an `<a href>` for a link with an address, an
`<input type="range">` for a slider a user can move, a native `<input>` or `<textarea>` for a
field the window's text input edits its store through), turns a scroll offset into a scroller
over an extent, maps static-text selection to text runs, and holds a live region's text until
the page has rendered it. Its headless Chrome suite (`tests/web_browser/accessibility.rs`) pins
each.

Which shape does the Web's accessibility follow: `accesskit_consumer`'s tree under the DOM, or the
mirror the host keeps?

## Decision drivers

- One record of the decision, and code that follows it.
- The DOM changes only where an update changed something: a screen reader re-reads what the page
  rewrites, and a browser recomputes its accessibility tree for every mutation.
- Per-node control of the element: the browser acts on native elements (a link's address, a range
  input a screen reader adjusts, a field the input method and the screen reader both edit).
- A refused update leaves the page as it was and asks the composition root for the whole tree.
- Small wasm: every dependency on wasm32 is downloaded by every user.
- The pure part runs on the Linux CI host; wasm32 is compiled by CI only.

## Considered options

1. The host keeps the mirror itself (`Mirror`, `aria::describe`, `dom`).
2. `accesskit_consumer`'s `Tree` under the DOM, its change handler writing the elements.
3. Wait for an AccessKit web adapter.

## Decision

Chosen option: 1, because it is the shape the code has and the tests pin, it gives each node the
element the browser must act on, and it costs no dependency on wasm32.

- **§1.** `accessibility::web` keeps its own tree of the nodes it received. `Mirror::apply` takes
  each `TreeUpdate` and returns the DOM operations it needs (`Op`), or refuses it (`Refused`),
  changing nothing; a refusal other than a lone subtree asks the composition root for the whole
  tree once. `accesskit_consumer` stays out of the Web host.
- **§2.** `aria::describe` maps one node to its `ElementSpec` (tag, role, attributes, text, value,
  box), and `dom` writes only the difference between the old and the new spec of an element.
- **§3.** The mirror stands in the semantics slot, the last of the hosts the window's root keeps
  after the canvas (ADR-0255 decision 16), so its order in the page holds whichever of the text
  input and accessibility adds its element first.

### Consequences

- Good, because the record states the code's shape, and the next change to the Web's accessibility
  starts from it.
- Good, because the DOM work per update is the diff between two specs, and the element of each node
  is chosen where the browser must act on it.
- Bad, because the host maintains a tree of its own that AccessKit's other adapters do not share: a
  property AccessKit adds reaches the Web only when `aria::describe` maps it.
- Neutral, because AccessKit's own computations the mirror does not need (a node's name from its
  `labelled_by`) are expressed in ARIA (`aria-labelledby`) and left to the browser.

### Confirmation

- Linux CI runs the pure part: `a_node_is_described_as_its_element`,
  `a_node_the_browser_must_act_on_is_its_native_element`, `a_text_field_is_a_native_field_of_its_kind`,
  `each_update_becomes_the_dom_operations_it_needs`,
  `an_update_that_cannot_stand_is_refused_and_changes_nothing` (`flui-native`).
- Headless Chrome runs the DOM: `the_tree_reaches_the_dom_once_the_placeholder_is_activated` (which
  also reads the placeholder and the semantics host in the last slot after the canvas),
  `the_window_keeps_its_canvas_and_hosts_in_order_and_takes_them_out` and the rest of
  `tests/web_browser/accessibility.rs`.
- `cargo tree -p flui-native --target wasm32-unknown-unknown` names no `accesskit_consumer`.
- A screen reader run (NVDA, VoiceOver, TalkBack) is a row of
  [`platform-parity.md`](../platform-parity.md) ("Tree and actions", Web).

## Pros and cons of the options

### 1. The host keeps the mirror

- Good, because each node's element is chosen for what the browser must do with it, and the DOM is
  written as a diff.
- Good, because a refused update is a value (`Refused`) the host answers by asking for the whole tree.
- Good, because it adds nothing to the wasm download.
- Bad, because the host carries a second tree beside the composition root's.

### 2. `accesskit_consumer` under the DOM

- Good, because AccessKit's own tree semantics (filtering, name computation, text ranges) come with it.
- Bad, because its change handler reports nodes, not the elements the browser must act on: the
  mapping to native elements, scrollers, run selection and live-region timing is written again on
  top of it, so the host keeps two trees instead of one.
- Bad, because its tree asserts what an update must hold and panics on one that does not, where
  the mirror refuses it; on wasm32 a panic aborts the instance, so no containment is possible.
- Bad, because it adds a dependency to every web download.

### 3. Wait for an AccessKit web adapter

- Bad, because AccessKit lists it as planned, with no date, while the Web needs a tree now.

## More information

- The mirror's rules and their tests are in `crates/flui-native/ARCHITECTURE.md` ("The web
  window's accessibility is a parallel ARIA DOM over the canvas").
- **Revisit when** AccessKit ships a web adapter that writes native elements: the mirror is then
  a second implementation of the same thing.
