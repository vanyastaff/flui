//! Pricing Table Example
//!
//! Demonstrates building a pricing comparison table using:
//! - Row for horizontal layout of pricing cards
//! - Card for each pricing tier
//! - Column for vertical content organization
//! - Container for styling and spacing
//! - Divider for visual separation

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Pricing table application
#[derive(Debug, Clone)]
struct PricingTableApp;

flui_core::impl_into_widget!(PricingTableApp, stateless);

impl StatelessWidget for PricingTableApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(40.0))
            .color(Color::rgb(250, 250, 250))
            .child(
                Column::builder()
                    .cross_axis_alignment(CrossAxisAlignment::Center)
                    .children(vec![
                        // Header
                        build_pricing_header(),

                        SizedBox::builder().height(40.0).build().into(),

                        // Pricing Cards
                        Row::builder()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .children(vec![
                                build_pricing_card(
                                    "Starter",
                                    "$9",
                                    "per month",
                                    vec![
                                        "5 Projects",
                                        "10 GB Storage",
                                        "Basic Support",
                                        "Email Notifications",
                                    ],
                                    Color::rgb(117, 117, 117),
                                    false,
                                ),
                                SizedBox::builder().width(24.0).build().into(),
                                build_pricing_card(
                                    "Professional",
                                    "$29",
                                    "per month",
                                    vec![
                                        "Unlimited Projects",
                                        "100 GB Storage",
                                        "Priority Support",
                                        "Advanced Analytics",
                                        "API Access",
                                        "Team Collaboration",
                                    ],
                                    Color::rgb(33, 150, 243),
                                    true, // Popular
                                ),
                                SizedBox::builder().width(24.0).build().into(),
                                build_pricing_card(
                                    "Enterprise",
                                    "$99",
                                    "per month",
                                    vec![
                                        "Unlimited Everything",
                                        "1 TB Storage",
                                        "24/7 Dedicated Support",
                                        "Custom Integrations",
                                        "SLA Guarantee",
                                        "Security Audit",
                                        "Training Sessions",
                                    ],
                                    Color::rgb(156, 39, 176),
                                    false,
                                ),
                            ])
                            .build()
                            .into(),
                    ])
                    .build()
            )
            .build()
    }
}

/// Build pricing page header
fn build_pricing_header() -> Widget {
    Column::builder()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .children(vec![
            Text::builder()
                .data("Choose Your Plan")
                .size(36.0)
                .color(Color::rgb(33, 33, 33))
                .build()
                .into(),
            SizedBox::builder().height(12.0).build().into(),
            Text::builder()
                .data("Select the perfect plan for your needs")
                .size(18.0)
                .color(Color::rgb(117, 117, 117))
                .build()
                .into(),
            SizedBox::builder().height(8.0).build().into(),
            Text::builder()
                .data("All plans include a 14-day free trial")
                .size(14.0)
                .color(Color::rgb(76, 175, 80))
                .build()
                .into(),
        ])
        .build()
}

/// Build a pricing card
fn build_pricing_card(
    name: &str,
    price: &str,
    period: &str,
    features: Vec<&str>,
    color: Color,
    popular: bool,
) -> Widget {
    let card_content = Column::builder()
        .main_axis_size(MainAxisSize::Min)
        .cross_axis_alignment(CrossAxisAlignment::Stretch)
        .children({
            let mut children = vec![];

            // Popular badge
            if popular {
                children.push(
                    Container::builder()
                        .padding(EdgeInsets::symmetric(6.0, 12.0))
                        .decoration(BoxDecoration {
                            color: Some(Color::rgb(255, 152, 0)),
                            border_radius: Some(BorderRadius::only(
                                12.0, 12.0, 0.0, 0.0
                            )),
                            ..Default::default()
                        })
                        .child(
                            Center::builder()
                                .child(
                                    Text::builder()
                                        .data("MOST POPULAR")
                                        .size(12.0)
                                        .color(Color::WHITE)
                                        .build()
                                )
                                .build()
                        )
                        .build()
                        .into()
                );
            }

            // Main card content
            children.push(
                Container::builder()
                    .padding(EdgeInsets::all(32.0))
                    .child(
                        Column::builder()
                            .main_axis_size(MainAxisSize::Min)
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .children({
                                let mut content = vec![
                                    // Plan name
                                    Text::builder()
                                        .data(name)
                                        .size(24.0)
                                        .color(Color::rgb(33, 33, 33))
                                        .build()
                                        .into(),

                                    SizedBox::builder().height(16.0).build().into(),

                                    // Price
                                    Row::builder()
                                        .main_axis_size(MainAxisSize::Min)
                                        .cross_axis_alignment(CrossAxisAlignment::End)
                                        .children(vec![
                                            Text::builder()
                                                .data(price)
                                                .size(48.0)
                                                .color(color)
                                                .build()
                                                .into(),
                                            SizedBox::builder().width(8.0).build().into(),
                                            Container::builder()
                                                .padding(EdgeInsets::only(0.0, 0.0, 0.0, 8.0))
                                                .child(
                                                    Text::builder()
                                                        .data(period)
                                                        .size(16.0)
                                                        .color(Color::rgb(117, 117, 117))
                                                        .build()
                                                )
                                                .build()
                                                .into(),
                                        ])
                                        .build()
                                        .into(),

                                    SizedBox::builder().height(24.0).build().into(),

                                    Divider::builder().build().into(),

                                    SizedBox::builder().height(24.0).build().into(),
                                ];

                                // Features list
                                for feature in features {
                                    content.push(
                                        Container::builder()
                                            .padding(EdgeInsets::symmetric(0.0, 8.0))
                                            .child(
                                                Row::builder()
                                                    .main_axis_size(MainAxisSize::Min)
                                                    .children(vec![
                                                        Container::builder()
                                                            .width(20.0)
                                                            .height(20.0)
                                                            .decoration(BoxDecoration {
                                                                color: Some(color.with_opacity(0.1)),
                                                                border_radius: Some(BorderRadius::circular(10.0)),
                                                                ..Default::default()
                                                            })
                                                            .child(
                                                                Center::builder()
                                                                    .child(
                                                                        Text::builder()
                                                                            .data("✓")
                                                                            .size(14.0)
                                                                            .color(color)
                                                                            .build()
                                                                    )
                                                                    .build()
                                                            )
                                                            .build()
                                                            .into(),
                                                        SizedBox::builder().width(12.0).build().into(),
                                                        Text::builder()
                                                            .data(feature)
                                                            .size(15.0)
                                                            .color(Color::rgb(66, 66, 66))
                                                            .build()
                                                            .into(),
                                                    ])
                                                    .build()
                                            )
                                            .build()
                                            .into()
                                    );
                                }

                                content.push(SizedBox::builder().height(32.0).build().into());

                                // CTA Button
                                content.push(
                                    Button::builder()
                                        .text(if popular { "Get Started" } else { "Choose Plan" })
                                        .color(if popular { color } else { Color::rgb(238, 238, 238) })
                                        .build()
                                        .into()
                                );

                                content
                            })
                            .build()
                    )
                    .build()
                    .into()
            );

            children
        })
        .build();

    SizedBox::builder()
        .width(280.0)
        .child(
            Card::builder()
                .child(card_content)
                .build()
        )
        .build()
}

fn main() -> Result<(), eframe::Error> {
    println!("=== Pricing Table Example ===");
    println!("Demonstrates:");
    println!("  • Horizontal layout with Row");
    println!("  • Multiple Card widgets for pricing tiers");
    println!("  • Feature lists with checkmarks");
    println!("  • Popular badge overlay");
    println!("  • Professional pricing page design");
    println!("  • Responsive card sizing");
    println!();

    run_app(PricingTableApp.into_widget())
}
