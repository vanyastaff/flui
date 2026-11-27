//! The field's interior view: the render view wrapping `RenderEditable`, the field's
//! appearance as one value, and its assembly.

#[cfg(doc)]
use super::{EditableText, PlatformSelection};
use std::{cell::Cell, ops::Range, rc::Rc};

use flui_foundation::TextDirection;
use flui_interaction::focus::FocusNode;
use flui_interaction::pointer::PressProbe;
use flui_layer::LayerLink;
use flui_objects::RenderEditable;
use flui_painting::{
    styling::Color,
    typography::{TextSpan, TextStyle},
};
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{BoxedView, RenderView, impl_render_view};

use super::FieldHandles;
use super::field_semantics::{FieldSemantics, FieldSemanticsActions};
use super::obscuring::obscure;
use crate::semantics::Semantics;
use crate::text::controller::TextEditingController;
use crate::text::selection_style::SelectionStyle;

#[derive(Clone, Debug)]
pub(super) struct EditableTextRenderView {
    pub(super) text: String,
    pub(super) text_sizing: Option<flui_painting::TextSizing>,
    pub(super) font_weight_adjustment: i32,
    pub(super) caret_byte_offset: usize,
    pub(super) show_caret: bool,
    /// The IME composing region to underline, gated on `enabled &&
    /// has_primary_focus()` by [`build_field_view`]. There is no `readOnly`
    /// field, so `enabled` stands in for it (see [`EditableText::enabled`]'s
    /// doc).
    pub(super) composing_range: Option<Range<usize>>,
    /// The selected byte range, in the SAME space as `text` — masked when the
    /// field is obscured, because [`build_field_view`] masks before this point
    /// and the render object never sees the source characters.
    pub(super) selection: Option<Range<usize>>,
    pub(super) caret_height: Option<f64>,
    pub(super) caret_color: Color,
    pub(super) selection_color: Color,
    pub(super) text_style: Option<TextStyle>,
    /// The links shown touch handles follow the start (or the caret) and
    /// the end under.
    pub(super) handle_links: (Option<LayerLink>, Option<LayerLink>),
    /// Where the field's store finds the probe of the window's pointer
    /// targets: set from the render-object context on every build and
    /// update, so it follows the UI runtime the field is mounted in.
    pub(super) press_probe: Rc<Cell<Option<PressProbe>>>,
}

/// What `build_field_view` mounts the editable under: the inner anchor that
/// publishes its `RenderId`, and the cell its render view hands the press
/// probe to.
pub(super) struct FieldMount {
    pub(super) inner_anchor: flui_objects::SubtreeAnchor,
    pub(super) press_probe: Rc<Cell<Option<PressProbe>>>,
}

impl EditableTextRenderView {
    pub(super) fn build_render_object(&self, direction: TextDirection) -> RenderEditable {
        let mut span = TextSpan::new(self.text.clone());
        if let Some(style) = self.text_style.clone() {
            span = span.with_style(style);
        }
        let mut editable = RenderEditable::new(span, direction)
            .with_text_sizing(self.text_sizing.clone())
            .with_font_weight_adjustment(self.font_weight_adjustment)
            .with_caret_byte_offset(self.caret_byte_offset)
            .with_show_caret(self.show_caret)
            .with_caret_width(2.0)
            .with_caret_height(self.caret_height)
            .with_caret_color(self.caret_color)
            .with_selection(self.selection.clone())
            .with_selection_color(self.selection_color)
            .with_composing_range(self.composing_range.clone());
        let (start, end) = self.handle_links;
        let _ = editable.set_selection_handles(start, end);
        editable
    }
}

impl RenderView for EditableTextRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderEditable;

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        self.press_probe.set(ctx.press_probe());
        // The ambient reading direction is the line's base direction, as it
        // is for `Text` without one; a dependency, so a flip relays out.
        self.build_render_object(crate::Directionality::of_render_object(ctx))
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        self.press_probe.set(ctx.press_probe());
        let mut span = TextSpan::new(self.text.clone());
        if let Some(style) = self.text_style.clone() {
            span = span.with_style(style);
        }
        let mut impact = render_object.set_text(span);
        impact |= render_object.set_text_direction(crate::Directionality::of_render_object(ctx));
        impact |= render_object.set_text_sizing(self.text_sizing.clone());
        impact |= render_object.set_font_weight_adjustment(self.font_weight_adjustment);
        impact |= render_object.set_caret_byte_offset(self.caret_byte_offset);
        impact |= render_object.set_show_caret(self.show_caret);
        impact |= render_object.set_caret_size(2.0, self.caret_height);
        impact |= render_object.set_caret_color(self.caret_color);
        impact |= render_object.set_selection(self.selection.clone());
        impact |= render_object.set_selection_color(self.selection_color);
        impact |= render_object.set_composing_range(self.composing_range.clone());
        let (start, end) = self.handle_links;
        impact |= render_object.set_selection_handles(start, end);
        impact
    }
}

impl_render_view!(EditableTextRenderView);

/// Everything about how the field looks, as one value: caret size and colour,
/// text style and sizing, selection style, the obscuring pair and the
/// field's semantics.
#[derive(Clone, Debug)]
pub(super) struct FieldAppearance {
    pub(super) text_sizing: Option<flui_painting::TextSizing>,
    pub(super) font_weight_adjustment: i32,
    pub(super) caret_height: Option<f64>,
    pub(super) caret_color: Color,
    pub(super) selection_style: SelectionStyle,
    pub(super) text_style: Option<TextStyle>,
    /// Paint each source character as [`Self::obscuring_character`].
    pub(super) obscure_text: bool,
    pub(super) obscuring_character: char,
    pub(super) semantics: FieldSemantics,
    /// The platform draws the focused field's caret, selection highlight and
    /// menu ([`PlatformSelection`]).
    pub(super) platform_selection: bool,
}

/// Assemble the visual render view for the text field interior.
///
/// Wraps `EditableTextRenderView` directly in `inner_anchor` (the inner
/// anchor, ADR-0030): a second, inner `SubtreeAnchor` whose only job is to publish
/// exactly the editable's own `RenderId`, so the IME cursor-area loop's
/// `transform_to` starts right at the editable — not at the outer `anchor`
/// wrapping this whole field (which also spans the `AnimatedBuilder` between
/// the two, zero-offset by convention only).
pub(super) fn build_field_view(
    controller: &TextEditingController,
    focus_node: &Rc<FocusNode>,
    enabled: bool,
    appearance: &FieldAppearance,
    mount: FieldMount,
    actions: &FieldSemanticsActions,
    handles: &FieldHandles,
) -> BoxedView {
    // `enabled` is defensive here: `did_update_view` already unfocuses a
    // field that becomes disabled while focused, so `has_primary_focus`
    // should already be `false` by the time this runs.
    let focused = enabled && focus_node.has_primary_focus();
    // The platform draws the focused field's caret, selection highlight and
    // menu over it; the field keeps its text and composing underline.
    let platform_draws = focused && appearance.platform_selection;
    // The masking happens HERE, at the one point the controller's text becomes
    // the render view's, so nothing below ever receives the real characters.
    // The masking happens HERE, so every offset below is in MASKED space and
    // the render object never receives the real characters — the selection
    // included. Mapping it here, beside the caret it has to stay consistent
    // with, is what keeps the two from being masked in different places.
    let source = controller.text();
    let source_selection = controller.selection();
    let (text, caret_byte_offset, selection) = if appearance.obscure_text {
        // Caret and both selection ends mapped in the one walk the mask needs
        // anyway, and by the one rule — see `obscure`.
        let mut offsets = [
            controller.caret_byte_offset(),
            source_selection.start,
            source_selection.end,
        ];
        let masked = obscure(&source, &mut offsets, appearance.obscuring_character);
        let [caret, start, end] = offsets;
        (masked, caret, start..end)
    } else {
        (source, controller.caret_byte_offset(), source_selection)
    };
    // A collapsed selection is the caret's business, and the render object
    // skips it anyway — `None` says so at the seam rather than relying on it.
    let selection = (!selection.is_empty() && !platform_draws).then_some(selection);
    // The field's node for assistive technology (text field, obscured, value),
    // outside the inner anchor so the IME loop still finds the editable as that anchor's first child. The
    // value is the text the render object shows — the mask when obscured.
    // The node is no semantics boundary, as Flutter's `RenderEditable` is none
    // (flutter/flutter@3.50.1 packages/flutter/lib/src/rendering/editable.dart:1323-1338):
    // a `Semantics` that annotates the field from above (a label, a hint)
    // absorbs it, so the field and its name are one node.
    let mut semantics = Semantics::new()
        .role(if appearance.obscure_text {
            flui_semantics::SemanticsRole::PasswordInput
        } else {
            flui_semantics::SemanticsRole::TextInput
        })
        .enabled(enabled)
        .focused(focused)
        .invalid(flui_semantics::Invalid::from_bool(
            appearance.semantics.invalid,
        ))
        .required(appearance.semantics.required)
        .identifier_if(appearance.semantics.identifier.as_ref())
        .value(text.clone());
    if let Some(label) = &appearance.semantics.label {
        semantics = semantics.label(label.as_str());
    }
    if let Some(placeholder) = &appearance.semantics.placeholder {
        semantics = semantics.placeholder(placeholder.as_str());
    }
    if let Some(hint) = &appearance.semantics.hint {
        semantics = semantics.hint(hint.as_str());
    }
    if enabled {
        let edit = actions.clone();
        semantics = semantics.on_set_value(move |ctx, text| edit.set_text(ctx, text));
        let obscuring = appearance
            .obscure_text
            .then_some(appearance.obscuring_character);
        let select = actions.clone();
        semantics = semantics.on_set_text_selection(move |ctx, selection| {
            select.set_selection(ctx, selection, obscuring);
        });
        let replace = actions.clone();
        semantics = semantics
            .on_replace_selected_text(move |ctx, text| replace.replace_selected_text(ctx, text));
        // Focus eligibility is independent of the enabled document's value
        // mutability. A queued focus request rechecks eligibility at delivery.
        if focus_node.can_request_focus() {
            let focus = actions.clone();
            semantics = semantics.on_focus(move |ctx| focus.focus(ctx));
        }
        if !platform_draws {
            let focus = actions.clone();
            semantics = crate::text::show_menu_action(semantics, handles.menu(), move |ctx| {
                focus.focus(ctx);
            });
        }
    }
    let field = crate::__private::AnchoredBox::new(
        mount.inner_anchor,
        EditableTextRenderView {
            text,
            text_sizing: appearance.text_sizing.clone(),
            font_weight_adjustment: appearance.font_weight_adjustment,
            caret_byte_offset,
            show_caret: focused && !controller.caret_hidden_by_ime() && !platform_draws,
            // Composing-region underline gated on the same `focused` check
            // as `show_caret`: an unfocused field must not keep painting a
            // stale composing underline for text it no longer owns input
            // for.
            // Suppressed entirely while obscured: the obscured branch builds a
            // plain span and never applies the
            // composing decoration. Two reasons, and the second is the one
            // that matters — the underline's extent would report how many
            // characters the in-progress IME composition holds, which is a
            // leak the mask exists to prevent; and the range is in SOURCE
            // byte space, so painting it against masked text would underline
            // the wrong run.
            composing_range: if focused && !appearance.obscure_text {
                controller.composing_range()
            } else {
                None
            },
            selection,
            caret_height: appearance.caret_height,
            caret_color: appearance.caret_color,
            // A blur keeps the selection and dims it, as in a `SelectionArea`.
            selection_color: if focused {
                appearance.selection_style.active
            } else {
                appearance.selection_style.inactive
            },
            text_style: appearance.text_style.clone(),
            handle_links: handles.published_links(),
            press_probe: mount.press_probe,
        },
    );
    semantics.child(field).boxed()
}

// The key handler, the obscuring mask and the render-view assembly, tested
// without a mounted tree. The mounted suite lives in
// `crates/flui-widgets/tests/editable_text.rs`.
