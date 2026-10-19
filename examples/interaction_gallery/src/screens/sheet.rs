//! An `AnchoredDrag` bottom sheet with three anchors over a map. The sheet
//! holds a scrolling list: at its top, a drag down moves the sheet; scrolled,
//! the list keeps the drag.

use std::fmt;
use std::rc::Rc;

use flui::prelude::*;
use flui::widgets::{
    Alternative, AnchorIntent, AnchoredDrag, AnchoredDragController, AnimatedBuilder, ControlSpec,
    DraggableAnchors, GestureAlternatives, GestureCommand, column, row,
};

use super::{divider, list_row, page};
use crate::painters::MapPainter;
use crate::style;

/// Where the sheet rests: how far its top sits below the top of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    Full,
    Half,
    Peek,
}

impl Stop {
    /// The sheet's top, in logical pixels below the map's.
    pub(crate) const fn offset(self) -> f64 {
        match self {
            Self::Full => 24.0,
            Self::Half => 240.0,
            Self::Peek => 430.0,
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "Full",
            Self::Half => "Half",
            Self::Peek => "Peek",
        })
    }
}

const PLACES: [(&str, &str); 14] = [
    ("Corner Bakery", "Café, 120 m"),
    ("River Park", "Park, 300 m"),
    ("City Library", "Library, 450 m"),
    ("Night Market", "Market, 600 m"),
    ("Old Town Hall", "Landmark, 650 m"),
    ("Glass Museum", "Museum, 800 m"),
    ("Harbour Steps", "Viewpoint, 900 m"),
    ("Tram Depot", "Transit, 1.0 km"),
    ("Hill Garden", "Park, 1.2 km"),
    ("Fish Hall", "Market, 1.3 km"),
    ("Clock Tower", "Landmark, 1.5 km"),
    ("Lighthouse", "Viewpoint, 1.9 km"),
    ("North Station", "Transit, 2.2 km"),
    ("Sea Baths", "Pool, 2.6 km"),
];

#[derive(Clone, StatefulView)]
pub(crate) struct SheetScreen;

pub(crate) struct SheetState {
    sheet: AnchoredDragController<Stop>,
}

impl StatefulView for SheetScreen {
    type State = SheetState;

    fn create_state(&self) -> Self::State {
        let anchors = DraggableAnchors::builder()
            .at(Stop::Full, Stop::Full.offset())
            .at(Stop::Half, Stop::Half.offset())
            .at(Stop::Peek, Stop::Peek.offset())
            .build()
            .expect("BUG: three distinct, ordered anchors");
        SheetState {
            sheet: AnchoredDragController::new(anchors, Stop::Half)
                .expect("BUG: Half is one of the anchors"),
        }
    }
}

impl ViewState<SheetScreen> for SheetState {
    fn build(&self, _view: &SheetScreen, _ctx: &dyn BuildContext) -> impl IntoView {
        let rows: Vec<BoxedView> = PLACES
            .iter()
            .flat_map(|&(name, kind)| {
                [
                    list_row(
                        56.0,
                        Column::new(column![
                            style::tinted(name, style::INK),
                            style::caption(kind)
                        ])
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Start),
                    )
                    .into_view()
                    .boxed(),
                    divider().into_view().boxed(),
                ]
            })
            .collect();
        let handle = Center::new().child(
            Container::new()
                .decoration(style::rounded(style::LINE, 3.0))
                .width(44.0)
                .height(6.0),
        );
        let sheet_face = Container::new()
            .decoration(style::rounded(style::SURFACE, 16.0))
            .child(
                Column::new(column![
                    SizedBox::height(10.0),
                    handle,
                    Padding::only(16.0, 12.0, 16.0, 8.0)
                        .child(Row::new(row![style::strong("Nearby places"),])),
                    Expanded::new(Scrollable::new().semantic_label("Places").child(
                        Column::new(rows).cross_axis_alignment(CrossAxisAlignment::Stretch)
                    )),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Stretch),
            );
        // The sheet moves between its stops by its Expand and Collapse
        // controls too, without a drag.
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Expand", AnchorIntent::Previous),
                Padding::symmetric(8.0, 8.0).child(style::quiet_button("Expand")),
            )
            .control(
                GestureCommand::new("Collapse", AnchorIntent::Next),
                Padding::symmetric(8.0, 8.0).child(style::quiet_button("Collapse")),
            ),
        ))
        .expect("BUG: two distinct labels");
        let sheet = AnchoredDrag::new(self.sheet.clone(), Axis::Vertical, sheet_face, alternatives)
            .semantics("Places sheet", |stop: Stop| stop.to_string());
        let map = CustomPaint::new()
            .painter(Rc::new(MapPainter))
            .child(SizedBox::expand());

        let controller = self.sheet.clone();
        let readout = AnimatedBuilder::new(self.sheet.listenable(), move || {
            let motion = if controller.is_moving() {
                ", moving"
            } else {
                ""
            };
            style::readout(format!("Sheet at {}{motion}", controller.current()))
        });
        page(
            "Bottom sheet",
            "Drag the sheet by its handle or its list between Full, Half and Peek, or use its \
             Expand and Collapse buttons. Tab to the sheet and use the up and down arrows to \
             step between them.",
            readout,
            ClipRRect::circular(12.0).child(Stack::new(vec![
                Positioned::fill(map).boxed(),
                Positioned::fill(sheet).boxed(),
            ])),
        )
    }
}
