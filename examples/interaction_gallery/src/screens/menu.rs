//! File rows under a `ContextMenuArea`: a right-click, a long press, the
//! menu key or Shift+F10 open the row's menu, whose items the arrow keys and
//! Enter choose and Escape dismisses.

use flui::prelude::*;
use flui::widgets::{ContextMenuArea, GestureCommand, row};

use super::{avatar, divider, list_row, page};
use crate::keyed::Keyed;
use crate::style;

/// Open this file.
#[derive(Clone, Debug)]
struct Open(File);
impl Intent for Open {}

/// Put a copy of this file after it.
#[derive(Clone, Debug)]
struct Duplicate(File);
impl Intent for Duplicate {}

/// Delete this file.
#[derive(Clone, Debug)]
struct Delete(File);
impl Intent for Delete {}

#[derive(Clone, Debug)]
struct File {
    id: u64,
    name: String,
}

#[derive(Clone, StatefulView)]
pub(crate) struct MenuScreen;

pub(crate) struct MenuState {
    files: Signal<Vec<File>>,
    last: Signal<String>,
    next_id: Signal<u64>,
}

impl StatefulView for MenuScreen {
    type State = MenuState;

    fn create_state(&self) -> Self::State {
        MenuState {
            files: Signal::default(),
            last: Signal::default(),
            next_id: Signal::default(),
        }
    }
}

impl ViewState<MenuScreen> for MenuState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let names = ["Report.pdf", "Holiday photos", "Budget.xlsx", "Notes.txt"];
        self.files = ctx.signal(
            names
                .iter()
                .zip(1..)
                .map(|(name, id)| File {
                    id,
                    name: (*name).to_owned(),
                })
                .collect(),
        );
        self.last = ctx.signal("nothing yet".to_owned());
        self.next_id = ctx.signal(names.len() as u64 + 1);
    }

    fn build(&self, _view: &MenuScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let (files, last, next_id) = (self.files, self.last, self.next_id);
        let list = files.get(ctx);
        let readout = format!("{} files, last action: {}", list.len(), last.get(ctx));
        let mut rows: Vec<BoxedView> = Vec::new();
        for file in &list {
            rows.push(Keyed::new(file.id, file_row(file)).boxed());
            rows.push(divider().into_view().boxed());
        }
        let content = Actions::new(
            Scrollable::new()
                .child(Column::new(rows).cross_axis_alignment(CrossAxisAlignment::Stretch)),
        )
        .action(CallbackAction::new(move |cx, Open(file): &Open| {
            last.set(cx, format!("opened {}", file.name))
        }))
        .action(CallbackAction::new(
            move |cx, Duplicate(file): &Duplicate| {
                let id = next_id.update(cx, |next| {
                    *next += 1;
                    *next - 1
                })?;
                let copy = File {
                    id,
                    name: format!("Copy of {}", file.name),
                };
                last.set(cx, format!("made {}", copy.name))?;
                files.update(cx, |list| {
                    let at = list
                        .iter()
                        .position(|other| other.id == file.id)
                        .map_or(list.len(), |index| index + 1);
                    list.insert(at, copy.clone());
                })
            },
        ))
        .action(CallbackAction::new(move |cx, Delete(file): &Delete| {
            files.update(cx, |list| list.retain(|other| other.id != file.id))?;
            last.set(cx, format!("deleted {}", file.name))
        }));
        page(
            "Context menu",
            "Right-click or long-press a file. With a row focused, the menu key or Shift+F10 \
             opens its menu; the arrows move, Enter chooses and Escape closes it.",
            style::readout(readout),
            content,
        )
    }
}

fn file_row(file: &File) -> impl IntoView {
    let name = file.name.as_str();
    ContextMenuArea::new(
        GestureCommand::new(format!("Open {name}"), Open(file.clone())),
        [
            GestureCommand::new(format!("Duplicate {name}"), Duplicate(file.clone())),
            GestureCommand::new(format!("Delete {name}"), Delete(file.clone())),
        ],
    )
    .expect("BUG: a file's commands are labelled by its name")
    .target(name)
    .child(list_row(
        56.0,
        Row::new(row![
            avatar(name.chars().next().unwrap_or('?'), style::PIN),
            SizedBox::width(12.0),
            style::tinted(name, style::INK),
        ]),
    ))
}
