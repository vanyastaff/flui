//! Profile Card Example - NEW View Architecture
//!
//! Demonstrates building a beautiful profile card using:
//! - Card widget for elevation and styling
//! - Row and Column for layout
//! - Container for spacing and decoration
//! - Text for content
//! - ClipOval for circular avatar
//! - Divider for visual separation

use flui_app::run_app;
use flui_core::element::ElementTree;
use flui_core::foundation::ElementId;
use flui_core::render::SingleRender;
use flui_core::view::SingleRenderBuilder;
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_engine::{layer::pool, BoxedLayer};
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
use flui_types::Offset;
use flui_types::{BoxConstraints, Color, EdgeInsets, Size};
use flui_widgets::prelude::*;
use flui_widgets::{
    Button, Card, Center, ClipOval, Column, ConstrainedBox, Container, Divider, Row, SizedBox, Text,
};

/// Simple Scaffold that fills entire screen with background color
#[derive(Clone)]
struct Scaffold {
    background_color: Color,
    padding: EdgeInsets,
    child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Scaffold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scaffold")
            .field("background_color", &self.background_color)
            .field("padding", &self.padding)
            .field("child", &"<AnyView>")
            .finish()
    }
}

impl Scaffold {
    fn new(background_color: Color, padding: EdgeInsets) -> Self {
        Self {
            background_color,
            padding,
            child: None,
        }
    }
}

/// RenderScaffold - fills entire screen with background color
#[derive(Debug)]
struct RenderScaffold {
    color: Color,
    padding: EdgeInsets,
    size: Size,
}

impl RenderScaffold {
    fn new(color: Color, padding: EdgeInsets) -> Self {
        Self {
            color,
            padding,
            size: Size::ZERO,
        }
    }
}

impl SingleRender for RenderScaffold {
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Fill entire available space
        let size = constraints.biggest();

        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderScaffold::layout: constraints={:?}, size={:?}",
            constraints,
            size
        );

        // Layout child with deflated constraints (subtract padding)
        let child_constraints = constraints.deflate(&self.padding);
        let _child_size = tree.layout_child(child_id, child_constraints);

        // Store size for paint
        self.size = size;

        #[cfg(debug_assertions)]
        tracing::debug!("RenderScaffold::layout: stored size={:?}", self.size);

        size
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "RenderScaffold::paint: size={:?}, color={:?}, offset={:?}",
            self.size,
            self.color,
            offset
        );

        // Create background layer
        let mut picture = pool::acquire_picture();

        // Draw background rectangle
        let paint = flui_engine::Paint {
            color: self.color,
            ..Default::default()
        };
        let rect = flui_types::Rect::from_ltwh(0.0, 0.0, self.size.width, self.size.height);

        #[cfg(debug_assertions)]
        tracing::debug!("RenderScaffold::paint: drawing rect={:?}", rect);

        picture.draw_rect(rect, paint);

        let background_layer: BoxedLayer = Box::new(flui_engine::PooledPictureLayer::new(picture));

        // Paint child with padding offset
        let padding_offset = Offset::new(self.padding.left, self.padding.top);
        let child_offset = offset + padding_offset;
        let child_layer = tree.paint_child(child_id, child_offset);

        // Combine layers
        let mut container = flui_engine::ContainerLayer::new();
        container.add_child(background_layer);
        container.add_child(child_layer);

        Box::new(container)
    }
}

impl View for Scaffold {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        SingleRenderBuilder::new(RenderScaffold::new(self.background_color, self.padding))
            .with_optional_child(self.child)
    }
}

/// Profile card application - wraps Scaffold directly
#[derive(Debug, Clone)]
struct ProfileCardApp {
    scaffold: Scaffold,
}

impl ProfileCardApp {
    fn new() -> Self {
        let mut scaffold = Scaffold::new(Color::rgb(240, 240, 245), EdgeInsets::all(40.0));
        scaffold.child = Some(Box::new(CenteredCard));
        Self { scaffold }
    }
}

impl View for ProfileCardApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        self.scaffold
    }
}

/// Centered card widget
#[derive(Debug, Clone)]
struct CenteredCard;

impl View for CenteredCard {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut center = Center::builder().build();
        center.child = Some(Box::new(ProfileCard));
        center
    }
}

/// Main profile card
#[derive(Debug, Clone)]
struct ProfileCard;

impl View for ProfileCard {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut card = Card::builder().elevation(2.0).build_card();

        card.child = Some(Box::new(CardContent));
        card
    }
}

/// Card content
#[derive(Debug, Clone)]
struct CardContent;

impl View for CardContent {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut container = Container::builder()
            .width(350.0)
            .padding(EdgeInsets::all(24.0))
            .build_container();

        container.child = Some(Box::new(ProfileColumn));
        container
    }
}

/// Profile column with all content
#[derive(Debug, Clone)]
struct ProfileColumn;

impl View for ProfileColumn {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Column::builder()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(vec![
                Box::new(Avatar) as Box<dyn AnyView>,
                Box::new(SizedBox::builder().height(16.0).build()),
                Box::new(NameText),
                Box::new(SizedBox::builder().height(8.0).build()),
                Box::new(TitleText),
                Box::new(SizedBox::builder().height(16.0).build()),
                Box::new(ProfileDivider),
                Box::new(SizedBox::builder().height(16.0).build()),
                Box::new(StatsRow),
                Box::new(SizedBox::builder().height(20.0).build()),
                Box::new(ActionButtons),
            ])
            .build()
    }
}

/// Avatar widget
#[derive(Debug, Clone)]
struct Avatar;

impl View for Avatar {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut clip = ClipOval::builder().build();
        clip.child = Some(Box::new(AvatarContainer));
        clip
    }
}

#[derive(Debug, Clone)]
struct AvatarContainer;

impl View for AvatarContainer {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut container = Container::builder()
            .width(100.0)
            .height(100.0)
            .color(Color::rgb(100, 181, 246))
            .build_container();

        container.child = Some(Box::new(AvatarCenter));
        container
    }
}

#[derive(Debug, Clone)]
struct AvatarCenter;

impl View for AvatarCenter {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut center = Center::builder().build();
        center.child = Some(Box::new(
            Text::builder()
                .data("JD")
                .size(40.0)
                .color(Color::WHITE)
                .build(),
        ));
        center
    }
}

/// Name text
#[derive(Debug, Clone)]
struct NameText;

impl View for NameText {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Text::builder()
            .data("John Doe")
            .size(24.0)
            .color(Color::rgb(33, 33, 33))
            .build()
    }
}

/// Title text
#[derive(Debug, Clone)]
struct TitleText;

impl View for TitleText {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Text::builder()
            .data("Senior Rust Developer")
            .size(16.0)
            .color(Color::rgb(117, 117, 117))
            .build()
    }
}

/// Divider
#[derive(Debug, Clone)]
struct ProfileDivider;

impl View for ProfileDivider {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Divider::builder()
            .color(Color::rgb(224, 224, 224))
            .build_divider()
    }
}

/// Stats row
#[derive(Debug, Clone)]
struct StatsRow;

impl View for StatsRow {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Row::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .children(vec![
                Box::new(Stat {
                    value: "128",
                    label: "Posts",
                }) as Box<dyn AnyView>,
                Box::new(Stat {
                    value: "2.5K",
                    label: "Followers",
                }),
                Box::new(Stat {
                    value: "312",
                    label: "Following",
                }),
            ])
            .build()
    }
}

/// Stat widget
#[derive(Debug, Clone)]
struct Stat {
    value: &'static str,
    label: &'static str,
}

impl View for Stat {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Column::builder()
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(vec![
                Box::new(
                    Text::builder()
                        .data(self.value)
                        .size(20.0)
                        .color(Color::rgb(33, 33, 33))
                        .build(),
                ) as Box<dyn AnyView>,
                Box::new(SizedBox::builder().height(4.0).build()),
                Box::new(
                    Text::builder()
                        .data(self.label)
                        .size(14.0)
                        .color(Color::rgb(117, 117, 117))
                        .build(),
                ),
            ])
            .build()
    }
}

/// Action buttons
#[derive(Debug, Clone)]
struct ActionButtons;

impl View for ActionButtons {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        Row::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .children(vec![
                Box::new(
                    Button::builder("Follow")
                        .color(Color::rgb(33, 150, 243))
                        .build(),
                ) as Box<dyn AnyView>,
                Box::new(
                    Button::builder("Message")
                        .color(Color::rgb(156, 39, 176))
                        .build(),
                ),
            ])
            .build()
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Profile Card Example - NEW View Architecture ===");
    println!("Demonstrates:");
    println!("  • Card widget with elevation");
    println!("  • Row and Column layout");
    println!("  • ClipOval for circular avatar");
    println!("  • Divider for visual separation");
    println!("  • Button widgets for actions");
    println!();

    run_app(Box::new(ProfileCardApp::new()))
}
