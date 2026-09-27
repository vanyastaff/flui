//! What every text form field shares, whichever input it draws: its
//! configuration, the controller kept one with the field's value, and the
//! [`FormField<String>`] around the input.
//!
//! [`RawTextFormField`](super::RawTextFormField) and
//! `flui_material::TextFormField` differ only in the input they build, so
//! that is the one thing each supplies; the design-system crates reach this
//! module through `flui_widgets::__private`.

use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_view::prelude::*;

use super::AutovalidateMode;
use super::form_field::{FormField, FormFieldHandle, FormFieldSetter, FormFieldValidator};
use crate::text::{SubmitCallback, TextEditingController};

/// A text form field's configuration, whichever input it draws.
///
/// Workspace plumbing for the text form fields of this crate and the
/// design-system crates; not application API. The owning view's builder
/// methods write these fields.
#[derive(Clone)]
pub struct TextFormFieldConfig {
    /// The caller's controller; `None` makes the field own one, starting at
    /// `initial_value`.
    pub controller: Option<TextEditingController>,
    /// The starting text when the field owns its controller.
    pub initial_value: String,
    /// Checks the text; `Some(message)` is the error to show.
    pub validator: Option<FormFieldValidator<String>>,
    /// Receives the text when the form saves.
    pub on_saved: Option<FormFieldSetter<String>>,
    /// Called after the field resets.
    pub on_reset: Option<Rc<dyn Fn()>>,
    /// Whether the field accepts input and autovalidates.
    pub enabled: bool,
    /// When the field validates on its own.
    pub autovalidate_mode: AutovalidateMode,
    /// Shown as the error, whatever the validator says.
    pub force_error_text: Option<String>,
    /// Whether every character shows as a bullet.
    pub obscure_text: bool,
    /// A caller-owned focus node for the input.
    pub focus_node: Option<Rc<FocusNode>>,
    /// Called with the text when Enter is pressed.
    pub on_submitted: Option<SubmitCallback>,
    /// The handle the caller drives the field through.
    pub handle: Option<FormFieldHandle<String>>,
}

impl std::fmt::Debug for TextFormFieldConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFormFieldConfig")
            .field("controller", &self.controller)
            .field("enabled", &self.enabled)
            .field("autovalidate_mode", &self.autovalidate_mode)
            .field("obscure_text", &self.obscure_text)
            .finish_non_exhaustive()
    }
}

impl TextFormFieldConfig {
    /// A field editing `controller`; its initial value is the controller's
    /// text when the field mounts.
    #[must_use]
    pub fn with_controller(controller: TextEditingController) -> Self {
        Self {
            controller: Some(controller),
            ..Self::with_initial_value(String::new())
        }
    }

    /// A field that owns its controller, starting at `value`.
    #[must_use]
    pub fn with_initial_value(value: String) -> Self {
        Self {
            controller: None,
            initial_value: value,
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
}

/// What a text form field's input is built from, handed to the input
/// builder on each build of the field.
#[derive(Clone)]
pub struct TextFormFieldInput {
    /// The controller the input edits.
    pub controller: TextEditingController,
    /// The field; read its `error_text` for the error to show.
    pub field: FormFieldHandle<String>,
    /// Whether the input accepts input.
    pub enabled: bool,
    /// Whether every character shows as a bullet.
    pub obscure_text: bool,
    /// A caller-owned focus node for the input.
    pub focus_node: Option<Rc<FocusNode>>,
    /// Called with the text when Enter is pressed.
    pub on_submitted: Option<SubmitCallback>,
}

impl std::fmt::Debug for TextFormFieldInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFormFieldInput")
            .field("controller", &self.controller)
            .field("field", &self.field)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl TextFormFieldInput {
    /// The input's `on_changed`: a user edit is the field's `did_change`.
    pub fn on_changed(&self) -> impl Fn(&str) + 'static {
        let field = self.field.clone();
        move |text| field.did_change(text.to_owned())
    }
}

/// A text form field's state: the controller it edits, kept one with its
/// field's value.
pub struct TextFormFieldCore {
    controller: TextEditingController,
    handle: FormFieldHandle<String>,
    initial_value: String,
}

impl std::fmt::Debug for TextFormFieldCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFormFieldCore")
            .field("controller", &self.controller)
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl TextFormFieldCore {
    /// The state for a field mounting with `config` — the caller's
    /// controller or a new one, bound to the caller's handle or a new one.
    #[must_use]
    pub fn new(config: &TextFormFieldConfig) -> Self {
        let controller = config
            .controller
            .clone()
            .unwrap_or_else(|| TextEditingController::with_text(config.initial_value.clone()));
        let handle = config.handle.clone().unwrap_or_default();
        bind_text_controller(&handle, &controller);
        Self {
            initial_value: controller.text(),
            controller,
            handle,
        }
    }

    /// Follow a reconfiguration — Flutter's
    /// `_TextFormFieldState.didUpdateWidget`: a new caller controller is
    /// edited from now on; dropping the caller's controller moves the text
    /// into one the field owns; a new handle takes the field over.
    pub fn update(&mut self, old: &TextFormFieldConfig, new: &TextFormFieldConfig) {
        let mut rebind = false;
        match (&old.controller, &new.controller) {
            (_, Some(controller)) if !controller.is_same_controller(&self.controller) => {
                self.controller = controller.clone();
                rebind = true;
            }
            (Some(_), None) => {
                self.controller = TextEditingController::with_text(self.controller.text());
                rebind = true;
            }
            _ => {}
        }
        if let Some(handle) = &new.handle
            && !handle.same_field(&self.handle)
        {
            self.handle = handle.clone();
            rebind = true;
        }
        if rebind {
            bind_text_controller(&self.handle, &self.controller);
        }
    }

    /// The field around the input `input` builds, configured by `config`.
    #[must_use]
    pub fn build(
        &self,
        config: &TextFormFieldConfig,
        input: impl Fn(TextFormFieldInput) -> BoxedView + 'static,
    ) -> FormField<String> {
        let controller = self.controller.clone();
        let enabled = config.enabled;
        let obscure_text = config.obscure_text;
        let focus_node = config.focus_node.clone();
        let on_submitted = config.on_submitted.clone();
        let mut field = FormField::new(self.initial_value.clone(), move |_ctx, field| {
            input(TextFormFieldInput {
                controller: controller.clone(),
                field: field.clone(),
                enabled,
                obscure_text,
                focus_node: focus_node.clone(),
                on_submitted: on_submitted.clone(),
            })
        })
        .enabled(config.enabled)
        .autovalidate_mode(config.autovalidate_mode)
        .handle(self.handle.clone());
        field.validator.clone_from(&config.validator);
        field.on_saved.clone_from(&config.on_saved);
        field.on_reset.clone_from(&config.on_reset);
        field.force_error_text.clone_from(&config.force_error_text);
        field
    }
}

/// Keep `handle`'s value and `controller`'s text one: a value change the
/// form makes (a reset, `set_value`) is written to the controller, and the
/// value is read from the controller before it is used, so a caller's own
/// controller edit is validated and saved too — without counting as the
/// user's interaction.
fn bind_text_controller(handle: &FormFieldHandle<String>, controller: &TextEditingController) {
    let sink = controller.clone();
    let source = controller.clone();
    handle.bind(
        // `set_text` is a no-op for an unchanged text, so the user's own edit,
        // already in the controller, is not written back.
        Rc::new(move |value: &String| sink.set_text(value.clone())),
        Rc::new(move || source.text()),
    );
}
