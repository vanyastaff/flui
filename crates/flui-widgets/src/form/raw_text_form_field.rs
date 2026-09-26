//! [`RawTextFormField`] — a [`RawTextField`] in a [`FormField<String>`].
//!
//! [`FormField<String>`]: super::FormField

use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_types::Color;
use flui_types::typography::TextStyle;
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::AutovalidateMode;
use super::form_field::FormFieldHandle;
use super::text_form_field_core::{TextFormFieldConfig, TextFormFieldCore};
use crate::flex::Column;
use crate::semantics::Semantics;
use crate::text::{RawTextField, Text, TextEditingController};
use flui_objects::{CrossAxisAlignment, MainAxisSize};

/// The error line's colour: Material's baseline error red, since this
/// theme-free field has no theme to take one from.
const ERROR_COLOR: Color = Color::rgb(176, 0, 32);

/// A [`RawTextField`] in a [`FormField<String>`](super::FormField) with a
/// plain error line — the theme-free stand-in for Flutter's `TextFormField`;
/// `flui_material::TextFormField` is the Flutter-parity type.
///
/// Named `Raw…` for the reason [`RawTextField`] is: the facade's
/// `TextFormField` means the Material one whenever that feature is on.
///
/// Two constructors put Flutter's "`initialValue` or `controller`, not both"
/// assert into the types: [`Self::new`] edits a caller's controller and
/// starts from its text, [`Self::with_initial_value`] owns its controller.
///
/// The error line is a live region, so assistive technology announces an
/// error when it appears.
#[derive(Clone, Debug)]
pub struct RawTextFormField {
    config: TextFormFieldConfig,
}

impl RawTextFormField {
    /// A field editing `controller`; its initial value is the controller's
    /// text when the field mounts.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            config: TextFormFieldConfig::with_controller(controller),
        }
    }

    /// A field that owns its controller, starting at `value`.
    #[must_use]
    pub fn with_initial_value(value: impl Into<String>) -> Self {
        Self {
            config: TextFormFieldConfig::with_initial_value(value.into()),
        }
    }

    /// Check the text; `Some(message)` is the error to show.
    #[must_use]
    pub fn validator(mut self, validator: impl Fn(&String) -> Option<String> + 'static) -> Self {
        self.config.validator = Some(Rc::new(validator));
        self
    }

    /// Receive the text when the form saves.
    #[must_use]
    pub fn on_saved(mut self, on_saved: impl Fn(&String) + 'static) -> Self {
        self.config.on_saved = Some(Rc::new(on_saved));
        self
    }

    /// Called after the field resets.
    #[must_use]
    pub fn on_reset(mut self, on_reset: impl Fn() + 'static) -> Self {
        self.config.on_reset = Some(Rc::new(on_reset));
        self
    }

    /// Whether the field accepts input and autovalidates (default `true`).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    /// When the field validates on its own.
    #[must_use]
    pub fn autovalidate_mode(mut self, mode: AutovalidateMode) -> Self {
        self.config.autovalidate_mode = mode;
        self
    }

    /// Show `text` as the error, whatever the validator says.
    #[must_use]
    pub fn force_error_text(mut self, text: impl Into<String>) -> Self {
        self.config.force_error_text = Some(text.into());
        self
    }

    /// Show every character as a bullet — a password field.
    #[must_use]
    pub fn obscure_text(mut self, obscure: bool) -> Self {
        self.config.obscure_text = obscure;
        self
    }

    /// Use a caller-owned focus node.
    #[must_use]
    pub fn focus_node(mut self, focus_node: Rc<FocusNode>) -> Self {
        self.config.focus_node = Some(focus_node);
        self
    }

    /// Called with the text when Enter is pressed.
    #[must_use]
    pub fn on_submitted(mut self, callback: impl Fn(&str) + 'static) -> Self {
        self.config.on_submitted = Some(Rc::new(callback));
        self
    }

    /// Drive the field through `handle`. A different handle on a later
    /// rebuild takes the mounted field over.
    #[must_use]
    pub fn handle(mut self, handle: FormFieldHandle<String>) -> Self {
        self.config.handle = Some(handle);
        self
    }
}

impl View for RawTextFormField {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// The state behind [`RawTextFormField`]: the controller it edits, bound to
/// its field's value.
#[derive(Debug)]
pub struct RawTextFormFieldState {
    core: TextFormFieldCore,
}

impl StatefulView for RawTextFormField {
    type State = RawTextFormFieldState;

    fn create_state(&self) -> RawTextFormFieldState {
        RawTextFormFieldState {
            core: TextFormFieldCore::new(&self.config),
        }
    }
}

impl ViewState<RawTextFormField> for RawTextFormFieldState {
    fn did_update_view(&mut self, old_view: &RawTextFormField, new_view: &RawTextFormField) {
        self.core.update(&old_view.config, &new_view.config);
    }

    fn build(&self, view: &RawTextFormField, _ctx: &dyn BuildContext) -> impl IntoView {
        self.core.build(&view.config, |input| {
            let mut field = RawTextField::new(input.controller.clone())
                .obscure_text(input.obscure_text)
                .enabled(input.enabled)
                .on_changed(input.on_changed());
            if let Some(node) = &input.focus_node {
                field = field.focus_node(Rc::clone(node));
            }
            if let Some(on_submitted) = input.on_submitted.clone() {
                field = field.on_submitted(move |text| on_submitted(text));
            }
            with_error_line(field.boxed(), input.field.error_text())
        })
    }
}

/// `input` above its error line, when there is an error.
fn with_error_line(input: BoxedView, error: Option<String>) -> BoxedView {
    let mut children = vec![input];
    if let Some(error) = error {
        children.push(
            Semantics::new()
                .container(true)
                .live_region(true)
                .child(Text::new(error).style(TextStyle::default().with_color(ERROR_COLOR)))
                .boxed(),
        );
    }
    Column::new(children)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .main_axis_size(MainAxisSize::Min)
        .boxed()
}
