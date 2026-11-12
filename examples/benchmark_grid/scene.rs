//! Shared, non-virtualized scene. No timing or host policy lives here.
use std::{cell::Cell, rc::Rc};

use flui_painting::{styling::Color, typography::TextStyle};
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, Column, Padding, RawButton, Row, SingleChildScrollView, SizedBox, Text,
};

pub const WIDTH: u32 = 1600;
pub const HEIGHT: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Labels,
    Buttons,
}

#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub columns: usize,
    pub rows: usize,
    pub sidebar: usize,
}

impl Shape {
    pub fn for_items(items: usize) -> Option<Self> {
        let (columns, rows, sidebar) = match items {
            100 => (10, 8, 20),
            1_000 => (20, 40, 200),
            10_000 => (50, 180, 1_000),
            53_000 => (80, 600, 5_000),
            _ => return None,
        };
        Some(Self {
            columns,
            rows,
            sidebar,
        })
    }

    pub fn items(self) -> usize {
        self.columns * self.rows + self.sidebar
    }
}

#[derive(Clone, StatefulView)]
struct Header {
    text: StateHandle<String>,
}

struct HeaderState {
    text: StateHandle<String>,
}

impl StatefulView for Header {
    type State = HeaderState;
    fn create_state(&self) -> Self::State {
        HeaderState {
            text: self.text.clone(),
        }
    }
}

impl ViewState<Header> for HeaderState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.text.bind(ctx);
    }
    fn build(&self, _view: &Header, _ctx: &dyn BuildContext) -> impl IntoView {
        Text::new(self.text.with(Clone::clone)).style(style(18.0))
    }
}

fn style(size: f64) -> TextStyle {
    TextStyle::new()
        .with_font_family("Roboto")
        .with_font_size(size)
        .with_color(Color::WHITE)
}

#[derive(Clone, StatelessView)]
pub struct GridScene {
    pub shape: Shape,
    pub kind: Kind,
    pub header: StateHandle<String>,
    pub activated: Rc<Cell<Option<usize>>>,
}

impl GridScene {
    pub fn new(shape: Shape, kind: Kind) -> Self {
        Self {
            shape,
            kind,
            header: StateHandle::new(String::from("Benchmark 0")),
            activated: Rc::new(Cell::new(None)),
        }
    }

    fn item(&self, id: usize, sidebar: bool) -> BoxedView {
        let (width, height, text) = if sidebar {
            (244.0, 22.0, format!("Item {id}"))
        } else {
            (14.0, 14.0, String::from("X"))
        };
        let content = ColoredBox::new(Color::rgb(40, 80, 120)).child(
            Text::new(text)
                .style(style(if sidebar { 12.0 } else { 9.0 }))
                .max_lines(1),
        );
        let content = match self.kind {
            Kind::Labels => content.boxed(),
            Kind::Buttons => {
                let activated = self.activated.clone();
                RawButton::new(content)
                    .on_press(move |_| activated.set(Some(id)))
                    .identifier(format!("item-{id}"))
                    .boxed()
            }
        };
        SizedBox::new(width, height).child(content).boxed()
    }
}

impl StatelessView for GridScene {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let sidebar: Vec<BoxedView> = (0..self.shape.sidebar)
            .map(|id| self.item(id, true))
            .collect();
        let rows: Vec<BoxedView> = (0..self.shape.rows)
            .map(|row| {
                let cells: Vec<BoxedView> = (0..self.shape.columns)
                    .map(|column| {
                        self.item(
                            self.shape.sidebar + row * self.shape.columns + column,
                            false,
                        )
                    })
                    .collect();
                Row::new(cells)
                    .spacing(2.0)
                    .main_axis_size(MainAxisSize::Min)
                    .boxed()
            })
            .collect();
        ColoredBox::new(Color::rgb(20, 24, 32)).child(
            Padding::all(8.0).child(
                Column::new(vec![
                    SizedBox::new(1584.0, 48.0)
                        .child(Header {
                            text: self.header.clone(),
                        })
                        .boxed(),
                    SizedBox::height(8.0).boxed(),
                    RepaintBoundary::new()
                        .child(Row::new(vec![
                            SizedBox::new(260.0, 928.0)
                                .child(
                                    Padding::all(8.0).child(
                                        SingleChildScrollView::new().child(
                                            Column::new(sidebar)
                                                .spacing(4.0)
                                                .main_axis_size(MainAxisSize::Min),
                                        ),
                                    ),
                                )
                                .boxed(),
                            SizedBox::width(8.0).boxed(),
                            SizedBox::new(1316.0, 928.0)
                                .child(
                                    SingleChildScrollView::new().child(
                                        Column::new(rows)
                                            .spacing(2.0)
                                            .main_axis_size(MainAxisSize::Min)
                                            .cross_axis_alignment(CrossAxisAlignment::Start),
                                    ),
                                )
                                .boxed(),
                        ]))
                        .boxed(),
                ])
                .cross_axis_alignment(CrossAxisAlignment::Start),
            ),
        )
    }
}
