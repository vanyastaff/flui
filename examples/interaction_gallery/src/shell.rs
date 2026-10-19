//! The frame around the screens: a sidebar on a wide window, a scrolling
//! tab strip on a narrow one, and the selected screen beside or below it.

use flui::prelude::*;
use flui::widgets::column;

use crate::screens::Screen;
use crate::style;

/// Below this width the sidebar becomes a tab strip above the screen.
const NARROW: f64 = 720.0;

#[derive(Clone, StatefulView)]
pub(crate) struct Shell;

pub(crate) struct ShellState {
    screen: StateCell<Screen>,
}

impl StatefulView for Shell {
    type State = ShellState;

    fn create_state(&self) -> Self::State {
        ShellState {
            screen: StateCell::new(Screen::Swipe),
        }
    }
}

impl ViewState<Shell> for ShellState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.screen.bind(ctx);
    }

    fn build(&self, _view: &Shell, _ctx: &dyn BuildContext) -> impl IntoView {
        let screen = self.screen.clone();
        LayoutBuilder::new(move |_ctx, constraints| {
            let selected = screen.get();
            // Each part is a traversal group, so Tab walks the whole
            // navigation and then the whole screen, rather than interleaving
            // the two by height.
            let content = FocusTraversalGroup::new(
                Container::new()
                    .color(style::BACKGROUND)
                    .padding(EdgeInsets::all(20.0))
                    .child(selected.view()),
            );
            if constraints.max_width < NARROW {
                Column::new(column![
                    Container::new().color(style::SURFACE).height(52.0).child(
                        FocusTraversalGroup::new(
                            Scrollable::new()
                                .scroll_direction(Axis::Horizontal)
                                .child(Row::new(entries(&screen, selected, false))),
                        )
                    ),
                    Expanded::new(content),
                ])
                .boxed()
            } else {
                Row::new(vec![
                    Container::new()
                        .color(style::SURFACE)
                        .width(220.0)
                        .padding(EdgeInsets::symmetric(16.0, 12.0))
                        .child(FocusTraversalGroup::new(
                            Column::new(
                                std::iter::once(
                                    Padding::only(8.0, 4.0, 8.0, 12.0)
                                        .child(style::strong("Interaction gallery"))
                                        .boxed(),
                                )
                                .chain(entries(&screen, selected, true))
                                .collect::<Vec<_>>(),
                            )
                            .cross_axis_alignment(CrossAxisAlignment::Stretch),
                        ))
                        .boxed(),
                    Expanded::new(content).boxed(),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Stretch)
                .boxed()
            }
        })
    }
}

/// One button per screen, the selected one highlighted.
fn entries(screen: &StateCell<Screen>, selected: Screen, vertical: bool) -> Vec<BoxedView> {
    Screen::ALL
        .iter()
        .map(|&entry| {
            let screen = screen.clone();
            let chosen = entry == selected;
            let (fill, ink) = if chosen {
                (style::ACCENT_SOFT, style::ACCENT)
            } else {
                (style::SURFACE, style::INK)
            };
            let label = Container::new()
                .decoration(style::rounded(fill, 8.0))
                .padding(EdgeInsets::symmetric(9.0, 12.0))
                .child(style::tinted(entry.name(), ink));
            let button = RawButton::new(label).on_press(move |_cx| screen.set(entry));
            if vertical {
                Padding::only(0.0, 2.0, 0.0, 2.0).child(button).boxed()
            } else {
                Padding::all(6.0).child(button).boxed()
            }
        })
        .collect()
}
