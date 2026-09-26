//! [`TextFormField`] — a Material [`TextField`] in a [`FormField<String>`].
//!
//! Flutter parity: `material/text_form_field.dart` `TextFormField` (tag
//! `3.44.0`): the field's error reaches the decoration's error line
//! (`decoration.copyWith(errorText: field.errorText)`), a user edit is the
//! field's `didChange`, and a reset writes the initial text back into the
//! controller. `flui_widgets::RawTextFormField` is the theme-free sibling.

use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::{
    AutovalidateMode, FormField, FormFieldHandle, FormFieldSetter, FormFieldValidator,
    SubmitCallback, TextEditingController,
};

use crate::input_decorator::InputDecoration;
use crate::text_field::TextField;

/// A Material [`TextField`] in a [`FormField<String>`] — Flutter's
/// `TextFormField`.
///
/// Two constructors put Flutter's "`initialValue` or `controller`, not both"
/// assert into the types: [`Self::new`] edits a caller's controller and
/// starts from its text, [`Self::with_initial_value`] owns its controller.
#[derive(Clone)]
pub struct TextFormField {
    controller: Option<TextEditingController>,
    initial_value: String,
    decoration: InputDecoration,
    validator: Option<FormFieldValidator<String>>,
    on_saved: Option<FormFieldSetter<String>>,
    on_reset: Option<Rc<dyn Fn()>>,
    enabled: bool,
    autovalidate_mode: AutovalidateMode,
    force_error_text: Option<String>,
    obscure_text: bool,
    focus_node: Option<Rc<FocusNode>>,
    on_submitted: Option<SubmitCallback>,
    handle: Option<FormFieldHandle<String>>,
}

impl std::fmt::Debug for TextFormField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFormField")
            .field("controller", &self.controller)
            .field("decoration", &self.decoration)
            .field("enabled", &self.enabled)
            .field("autovalidate_mode", &self.autovalidate_mode)
            .field("obscure_text", &self.obscure_text)
            .finish_non_exhaustive()
    }
}

impl TextFormField {
    /// A field editing `controller`; its initial value is the controller's
    /// text when the field mounts.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller: Some(controller),
            ..Self::with_initial_value("")
        }
    }

    /// A field that owns its controller, starting at `value`.
    #[must_use]
    pub fn with_initial_value(value: impl Into<String>) -> Self {
        Self {
            controller: None,
            initial_value: value.into(),
            decoration: InputDecoration::default(),
            validator: None,
            on_saved: None,
            on_reset: None,
            enabled: true,
            autovalidate_mode: AutovalidateMode::Disabled,
            force_error_text: None,
            obscure_text: false,
            focus_node: None,
            on_submitted: None,
            handle: None,
        }
    }

    /// The field's decoration; its `error_text` is replaced by the field's
    /// error.
    #[must_use]
    pub fn decoration(mut self, decoration: InputDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    /// Check the text; `Some(message)` is the error to show.
    #[must_use]
    pub fn validator(mut self, validator: impl Fn(&String) -> Option<String> + 'static) -> Self {
        self.validator = Some(Rc::new(validator));
        self
    }

    /// Receive the text when the form saves.
    #[must_use]
    pub fn on_saved(mut self, on_saved: impl Fn(&String) + 'static) -> Self {
        self.on_saved = Some(Rc::new(on_saved));
        self
    }

    /// Called after the field resets.
    #[must_use]
    pub fn on_reset(mut self, on_reset: impl Fn() + 'static) -> Self {
        self.on_reset = Some(Rc::new(on_reset));
        self
    }

    /// Whether the field accepts input and autovalidates (default `true`).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// When the field validates on its own.
    #[must_use]
    pub fn autovalidate_mode(mut self, mode: AutovalidateMode) -> Self {
        self.autovalidate_mode = mode;
        self
    }

    /// Show `text` as the error, whatever the validator says.
    #[must_use]
    pub fn force_error_text(mut self, text: impl Into<String>) -> Self {
        self.force_error_text = Some(text.into());
        self
    }

    /// Show every character as a bullet — a password field.
    #[must_use]
    pub fn obscure_text(mut self, obscure: bool) -> Self {
        self.obscure_text = obscure;
        self
    }

    /// Use a caller-owned focus node.
    #[must_use]
    pub fn focus_node(mut self, focus_node: Rc<FocusNode>) -> Self {
        self.focus_node = Some(focus_node);
        self
    }

    /// Called with the text when Enter is pressed.
    #[must_use]
    pub fn on_submitted(mut self, callback: impl Fn(&str) + 'static) -> Self {
        self.on_submitted = Some(Rc::new(callback));
        self
    }

    /// Drive the field through `handle`.
    #[must_use]
    pub fn handle(mut self, handle: FormFieldHandle<String>) -> Self {
        self.handle = Some(handle);
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
pub struct TextFormFieldState {
    controller: TextEditingController,
    handle: FormFieldHandle<String>,
    initial_value: String,
}

impl std::fmt::Debug for TextFormFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFormFieldState")
            .field("controller", &self.controller)
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl StatefulView for TextFormField {
    type State = TextFormFieldState;

    fn create_state(&self) -> TextFormFieldState {
        let controller = self
            .controller
            .clone()
            .unwrap_or_else(|| TextEditingController::with_text(self.initial_value.clone()));
        let handle = self.handle.clone().unwrap_or_default();
        flui_widgets::__private::bind_text_controller(&handle, &controller);
        TextFormFieldState {
            initial_value: controller.text(),
            controller,
            handle,
        }
    }
}

impl ViewState<TextFormField> for TextFormFieldState {
    fn did_update_view(&mut self, _old_view: &TextFormField, new_view: &TextFormField) {
        if let Some(controller) = &new_view.controller
            && !controller.is_same_controller(&self.controller)
        {
            self.controller = controller.clone();
            flui_widgets::__private::bind_text_controller(&self.handle, &self.controller);
        }
    }

    fn build(&self, view: &TextFormField, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let decoration = view.decoration.clone();
        let obscure_text = view.obscure_text;
        let enabled = view.enabled;
        let focus_node = view.focus_node.clone();
        let on_submitted = view.on_submitted.clone();
        let mut field = FormField::new(self.initial_value.clone(), move |_ctx, field| {
            let edits = field.clone();
            let mut decoration = decoration.clone();
            decoration.error_text = field.error_text();
            let mut input = TextField::new(controller.clone())
                .decoration(decoration)
                .enabled(enabled)
                .obscure_text(obscure_text)
                .on_changed(move |text| edits.did_change(text.to_owned()));
            if let Some(node) = &focus_node {
                input = input.focus_node(Rc::clone(node));
            }
            if let Some(on_submitted) = on_submitted.clone() {
                input = input.on_submitted(move |text| on_submitted(text));
            }
            input.boxed()
        })
        .enabled(view.enabled)
        .autovalidate_mode(view.autovalidate_mode)
        .handle(self.handle.clone());
        if let Some(validator) = view.validator.clone() {
            field = field.validator(move |value| validator(value));
        }
        if let Some(on_saved) = view.on_saved.clone() {
            field = field.on_saved(move |value| on_saved(value));
        }
        if let Some(on_reset) = view.on_reset.clone() {
            field = field.on_reset(move || on_reset());
        }
        if let Some(text) = view.force_error_text.clone() {
            field = field.force_error_text(text);
        }
        field
    }
}
