//! Form — a sign-up form: three validated `TextFormField`s in a `Form`,
//! submitted through a `FormHandle`.
//!
//! - **Name** is required.
//! - **Email** must contain `@`.
//! - **Password** is obscured, needs 8 or more characters, and validates
//!   when the focus leaves it (`AutovalidateMode::OnUnfocus`).
//!
//! **Submit** runs `validate()`; when every field passes it runs `save()`,
//! which hands each field's text to its `on_saved`, and shows a summary.
//! **Reset** puts every field back to its initial text and clears the
//! errors.
//!
//! Keyboard: Tab and Shift+Tab move between the fields; Ctrl+C, Ctrl+X and
//! Ctrl+V (Cmd on macOS) copy, cut and paste in the focused field — the
//! password field copies nothing.
//!
//! Run with: cargo run --example form --features material

use flui::material::{ElevatedButton, InputDecoration, TextFormField, Theme, ThemeData};
use flui::prelude::*;
use flui::widgets::{AutovalidateMode, Form, FormHandle, SafeArea, column, row};

/// The width every field is laid out at.
const FIELD_WIDTH: f32 = 360.0;

#[derive(Clone, Default)]
struct SignUp {
    name: String,
    email: String,
    password_length: usize,
}

#[derive(Clone, StatelessView)]
struct FormApp;

impl StatelessView for FormApp {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Theme::new(ThemeData::light(), SafeArea::new().child(SignUpForm))
    }
}

#[derive(Clone, StatefulView)]
struct SignUpForm;

struct SignUpState {
    form: FormHandle,
    /// Filled by the fields' `on_saved`.
    saved: StateHandle<SignUp>,
    /// What the last submit found, shown under the buttons.
    summary: StateHandle<String>,
}

impl StatefulView for SignUpForm {
    type State = SignUpState;

    fn create_state(&self) -> Self::State {
        SignUpState {
            form: FormHandle::new(),
            saved: StateHandle::new(SignUp::default()),
            summary: StateHandle::new(String::new()),
        }
    }
}

fn labelled(label: &str) -> InputDecoration {
    InputDecoration {
        label_text: Some(label.to_owned()),
        ..InputDecoration::default()
    }
}

impl ViewState<SignUpForm> for SignUpState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.saved.bind(ctx);
        self.summary.bind(ctx);
    }

    fn build(&self, _view: &SignUpForm, _ctx: &dyn BuildContext) -> impl IntoView {
        let (save_name, save_email, save_password) =
            (self.saved.clone(), self.saved.clone(), self.saved.clone());
        let name = TextFormField::with_initial_value("")
            .decoration(labelled("Name"))
            .validator(|value| {
                value
                    .trim()
                    .is_empty()
                    .then(|| "Enter your name".to_owned())
            })
            .on_saved(move |value| save_name.update(|saved| saved.name.clone_from(value)));
        let email = TextFormField::with_initial_value("")
            .decoration(labelled("Email"))
            .validator(|value| (!value.contains('@')).then(|| "Enter an email address".to_owned()))
            .on_saved(move |value| save_email.update(|saved| saved.email.clone_from(value)));
        let password = TextFormField::with_initial_value("")
            .decoration(labelled("Password"))
            .obscure_text(true)
            .autovalidate_mode(AutovalidateMode::OnUnfocus)
            .validator(|value| {
                (value.chars().count() < 8).then(|| "Use 8 or more characters".to_owned())
            })
            .on_saved(move |value| {
                save_password.update(|saved| saved.password_length = value.chars().count());
            });

        let submit_form = self.form.clone();
        let submit_saved = self.saved.clone();
        let submit_summary = self.summary.clone();
        let reset_form = self.form.clone();
        let reset_summary = self.summary.clone();
        let summary = self.summary.with(Clone::clone);

        Form::new(Column::new(column![
            SizedBox::width(FIELD_WIDTH).child(name),
            SizedBox::width(FIELD_WIDTH).child(email),
            SizedBox::width(FIELD_WIDTH).child(password),
            SizedBox::height(16.0),
            Row::new(row![
                ElevatedButton::new(Text::new("Submit")).on_pressed(move || {
                    if submit_form.validate() {
                        submit_form.save();
                        let saved = submit_saved.with(Clone::clone);
                        submit_summary.update(|summary| {
                            *summary = format!(
                                "Signed up {} <{}> with a {}-character password",
                                saved.name, saved.email, saved.password_length
                            );
                        });
                    } else {
                        submit_summary.update(|summary| {
                            "Fix the fields marked in red".clone_into(summary);
                        });
                    }
                }),
                SizedBox::width(8.0),
                ElevatedButton::new(Text::new("Reset")).on_pressed(move || {
                    reset_form.reset();
                    reset_summary.update(String::clear);
                }),
            ]),
            SizedBox::height(16.0),
            Text::new(summary),
        ]))
        .handle(self.form.clone())
    }
}

fn main() {
    run_app(FormApp);
}
