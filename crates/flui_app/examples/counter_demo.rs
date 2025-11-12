//! Universal Counter Demo for FLUI
//!
//! This example works on all platforms:
//! - Desktop (Windows, Linux, macOS)
//! - Mobile (Android, iOS)
//! - Web (WebAssembly)
//!
//! # Building
//!
//! ## Desktop
//! ```bash
//! cargo run -p flui_app --example counter_demo
//! ```
//!
//! ## Android
//! ```bash
//! flui build --platform android --example counter_demo --release
//! flui install --platform android
//! flui run --platform android
//! ```
//!
//! ## iOS (macOS only)
//! ```bash
//! flui build --platform ios --example counter_demo --release
//! flui run --platform ios
//! ```
//!
//! ## Web
//! ```bash
//! flui build --platform web --example counter_demo --release
//! flui run --platform web
//! # Opens browser at http://localhost:8080
//! ```

use flui_app::run_app;
use flui_core::prelude::*;
use flui_core::hooks::{use_signal, use_memo};
use flui_widgets::*;

#[derive(Debug)]
struct CounterApp;

impl View for CounterApp {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        let count = use_signal(ctx, 0);
        let theme_index = use_signal(ctx, 0);
        
        // Available themes
        let themes = vec![
            Theme {
                name: "Blue",
                bg: (245, 245, 250),
                text: (30, 30, 30),
                primary: (100, 150, 255),
                secondary: (200, 210, 255),
            },
            Theme {
                name: "Dark",
                bg: (30, 30, 40),
                text: (240, 240, 245),
                primary: (255, 100, 150),
                secondary: (100, 50, 75),
            },
            Theme {
                name: "Green",
                bg: (250, 252, 245),
                text: (40, 40, 30),
                primary: (100, 200, 100),
                secondary: (180, 230, 180),
            },
            Theme {
                name: "Purple",
                bg: (248, 245, 252),
                text: (40, 30, 50),
                primary: (150, 100, 255),
                secondary: (200, 180, 255),
            },
        ];
        
        let current_theme = &themes[theme_index.get(ctx) % themes.len()];
        
        // Computed value - doubled count
        let doubled = use_memo(ctx, |ctx| count.get(ctx) * 2);
        
        Container::new(
            Column::new()
                .spacing(24.0)
                .padding(32.0)
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .children(vec![
                    // Title
                    Box::new(
                        Text::new("FLUI Counter")
                            .size(48.0)
                            .weight(FontWeight::Bold)
                            .color(current_theme.text)
                    ),
                    
                    // Platform badge
                    Box::new(
                        Container::new(
                            Text::new(get_platform_info())
                                .size(16.0)
                                .weight(FontWeight::Medium)
                                .color((255, 255, 255))
                        )
                        .background(current_theme.primary)
                        .padding_all(12.0)
                        .border_radius(20.0)
                    ),
                    
                    // Spacer
                    Box::new(Spacer::new(32.0)),
                    
                    // Main counter display
                    Box::new(
                        Container::new(
                            Column::new()
                                .spacing(8.0)
                                .cross_axis_alignment(CrossAxisAlignment::Center)
                                .children(vec![
                                    Box::new(
                                        Text::new(format!("{}", count.get(ctx)))
                                            .size(96.0)
                                            .weight(FontWeight::Bold)
                                            .color(current_theme.primary)
                                    ),
                                    Box::new(
                                        Text::new(format!("Doubled: {}", doubled.get(ctx)))
                                            .size(20.0)
                                            .color(current_theme.text)
                                            .opacity(0.7)
                                    ),
                                ])
                        )
                        .padding_all(40.0)
                        .border(3.0, current_theme.primary)
                        .border_radius(24.0)
                        .background(current_theme.secondary)
                    ),
                    
                    // Spacer
                    Box::new(Spacer::new(24.0)),
                    
                    // Control buttons row
                    Box::new(
                        Row::new()
                            .spacing(16.0)
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .children(vec![
                                // Decrement button
                                Box::new(
                                    Button::new("−")
                                        .size(ButtonSize::Large)
                                        .color(current_theme.primary)
                                        .on_pressed(move || {
                                            count.update(|n| *n = (*n - 1).max(0));
                                        })
                                ),
                                
                                // Reset button
                                Box::new(
                                    Button::new("Reset")
                                        .size(ButtonSize::Large)
                                        .variant(ButtonVariant::Outlined)
                                        .color(current_theme.primary)
                                        .on_pressed(move || {
                                            count.set(0);
                                        })
                                ),
                                
                                // Increment button
                                Box::new(
                                    Button::new("+")
                                        .size(ButtonSize::Large)
                                        .color(current_theme.primary)
                                        .on_pressed(move || {
                                            count.update(|n| *n += 1);
                                        })
                                ),
                            ])
                    ),
                    
                    // Quick increment buttons
                    Box::new(
                        Row::new()
                            .spacing(12.0)
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .children(vec![
                                Box::new(
                                    Button::new("+10")
                                        .size(ButtonSize::Small)
                                        .variant(ButtonVariant::Text)
                                        .on_pressed(move || count.update(|n| *n += 10))
                                ),
                                Box::new(
                                    Button::new("+100")
                                        .size(ButtonSize::Small)
                                        .variant(ButtonVariant::Text)
                                        .on_pressed(move || count.update(|n| *n += 100))
                                ),
                                Box::new(
                                    Button::new("+1000")
                                        .size(ButtonSize::Small)
                                        .variant(ButtonVariant::Text)
                                        .on_pressed(move || count.update(|n| *n += 1000))
                                ),
                            ])
                    ),
                    
                    // Theme selector
                    Box::new(Spacer::new(32.0)),
                    
                    Box::new(
                        Column::new()
                            .spacing(12.0)
                            .cross_axis_alignment(CrossAxisAlignment::Center)
                            .children(vec![
                                Box::new(
                                    Text::new(format!("Theme: {}", current_theme.name))
                                        .size(18.0)
                                        .color(current_theme.text)
                                ),
                                Box::new(
                                    Button::new("Change Theme")
                                        .size(ButtonSize::Medium)
                                        .variant(ButtonVariant::Outlined)
                                        .color(current_theme.primary)
                                        .on_pressed(move || {
                                            theme_index.update(|i| *i += 1);
                                        })
                                ),
                            ])
                    ),
                    
                    // Footer
                    Box::new(Spacer::new(24.0)),
                    
                    Box::new(
                        Text::new("Built with FLUI 🦀")
                            .size(14.0)
                            .color(current_theme.text)
                            .opacity(0.5)
                    ),
                ])
        )
        .background(current_theme.bg)
        .width(Length::Fill)
        .height(Length::Fill)
    }
}

#[derive(Clone, Copy)]
struct Theme {
    name: &'static str,
    bg: (u8, u8, u8),
    text: (u8, u8, u8),
    primary: (u8, u8, u8),
    secondary: (u8, u8, u8),
}

fn get_platform_info() -> String {
    #[cfg(target_os = "android")]
    return format!("Android • {}", get_device_model());
    
    #[cfg(target_os = "ios")]
    return format!("iOS • {}", get_device_model());
    
    #[cfg(target_arch = "wasm32")]
    return "Web (WebAssembly)".to_string();
    
    #[cfg(target_os = "windows")]
    return format!("Windows {}", std::env::consts::ARCH);
    
    #[cfg(target_os = "linux")]
    return format!("Linux {}", std::env::consts::ARCH);
    
    #[cfg(target_os = "macos")]
    return format!("macOS {}", std::env::consts::ARCH);
    
    #[cfg(not(any(
        target_os = "android",
        target_os = "ios",
        target_arch = "wasm32",
        target_os = "windows",
        target_os = "linux",
        target_os = "macos"
    )))]
    return "Unknown Platform".to_string();
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn get_device_model() -> &'static str {
    // In production, query actual device model
    "Device"
}

// ============================================================================
// Platform Entry Points
// ============================================================================

/// Desktop entry point
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();
    
    log::info!("Starting FLUI Counter on Desktop");
    run_app(CounterApp);
}

/// Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    
    // Initialize Android logging
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("FLUI")
    );
    
    log::info!("Starting FLUI Counter on Android");
    log::info!("Device: {}", get_device_model());
    
    run_app(CounterApp);
}

/// iOS entry point
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn start_flui_counter() {
    // iOS logging goes to Console.app
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();
    
    log::info!("Starting FLUI Counter on iOS");
    log::info!("Device: {}", get_device_model());
    
    run_app(CounterApp);
}

/// Web (WebAssembly) entry point
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn wasm_main() {
    // Setup panic hook for better error messages
    console_error_panic_hook::set_once();
    
    // Initialize logging to browser console
    wasm_logger::init(wasm_logger::Config::default());
    
    log::info!("Starting FLUI Counter on Web");
    run_app(CounterApp);
}
