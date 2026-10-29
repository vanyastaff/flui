# ADR-0251: A touchpad pan is a scroll-fed contact in the gesture arena

- **Status:** Accepted
- **Date:** 2026-10-21
- **Amends:** [ADR-0208](ADR-0208-gesture-ownership-table-and-composition.md) (the table's
  rows also decide a scroll-fed contact), [ADR-0217](ADR-0217-interaction-module-layers-and-pointer-assembly.md)
  §4 (what a phased `ScrollEvent` feeds)
- **Related:** [ADR-0209](ADR-0209-nested-scroll-connection.md) (the channel, unchanged),
  [ADR-0207](ADR-0207-host-input-boundary-and-timeline.md) (scroll latches)

## Context

A two-finger touchpad pan arrives as a phased `ScrollEvent` (`Began`, `Changed`, `Ended`, then
platform momentum), not as a `PanZoomEvent` (ADR-0217 §4). The binding latches the sequence to the
first scroll consumer on the hit path (`ScrollSequences`, `ScrollRoute`) and never offers it to the
arena. Two things follow:

- A `GestureDetector` with only `on_pan_*` or a drag slot gets nothing from a touchpad pan, on
  every backend.
- A `Scrollable` inside a draggable (a list in a swipe-to-dismiss pane, a canvas in a scrolling
  page) wins the touchpad pan by being innermost with a scroll target, while the same
  `Scrollable` has to earn a touch drag through ADR-0208's table. The two devices disagree about
  who owns the same motion.

An attempt to route `PanZoomEvent` to drag recognizers (#1811, removed before merge) fixed
neither: no backend sends a pan there.

## Decision

1. **Scope.** A *scroll-fed contact* is a phased, precise (`ScrollPrecision::Precise`) scroll
   sequence from `Began` to `Ended` or `Cancelled`. Wheel ticks (no phase) and the `Momentum*`
   phases keep today's path: the first scroll consumer owns them (ADR-0207 §4).

2. **Members.** At `Began` the binding collects, along the hit path at the focal point:
   - pointer targets that feed a drag-family recognizer and declare it
     (`PointerTarget::feeds_scroll_contacts`). A recognizer reaches the arena through its
     `Listener`'s pointer callbacks, so the binding cannot tell a feed from a raw `Listener` by
     looking at the recognizer: the declaration is on the target, made by `GestureDetector` for
     its pan and drag slots and never by the recognizer a `Scrollable` builds for itself, so a
     `Scrollable` is never its own competitor;
   - scroll targets that can move on their axis (extent above zero), each carrying a motion role
     (`Axial(axis)`) and standing for its `Scrollable`.
   Same-axis ancestors are not members (ADR-0208 §2 rule 3: the innermost claimant owns, the
   ancestor is a channel participant).

3. **Fast path.** One member left: it wins at `Began` with no buffering, which is the arena's
   ordinary single-member resolution. No drag recognizer on the path leaves every member out but
   a scroll target, so the sequence runs through `ScrollRoute` exactly as before this ADR.
   Touchpad scrolling inside a plain list pays nothing.

4. **The contact.** With two or more members the binding opens an arena contact of kind
   `Trackpad`, at the `Began` focal point, under a pointer id of its own: the latch's
   `InputSequenceId` with the top bit set, a range the binding reserves for the contacts it
   mints itself. The contact takes the sequence identity the pointer path mints at its `Down`,
   as any contact does. Each `Changed` delta
   moves its position by the finger displacement (the negated scroll delta: positive scroll
   offset grows as the fingers move up), in root logical pixels. Recognizers see ordinary
   `Down`, `Move` and `Up` samples, so slop, the ownership table, velocity tracking and
   `GestureEndReason` are the ones touch uses. There is no second rule set, and every row of
   `assert_ownership_table` runs for both a touch drag and a scroll-fed contact.

5. **Verdicts.**
   - A recognizer wins: it owns the contact from the verdict, with the samples it was sent. The
     latch's route is set to nothing, so no scroll consumer sees the sequence, and the
     `Momentum*` packets that follow are dropped: the recognizer's end already carried the
     release velocity.
   - A scroll target wins: its route becomes the latch's route (as a first claimant's does
     today). The packets buffered since `Began` are delivered to it in order, so the slop wait
     loses no motion, and later packets, momentum included, reach it as today.
   - Different axes are decided on the first delta that crosses slop; the table does the rest.

6. **Latency is the table's.** The buffered wait exists only while a drag recognizer and a scroll
   target really compete, and ends at slop, as it does for touch. A touchpad pan beside no
   recognizer, or with an unambiguous winner, is never buffered.

7. **Only feeds see the contact.** The binding builds the contact's `Down`, `Move` and `Up` and
   delivers them to the declaring targets alone. Raw `Listener` pointer callbacks keep receiving
   `ScrollEvent`s and no synthetic sample, and the cancel-on-hand-off of ADR-0208 rule 5 sends
   them none for this contact. Hover stays frozen for the latch's life.

8. **Lifetime.** The contact ends with the latch: `Ended` is its `Up`, `Cancelled` and an idle
   lapse (ADR-0209 §5) are its cancel. A replaced or withdrawn latch cancels it.

## Consequences and verification

- A pan-only `GestureDetector` is driven by a touchpad pan, and a `Scrollable` inside a draggable
  is ordered by the same table on both devices.
- `ScrollTarget` gains a motion role and an eligibility probe, and `PointerTarget` gains the
  declaration of §2; `PointerKindSet` is untouched, so a pen or touch default does not move.
- Tests, in `flui-interaction/tests/` as one module of `main.rs`: the ownership table parameterized
  by source (touch contact, scroll-fed contact); fast path (a lone scroll target never opens a
  contact); buffered replay on a scroll-target win; momentum dropped after a recognizer win;
  `Cancelled` and idle lapse end a contact with its cancel; wheel ticks unchanged.
- `docs/platform-parity.md` gets a row for the touchpad pan as a drag. It is ✅ only after a run on
  Windows and macOS with a precision touchpad, which this change cannot make from Linux.
- Built in order: binding admission and the contact, the scroll target as a member, the
  `GestureDetector` opt-in and widget tests.

## Alternatives rejected

- **A static rule instead of the arena** (a drag recognizer wins unless a scroll target of the
  same axis is on the path). It decides a free pan against a `Scrollable` by a fixed order the
  table does not give touch, and cannot split different axes by direction.
- **Routing the pan to `PanZoomEvent` recognizers.** No backend reports a pan there
  (ADR-0217 §4), so it never fires.
- **Delivering every scroll-fed contact through the arena.** It would hand a plain list's touchpad
  scroll to the drag path and lose the platform's momentum for no competitor.
