//! [`RawTextFormField`] — a [`RawTextField`] in a [`FormField<String>`].

use std::rc::Rc;

use flui_interaction::FocusNode;
use flui_types::Color;
use flui_types::typography::TextStyle;
use flui_view::element::ElementKind;
use flui_view::prelude::*;

use super::AutovalidateMode;
use super::form_field::{FormField, FormFieldHandle, FormFieldSetter, FormFieldValidator};
use crate::flex::Column;
use crate::semantics::Semantics;
use crate::text::{RawTextField, SubmitCallback, Text, TextEditingController};
use flui_objects::{CrossAxisAlignment, MainAxisSize};

/// The error line's colour: Material's baseline error red, since this
/// theme-free field has no theme to take one from.
const ERROR_COLOR: Color = Color::rgb(176, 0, 32);

/// Keep `handle`'s value and `controller`'s text one: a value change the
/// form makes (a reset) is written to the controller, and the value is read
/// from the controller before it is used, so a caller's own controller edit
/// is validated and saved too — without counting as the user's interaction.
///
/// Workspace plumbing for the text form fields of the design-system crates;
/// not application API.
pub fn bind_text_controller(handle: &FormFieldHandle<String>, controller: &TextEditingController) {
    let sink = controller.clone();
    let source = controller.clone();
    handle.bind(
        // `set_text` is a no-op for an unchanged text, so the user's own edit,
        // already in the controller, is not written back.
        Rc::new(move |value: &String| sink.set_text(value.clone())),
        Rc::new(move || source.text()),
    );
}

/// A [`RawTextField`] in a [`FormField<String>`] with a plain error line —
/// the theme-free stand-in for Flutter's `TextFormField`;
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
#[derive(Clone)]
pub struct RawTextFormField {
    controller: Option<TextEditingController>,
    initial_value: String,
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

impl std::fmt::Debug for RawTextFormField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawTextFormField")
            .field("controller", &self.controller)
            .field("enabled", &self.enabled)
            .field("autovalidate_mode", &self.autovalidate_mode)
            .field("obscure_text", &self.obscure_text)
            .finish_non_exhaustive()
    }
}

impl RawTextFormField {
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

impl View for RawTextFormField {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// The state behind [`RawTextFormField`]: the controller it edits, bound to
/// its field's value.
pub struct RawTextFormFieldState {
    controller: TextEditingController,
    handle: FormFieldHandle<String>,
    initial_value: String,
}

impl std::fmt::Debug for RawTextFormFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawTextFormFieldState")
            .field("controller", &self.controller)
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl StatefulView for RawTextFormField {
    type State = RawTextFormFieldState;

    fn create_state(&self) -> RawTextFormFieldState {
        let controller = self
            .controller
            .clone()
            .unwrap_or_else(|| TextEditingController::with_text(self.initial_value.clone()));
        let handle = self.handle.clone().unwrap_or_default();
        bind_text_controller(&handle, &controller);
        RawTextFormFieldState {
            initial_value: controller.text(),
            controller,
            handle,
        }
    }
}

impl ViewState<RawTextFormField> for RawTextFormFieldState {
    fn did_update_view(&mut self, _old_view: &RawTextFormField, new_view: &RawTextFormField) {
        if let Some(controller) = &new_view.controller
            && !controller.is_same_controller(&self.controller)
        {
            self.controller = controller.clone();
            bind_text_controller(&self.handle, &self.controller);
        }
    }

    fn build(&self, view: &RawTextFormField, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let obscure_text = view.obscure_text;
        let enabled = view.enabled;
        let focus_node = view.focus_node.clone();
        let on_submitted = view.on_submitted.clone();
        let mut field = FormField::new(self.initial_value.clone(), move |_ctx, field| {
            let edits = field.clone();
            let mut input = RawTextField::new(controller.clone())
                .obscure_text(obscure_text)
                .enabled(enabled)
                .on_changed(move |text| edits.did_change(text.to_owned()));
            if let Some(node) = &focus_node {
                input = input.focus_node(Rc::clone(node));
            }
            if let Some(on_submitted) = on_submitted.clone() {
                input = input.on_submitted(move |text| on_submitted(text));
            }
            with_error_line(input.boxed(), field.error_text())
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
