//! Obscured fields: the masked text a render object shows, and the mapping of
//! the offsets a pointer query returns from masked back to source bytes.

#[cfg(doc)]
use super::field_view::build_field_view;
#[cfg(doc)]
use crate::text::controller::TextEditingController;
#[cfg(doc)]
use flui_objects::RenderEditable;
use flui_painting::text_boundaries::graphemes;
use std::ops::Range;

use flui_foundation::geometry::Offset;
use flui_rendering::pipeline::PipelineCell;

use crate::text::text_store::with_editable_global;

/// The default mask character, U+2022 BULLET.
pub(super) const DEFAULT_OBSCURING_CHARACTER: char = '\u{2022}';

/// The masked text, and each of `offsets` mapped into it.
///
/// `offsets` is rewritten in place: every entry arrives as a byte offset into
/// `text` and leaves as the corresponding byte offset into the returned mask.
/// A slice rather than one value because the caret is never the only offset
/// that has to make the trip — the selection's two ends do too, and mapping
/// them separately meant three walks over the text where the mask itself only
/// needs one. It also keeps the correspondence in a single place: a selection
/// mapped by a different rule than the caret would paint a highlight that does
/// not line up with the caret inside it.
///
/// One mask character per SOURCE **extended grapheme cluster** — the
/// user-perceived character, not the Unicode scalar and not the UTF-16 code
/// unit.
///
/// **Why not UTF-16 code units.** Counting code units would turn a single emoji
/// into TWO bullets and a family-emoji ZWJ sequence into eleven. That is an
/// artifact of the string representation rather than a designed contract, and
/// it leaks: the bullet count tells an onlooker which keystrokes were astral.
///
/// **Why the grapheme.** `TextEditingController` moves and deletes by
/// grapheme cluster (see its "Character unit" doc), so the mask counts the
/// same unit: one Backspace removes one cluster and one bullet. Masking per
/// scalar while the caret stepped per cluster would put the two out of step —
/// a Backspace on a family emoji would remove five bullets at once — and the
/// caret would land between bullets that correspond to no boundary the
/// controller can produce.
///
/// Every offset in `offsets` sits at a grapheme boundary (the controller only
/// ever produces those), so "clusters strictly before it" is the count that
/// maps. Average and worst case O(clusters × offsets); `offsets` is three
/// entries at its largest, so this is the single walk the mask needs either
/// way.
pub(in crate::text) fn obscure(text: &str, offsets: &mut [usize], mask: char) -> String {
    let mask_len = mask.len_utf8();
    let mut masked = String::with_capacity(text.len());
    let mut mapped = vec![0_usize; offsets.len()];
    for cluster in graphemes(text) {
        for (slot, source) in mapped.iter_mut().zip(offsets.iter()) {
            if cluster.start < *source {
                *slot += mask_len;
            }
        }
        masked.push(mask);
    }
    offsets.copy_from_slice(&mapped);
    masked
}

/// The SOURCE byte offset a point in the root's coordinate space falls on,
/// or `None` when the field is not mounted, not laid out, or the point misses
/// it.
///
/// # Why the global point and not the listener's local one
///
/// `Listener` hands its callbacks the position in its own box as well as the
/// root's. The listener's box is not the editable's: `AnchoredBox` sits
/// between them, and relying on proxies being zero-offset is an unstated
/// coupling that a later wrapper would break silently. Mapping the root's
/// point through `transform_to` is what `EditableTextState::global_caret_rect`
/// already does in the other direction, and it stays correct whatever the
/// subtree grows.
///
/// # Why the answer is in SOURCE space
///
/// The render object holds masked text on an obscured field, so the offset it
/// returns is a masked one while [`TextEditingController`] holds source bytes.
/// The conversion happens here, at the same seam
/// [`build_field_view`] masks at, so the two directions cannot drift apart.
/// `source_text` must be the CONTROLLER's own source string (the caller's
/// job to supply — `RenderEditable::plain_text()` is the MASKED string on
/// an obscured field, the wrong input for
/// [`source_offset_for_masked_offset`], whose own doc spells out why: it
/// walks `source`'s grapheme clusters to answer a SOURCE byte offset, and
/// walking the masked string instead just answers back the masked offset
/// it was given, silently corrupting every obscured-field tap).
pub(super) fn source_offset_at_global(
    owner: &PipelineCell,
    inner_anchor: &flui_objects::SubtreeAnchor,
    global: Offset<f64>,
    obscuring: Option<char>,
    source_text: &str,
) -> Option<usize> {
    // A pointer event can land in a callback a frame phase happens to drive;
    // "the tree is busy" is a real answer here — the gesture does nothing for
    // that event — which is why the helper uses `try_with`.
    with_editable_global(owner, inner_anchor, |editable, to_root| {
        let (x, y) = to_root.try_inverse()?.transform_point(global.dx, global.dy);
        let masked = editable.byte_offset_for_local_offset(Offset::new(x, y))?;
        Some(match obscuring {
            Some(mask) => source_offset_for_masked_offset(source_text, masked, mask),
            None => masked,
        })
    })
}

/// The SOURCE byte range of the word under a point in the root's
/// coordinate space — the double-tap counterpart of
/// [`source_offset_at_global`]; see its doc for the coordinate mapping
/// and masked/source distinction, both shared verbatim here.
///
/// Delegates to [`RenderEditable::word_range_at_local_offset`]
/// (`flui-objects`), which itself delegates to
/// [`flui_painting::TextPainter::get_word_boundary`] — ICU4X word
/// segmentation over the laid-out text. This widget's own Ctrl/Alt+Arrow
/// word-jump (the controller stops at
/// [`flui_painting::text_boundaries::word_starts`]) walks the same ICU4X
/// segments but answers a different question: a
/// directional "next/previous stop" with its own asymmetric tie-break (see
/// the controller's `# Word unit` doc), where this one answers "which
/// segment is under this exact position". A double-tap and a keyboard
/// word-jump can therefore land on different boundaries of the same
/// segments, such as for a caret sitting exactly on a segment boundary.
///
/// `source_text` must be the CONTROLLER's own source string — see
/// [`source_offset_at_global`]'s doc for why `RenderEditable::plain_text()`
/// (the masked string on an obscured field) is the wrong input here too.
pub(super) fn source_word_range_at_global(
    owner: &PipelineCell,
    inner_anchor: &flui_objects::SubtreeAnchor,
    global: Offset<f64>,
    obscuring: Option<char>,
    source_text: &str,
) -> Option<Range<usize>> {
    with_editable_global(owner, inner_anchor, |editable, to_root| {
        let (x, y) = to_root.try_inverse()?.transform_point(global.dx, global.dy);
        let masked = editable.word_range_at_local_offset(Offset::new(x, y))?;
        Some(match obscuring {
            Some(mask) => {
                source_offset_for_masked_offset(source_text, masked.start, mask)
                    ..source_offset_for_masked_offset(source_text, masked.end, mask)
            }
            None => masked,
        })
    })
}

/// The source byte offset a masked byte offset corresponds to — the inverse
/// of [`obscure`]'s mapping.
///
/// Needed because [`build_field_view`] masks the text *before* it reaches the
/// render object, so every offset a pointer query returns is in MASKED byte
/// space while [`TextEditingController`] holds SOURCE bytes. Writing one into
/// the other is a silent corruption on any obscured field:
/// [`DEFAULT_OBSCURING_CHARACTER`] is `U+2022`, three bytes, so the two spaces
/// diverge at the very first character.
///
/// The correspondence is the one [`obscure`] establishes — exactly one mask
/// character per source grapheme cluster — read backwards: the masked offset
/// divided by the mask's width is a cluster index, and that cluster's byte
/// offset is the answer.
///
/// A masked offset past the end clamps to the source's end, and one that is
/// not a multiple of the mask width rounds down to the mask character it falls
/// inside. Neither should occur — the render object clamps to its own char
/// boundaries, which for masked text are multiples of the mask width — but
/// clamping rather than asserting keeps a wrong offset from panicking a field.
pub(in crate::text) fn source_offset_for_masked_offset(
    source: &str,
    masked_offset: usize,
    mask: char,
) -> usize {
    let cluster_index = masked_offset / mask.len_utf8();
    graphemes(source)
        .nth(cluster_index)
        .map_or(source.len(), |cluster| cluster.start)
}
