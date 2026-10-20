//! A `CollapsingHeader` over an itinerary: scrolling the list collapses the
//! header first, and scrolling back to the top expands it.

use flui::painting::typography::{FontWeight, TextStyle};
use flui::prelude::*;
use flui::widgets::{AnimatedBuilder, CollapsingHeader, HeaderExtents, column, row};

use super::{divider, list_row, page};
use crate::style;

/// The header's extents: a title bar collapsed, a hero panel expanded.
const COLLAPSED: f64 = 64.0;
const EXPANDED: f64 = 200.0;

const STOPS: [(&str, &str); 12] = [
    ("08:10", "Train to Porto"),
    ("11:30", "Ribeira walk"),
    ("13:00", "Lunch at the market"),
    ("14:30", "Port cellar tour"),
    ("16:00", "Livraria Lello"),
    ("17:15", "Clérigos tower"),
    ("18:30", "Sunset at the garden"),
    ("20:00", "Dinner by the river"),
    ("22:00", "Back to the hotel"),
    ("09:00", "Breakfast, day two"),
    ("10:30", "Serralves museum"),
    ("13:00", "Beach at Foz"),
];

#[derive(Clone, StatefulView)]
pub(crate) struct CollapsingScreen;

pub(crate) struct CollapsingState {
    list: ScrollController,
}

impl StatefulView for CollapsingScreen {
    type State = CollapsingState;

    fn create_state(&self) -> Self::State {
        CollapsingState {
            list: ScrollController::new(),
        }
    }
}

impl ViewState<CollapsingScreen> for CollapsingState {
    fn build(&self, _view: &CollapsingScreen, _ctx: &dyn BuildContext) -> impl IntoView {
        let rows: Vec<BoxedView> = STOPS
            .iter()
            .flat_map(|&(time, what)| {
                [
                    list_row(
                        56.0,
                        Row::new(row![
                            SizedBox::width(56.0).child(style::caption(time)),
                            style::tinted(what, style::INK),
                        ]),
                    )
                    .into_view()
                    .boxed(),
                    divider().into_view().boxed(),
                ]
            })
            .collect();
        let header = Container::new()
            .color(style::ACCENT)
            .padding(EdgeInsets::all(20.0))
            .alignment(Alignment::BOTTOM_LEFT)
            .child(
                Column::new(column![
                    Text::new("Porto, two days").style(
                        TextStyle::new()
                            .with_color(style::SURFACE)
                            .with_font_size(26.0)
                            .with_font_weight(FontWeight::BOLD),
                    ),
                    SizedBox::height(4.0),
                    style::tinted("12 stops", style::ACCENT_SOFT),
                ])
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Start),
            );
        let extents =
            HeaderExtents::new(COLLAPSED, EXPANDED).expect("BUG: ordered, finite extents");
        let body = CollapsingHeader::new(
            extents,
            header,
            Scrollable::new()
                .controller(self.list.clone())
                .semantic_label("Itinerary")
                .child(Column::new(rows).cross_axis_alignment(CrossAxisAlignment::Stretch)),
        )
        .semantic_label("Trip header");

        let list = self.list.clone();
        let readout = AnimatedBuilder::new(self.list.listenable(), move || {
            style::readout(format!("Itinerary scrolled {} px", list.pixels().round()))
        });
        page(
            "Collapsing header",
            "Scroll the itinerary: the blue header collapses to a bar before the list moves, \
             and expands again once the list is back at its top.",
            readout,
            ClipRRect::circular(12.0).child(body),
        )
    }
}
