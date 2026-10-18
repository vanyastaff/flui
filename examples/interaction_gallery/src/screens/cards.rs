//! Two columns of cards: a card dragged onto the other column moves there.
//! Ctrl+X on a focused card and Ctrl+V on a focused column move it by
//! keyboard, and the Cut and Paste buttons with one pointer.

use flui::interaction::drag::{DragItem, TransferActions};
use flui::prelude::*;
use flui::widgets::{CommandAppearance, DragCommands, DropCommands, DropHover, column};

use super::page;
use crate::keyed::Keyed;
use crate::style;

/// A column of the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lane {
    Todo,
    Done,
}

impl Lane {
    const fn name(self) -> &'static str {
        match self {
            Self::Todo => "To do",
            Self::Done => "Done",
        }
    }
}

#[derive(Clone, Debug)]
struct Card {
    id: u64,
    title: &'static str,
    lane: Lane,
}

#[derive(Clone, StatefulView)]
pub(crate) struct CardsScreen;

pub(crate) struct CardsState {
    cards: Signal<Vec<Card>>,
    last: Signal<Option<(&'static str, Lane)>>,
}

impl StatefulView for CardsScreen {
    type State = CardsState;

    fn create_state(&self) -> Self::State {
        CardsState {
            cards: Signal::default(),
            last: Signal::default(),
        }
    }
}

impl ViewState<CardsScreen> for CardsState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let card = |id, title| Card {
            id,
            title,
            lane: Lane::Todo,
        };
        self.cards = ctx.signal(vec![
            card(1, "Write the release notes"),
            card(2, "Review the layout fix"),
            card(3, "Record the demo"),
        ]);
        self.last = ctx.signal(None);
    }

    fn build(&self, _view: &CardsScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let cards = self.cards.get(ctx);
        let count = |lane| cards.iter().filter(|card| card.lane == lane).count();
        let last = match self.last.get(ctx) {
            Some((title, lane)) => format!(", moved {title} to {}", lane.name()),
            None => String::new(),
        };
        let readout = format!(
            "To do {}, done {}{last}",
            count(Lane::Todo),
            count(Lane::Done)
        );
        let board = Row::new(vec![
            Expanded::new(lane(Lane::Todo, &cards, self.cards, self.last)).boxed(),
            SizedBox::width(16.0).boxed(),
            Expanded::new(lane(Lane::Done, &cards, self.cards, self.last)).boxed(),
        ])
        .cross_axis_alignment(CrossAxisAlignment::Stretch);
        page(
            "Drag and drop",
            "Drag a card onto the other column. With a card focused, Ctrl+X cuts it; with a \
             column focused, Ctrl+V drops it there.",
            style::readout(readout),
            board,
        )
    }
}

/// A column: a drop target holding its cards, tinted while a drag over it
/// would land.
fn lane(
    lane: Lane,
    cards: &[Card],
    model: Signal<Vec<Card>>,
    last: Signal<Option<(&'static str, Lane)>>,
) -> impl IntoView {
    let here: Vec<Card> = cards
        .iter()
        .filter(|card| card.lane == lane)
        .cloned()
        .collect();
    DropTarget::new(
        DropCommands::new(CommandAppearance::new(
            format!("Paste into {}", lane.name()),
            Padding::symmetric(10.0, 6.0).child(style::tinted("Paste", style::ACCENT)),
        )),
        |drag| drag.value::<u64>().map(|_| TransferActions::MOVE),
        move |hover| {
            let fill = match hover {
                DropHover::Accepting(_) => style::ACCENT_SOFT,
                _ => Color::rgb(232, 235, 241),
            };
            let mut children: Vec<BoxedView> = vec![
                style::strong(format!("{} ({})", lane.name(), here.len())).boxed(),
                SizedBox::height(12.0).boxed(),
            ];
            for card in &here {
                children.push(Keyed::new(card.id, card_view(card)).boxed());
                children.push(SizedBox::height(10.0).boxed());
            }
            Container::new()
                .decoration(style::rounded(fill, 12.0))
                .padding(EdgeInsets::all(14.0))
                .child(Column::new(children).cross_axis_alignment(CrossAxisAlignment::Stretch))
                .into_view()
                .boxed()
        },
    )
    .expect("BUG: a column's paste command is labelled by its name")
    .target(lane.name())
    .on_drop(move |cx, drag, _at, _action| {
        let Some(&id) = drag.value::<u64>() else {
            return Ok(());
        };
        let mut moved = None;
        model.update(cx, |list| {
            if let Some(card) = list.iter_mut().find(|card| card.id == id)
                && card.lane != lane
            {
                card.lane = lane;
                moved = Some(card.title);
            }
        })?;
        match moved {
            Some(title) => last.set(cx, Some((title, lane))),
            None => Ok(()),
        }
    })
}

fn card_view(card: &Card) -> impl IntoView {
    let id = card.id;
    let title = card.title;
    let face = move || {
        Container::new()
            .decoration(style::rounded(style::SURFACE, 10.0))
            .padding(EdgeInsets::all(14.0))
            .child(
                Column::new(column![
                    style::tinted(title, style::INK),
                    SizedBox::height(4.0),
                    style::caption("Drag me"),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Start),
            )
    };
    Draggable::new(
        DragCommands::new(CommandAppearance::new(
            format!("Cut {title}"),
            Padding::symmetric(10.0, 6.0).child(style::tinted("Cut", style::ACCENT)),
        )),
        move || vec![DragItem::new().with_value(id).with_text(title)],
        face(),
    )
    .expect("BUG: a card's cut command is labelled by its title")
    .target(title)
    .feedback(move || {
        Opacity::new(0.85)
            .child(SizedBox::width(240.0).child(face()))
            .into_view()
            .boxed()
    })
    .child_when_dragging(move || {
        Container::new()
            .decoration(style::rounded(Color::rgb(214, 219, 228), 10.0))
            .height(64.0)
            .into_view()
            .boxed()
    })
}
