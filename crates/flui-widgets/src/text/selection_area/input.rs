//! A [`SelectionArea`](super::SelectionArea)'s pointer and keys: a press and
//! drag selects by character, a double press by word and a triple press by
//! text; Shift extends from the anchor; Shift with the arrows, Home and End
//! moves the moving end; select all, copy and dismiss answer their intents.
//!
//! Every position is found where the texts are laid out, in the reading order
//! the published document uses, so the pointer selects what an adapter reads.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_foundation::panic::FirstPanic;
use flui_interaction::{
    GestureRecognizer, KeyEventResult, LongPressGestureRecognizer, PointerKindSet,
    TapAndDragGestureRecognizer, TapGestureRecognizer,
};
use flui_objects::RenderParagraph;
use flui_painting::text_boundaries;
use flui_painting::typography::{LineMetrics, TextAffinity, TextPosition};
use flui_platform::TargetPlatform;
use flui_platform::keyboard::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_rendering::hit_testing::{HitTestBehavior, PointerEvent};
use flui_rendering::semantics::{DocumentPosition, DocumentSelection};
use flui_view::BoxedView;
use flui_view::prelude::*;

use super::{
    HandleSide, SelectableEntry, SelectionChangeCause, SelectionInput, SelectionModel,
    paragraph_beneath,
};
use crate::interaction::recognizer_attachment::RecognizerAttachment;
use crate::interaction::{
    Action, ActionOutcome, CopySelectionTextIntent, DismissIntent, GesturePolicy, Listener,
    SelectAllTextIntent,
};
use crate::support::command_modifiers;
use crate::text::editable_text::is_word_jump_modifier;

/// How far a press selects, and how far a drag from it moves at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Character,
    Word,
    Text,
}

impl Unit {
    /// The unit a press of `count` consecutive taps selects by.
    fn of_taps(count: u32) -> Self {
        match count {
            0 | 1 => Self::Character,
            2 => Self::Word,
            _ => Self::Text,
        }
    }
}

/// A drag in progress: the input moving it, the unit it selects by and the
/// unit it started on, which stays selected whichever way it goes.
#[derive(Debug, Clone)]
struct Drag {
    input: SelectionInput,
    unit: Unit,
    start: DocumentPosition,
    end: DocumentPosition,
}

/// What the keys remember about the moving end between presses, forgotten
/// whenever anything else changes the selection.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CaretMemory {
    /// The end sits at the end of its line, not the start of the next.
    upstream: bool,
    /// The horizontal place, in render-root coordinates, that consecutive
    /// line steps keep to across shorter lines.
    column: Option<f64>,
}

impl CaretMemory {
    /// What a line step that landed at `at` (a hit test at `column`)
    /// leaves: the column, and the hit's affinity, which places an end at
    /// a soft wrap on the line it ends.
    fn after_step(column: f64, at: TextPosition) -> Self {
        Self {
            upstream: at.affinity == TextAffinity::Upstream,
            column: Some(column),
        }
    }

    /// What a pointer selection whose moving end is `extent` leaves: the
    /// affinity of `hit` when the end is where the pointer hit, so an end
    /// dropped at a soft wrap stays on the line it ends.
    fn after_hit(extent: &DocumentPosition, hit: &Hit) -> Self {
        Self {
            upstream: *extent == hit.at && hit.affinity == TextAffinity::Upstream,
            column: None,
        }
    }
}

/// Where the pointer hit a text, and on which side of a soft wrap.
struct Hit {
    at: DocumentPosition,
    affinity: TextAffinity,
}

/// What the pointer is doing in the area.
#[derive(Debug, Default)]
pub(super) struct PointerState {
    drag: RefCell<Option<Drag>>,
    /// Whether Shift was held when the current contact went down.
    shift: Cell<bool>,
}

/// A registered text in reading order, with its plain text as of now and
/// the byte its laid-out text ends at: a line limit can show less than the
/// whole text, and the keys move only through what is shown.
pub(super) struct Ordered {
    pub(super) entry: Rc<SelectableEntry>,
    pub(super) text: String,
    pub(super) shown: usize,
}

impl SelectionModel {
    /// The presented texts in reading order.
    pub(super) fn ordered(&self) -> Vec<Ordered> {
        let entries = self.registry.entries.borrow().clone();
        self.reading_order(&entries)
            .into_iter()
            .map(|(index, shown)| {
                let text = entries[index].text.borrow().clone();
                Ordered {
                    entry: Rc::clone(&entries[index]),
                    shown: shown.unwrap_or(text.len()).min(text.len()),
                    text,
                }
            })
            .collect()
    }

    /// Runs `f` with the laid-out paragraph of `entry` and its transform to
    /// the render root; `None` before layout or while the pipeline is busy.
    fn with_paragraph<R>(
        &self,
        entry: &SelectableEntry,
        f: impl FnOnce(&RenderParagraph, &Matrix4) -> Option<R>,
    ) -> Option<R> {
        let pipeline = self.pipeline.borrow().clone()?;
        pipeline
            .try_with_layout(|owner| {
                let (node, paragraph) = paragraph_beneath(owner, &entry.subtree)?;
                if !paragraph.painter().has_layout() {
                    return None;
                }
                let to_root = owner.transform_to(node, owner.root_id()?)?;
                f(paragraph, &to_root)
            })
            .flatten()
    }

    /// The selection's `side` end in reading order, with its text: where
    /// the first (or last) text with selected bytes starts (or stops)
    /// them, the end the paragraph publishes for its handle. An end at a
    /// text's edge that selects nothing of it is the next text's.
    fn end(&self, ordered: &[Ordered], side: HandleSide) -> Option<(usize, DocumentPosition)> {
        let selection = self.selection.borrow().clone()?;
        let (covered, _) = self.covered(&selection)?;
        let mut parts = covered.iter().filter(|part| !part.range.is_empty());
        let part = match side {
            HandleSide::End => parts.next_back(),
            _ => parts.next(),
        }?;
        let byte = match side {
            HandleSide::End => part.range.end,
            _ => part.range.start,
        };
        let entry = Rc::clone(self.registry.entries.borrow().get(part.index)?);
        let index = ordered
            .iter()
            .position(|text| Rc::ptr_eq(&text.entry, &entry))?;
        Some((index, DocumentPosition::new(entry.anchor.clone(), byte)))
    }

    /// Where `side`'s end sits: the middle of its line, in render-root
    /// coordinates, and the line's height. The start sits on the line after
    /// it and the end on the line it ends, as the paragraph publishes them.
    fn end_line(&self, side: HandleSide) -> Option<(Offset<f64>, f64)> {
        let ordered = self.ordered();
        let (index, at) = self.end(&ordered, side)?;
        self.with_paragraph(&ordered[index].entry, |paragraph, to_root| {
            let painter = paragraph.painter();
            // The end sits where the highlight ends, on the current text's
            // grapheme boundaries.
            let text = painter
                .text()
                .map(flui_painting::typography::InlineSpan::to_plain_text)
                .unwrap_or_default();
            let byte = text_boundaries::grapheme_floor(&text, at.byte());
            let upstream = side == HandleSide::End;
            let position = if upstream {
                TextPosition::upstream(byte)
            } else {
                TextPosition::downstream(byte)
            };
            let caret = painter.get_offset_for_caret(position);
            let height = line_of(&painter.get_line_metrics(), byte, upstream)?.height;
            let (x, y) = to_root.transform_point(caret.dx, caret.dy + height / 2.0);
            Some((Offset::new(x, y), height))
        })
    }

    /// The height of the line `side`'s end sits on, while it is laid out.
    pub(super) fn end_line_height(&self, side: HandleSide) -> Option<f64> {
        self.end_line(side).map(|(_, height)| height)
    }

    /// A drag of `side`'s handle began with the contact at `global`: the
    /// end keeps the distance it had from the contact, so the handle does
    /// not jump under the finger.
    pub(super) fn handle_drag_started(&self, side: HandleSide, global: Offset<f64>) {
        let grip = self.end_line(side).map(|(middle, _)| middle - global);
        self.handles.grip.set(grip.map(|grip| (side, grip)));
    }

    /// `side`'s handle was dragged to `global`: its end moves to the
    /// position there, the other end stays, and an end never passes the
    /// other, so the handles keep their sides.
    pub(super) fn handle_dragged(&self, side: HandleSide, global: Offset<f64>) {
        let Some((gripped, grip)) = self.handles.grip.get() else {
            return;
        };
        if gripped != side {
            return;
        }
        let ordered = self.ordered();
        let other = match side {
            HandleSide::End => HandleSide::Start,
            _ => HandleSide::End,
        };
        let (Some(hit), Some((_, fixed))) = (
            self.position_at(&ordered, global + grip),
            self.end(&ordered, other),
        ) else {
            return;
        };
        let keeps_side = match side {
            HandleSide::End => Self::compare(&ordered, &fixed, &hit.at) == Ordering::Less,
            _ => Self::compare(&ordered, &hit.at, &fixed) == Ordering::Less,
        };
        if !keeps_side {
            return;
        }
        let caret = CaretMemory::after_hit(&hit.at, &hit);
        self.select_from_input(
            Some(DocumentSelection::new(fixed, hit.at)),
            caret,
            SelectionInput::Touch,
        );
    }

    /// A touch long press at `global`: select the word under it and show
    /// the handles. The same touch then extends the selection by words as
    /// it moves, keeping that word selected.
    fn long_pressed(self: &Rc<Self>, global: Offset<f64>) {
        let ordered = self.ordered();
        let Some(hit) = self.position_at(&ordered, global) else {
            return;
        };
        let (start, end) = self.unit_at(&ordered, &hit.at, Unit::Word);
        let caret = CaretMemory::after_hit(&end, &hit);
        let held = Drag {
            input: SelectionInput::Touch,
            unit: Unit::Word,
            start: start.clone(),
            end: end.clone(),
        };
        let word = DocumentSelection::new(start, end);
        let committed = self.registry.snap(&word);
        // A listener that panics still leaves the word selected, so its
        // handles are shown before the first failure is resumed.
        let mut failure = FirstPanic::new();
        self.select_contained(Some(word), caret, SelectionInput::Touch, &mut failure);
        // A listener that moved the focus away has taken the handles and the
        // rest of the press with it, and one that selected something else has
        // replaced the word they were for.
        let still_the_word = committed.is_some() && *self.selection.borrow() == committed;
        if still_the_word && self.focus.has_primary_focus() {
            *self.pointer.drag.borrow_mut() = Some(held);
            self.show_handles(&mut failure);
        }
        failure.resume();
    }

    /// The position nearest `global` (render-root coordinates): in the text
    /// under it, else in the nearest text, nearer by line first.
    fn position_at(&self, ordered: &[Ordered], global: Offset<f64>) -> Option<Hit> {
        ordered
            .iter()
            .filter_map(|text| {
                self.with_paragraph(&text.entry, |paragraph, to_root| {
                    let (x, y) = to_root.try_inverse()?.transform_point(global.dx, global.dy);
                    let size = paragraph.painter().size();
                    // Texts are ranked by distance in root coordinates, so
                    // a scaled ancestor does not shrink its texts' gaps.
                    let bounds =
                        to_root.transform_rect(&Rect::from_ltwh(0.0, 0.0, size.width, size.height));
                    let gap = |at: f64, low: f64, high: f64| {
                        if at < low {
                            low - at
                        } else if at > high {
                            at - high
                        } else {
                            0.0
                        }
                    };
                    let distance = (
                        gap(global.dy, bounds.top(), bounds.bottom()),
                        gap(global.dx, bounds.left(), bounds.right()),
                    );
                    let hit = paragraph
                        .painter()
                        .get_position_for_offset(Offset::new(x, y));
                    Some((
                        distance,
                        Hit {
                            at: DocumentPosition::new(text.entry.anchor.clone(), hit.offset),
                            affinity: hit.affinity,
                        },
                    ))
                })
            })
            .min_by(|(a, _), (b, _)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, at)| at)
    }

    /// The ends of the `unit` around `at`.
    fn unit_at(
        &self,
        ordered: &[Ordered],
        at: &DocumentPosition,
        unit: Unit,
    ) -> (DocumentPosition, DocumentPosition) {
        let within = |byte: usize| DocumentPosition::new(at.text().clone(), byte);
        let Some(text) = ordered.iter().find(|text| text.entry.anchor == *at.text()) else {
            return (at.clone(), at.clone());
        };
        match unit {
            Unit::Character => (at.clone(), at.clone()),
            Unit::Text => (within(0), within(text.text.len())),
            Unit::Word => self
                .with_paragraph(&text.entry, |paragraph, _| {
                    Some(
                        paragraph
                            .painter()
                            .get_word_boundary(TextPosition::downstream(at.byte())),
                    )
                })
                .map_or_else(
                    || (at.clone(), at.clone()),
                    |word| (within(word.start), within(word.end)),
                ),
        }
    }

    /// Where `a` and `b` fall in reading order.
    fn compare(ordered: &[Ordered], a: &DocumentPosition, b: &DocumentPosition) -> Ordering {
        let rank = |at: &DocumentPosition| {
            ordered
                .iter()
                .position(|text| text.entry.anchor == *at.text())
        };
        (rank(a), a.byte()).cmp(&(rank(b), b.byte()))
    }

    /// Takes `selection` from the user's pointer or keys: the area takes the
    /// focus, so its highlight is the active one, and the listener hears of
    /// the change through the area's writer. `caret` is what the keys keep
    /// about the new moving end.
    fn select_from_input(
        &self,
        selection: Option<DocumentSelection>,
        caret: CaretMemory,
        input: SelectionInput,
    ) {
        let mut failure = FirstPanic::new();
        self.select_contained(selection, caret, input, &mut failure);
        failure.resume();
    }

    /// [`Self::select_from_input`], holding a listener's panic in `failure`
    /// for the caller to resume once it has finished its own work.
    fn select_contained(
        &self,
        selection: Option<DocumentSelection>,
        caret: CaretMemory,
        input: SelectionInput,
        failure: &mut FirstPanic,
    ) {
        // A click that selects nothing still makes this area the target of
        // the keys, so select all and copy act on the area last clicked. A
        // blur listener that panics still lets the selection commit and be
        // reported.
        if !self.focus.has_primary_focus() {
            failure.contain(|| {
                let _outcome = self.focus.request_focus();
            });
        }
        let writer = self.writer.borrow().clone();
        if let Some(writer) = writer {
            failure.contain(|| {
                writer.write(|cx| {
                    self.commit(
                        cx,
                        selection.as_ref(),
                        SelectionChangeCause::User(input),
                        caret,
                    );
                });
            });
        }
    }

    /// A press at `global` began a drag by `unit`: select that unit and
    /// extend from it. With Shift held, the drag extends the current
    /// selection from its anchor instead.
    fn drag_started(&self, pressed: Offset<f64>, unit: Unit) {
        // A mouse, pen or trackpad selection shows no handles; a wake that
        // fails while they are taken away still lets the press select.
        let mut failure = FirstPanic::new();
        self.hide_handles(&mut failure);
        failure.contain(|| self.drag_selects(pressed, unit));
        failure.resume();
    }

    fn drag_selects(&self, pressed: Offset<f64>, unit: Unit) {
        let ordered = self.ordered();
        let anchor = self
            .selection
            .borrow()
            .as_ref()
            .filter(|_| self.pointer.shift.get())
            .map(|selection| selection.anchor().clone());
        if let Some(anchor) = anchor {
            *self.pointer.drag.borrow_mut() = Some(Drag {
                input: SelectionInput::Pointer,
                unit: Unit::Character,
                start: anchor.clone(),
                end: anchor,
            });
            return;
        }
        let Some(hit) = self.position_at(&ordered, pressed) else {
            return;
        };
        let (start, end) = self.unit_at(&ordered, &hit.at, unit);
        *self.pointer.drag.borrow_mut() = Some(Drag {
            input: SelectionInput::Pointer,
            unit,
            start: start.clone(),
            end: end.clone(),
        });
        let caret = CaretMemory::after_hit(&end, &hit);
        self.select_from_input(
            Some(DocumentSelection::new(start, end)),
            caret,
            SelectionInput::Pointer,
        );
    }

    /// The drag `input` moves went to `global`: select from its first unit
    /// to the unit there, keeping the first unit whichever way the drag went.
    fn drag_moved(&self, global: Offset<f64>, input: SelectionInput) {
        let drag = self.pointer.drag.borrow().clone();
        let Some(drag) = drag.filter(|drag| drag.input == input) else {
            return;
        };
        let ordered = self.ordered();
        let Some(hit) = self.position_at(&ordered, global) else {
            return;
        };
        let (start, end) = self.unit_at(&ordered, &hit.at, drag.unit);
        let selection = if Self::compare(&ordered, &start, &drag.start) == Ordering::Less {
            DocumentSelection::new(drag.end, start)
        } else {
            DocumentSelection::new(drag.start, end)
        };
        let caret = CaretMemory::after_hit(selection.focus(), &hit);
        self.select_from_input(Some(selection), caret, input);
    }

    /// The drag `input` moves ended; a drag by another input goes on.
    pub(super) fn drag_ended(&self, input: SelectionInput) {
        let mut drag = self.pointer.drag.borrow_mut();
        if drag.as_ref().is_some_and(|drag| drag.input == input) {
            *drag = None;
        }
    }

    /// A text the drag in progress started on left: the drag goes on from
    /// `kept`'s anchor, the end that stays put, by character; without a
    /// selection left, it ends. `held` says whether a position's text is
    /// still in the area.
    pub(super) fn rebase_drag(
        &self,
        held: impl Fn(&DocumentPosition) -> bool,
        kept: Option<&DocumentSelection>,
    ) {
        let mut drag = self.pointer.drag.borrow_mut();
        let Some(current) = drag.as_ref() else {
            return;
        };
        if held(&current.start) && held(&current.end) {
            return;
        }
        let input = current.input;
        *drag = kept.map(|kept| Drag {
            input,
            unit: Unit::Character,
            start: kept.anchor().clone(),
            end: kept.anchor().clone(),
        });
    }

    /// A press of `count` consecutive taps at `global` that did not drag: a
    /// single one clears the selection, a double one selects the word under
    /// it and a triple one the text. With Shift held, it extends the
    /// selection from its anchor.
    fn tapped(&self, global: Offset<f64>, count: u32, input: SelectionInput) {
        // A tap, by touch or not, hides the handles; a wake that fails while
        // they are taken away still lets the tap select.
        let mut failure = FirstPanic::new();
        self.hide_handles(&mut failure);
        failure.contain(|| self.tap_selects(global, count, input));
        failure.resume();
    }

    fn tap_selects(&self, global: Offset<f64>, count: u32, input: SelectionInput) {
        let ordered = self.ordered();
        let anchor = self
            .selection
            .borrow()
            .as_ref()
            .filter(|_| self.pointer.shift.get())
            .map(|selection| selection.anchor().clone());
        if let Some(anchor) = anchor {
            if let Some(hit) = self.position_at(&ordered, global) {
                let caret = CaretMemory::after_hit(&hit.at, &hit);
                self.select_from_input(Some(DocumentSelection::new(anchor, hit.at)), caret, input);
            }
            return;
        }
        let unit = Unit::of_taps(count);
        if unit == Unit::Character {
            self.select_from_input(None, CaretMemory::default(), input);
            return;
        }
        if let Some(hit) = self.position_at(&ordered, global) {
            let (start, end) = self.unit_at(&ordered, &hit.at, unit);
            let caret = CaretMemory::after_hit(&end, &hit);
            self.select_from_input(Some(DocumentSelection::new(start, end)), caret, input);
        }
    }

    /// Select every presented text whole.
    fn select_all(&self, cx: &mut EventCx<'_>) -> ActionOutcome {
        let ordered = self.ordered();
        let (Some(first), Some(last)) = (ordered.first(), ordered.last()) else {
            return ActionOutcome::NotPerformed;
        };
        let selection = DocumentSelection::new(
            DocumentPosition::new(first.entry.anchor.clone(), 0),
            DocumentPosition::new(last.entry.anchor.clone(), last.shown),
        );
        self.select(cx, Some(&selection), SelectionInput::Keyboard);
        ActionOutcome::Performed
    }

    /// Write the selected text to the clipboard.
    fn copy(&self) -> ActionOutcome {
        let clipboard = self.clipboard.borrow().clone();
        let selection = self.selection.borrow().clone();
        let (Some(clipboard), Some(selection)) = (clipboard, selection) else {
            return ActionOutcome::NotPerformed;
        };
        let Some((covered, _)) = self.covered(&selection) else {
            return ActionOutcome::NotPerformed;
        };
        let selected = self.joined(&covered);
        if selected.text.is_empty() {
            return ActionOutcome::NotPerformed;
        }
        clipboard.write_text(selected.text);
        ActionOutcome::Performed
    }

    /// Shift with an arrow, Home or End moves the selection's moving end: by
    /// character, by word with the platform's word modifier, to the line's
    /// edge, or a line up or down, crossing into the neighbouring text at an
    /// edge.
    pub(super) fn key(&self, cx: &mut EventCx<'_>, event: &KeyEvent) -> KeyEventResult {
        if event.state() != KeyState::Down {
            return KeyEventResult::Ignored;
        }
        let modifiers = command_modifiers(event.modifiers);
        if !modifiers.contains(Modifiers::SHIFT) {
            return KeyEventResult::Ignored;
        }
        let Some(selection) = self.selection.borrow().clone() else {
            return KeyEventResult::Ignored;
        };
        let by_word = is_word_jump_modifier(modifiers, TargetPlatform::current());
        let memory = self.caret.get();
        let mut next_memory = CaretMemory::default();
        let ordered = self.ordered();
        let Some(index) = ordered
            .iter()
            .position(|text| text.entry.anchor == *selection.focus().text())
        else {
            return KeyEventResult::Ignored;
        };
        // Only the shown text: a line limit's hidden tail is never stepped
        // through, and crossing happens at the shown end.
        let text = ordered[index]
            .text
            .get(..ordered[index].shown)
            .unwrap_or(&ordered[index].text);
        let byte = selection.focus().byte().min(text.len());
        let at = |index: usize, byte: usize| {
            DocumentPosition::new(ordered[index].entry.anchor.clone(), byte)
        };
        let focus = match &event.key {
            Key::Named(NamedKey::ArrowRight) if byte == text.len() => {
                (index + 1 < ordered.len()).then(|| at(index + 1, 0))
            }
            Key::Named(NamedKey::ArrowRight) if by_word => {
                Some(at(index, text_boundaries::next_word_start(text, byte)))
            }
            Key::Named(NamedKey::ArrowRight) => Some(at(
                index,
                text_boundaries::next_grapheme_boundary(text, byte),
            )),
            Key::Named(NamedKey::ArrowLeft) if byte == 0 => index
                .checked_sub(1)
                .map(|previous| at(previous, ordered[previous].shown)),
            Key::Named(NamedKey::ArrowLeft) if by_word => {
                Some(at(index, text_boundaries::previous_word_start(text, byte)))
            }
            Key::Named(NamedKey::ArrowLeft) => Some(at(
                index,
                text_boundaries::previous_grapheme_boundary(text, byte),
            )),
            Key::Named(NamedKey::Home) => self
                .line_edge(&ordered[index].entry, byte, false, memory.upstream)
                .map(|edge| at(index, edge)),
            Key::Named(NamedKey::End) => {
                next_memory.upstream = true;
                self.line_edge(&ordered[index].entry, byte, true, memory.upstream)
                    .map(|edge| at(index, edge))
            }
            Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) => {
                let down = matches!(event.key, Key::Named(NamedKey::ArrowDown));
                self.line_step(&ordered, index, byte, down, memory)
                    .map(|(focus, memory)| {
                        next_memory = memory;
                        focus
                    })
            }
            _ => return KeyEventResult::Ignored,
        };
        if let Some(focus) = focus {
            let selection = DocumentSelection::new(selection.anchor().clone(), focus);
            self.commit(
                cx,
                Some(&selection),
                SelectionChangeCause::User(SelectionInput::Keyboard),
                next_memory,
            );
        }
        KeyEventResult::Handled
    }

    /// The start or end of the line `byte` is on; `upstream` places a byte
    /// at a soft wrap on the line it ends.
    fn line_edge(
        &self,
        entry: &SelectableEntry,
        byte: usize,
        end: bool,
        upstream: bool,
    ) -> Option<usize> {
        self.with_paragraph(entry, |paragraph, _| {
            let lines = paragraph.painter().get_line_metrics();
            let line = line_of(&lines, byte, upstream)?;
            Some(if end {
                line.end_index
            } else {
                line.start_index
            })
        })
    }

    /// The position a line below (or above) `byte`, at the horizontal place
    /// the previous line steps kept to (else the caret's), crossing into the
    /// next (or previous) text from its last (or first) line; with what the
    /// step leaves the keys to remember.
    fn line_step(
        &self,
        ordered: &[Ordered],
        index: usize,
        byte: usize,
        down: bool,
        memory: CaretMemory,
    ) -> Option<(DocumentPosition, CaretMemory)> {
        let entry = &ordered[index].entry;
        // The horizontal place in root coordinates, and the next line
        // within this text, if any.
        let (column, within) = self.with_paragraph(entry, |paragraph, to_root| {
            let painter = paragraph.painter();
            let caret = painter.get_offset_for_caret(if memory.upstream {
                TextPosition::upstream(byte)
            } else {
                TextPosition::downstream(byte)
            });
            let (caret_x, caret_y) = to_root.transform_point(caret.dx, caret.dy);
            let column = memory.column.unwrap_or(caret_x);
            let (local_x, _) = to_root.try_inverse()?.transform_point(column, caret_y);
            let lines = painter.get_line_metrics();
            let line = line_of(&lines, byte, memory.upstream)?;
            let next = if down {
                lines.get(line.line_number + 1)
            } else {
                line.line_number
                    .checked_sub(1)
                    .and_then(|number| lines.get(number))
            };
            let within = next
                .map(|next| painter.get_position_for_offset(Offset::new(local_x, next.baseline)));
            Some((column, within))
        })?;
        if let Some(within) = within {
            return Some((
                DocumentPosition::new(entry.anchor.clone(), within.offset),
                CaretMemory::after_step(column, within),
            ));
        }
        let neighbour = if down {
            ordered.get(index + 1)?
        } else {
            ordered.get(index.checked_sub(1)?)?
        };
        self.with_paragraph(&neighbour.entry, |paragraph, to_root| {
            let painter = paragraph.painter();
            let lines = painter.get_line_metrics();
            let line = if down { lines.first()? } else { lines.last()? };
            let (_, baseline) = to_root.transform_point(0.0, line.baseline);
            let (x, y) = to_root.try_inverse()?.transform_point(column, baseline);
            let at = painter.get_position_for_offset(Offset::new(x, y));
            Some((
                DocumentPosition::new(neighbour.entry.anchor.clone(), at.offset),
                CaretMemory::after_step(column, at),
            ))
        })
    }
}

/// The line holding `byte`: the last line starting at or before it, or with
/// `upstream`, a line ending at it (the end of a soft-wrapped line).
fn line_of(lines: &[LineMetrics], byte: usize, upstream: bool) -> Option<&LineMetrics> {
    let ending = || {
        lines
            .iter()
            .find(|line| line.start_index < byte && line.end_index == byte)
    };
    upstream.then(ending).flatten().or_else(|| {
        lines
            .iter()
            .take_while(|line| line.start_index <= byte)
            .last()
            .or_else(|| lines.first())
    })
}

/// Answers the area's intents: select all, copy and dismiss.
pub(super) struct AreaAction(pub(super) Weak<SelectionModel>);

impl Action<SelectAllTextIntent> for AreaAction {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &SelectAllTextIntent) -> ActionOutcome {
        self.0
            .upgrade()
            .map_or(ActionOutcome::NotPerformed, |model| model.select_all(cx))
    }
}

impl Action<CopySelectionTextIntent> for AreaAction {
    fn is_enabled(&self, intent: &CopySelectionTextIntent) -> bool {
        *intent == CopySelectionTextIntent::Copy
            && self
                .0
                .upgrade()
                .is_some_and(|model| model.selection.borrow().is_some())
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &CopySelectionTextIntent) -> ActionOutcome {
        self.0
            .upgrade()
            .map_or(ActionOutcome::NotPerformed, |model| model.copy())
    }
}

impl Action<DismissIntent> for AreaAction {
    fn is_enabled(&self, _intent: &DismissIntent) -> bool {
        self.0
            .upgrade()
            .is_some_and(|model| model.selection.borrow().is_some())
    }

    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &DismissIntent) -> ActionOutcome {
        let Some(model) = self.0.upgrade() else {
            return ActionOutcome::NotPerformed;
        };
        model.select(cx, None, SelectionInput::Keyboard);
        ActionOutcome::Performed
    }
}

/// The area's pointer gestures over its content: a tap-and-drag for mouse,
/// pen and trackpad, and a tap for touch, which clears. Touch selection
/// comes with the handles.
#[derive(Clone)]
pub(super) struct SelectionGestures {
    pub(super) model: Rc<SelectionModel>,
    pub(super) child: BoxedView,
}

impl std::fmt::Debug for SelectionGestures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionGestures").finish_non_exhaustive()
    }
}

impl View for SelectionGestures {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for SelectionGestures {
    type State = SelectionGesturesState;

    fn create_state(&self) -> Self::State {
        SelectionGesturesState {
            model: Rc::clone(&self.model),
            built_for: None,
            recognizers: None,
            tap_and_drag: Rc::new(RecognizerAttachment::default()),
            touch_tap: Rc::new(RecognizerAttachment::default()),
            touch_long_press: Rc::new(RecognizerAttachment::default()),
        }
    }
}

struct Recognizers {
    tap_and_drag: Rc<TapAndDragGestureRecognizer>,
    touch_tap: Rc<TapGestureRecognizer>,
    touch_long_press: Rc<LongPressGestureRecognizer>,
}

impl Recognizers {
    fn cancel(&self) {
        let _ = self.tap_and_drag.cancel();
        let _ = self.touch_tap.cancel();
        let _ = self.touch_long_press.cancel();
    }
}

#[doc(hidden)]
pub struct SelectionGesturesState {
    model: Rc<SelectionModel>,
    built_for: Option<GesturePolicy>,
    recognizers: Option<Recognizers>,
    tap_and_drag: Rc<RecognizerAttachment<TapAndDragGestureRecognizer>>,
    touch_tap: Rc<RecognizerAttachment<TapGestureRecognizer>>,
    touch_long_press: Rc<RecognizerAttachment<LongPressGestureRecognizer>>,
}

impl std::fmt::Debug for SelectionGesturesState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SelectionGesturesState")
            .finish_non_exhaustive()
    }
}

impl SelectionGesturesState {
    fn build_recognizers(&mut self, policy: GesturePolicy) {
        if self.built_for.as_ref() == Some(&policy) {
            return;
        }
        let GesturePolicy { arena, settings } = policy.clone();
        let weak = Rc::downgrade(&self.model);
        let tap_and_drag = {
            let (up, start, update, end, cancel) = (
                weak.clone(),
                weak.clone(),
                weak.clone(),
                weak.clone(),
                weak.clone(),
            );
            let pressed: Rc<Cell<Option<Offset<f64>>>> = Rc::new(Cell::new(None));
            let (pressed_down, pressed_start) = (Rc::clone(&pressed), Rc::clone(&pressed));
            TapAndDragGestureRecognizer::builder(arena.clone())
                .settings(settings.clone())
                .allowed_devices(
                    PointerKindSet::MOUSE
                        .union(PointerKindSet::PEN)
                        .union(PointerKindSet::TRACKPAD)
                        .union(PointerKindSet::VIRTUAL),
                )
                .on_tap_down(move |details| {
                    pressed_down.set(Some(details.global_position));
                })
                .on_tap_up(move |details| {
                    if let Some(model) = up.upgrade() {
                        model.tapped(
                            details.global_position,
                            details.consecutive_tap_count,
                            SelectionInput::Pointer,
                        );
                    }
                })
                .on_drag_start(move |details| {
                    if let Some(model) = start.upgrade() {
                        let at = pressed_start.get().unwrap_or(details.global_position);
                        model.drag_started(at, Unit::of_taps(details.consecutive_tap_count));
                        model.drag_moved(details.global_position, SelectionInput::Pointer);
                    }
                })
                .on_drag_update(move |details| {
                    if let Some(model) = update.upgrade() {
                        model.drag_moved(details.global_position, SelectionInput::Pointer);
                    }
                })
                .on_drag_end(move |details| {
                    if let Some(model) = end.upgrade() {
                        // The release can lie past the last update, which
                        // smoothing may also have moved.
                        model.drag_moved(details.global_position, SelectionInput::Pointer);
                        model.drag_ended(SelectionInput::Pointer);
                    }
                })
                .on_cancel(move || {
                    if let Some(model) = cancel.upgrade() {
                        model.drag_ended(SelectionInput::Pointer);
                    }
                })
                .build()
        };
        let touch_tap = {
            let tapped = weak.clone();
            TapGestureRecognizer::builder(arena.clone())
                .settings(settings.clone())
                .allowed_devices(PointerKindSet::TOUCH)
                .on_tap_up(move |details| {
                    if let Some(model) = tapped.upgrade() {
                        model.tapped(details.global_position, 1, SelectionInput::Touch);
                    }
                })
                .build()
        };
        let touch_long_press = {
            let (pressed, moved, released, cancelled) =
                (weak.clone(), weak.clone(), weak.clone(), weak);
            LongPressGestureRecognizer::builder(arena)
                .settings(settings)
                .allowed_devices(PointerKindSet::TOUCH)
                .on_long_press_start(move |details| {
                    if let Some(model) = pressed.upgrade() {
                        model.long_pressed(details.global_position);
                    }
                })
                .on_long_press_move_update(move |details| {
                    if let Some(model) = moved.upgrade() {
                        model.drag_moved(details.global_position, SelectionInput::Touch);
                    }
                })
                .on_long_press_end(move |details| {
                    if let Some(model) = released.upgrade() {
                        model.drag_moved(details.global_position, SelectionInput::Touch);
                        model.drag_ended(SelectionInput::Touch);
                    }
                })
                .on_long_press_cancel(move |_details| {
                    if let Some(model) = cancelled.upgrade() {
                        model.drag_ended(SelectionInput::Touch);
                    }
                })
                .build()
        };
        self.tap_and_drag.attach(&tap_and_drag);
        self.touch_tap.attach(&touch_tap);
        self.touch_long_press.attach(&touch_long_press);
        let outgoing = self.recognizers.replace(Recognizers {
            tap_and_drag,
            touch_tap,
            touch_long_press,
        });
        self.built_for = Some(policy);
        if let Some(outgoing) = outgoing {
            outgoing.cancel();
        }
    }
}

impl ViewState<SelectionGestures> for SelectionGesturesState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    /// Rebuilds the recognizers when the arena or its settings changed.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.build_recognizers(GesturePolicy::of(ctx));
    }

    fn dispose(&mut self) {
        self.tap_and_drag.clear();
        self.touch_tap.clear();
        self.touch_long_press.clear();
        if let Some(recognizers) = self.recognizers.take() {
            recognizers.cancel();
        }
    }

    fn build(&self, view: &SelectionGestures, _ctx: &dyn BuildContext) -> impl IntoView {
        let shift = Rc::downgrade(&view.model);
        // The whole area takes a press, not only its glyphs: a drag may
        // start in a margin.
        Listener::new()
            .behavior(HitTestBehavior::Translucent)
            .on_pointer_down(move |_cx, dispatch| {
                if let (Some(model), PointerEvent::Down(press)) = (shift.upgrade(), dispatch.local)
                {
                    model
                        .pointer
                        .shift
                        .set(press.modifiers.contains(Modifiers::SHIFT));
                }
            })
            .recognizer(&self.tap_and_drag)
            .recognizer(&self.touch_tap)
            .recognizer(&self.touch_long_press)
            .child(view.child.clone())
    }
}
