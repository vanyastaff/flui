//! [`TextFormField`] — a Material [`TextField`] in a [`FormField<String>`].
//!
//! Flutter parity: `material/text_form_field.dart` `TextFormField` (tag
//! `3.44.0`): the field's error reaches the decoration's error line
//! (`decoration.copyWith(errorText: field.errorText)`), a user edit is the
//! field's `didChange`, and a reset writes the initial text back into the
//! controller. `flui_widgets::RawTextFormField` is the theme-free sibling;
//! both share `flui_widgets::__private::TextFormFieldCore`, and this type
//! supplies only the Material input.
//!
//! [`FormField<String>`]: flui_widgets::FormField

use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::__private::{TextFormFieldConfig, TextFormFieldCore};
use flui_widgets::{AutovalidateMode, FormFieldHandle, TextEditingController};

use crate::input_decorator::InputDecoration;
use crate::text_field::TextField;

/// A Material [`TextField`] in a [`FormField<String>`] — Flutter's
/// `TextFormField`.
///
/// Two constructors put Flutter's "`initialValue` or `controller`, not both"
/// assert into the types: [`Self::new`] edits a caller's controller and
/// starts from its text, [`Self::with_initial_value`] owns its controller.
///
/// [`FormField<String>`]: flui_widgets::FormField
#[derive(Clone)]
pub struct TextFormField {
    config: TextFormFieldConfig,
    decoration: InputDecoration,
}

impl std::fmt::Debug for TextFormField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFormField")
            .field("config", &self.config)
            .field("decoration", &self.decoration)
            .finish()
    }
}

impl TextFormField {
    /// A field editing `controller`; its initial value is the controller's
    /// text when the field mounts.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            config: TextFormFieldConfig::with_controller(controller),
            decoration: InputDecoration::default(),
        }
    }

    /// A field that owns its controller, starting at `value`.
    #[must_use]
    pub fn with_initial_value(value: impl Into<String>) -> Self {
        Self {
            config: TextFormFieldConfig::with_initial_value(value.into()),
            decoration: InputDecoration::default(),
        }
    }

    /// The field's decoration. The field's error, when it has one, replaces
    /// the decoration's `error_text`; without one, a caller-set `error_text`
    /// shows.
    #[must_use]
    pub fn decoration(mut self, decoration: InputDecoration) -> Self {
        self.decoration = decoration;
        self
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

impl View for TextFormField {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// The state behind [`TextFormField`]: the controller it edits, bound to its
/// field's value.
#[derive(Debug)]
pub struct TextFormFieldState {
    core: TextFormFieldCore,
}

impl StatefulView for TextFormField {
    type State = TextFormFieldState;

    fn create_state(&self) -> TextFormFieldState {
        TextFormFieldState {
            core: TextFormFieldCore::new(&self.config),
        }
    }
}

impl ViewState<TextFormField> for TextFormFieldState {
    fn did_update_view(&mut self, old_view: &TextFormField, new_view: &TextFormField) {
        self.core.update(&old_view.config, &new_view.config);
    }

    fn build(&self, view: &TextFormField, _ctx: &dyn BuildContext) -> impl IntoView {
        let decoration = view.decoration.clone();
        self.core.build(&view.config, move |input| {
            let mut decoration = decoration.clone();
            // `copyWith(errorText: field.errorText)`: a field error replaces
            // the caller's, and no field error keeps it.
            if let Some(error) = input.field.error_text() {
                decoration.error_text = Some(error);
            }
            let mut field = TextField::new(input.controller.clone())
                .decoration(decoration)
                .enabled(input.enabled)
                .obscure_text(input.obscure_text)
                .on_changed(input.on_changed());
            if let Some(node) = &input.focus_node {
                field = field.focus_node(Rc::clone(node));
            }
            if let Some(on_submitted) = input.on_submitted.clone() {
                field = field.on_submitted(move |text| on_submitted(text));
            }
            field.boxed()
        })
    }
}
