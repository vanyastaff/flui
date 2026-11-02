# ADR-0283: Overridden spans in selectable text hold their own runs

- **Status:** Accepted (2026-10-31)
- **Date:** 2026-10-31
- **Amends:** [ADR-0282](ADR-0282-links-in-selectable-text-hold-their-own-runs.md) §2, whose
  regions are a paragraph's links and the text between them, now also its overridden spans;
  and §5, whose "a region publishes nothing" no longer covers a span an override silences.
  [ADR-0214](ADR-0214-selection-area-text-runs.md)'s status, whose text with a span override
  "publishes no runs yet", now publishes them.
- **Related:** [ADR-0204](ADR-0204-synthetic-semantics-nodes.md) (synthetic run nodes)

## Context and problem statement

A span's accessibility override (`TextSpan::with_semantics_label`) replaces what assistive
technology reads for that span's own text, and an empty override silences it; the glyphs, the
shaping and the selection offsets stay the authored text. A paragraph inside a `SelectionArea`
publishes runs of its authored text so a screen reader can read it by character and select it
(ADR-0214). Until now a selectable paragraph with an override kept one label, its text with
each override in place, and published no runs, so an abbreviation read as its word could not be
selected by an assistive user at all. ADR-0282 gave a paragraph regions that each hold a range of
its runs. The question is how an override keeps deciding what is read while every authored
character stays addressable.

## Decision drivers

- An override is what an object read reads: the word for an abbreviation, nothing for a
  decorative glyph.
- Selection, copy and assistive positions work on authored bytes (ADR-0214 §4), so every run has
  one owner and every authored byte a position.
- One mechanism: regions holding ranges of one `TextContent` (ADR-0282 §2), not a second one.

## Considered options

1. Each overridden span is a region of its own that holds its authored runs under its override;
   a silenced span is a hidden region that holds them.
2. As 1, but a silenced span publishes no runs.
3. Keep overridden text on one label without runs (the state before this record).

## Decision

Chosen option: 1, because it keeps the override as the reading of an object and gives every
authored character an owner.

1. **Cuts and regions.** A selectable paragraph in which a span sets an override is read through
   regions, as a paragraph with routed links is. Its runs are also cut where an overridden span's
   own text starts or ends (`SpanRuns::region_edges`). Outside a link, each overridden span is a
   region of its own whose label is its override; the text between such spans and links stays a
   `Label` region. A link that contains an override stays one region: its name is its text with
   the override in place, and it holds the authored runs. An override covers its span's own text,
   not its children's, as in `InlineSpan::to_semantics_text`.
2. **A silenced span is hidden.** A span whose override is empty is a region with no label,
   marked `hidden`, that holds its runs: an object read skips it, and a selection across it covers
   its characters. Text silenced whole publishes nothing, as before.
3. **Reading model.** An object read reads a region's override; reading by character or line
   reads the runs, which are the authored text. This is how static text with inline text boxes
   reads in Chromium, whose `aria-label` names an object and leaves its text boxes as painted.
4. **Merge boundary.** Such a paragraph is a merge boundary, as a linked one is (ADR-0282 §6), so
   its regions keep their nodes inside an outer merge.
5. **Fallback.** ADR-0282 §5 applies unchanged otherwise: when runs leave byte order, cross a
   region edge, or lose a region to a clip, the paragraph keeps its runs. Text that is not
   selectable keeps one label with each override in place.

### Consequences

- Good, because an assistive user selects and reads by character text whose object reading an
  author overrode, and a sighted user's selection and an assistive one cover the same bytes.
- Good, because no new mechanism: overrides join links as regions holding run ranges.
- Bad, because reading by character reads a silenced span's authored characters: the override
  hides the object, not its text boxes.
- Neutral, because the web host shows a hidden region's element with `aria-hidden` and no text.

### Confirmation

- `span_overrides_read_their_override_and_hold_their_runs` (`flui-widgets`): an abbreviation
  read as its word holds its letters' runs, a silenced span is a hidden region holding its runs
  and a selection across it reports its characters, and a link with an override is named by the
  override and holds its authored runs.
- `a_silenced_text_is_left_out_of_the_selection`: text silenced whole still publishes nothing.
- The debug assertion of ADR-0282 §3 runs on every such paragraph.
- Native `TextPattern`, VoiceOver and Orca rows run on a host, as for ADR-0214.

## Pros and cons of the options

### 1. Overridden spans are regions; silenced ones hidden

- Good, because every run has an owner, so no assistive position is missing for an authored
  byte.
- Bad, because character reading does not honour a silenced span.

### 2. Silenced spans publish no runs

- Good, because character reading skips the silenced text too.
- Bad, because the silenced bytes have no position: a selection could not start or end inside
  them, and the runs before and after would no longer be one consecutive cover of the text.

### 3. One label without runs

- Good, because it costs nothing.
- Bad, because an assistive user cannot select overridden text, which a sighted user can.

## More information

Revisit if AccessKit gains a way to mark a run as excluded from character reading: a silenced
span would then keep its runs and skip them in reading.
