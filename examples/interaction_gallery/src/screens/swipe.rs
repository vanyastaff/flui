//! `SwipeActions` rows that pin and archive, and `Dismissible` rows that a
//! swipe completes.

use flui::prelude::*;
use flui::widgets::{
    Alternative, ControlSpec, Dismissible, GestureAlternatives, GestureCommand, SwipeAction,
    SwipeActions, SwipeAlternative, SwipeEdge, column, row,
};

use super::{avatar, divider, list_row, page};
use crate::keyed::Keyed;
use crate::style;

/// Pin or unpin the message with this id.
#[derive(Clone, Copy, Debug)]
struct Pin(u64);
impl Intent for Pin {}

/// Archive the message with this id.
#[derive(Clone, Copy, Debug)]
struct Archive(u64);
impl Intent for Archive {}

/// Complete the task with this id.
#[derive(Clone, Copy, Debug)]
struct Complete(u64);
impl Intent for Complete {}

#[derive(Clone, Debug)]
struct Message {
    id: u64,
    from: &'static str,
    subject: &'static str,
    pinned: bool,
}

#[derive(Clone, Debug)]
struct Task {
    id: u64,
    title: &'static str,
}

#[derive(Clone, StatefulView)]
pub(crate) struct SwipeScreen;

pub(crate) struct SwipeState {
    inbox: Signal<Vec<Message>>,
    tasks: Signal<Vec<Task>>,
    archived: Signal<usize>,
    completed: Signal<usize>,
}

impl StatefulView for SwipeScreen {
    type State = SwipeState;

    fn create_state(&self) -> Self::State {
        SwipeState {
            inbox: Signal::default(),
            tasks: Signal::default(),
            archived: Signal::default(),
            completed: Signal::default(),
        }
    }
}

impl ViewState<SwipeScreen> for SwipeState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let message = |id, from, subject| Message {
            id,
            from,
            subject,
            pinned: false,
        };
        self.inbox = ctx.signal(vec![
            message(1, "Ada", "Design review moved to Thursday"),
            message(2, "Brook", "Your order has shipped"),
            message(3, "Chen", "Lunch on Friday?"),
            message(4, "Dana", "Quarterly numbers attached"),
        ]);
        self.tasks = ctx.signal(vec![
            Task {
                id: 1,
                title: "Water the plants",
            },
            Task {
                id: 2,
                title: "Book the train",
            },
            Task {
                id: 3,
                title: "Call the bank",
            },
        ]);
        self.archived = ctx.signal(0);
        self.completed = ctx.signal(0);
    }

    fn build(&self, _view: &SwipeScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let (inbox, tasks, archived, completed) =
            (self.inbox, self.tasks, self.archived, self.completed);
        let pinned = inbox.with(ctx, |list| list.iter().filter(|m| m.pinned).count());
        let readout = format!(
            "{} in inbox, {} pinned, {} archived, {} tasks left, {} completed",
            inbox.with(ctx, Vec::len),
            pinned,
            archived.get(ctx),
            tasks.with(ctx, Vec::len),
            completed.get(ctx),
        );

        let mut rows: Vec<BoxedView> = vec![
            style::strong("Inbox").boxed(),
            SizedBox::height(8.0).boxed(),
        ];
        inbox.with(ctx, |list| {
            for message in list {
                rows.push(Keyed::new(message.id, message_row(message)).boxed());
                rows.push(divider().into_view().boxed());
            }
        });
        rows.push(SizedBox::height(20.0).boxed());
        rows.push(style::strong("Tasks").boxed());
        rows.push(SizedBox::height(8.0).boxed());
        tasks.with(ctx, |list| {
            for task in list {
                rows.push(Keyed::new(task.id, task_row(task)).boxed());
                rows.push(divider().into_view().boxed());
            }
        });

        let content = Actions::new(
            Scrollable::new()
                .child(Column::new(rows).cross_axis_alignment(CrossAxisAlignment::Stretch)),
        )
        .action(CallbackAction::new(move |cx, intent: &Pin| {
            inbox.update(cx, |list| {
                if let Some(message) = list.iter_mut().find(|m| m.id == intent.0) {
                    message.pinned = !message.pinned;
                }
            })
        }))
        .action(CallbackAction::new(move |cx, intent: &Archive| {
            inbox.update(cx, |list| list.retain(|m| m.id != intent.0))?;
            archived.update(cx, |n| *n += 1)
        }))
        .action(CallbackAction::new(move |cx, intent: &Complete| {
            tasks.update(cx, |list| list.retain(|t| t.id != intent.0))?;
            completed.update(cx, |n| *n += 1)
        }));

        page(
            "Swipe actions",
            "Swipe a message left to archive it or right to pin it. Swipe a task away to complete \
             it. Right-click or long-press a message for the same actions; Tab reaches each row.",
            style::readout(readout),
            content,
        )
    }
}

fn message_row(message: &Message) -> impl IntoView {
    let pin_label = if message.pinned { "Unpin" } else { "Pin" };
    let content = list_row(
        64.0,
        Row::new(row![
            avatar(message.from.chars().next().unwrap_or('?'), style::ACCENT),
            SizedBox::width(12.0),
            Expanded::new(
                Column::new(column![
                    style::strong(if message.pinned {
                        format!("{} (pinned)", message.from)
                    } else {
                        message.from.to_owned()
                    }),
                    style::caption(message.subject),
                ])
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Start),
            ),
        ]),
    );
    SwipeActions::new(
        content,
        [SwipeAction::new(
            GestureCommand::new(format!("{pin_label} {}", message.from), Pin(message.id)),
            action_face(pin_label, style::PIN),
        )],
        [SwipeAction::new(
            GestureCommand::new(format!("Archive {}", message.from), Archive(message.id)),
            action_face("Archive", style::ARCHIVE),
        )],
        SwipeAlternative::ContextMenu,
    )
    .expect("BUG: a message's two actions are labelled by its sender")
    .full_swipe(SwipeEdge::Trailing)
    .target(message.from)
}

fn task_row(task: &Task) -> impl IntoView {
    let done = GestureCommand::new(format!("Complete {}", task.title), Complete(task.id));
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        done,
        Padding::symmetric(12.0, 0.0).child(style::tinted("Done", style::ACCENT)),
    )))
    .expect("BUG: a task's one command is labelled by its title")
    .target(task.title);
    Dismissible::new(
        list_row(56.0, style::tinted(task.title, style::INK)),
        alternatives,
    )
    .background(swipe_background("Completed", Alignment::CENTER_LEFT))
    .secondary_background(swipe_background("Completed", Alignment::CENTER_RIGHT))
}

/// What a revealed action shows: its label on its colour.
fn action_face(label: &str, color: Color) -> impl IntoView {
    Container::new()
        .color(color)
        .alignment(Alignment::CENTER)
        .child(style::tinted(label, style::SURFACE))
}

/// What a dismissal uncovers behind the sliding row.
fn swipe_background(label: &str, alignment: Alignment) -> impl IntoView {
    Container::new()
        .color(style::ARCHIVE)
        .padding(EdgeInsets::symmetric(0.0, 20.0))
        .alignment(alignment)
        .child(style::tinted(label, style::SURFACE))
}
