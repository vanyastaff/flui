//! Keyboard focus: a form with a text field and buttons, and a nested
//! navigator with a Back button. Tab and Shift+Tab move through them in
//! reading order, the focus ring marks the focused control, Enter and Space
//! press a button, and the readout names what has focus.

use flui::prelude::*;
use flui::widgets::{SimpleRoute, column, row};

use super::page;
use crate::style;

/// The settings pages the nested navigator opens.
const SECTIONS: [&str; 2] = ["Privacy", "Notifications"];

#[derive(Clone, StatefulView)]
pub(crate) struct FocusScreen;

pub(crate) struct FocusState {
    name: TextEditingController,
    saved: Signal<String>,
    focused: Signal<&'static str>,
    section: Signal<&'static str>,
    settings: NavigatorHandle,
}

impl StatefulView for FocusScreen {
    type State = FocusState;

    fn create_state(&self) -> Self::State {
        FocusState {
            name: TextEditingController::new(),
            saved: Signal::default(),
            focused: Signal::default(),
            section: Signal::default(),
            settings: NavigatorHandle::new(),
        }
    }
}

impl ViewState<FocusScreen> for FocusState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.saved = ctx.signal(String::new());
        self.focused = ctx.signal("nothing");
        self.section = ctx.signal("Settings");
        let (focused, section) = (self.focused, self.section);
        self.settings.seed_initial(
            SimpleRoute::<()>::new(move |ctx| settings_home(ctx, focused, section)).named("/"),
        );
    }

    fn build(&self, _view: &FocusScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let (saved, focused) = (self.saved, self.focused);
        let saved_text = saved.get(ctx);
        let readout = format!(
            "Focused: {}; saved name: {}; settings page: {}",
            focused.get(ctx),
            if saved_text.is_empty() {
                "none"
            } else {
                saved_text.as_str()
            },
            self.section.get(ctx),
        );

        let (submit, save, clear) = (self.name.clone(), self.name.clone(), self.name.clone());
        let form = style::card(FocusTraversalGroup::new(
            Column::new(column![
                style::strong("Profile"),
                SizedBox::height(12.0),
                style::caption("Name"),
                SizedBox::height(6.0),
                tracked(
                    focused,
                    "Name field",
                    RawTextField::new(self.name.clone())
                        .on_submitted(move |cx, text| save_name(cx, saved, &submit, text)),
                ),
                SizedBox::height(16.0),
                Row::new(row![
                    tracked(
                        focused,
                        "Save",
                        RawButton::new(style::button("Save")).on_press(move |cx| save_name(
                            cx,
                            saved,
                            &save,
                            &save.text()
                        )),
                    ),
                    SizedBox::width(12.0),
                    tracked(
                        focused,
                        "Clear",
                        RawButton::new(style::quiet_button("Clear")).on_press(move |cx| {
                            clear.clear();
                            saved.set(cx, String::new())
                        }),
                    ),
                ]),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Stretch),
        ));

        let settings = style::card(FocusTraversalGroup::new(Navigator::new(
            self.settings.clone(),
        )));

        page(
            "Focus and keys",
            "Tab and Shift+Tab move the focus ring in reading order; Enter or Space press the \
             focused button; Enter in the field saves it. The settings panel is a navigator of \
             its own with a Back button.",
            style::readout(readout),
            Row::new(vec![
                Expanded::new(form).boxed(),
                SizedBox::width(16.0).boxed(),
                Expanded::new(settings).boxed(),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Start),
        )
    }
}

/// `child`, reporting to `focused` while it or anything in it has focus.
/// It takes no focus itself, so it adds no stop to traversal.
fn tracked(focused: Signal<&'static str>, name: &'static str, child: impl IntoView) -> Focus {
    Focus::new(child)
        .can_request_focus(false)
        .skip_traversal(true)
        .on_focus_change(move |cx, has_focus| {
            if has_focus {
                focused.set(cx, name)
            } else {
                Ok(())
            }
        })
}

fn save_name(
    cx: &mut EventCx<'_>,
    saved: Signal<String>,
    field: &TextEditingController,
    text: &str,
) -> Result<(), flui::view::SignalError> {
    let text = text.trim().to_owned();
    field.set_text(text.clone());
    saved.set(cx, text)
}

/// The navigator's first page: one entry per section.
fn settings_home(
    ctx: &dyn BuildContext,
    focused: Signal<&'static str>,
    section: Signal<&'static str>,
) -> BoxedView {
    let navigator = NavigatorHandle::maybe_of(ctx);
    let mut children: Vec<BoxedView> = vec![
        style::strong("Settings").boxed(),
        SizedBox::height(12.0).boxed(),
    ];
    for name in SECTIONS {
        let navigator = navigator.clone();
        let label = match name {
            "Privacy" => "Open Privacy",
            _ => "Open Notifications",
        };
        children.push(
            tracked(
                focused,
                label,
                RawButton::new(style::quiet_button(label)).on_press(move |cx| {
                    if let Some(navigator) = &navigator {
                        let back = navigator.clone();
                        navigator.push(
                            SimpleRoute::<()>::new(move |_ctx| {
                                settings_section(name, back.clone(), focused, section)
                            })
                            .named(name),
                        );
                    }
                    section.set(cx, name)
                }),
            )
            .into_view()
            .boxed(),
        );
        children.push(SizedBox::height(8.0).boxed());
    }
    Column::new(children)
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .boxed()
}

/// A section page: its heading, a line about it and Back.
fn settings_section(
    name: &'static str,
    navigator: NavigatorHandle,
    focused: Signal<&'static str>,
    section: Signal<&'static str>,
) -> BoxedView {
    Column::new(column![
        style::title(name),
        SizedBox::height(8.0),
        style::caption(format!("{name} options would live here.")),
        SizedBox::height(16.0),
        tracked(
            focused,
            "Back",
            RawButton::new(style::button("Back")).on_press(move |cx| {
                navigator.pop();
                section.set(cx, "Settings")
            }),
        ),
    ])
    .cross_axis_alignment(CrossAxisAlignment::Start)
    .boxed()
}
