//! Dashboard Layout Example
//!
//! Demonstrates building a dashboard with multiple cards using:
//! - Row and Column for main layout
//! - Card widgets for information panels
//! - Container for spacing and styling
//! - Wrap for responsive grid
//! - Stack for layered content

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Dashboard application
#[derive(Debug, Clone)]
struct DashboardApp;

flui_core::impl_into_widget!(DashboardApp, stateless);

impl StatelessWidget for DashboardApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(245, 245, 245))
            .child(
                Column::builder()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(vec![
                        // Header
                        build_header(),

                        SizedBox::builder()
                            .height(20.0)
                            .build()
                            .into(),

                        // Stats Row
                        Row::builder()
                            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                            .children(vec![
                                Flexible::builder()
                                    .child(build_stat_card(
                                        "Total Users",
                                        "12,458",
                                        "+12.5%",
                                        Color::rgb(76, 175, 80),
                                    ))
                                    .build()
                                    .into(),
                                SizedBox::builder().width(16.0).build().into(),
                                Flexible::builder()
                                    .child(build_stat_card(
                                        "Revenue",
                                        "$45,678",
                                        "+8.2%",
                                        Color::rgb(33, 150, 243),
                                    ))
                                    .build()
                                    .into(),
                                SizedBox::builder().width(16.0).build().into(),
                                Flexible::builder()
                                    .child(build_stat_card(
                                        "Active Sessions",
                                        "1,892",
                                        "-3.1%",
                                        Color::rgb(244, 67, 54),
                                    ))
                                    .build()
                                    .into(),
                            ])
                            .build()
                            .into(),

                        SizedBox::builder()
                            .height(20.0)
                            .build()
                            .into(),

                        // Activity Feed and Quick Actions
                        Row::builder()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .children(vec![
                                Flexible::builder()
                                    .flex(2)
                                    .child(build_activity_feed())
                                    .build()
                                    .into(),
                                SizedBox::builder().width(16.0).build().into(),
                                Flexible::builder()
                                    .flex(1)
                                    .child(build_quick_actions())
                                    .build()
                                    .into(),
                            ])
                            .build()
                            .into(),
                    ])
                    .build()
            )
            .build()
    }
}

/// Build dashboard header
fn build_header() -> Widget {
    Container::builder()
        .padding(EdgeInsets::symmetric(0.0, 16.0))
        .child(
            Row::builder()
                .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .children(vec![
                    Column::builder()
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children(vec![
                            Text::builder()
                                .data("Dashboard")
                                .size(32.0)
                                .color(Color::rgb(33, 33, 33))
                                .build()
                                .into(),
                            SizedBox::builder().height(4.0).build().into(),
                            Text::builder()
                                .data("Welcome back, John!")
                                .size(16.0)
                                .color(Color::rgb(117, 117, 117))
                                .build()
                                .into(),
                        ])
                        .build()
                        .into(),
                    Button::builder()
                        .text("Refresh")
                        .color(Color::rgb(103, 58, 183))
                        .build()
                        .into(),
                ])
                .build()
        )
        .build()
}

/// Build a stat card
fn build_stat_card(title: &str, value: &str, change: &str, color: Color) -> Widget {
    Card::builder()
        .child(
            Container::builder()
                .padding(EdgeInsets::all(20.0))
                .child(
                    Column::builder()
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children(vec![
                            Text::builder()
                                .data(title)
                                .size(14.0)
                                .color(Color::rgb(117, 117, 117))
                                .build()
                                .into(),
                            SizedBox::builder().height(8.0).build().into(),
                            Text::builder()
                                .data(value)
                                .size(28.0)
                                .color(Color::rgb(33, 33, 33))
                                .build()
                                .into(),
                            SizedBox::builder().height(8.0).build().into(),
                            Container::builder()
                                .padding(EdgeInsets::symmetric(6.0, 8.0))
                                .decoration(BoxDecoration {
                                    color: Some(color.with_opacity(0.1)),
                                    border_radius: Some(BorderRadius::circular(4.0)),
                                    ..Default::default()
                                })
                                .child(
                                    Text::builder()
                                        .data(change)
                                        .size(12.0)
                                        .color(color)
                                        .build()
                                )
                                .build()
                                .into(),
                        ])
                        .build()
                )
                .build()
        )
        .build()
}

/// Build activity feed
fn build_activity_feed() -> Widget {
    Card::builder()
        .child(
            Container::builder()
                .padding(EdgeInsets::all(20.0))
                .child(
                    Column::builder()
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children(vec![
                            Text::builder()
                                .data("Recent Activity")
                                .size(20.0)
                                .color(Color::rgb(33, 33, 33))
                                .build()
                                .into(),
                            SizedBox::builder().height(16.0).build().into(),
                            Divider::builder().build().into(),
                            SizedBox::builder().height(12.0).build().into(),
                            build_activity_item("New user registration", "2 min ago"),
                            build_activity_item("Payment received: $299", "15 min ago"),
                            build_activity_item("Server backup completed", "1 hour ago"),
                            build_activity_item("System update available", "2 hours ago"),
                        ])
                        .build()
                )
                .build()
        )
        .build()
}

/// Build activity item
fn build_activity_item(title: &str, time: &str) -> Widget {
    Container::builder()
        .padding(EdgeInsets::symmetric(0.0, 8.0))
        .child(
            Row::builder()
                .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .children(vec![
                    Text::builder()
                        .data(title)
                        .size(14.0)
                        .color(Color::rgb(33, 33, 33))
                        .build()
                        .into(),
                    Text::builder()
                        .data(time)
                        .size(12.0)
                        .color(Color::rgb(158, 158, 158))
                        .build()
                        .into(),
                ])
                .build()
        )
        .build()
}

/// Build quick actions panel
fn build_quick_actions() -> Widget {
    Card::builder()
        .child(
            Container::builder()
                .padding(EdgeInsets::all(20.0))
                .child(
                    Column::builder()
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .children(vec![
                            Text::builder()
                                .data("Quick Actions")
                                .size(20.0)
                                .color(Color::rgb(33, 33, 33))
                                .build()
                                .into(),
                            SizedBox::builder().height(16.0).build().into(),
                            Divider::builder().build().into(),
                            SizedBox::builder().height(12.0).build().into(),
                            Button::builder()
                                .text("Add User")
                                .color(Color::rgb(76, 175, 80))
                                .build()
                                .into(),
                            SizedBox::builder().height(8.0).build().into(),
                            Button::builder()
                                .text("Generate Report")
                                .color(Color::rgb(33, 150, 243))
                                .build()
                                .into(),
                            SizedBox::builder().height(8.0).build().into(),
                            Button::builder()
                                .text("View Analytics")
                                .color(Color::rgb(255, 152, 0))
                                .build()
                                .into(),
                            SizedBox::builder().height(8.0).build().into(),
                            Button::builder()
                                .text("Settings")
                                .color(Color::rgb(158, 158, 158))
                                .build()
                                .into(),
                        ])
                        .build()
                )
                .build()
        )
        .build()
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Dashboard Example ===");
    println!("Demonstrates:");
    println!("  • Complex layout with Row and Column");
    println!("  • Multiple Card widgets for panels");
    println!("  • Flexible widgets for responsive layout");
    println!("  • Stats cards with dynamic data");
    println!("  • Activity feed and quick actions");
    println!();

    run_app(DashboardApp.into_widget())
}
