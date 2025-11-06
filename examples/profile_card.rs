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
use flui_core::view::{AnyView, IntoElement, View};
use flui_core::BuildContext;
use flui_widgets::prelude::*;
use flui_widgets::{
    Button, Card, Center, ClipOval, Column, Container, Divider, Row, SizedBox, Text,
};
use flui_types::{Color, EdgeInsets};
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

/// Profile card application
#[derive(Debug, Clone)]
struct ProfileCardApp;

impl View for ProfileCardApp {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut root = Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(240, 240, 245))
            .build_container();

        root.child = Some(Box::new(CenteredCard));
        root
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

    run_app(Box::new(ProfileCardApp))
}
