//! Settings Page Example
//!
//! Demonstrates building a settings interface using:
//! - Card for grouped sections
//! - Row for horizontal layouts
//! - Divider for visual separation
//! - Container for spacing
//! - GestureDetector for interactions

use flui_app::run_app;
use flui_core::{BuildContext, IntoWidget, StatelessWidget, Widget};
use flui_widgets::prelude::*;

/// Settings page application
#[derive(Debug, Clone)]
struct SettingsPageApp;

flui_core::impl_into_widget!(SettingsPageApp, stateless);

impl StatelessWidget for SettingsPageApp {
    fn build(&self, _ctx: &BuildContext) -> Widget {
        Container::builder()
            .padding(EdgeInsets::all(20.0))
            .color(Color::rgb(245, 245, 245))
            .child(
                Column::builder()
                    .cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .children(vec![
                        // Header
                        build_settings_header(),

                        SizedBox::builder().height(24.0).build().into(),

                        // Account Section
                        build_section_card(
                            "Account",
                            vec![
                                build_setting_item("Email", "john.doe@example.com", true),
                                build_setting_item("Password", "••••••••", true),
                                build_setting_item("Two-Factor Auth", "Enabled", false),
                            ]
                        ),

                        SizedBox::builder().height(16.0).build().into(),

                        // Preferences Section
                        build_section_card(
                            "Preferences",
                            vec![
                                build_setting_item("Language", "English", true),
                                build_setting_item("Theme", "Light", true),
                                build_setting_item("Notifications", "On", true),
                                build_setting_item("Auto-save", "Enabled", false),
                            ]
                        ),

                        SizedBox::builder().height(16.0).build().into(),

                        // Privacy Section
                        build_section_card(
                            "Privacy & Security",
                            vec![
                                build_setting_item("Profile Visibility", "Public", true),
                                build_setting_item("Activity Status", "Visible", true),
                                build_setting_item("Data Collection", "Limited", false),
                            ]
                        ),

                        SizedBox::builder().height(24.0).build().into(),

                        // Action Buttons
                        Row::builder()
                            .main_axis_alignment(MainAxisAlignment::End)
                            .children(vec![
                                Button::builder()
                                    .text("Reset to Defaults")
                                    .color(Color::rgb(158, 158, 158))
                                    .build()
                                    .into(),
                                SizedBox::builder().width(12.0).build().into(),
                                Button::builder()
                                    .text("Save Changes")
                                    .color(Color::rgb(76, 175, 80))
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

/// Build settings page header
fn build_settings_header() -> Widget {
    Row::builder()
        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .children(vec![
            Column::builder()
                .main_axis_size(MainAxisSize::Min)
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .children(vec![
                    Text::builder()
                        .data("Settings")
                        .size(32.0)
                        .color(Color::rgb(33, 33, 33))
                        .build()
                        .into(),
                    SizedBox::builder().height(4.0).build().into(),
                    Text::builder()
                        .data("Manage your account and preferences")
                        .size(16.0)
                        .color(Color::rgb(117, 117, 117))
                        .build()
                        .into(),
                ])
                .build()
                .into(),
            // User avatar
            ClipOval::builder()
                .child(
                    Container::builder()
                        .width(60.0)
                        .height(60.0)
                        .color(Color::rgb(156, 39, 176))
                        .child(
                            Center::builder()
                                .child(
                                    Text::builder()
                                        .data("JD")
                                        .size(24.0)
                                        .color(Color::WHITE)
                                        .build()
                                )
                                .build()
                        )
                        .build()
                )
                .build()
                .into(),
        ])
        .build()
}

/// Build a section card with settings items
fn build_section_card(title: &str, items: Vec<Widget>) -> Widget {
    Card::builder()
        .child(
            Container::builder()
                .padding(EdgeInsets::all(20.0))
                .child(
                    Column::builder()
                        .main_axis_size(MainAxisSize::Min)
                        .cross_axis_alignment(CrossAxisAlignment::Start)
                        .children({
                            let mut children = vec![
                                Text::builder()
                                    .data(title)
                                    .size(20.0)
                                    .color(Color::rgb(33, 33, 33))
                                    .build()
                                    .into(),
                                SizedBox::builder().height(16.0).build().into(),
                                Divider::builder().build().into(),
                                SizedBox::builder().height(8.0).build().into(),
                            ];

                            // Add items with dividers between them
                            for (i, item) in items.into_iter().enumerate() {
                                if i > 0 {
                                    children.push(SizedBox::builder().height(8.0).build().into());
                                    children.push(Divider::builder()
                                        .color(Color::rgb(240, 240, 240))
                                        .build()
                                        .into());
                                    children.push(SizedBox::builder().height(8.0).build().into());
                                }
                                children.push(item);
                            }

                            children
                        })
                        .build()
                )
                .build()
        )
        .build()
}

/// Build a setting item row
fn build_setting_item(label: &str, value: &str, has_arrow: bool) -> Widget {
    GestureDetector::builder()
        .child(
            Container::builder()
                .padding(EdgeInsets::symmetric(0.0, 12.0))
                .child(
                    Row::builder()
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .children(vec![
                            Text::builder()
                                .data(label)
                                .size(16.0)
                                .color(Color::rgb(33, 33, 33))
                                .build()
                                .into(),
                            Row::builder()
                                .main_axis_size(MainAxisSize::Min)
                                .children({
                                    let mut children = vec![
                                        Text::builder()
                                            .data(value)
                                            .size(16.0)
                                            .color(Color::rgb(117, 117, 117))
                                            .build()
                                            .into(),
                                    ];

                                    if has_arrow {
                                        children.push(SizedBox::builder().width(8.0).build().into());
                                        children.push(
                                            Container::builder()
                                                .width(20.0)
                                                .height(20.0)
                                                .child(
                                                    Center::builder()
                                                        .child(
                                                            Text::builder()
                                                                .data(">")
                                                                .size(16.0)
                                                                .color(Color::rgb(189, 189, 189))
                                                                .build()
                                                        )
                                                        .build()
                                                )
                                                .build()
                                                .into()
                                        );
                                    }

                                    children
                                })
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
    println!("=== Settings Page Example ===");
    println!("Demonstrates:");
    println!("  • Sectioned settings layout with Cards");
    println!("  • Dividers for visual separation");
    println!("  • Row layouts for setting items");
    println!("  • GestureDetector for interactive items");
    println!("  • Professional settings UI design");
    println!();

    run_app(SettingsPageApp.into_widget())
}
