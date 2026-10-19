//! A `PageView` of three onboarding pages, turned by a swipe, the arrow
//! keys while it has focus, or the Previous and Next buttons.

use flui::prelude::*;
use flui::widgets::{
    Alternative, ControlSpec, GestureAlternatives, GestureCommand, PageIntent, column,
};

use super::page;
use crate::style;

const PAGES: [(&str, &str, Color); 3] = [
    (
        "Plan",
        "Collect the places you want to see in one list.",
        Color::rgb(46, 92, 230),
    ),
    (
        "Pack",
        "A checklist that remembers what you forgot last time.",
        Color::rgb(56, 142, 92),
    ),
    (
        "Go",
        "Tickets, maps and bookings, offline on the day.",
        Color::rgb(214, 69, 65),
    ),
];

#[derive(Clone, StatefulView)]
pub(crate) struct PagesScreen;

pub(crate) struct PagesState {
    pages: PageController,
    current: Signal<usize>,
}

impl StatefulView for PagesScreen {
    type State = PagesState;

    fn create_state(&self) -> Self::State {
        PagesState {
            pages: PageController::new(),
            current: Signal::default(),
        }
    }
}

impl ViewState<PagesScreen> for PagesState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.current = ctx.signal(0);
    }

    fn build(&self, _view: &PagesScreen, ctx: &dyn BuildContext) -> impl IntoView {
        let current = self.current;
        let shown = current.get(ctx);
        // The pager turns its own pages for its Previous and Next controls,
        // without a swipe.
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Previous", PageIntent::Previous),
                Padding::symmetric(8.0, 8.0).child(style::quiet_button("Previous")),
            )
            .control(
                GestureCommand::new("Next", PageIntent::Next),
                Padding::symmetric(8.0, 8.0).child(style::button("Next")),
            ),
        ))
        .expect("BUG: two distinct labels");
        let pager = PageView::new(
            PAGES
                .iter()
                .enumerate()
                .map(|(index, &(name, line, color))| {
                    page_face(index, name, line, color).into_view().boxed()
                })
                .collect::<Vec<_>>(),
            alternatives,
        )
        .controller(self.pages.clone())
        .on_page_changed(move |cx, page| current.set(cx, page));

        page(
            "Pages",
            "Swipe between the pages, or focus the pager and use the arrow keys, Page Up and \
             Page Down, Home and End.",
            style::readout(format!(
                "Page {} of {}: {}",
                shown + 1,
                PAGES.len(),
                PAGES.get(shown).map_or("", |page| page.0)
            )),
            ClipRect::new().child(pager),
        )
    }
}

fn page_face(index: usize, name: &str, line: &str, color: Color) -> impl IntoView {
    Container::new()
        .decoration(style::rounded(color, 16.0))
        .margin(EdgeInsets::symmetric(0.0, 4.0))
        .padding(EdgeInsets::all(32.0))
        .alignment(Alignment::BOTTOM_LEFT)
        .child(
            Column::new(column![
                style::tinted(format!("Step {}", index + 1), style::ACCENT_SOFT),
                SizedBox::height(8.0),
                Text::new(name).style(
                    flui::painting::typography::TextStyle::new()
                        .with_color(style::SURFACE)
                        .with_font_size(40.0),
                ),
                SizedBox::height(8.0),
                style::tinted(line, style::SURFACE),
            ])
            .cross_axis_alignment(CrossAxisAlignment::Start),
        )
}
