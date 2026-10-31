# ADR-0282: Links in selectable text hold their own runs

- **Status:** Accepted (2026-10-31)
- **Date:** 2026-10-31
- **Amended-by:** [ADR-0283](ADR-0283-overridden-spans-in-selectable-text-hold-their-own-runs.md) — §2 and
  §5: a selectable paragraph's overridden spans are regions too, a silenced one hidden.
- **Amends:** [ADR-0214](ADR-0214-selection-area-text-runs.md) §2, whose runs are published as
  the paragraph's children; for a paragraph with routed links they are now its regions' children.
- **Related:** [ADR-0204](ADR-0204-synthetic-semantics-nodes.md) (synthetic run nodes and their
  identities), [ADR-0206](ADR-0206-typed-semantics-attributes.md) (assembly decisions)

## Context and problem statement

A paragraph inside a `SelectionArea` publishes `TextRun` children so that a screen reader can
read it by character and word and move its selection (ADR-0214). A paragraph whose spans carry
routed links publishes a node per link instead, so each link keeps its own name and `Click`
handler (row `links_in_a_selection_area_keep_their_own_nodes`), and until now such a paragraph
published no runs at all: an assistive user could follow the links but could not select the
text, nor read it by character. The question is where a linked paragraph's runs live so that
both hold: every link stays a node of its own, and every character of the paragraph is
addressable by a (run, character) position the area's selection accepts.

`accesskit_consumer` gathers a range's runs through intermediate nodes (`text_node_filter`
excludes the node but keeps descending), so runs may sit under a `Link` node and still belong to
the selection of the `Document` above it. A node offers text ranges on its own only with the
`Label`, `Document` or text-input role.

## Decision drivers

- A link keeps one node, one name and one handler, exactly as outside a `SelectionArea`.
- One identity space per paragraph: run identities, the area's selection conversion
  (`document_selection`, run to text and byte) and stale-position refusal stay as ADR-0214 §4 has
  them.
- A run never straddles a link boundary, so a run's node is under exactly one region.
- Text that cannot be split that way keeps working as it does now rather than publishing a
  wrong tree.

## Considered options

1. Runs stay on the paragraph's `TextContent`; each link region and each region of text between
   links names the index range of runs it hosts, and publication lists those runs under it.
2. Each region carries a `TextContent` of its own.
3. Keep the runs off a linked paragraph (the state before this record).

## Decision

Chosen option: 1, because it puts runs under their links without a second identity space, so
the selection path does not change.

1. **Runs are cut at link edges.** `RenderParagraph` builds a selectable paragraph's runs with
   the byte offsets where the link changes between consecutive runs as extra cuts, beside the
   line ends, direction changes and 256-character limit of ADR-0214 §2.
2. **Regions host index ranges.** `SemanticsConfiguration::set_hosted_runs(Option<Range<usize>>)`
   records, on a paragraph's semantics region, the range of the paragraph's runs (indices into its
   `TextContent`) it holds. `RenderParagraph` gives each run to the region whose bytes contain it
   whole; a region's runs are one range. The paragraph keeps its `TextContent`, so run identities
   are still assigned once per paragraph by the owner (`assign_text_runs`).
3. **The assembly checks the ranges.** In debug builds the semantics owner asserts that the
   regions of a paragraph hold runs only when the paragraph declares text content, and that their
   ranges are disjoint, cover every run and come in document order
   (`debug_assert_hosted_runs` in `flui-rendering`'s semantics owner).
4. **Publication lists runs under their region.** `SemanticsTree` publishes a hosting region's
   runs as its children, from the parent paragraph's published identities in that range, when the
   paragraph's regions in the tree hold all its runs; the paragraph then publishes none of its
   own. The assembly rebuilds a paragraph's regions with it, so an update that changes which
   runs a region lists carries the region (row `a_clipped_link_leaves_its_runs_with_the_text`,
   live adapter). A region of text between links keeps its label and becomes a `Label` node,
   which offers text ranges itself, as static text with inline text boxes does in Chromium; a
   link keeps the `Link` role. The paragraph publishes no label of its own: its regions carry its
   reading.
5. **Hosting falls back, it does not break.** The regions host nothing and the paragraph keeps
   its runs, so the links stay nodes and the text stays selectable, when the regions' runs would
   not come in byte order (a link across a direction change reorders runs visually), when a run
   crosses a link edge (a link that starts inside a grapheme cluster), when a region publishes
   nothing (a stretch an ellipsis cuts), or when the assembly leaves a region out of the tree (a
   clip drops it).
6. **A linked paragraph is a merge boundary.** It does not merge its descendants: it keeps its
   links and text regions as nodes. It is a merge boundary (ADR-0214 §3), so an outer merging
   ancestor stops at it and its node keeps the text content and accepts `SetTextSelection` as
   ADR-0214 §3 says. Regions hold ranges of the runs as authored: a transform that withholds the
   text's runs leaves the ranges with nothing to publish.
7. **The web host follows the regions.** `flui-native`'s web mirror shows a hosting link's or
   region's runs as its element's text, and a link holding runs is named by that content. When
   the text keeps its runs (decision 5), its element shows them, and its regions show no text: a
   link is named by its words in `aria-label`.

### Consequences

- Good, because an assistive user selects across and inside links with the same positions as
  elsewhere, and reads a link by character.
- Good, because the run model, identity allocation and selection conversion are unchanged.
- Bad, because a linked paragraph publishes one more node per text region between links.
- Bad, because in the cases of decision 5 a link holds no runs: its text is selected and read
  by the paragraph's runs, not under the link.
- Neutral, because spans with accessibility overrides host their runs the same way (ADR-0283).

### Confirmation

- `links_in_selectable_text_hold_their_own_runs` (`flui-widgets`): each link's runs, the text
  between them under a `Label`, and an assistive selection from before the first link into the
  second read back as the selected text.
- `a_wrapped_link_holds_a_run_on_each_line`: a link wrapping across two lines holds a run per
  line, and a selection across them reads the link's whole text.
- `links_in_a_selection_area_keep_their_own_nodes`: names and handlers unchanged.
- `a_clipped_link_leaves_its_runs_with_the_text`: a clip drops a link, and the text keeps every
  run in a full tree and in an update to a live adapter, each run listed once.
- `a_link_inside_a_cluster_leaves_the_runs_with_the_text`: a link starting with a combining mark.
- `a_link_in_selectable_text_shows_its_runs`,
  `a_labelled_wrapper_over_selectable_text_keeps_its_text` and
  `a_text_that_kept_its_runs_shows_its_words_once` (`flui-native`, web mirror).
- `linked_text_under_a_merging_ancestor_keeps_its_runs` and
  `linked_text_too_large_to_place_withholds_its_runs`.
- The debug assertion of decision 3 runs in every debug test that lays out a linked paragraph.
- Native `TextPattern`, VoiceOver and Orca rows run on a host, as for ADR-0214.

## Pros and cons of the options

### 1. Regions host index ranges of the paragraph's runs

- Good, because one `TextContent` and one run identity space per paragraph remain.
- Good, because the selection path and its stale-position refusal need no change.
- Bad, because publication has to look at the parent's runs when it lists a region's children.

### 2. A `TextContent` per region

- Good, because each region publishes its runs like any other text.
- Bad, because a paragraph would own several identity spaces, and the area's conversion from a
  run to a paragraph byte would need a region step; a selection across regions would cross text
  owners inside one paragraph.

### 3. No runs on a linked paragraph

- Good, because it costs nothing.
- Bad, because linked text inside a `SelectionArea` cannot be selected or read by character with
  a screen reader, which a sighted user can do.

## More information

Spans with accessibility overrides host their runs the same way, with their override as the
reading ([ADR-0283](ADR-0283-overridden-spans-in-selectable-text-hold-their-own-runs.md)).
