//! Text widget - displays a string of text with a single style.
//!
//! Similar to Flutter's Text widget, this is the fundamental widget for displaying text.
//! For text with multiple styles, use Text::rich().
//!
//! # Example
//!
//! ```rust,no_run
//! use nebula_ui::widgets::primitives::Text;
//! use nebula_ui::types::typography::{TextStyle, TextAlign, TextOverflow};
//! use nebula_ui::types::core::Color;
//!
//! # fn example(ui: &mut egui::Ui) {
//! // Simple text
//! Text::new("Hello World").ui(ui);
//!
//! // Styled text using bon builder
//! Text::builder()
//!     .data("Hello World")
//!     .style(TextStyle::headline1())
//!     .text_align(TextAlign::Center)
//!     .max_lines(Some(2))
//!     .overflow(TextOverflow::Ellipsis)
//!     .ui(ui);
//! # }
//! ```

use bon::Builder;
use crate::types::core::Color;
use crate::types::typography::{
    TextStyle, TextAlign, TextDirection, TextOverflow, TextScaler, TextSpan,
    text_style_to_rich_text, default_egui_style, text_style_to_egui
};
use crate::widgets::WidgetExt;
use egui;

/// A widget that displays a string of text with a single style.
///
/// This is one of the most fundamental widgets for displaying text in your UI.
/// For text with multiple styles within it, use Text::rich() constructor.
///
/// ## Usage Patterns
///
/// ### 1. Simple Constructor (for quick text)
/// ```ignore
/// Text::new("Hello World").ui(ui);
/// ```
///
/// ### 2. bon Builder (Type-safe - for styled text)
/// ```ignore
/// Text::builder()
///     .data("Hello World")
///     .style(TextStyle::headline1())
///     .text_align(TextAlign::Center)
///     .max_lines(Some(2))
///     .ui(ui);
/// ```
///
/// ### 3. Rich Text (for inline spans with multiple styles)
/// ```ignore
/// // TODO: Implement InlineSpan and Text::rich()
/// // Text::rich(text_span).ui(ui);
/// ```
#[derive(Builder)]
#[builder(
    on(Color, into),
    on(TextStyle, into),
    finish_fn(vis = "", name = build_internal)  // Make standard build private
)]
pub struct Text {
    /// Optional key for widget identification and state persistence
    ///
    /// When provided, egui will use this ID to persist state across frames.
    #[builder(into)]
    pub key: Option<egui::Id>,

    /// The text to display (for Text::new constructor)
    ///
    /// This is used for simple text with a single style.
    /// Mutually exclusive with text_span.
    #[builder(into)]
    pub data: Option<String>,

    /// Rich text span (for Text::rich constructor)
    ///
    /// This allows multiple styles within the same text widget.
    /// Mutually exclusive with data.
    #[builder(skip)]
    pub text_span: Option<TextSpan>,

    /// Text style (font, size, color, decoration)
    ///
    /// If not provided, uses default egui text style.
    pub style: Option<TextStyle>,

    /// How to align the text horizontally
    #[builder(default = TextAlign::Left)]
    pub text_align: TextAlign,

    /// The directionality of the text
    ///
    /// This affects how Start and End alignments are resolved.
    pub text_direction: Option<TextDirection>,

    /// Whether the text should break at soft line breaks
    ///
    /// If false, text will be a single line regardless of width.
    /// If true (default), text wraps at available width.
    #[builder(default = true)]
    pub soft_wrap: bool,

    /// How visual overflow should be handled
    #[builder(default = TextOverflow::Clip)]
    pub overflow: TextOverflow,

    /// The text scaler to use for scaling text
    ///
    /// Defaults to no scaling (1.0).
    pub text_scaler: Option<TextScaler>,

    /// Maximum number of lines for the text to span
    ///
    /// If the text exceeds this, it will be truncated according to overflow.
    pub max_lines: Option<usize>,
}

impl Text {
    /// Create a simple text widget with the given string.
    ///
    /// This is the most common way to create a Text widget.
    ///
    /// # Example
    /// ```ignore
    /// Text::new("Hello World").ui(ui);
    /// ```
    pub fn new(data: impl Into<String>) -> Self {
        Self {
            key: None,
            data: Some(data.into()),
            text_span: None,
            style: None,
            text_align: TextAlign::Left,
            text_direction: None,
            soft_wrap: true,
            overflow: TextOverflow::Clip,
            text_scaler: None,
            max_lines: None,
        }
    }

    /// Create a text widget with rich text (multiple styles).
    ///
    /// # Example
    /// ```ignore
    /// let span = TextSpan::new("Hello")
    ///     .with_style(TextStyle::body().bold())
    ///     .with_child(TextSpan::new(" World"));
    /// Text::rich(span).ui(ui);
    /// ```
    pub fn rich(text_span: TextSpan) -> Self {
        Self {
            key: None,
            data: None,
            text_span: Some(text_span),
            style: None,
            text_align: TextAlign::Left,
            text_direction: None,
            soft_wrap: true,
            overflow: TextOverflow::Clip,
            text_scaler: None,
            max_lines: None,
        }
    }

    /// Validate text configuration for potential issues.
    ///
    /// Checks for:
    /// - Conflicting data and text_span (only one should be set)
    /// - Invalid max_lines values
    /// - Missing data/text_span
    ///
    /// Returns Ok(()) if validation passes, or an error message describing the issue.
    pub fn validate(&self) -> Result<(), String> {
        // Check that we have either data or text_span (but not both)
        match (&self.data, &self.text_span) {
            (None, None) => {
                return Err("Text widget must have either 'data' or 'text_span'".to_string());
            }
            (Some(_), Some(_)) => {
                return Err("Text widget cannot have both 'data' and 'text_span'".to_string());
            }
            _ => {}
        }

        // Validate max_lines
        if let Some(max_lines) = self.max_lines {
            if max_lines == 0 {
                return Err("max_lines must be greater than 0".to_string());
            }
        }

        Ok(())
    }

    /// Get the effective text scaler
    fn get_text_scaler(&self) -> TextScaler {
        self.text_scaler.unwrap_or_else(TextScaler::none)
    }

    /// Get the text to display (handling semantics_label override)
    fn get_display_text(&self) -> &str {
        // If semantics_label is present, use it for accessibility
        // (though in this implementation, we'll still render the actual text)
        if let Some(ref data) = self.data {
            data.as_str()
        } else {
            "" // Rich text not yet implemented
        }
    }
}

impl Default for Text {
    fn default() -> Self {
        Self {
            key: None,
            data: None,
            text_span: None,
            style: None,
            text_align: TextAlign::Left,
            text_direction: None,
            soft_wrap: true,
            overflow: TextOverflow::Clip,
            text_scaler: None,
            max_lines: None,
        }
    }
}

// Import bon builder traits for custom setter and finishing functions
use text_builder::IsComplete;

// Custom finishing functions for ergonomic API
impl<S: IsComplete> TextBuilder<S> {
    /// Build the text widget and immediately render it to UI.
    ///
    /// This is the most convenient way to use the builder - combines build + ui in one call.
    ///
    /// # Example
    /// ```ignore
    /// Text::builder()
    ///     .data("Hello World")
    ///     .style(TextStyle::headline1())
    ///     .ui(ui);
    /// ```
    pub fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let text = self.build_internal();
        egui::Widget::ui(text, ui)
    }

    /// Build and render to UI with validation.
    ///
    /// This is the most convenient validated API - combines build + validate + render.
    ///
    /// # Example
    /// ```ignore
    /// Text::builder()
    ///     .data("Hello World")
    ///     .max_lines(Some(2))
    ///     .build(ui)?;  // ← Validates and renders!
    /// ```
    pub fn build(self, ui: &mut egui::Ui) -> Result<egui::Response, String> {
        let text = self.build_internal();
        text.validate()?;
        Ok(egui::Widget::ui(text, ui))
    }

    /// Build the text widget with validation (returns Text for reuse).
    ///
    /// Returns an error if the widget has invalid configuration.
    ///
    /// # Example
    /// ```ignore
    /// let text = Text::builder()
    ///     .data("Hello World")
    ///     .max_lines(Some(0))  // ← Invalid!
    ///     .try_build()?;  // Returns Err
    /// ```
    pub fn try_build(self) -> Result<Text, String> {
        let text = self.build_internal();
        text.validate()?;
        Ok(text)
    }
}

impl egui::Widget for Text {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // If key is provided, wrap in an ID scope for state persistence
        if let Some(key) = self.key {
            ui.push_id(key, |ui| self.render(ui)).inner
        } else {
            self.render(ui)
        }
    }
}

impl Text {
    /// Internal rendering method (separated to handle key scoping)
    fn render(self, ui: &mut egui::Ui) -> egui::Response {
        let scaler = self.get_text_scaler();

        // Resolve text alignment with direction
        let resolved_align = if let Some(direction) = self.text_direction {
            self.text_align.resolve(direction)
        } else {
            self.text_align
        };

        // Check if we have rich text (TextSpan) or simple text
        if let Some(text_span) = self.text_span.clone() {
            // Rich text rendering using LayoutJob
            use crate::types::typography::InlineSpan;
            return self.render_rich_text(ui, &text_span, &scaler, resolved_align);
        }

        // Simple text rendering
        let text = self.get_display_text();

        // Create RichText with style using helper function
        let rich_text = if let Some(style) = &self.style {
            text_style_to_rich_text(text, style, &scaler)
        } else {
            // Use default style
            let (font_id, color) = default_egui_style(&scaler);
            egui::RichText::new(text).font(font_id.clone()).color(color)
        };

        let font_id = if let Some(style) = &self.style {
            let (font_id, _) = text_style_to_egui(style, &scaler);
            font_id
        } else {
            let (font_id, _) = default_egui_style(&scaler);
            font_id
        };

        // Create label with appropriate settings
        let mut label = if self.soft_wrap {
            egui::Label::new(rich_text).wrap()
        } else {
            egui::Label::new(rich_text)
        };

        // Handle overflow
        if self.overflow == TextOverflow::Ellipsis {
            label = label.truncate();
        }

        // Handle alignment by wrapping in a container if not left-aligned
        match resolved_align {
            TextAlign::Left => {
                // Simple case - just render the label
                if let Some(max_lines) = self.max_lines {
                    // Limit height for max_lines
                    let line_height = font_id.size * 1.5;
                    let max_height = line_height * max_lines as f32;
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), max_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| ui.add(label),
                    ).inner
                } else {
                    ui.add(label)
                }
            }
            TextAlign::Center => {
                // Center the text
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    if let Some(max_lines) = self.max_lines {
                        let line_height = font_id.size * 1.5;
                        let max_height = line_height * max_lines as f32;
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), max_height),
                            egui::Layout::top_down(egui::Align::Center),
                            |ui| ui.add(label),
                        ).inner
                    } else {
                        ui.add(label)
                    }
                }).inner
            }
            TextAlign::Right => {
                // Right-align the text
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    if let Some(max_lines) = self.max_lines {
                        let line_height = font_id.size * 1.5;
                        let max_height = line_height * max_lines as f32;
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), max_height),
                            egui::Layout::top_down(egui::Align::Max),
                            |ui| ui.add(label),
                        ).inner
                    } else {
                        ui.add(label)
                    }
                }).inner
            }
            _ => {
                // For other alignments, use left for now
                if let Some(max_lines) = self.max_lines {
                    let line_height = font_id.size * 1.5;
                    let max_height = line_height * max_lines as f32;
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), max_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| ui.add(label),
                    ).inner
                } else {
                    ui.add(label)
                }
            }
        }
    }

    /// Render rich text using LayoutJob
    fn render_rich_text(
        self,
        ui: &mut egui::Ui,
        text_span: &TextSpan,
        scaler: &TextScaler,
        resolved_align: TextAlign,
    ) -> egui::Response {
        use crate::types::typography::InlineSpan;

        // Convert TextSpan to LayoutJob
        let mut layout_job = text_span.to_layout_job(scaler);

        // Apply alignment
        layout_job.halign = match resolved_align {
            TextAlign::Left => egui::Align::Min,
            TextAlign::Center => egui::Align::Center,
            TextAlign::Right => egui::Align::Max,
            _ => egui::Align::Min,
        };

        // Apply wrapping
        layout_job.wrap.max_width = if self.soft_wrap {
            ui.available_width()
        } else {
            f32::INFINITY
        };

        // Apply max_rows for max_lines
        if let Some(max_lines) = self.max_lines {
            layout_job.wrap.max_rows = max_lines;
        }

        // Handle overflow
        if self.overflow == TextOverflow::Ellipsis {
            layout_job.wrap.break_anywhere = true;
            layout_job.wrap.overflow_character = Some('…');
        }

        // Render the label with LayoutJob
        ui.label(layout_job)
    }
}

// Implement nebula-ui WidgetExt trait (extension of egui::Widget)
impl WidgetExt for Text {
    fn id(&self) -> Option<egui::Id> {
        self.key
    }

    fn validate(&self) -> Result<(), String> {
        Text::validate(self)
    }

    fn debug_name(&self) -> &'static str {
        "Text"
    }

    fn size_hint(&self, _ui: &egui::Ui) -> Option<egui::Vec2> {
        // For single-line text, we could potentially calculate size
        // But since we're using egui's Label, we'll let egui handle sizing
        // Size hints are optional and this is a reasonable default
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::core::Color;

    #[test]
    fn test_text_creation() {
        let text = Text::new("Hello World");
        assert_eq!(text.data, Some("Hello World".to_string()));
        assert!(text.text_span.is_none());
        assert!(text.style.is_none());
    }

    #[test]
    fn test_text_with_style() {
        let style = TextStyle::headline1();
        let text = Text {
            data: Some("Hello".to_string()),
            style: Some(style.clone()),
            ..Default::default()
        };

        assert!(text.style.is_some());
        assert_eq!(text.style.unwrap().size, style.size);
    }

    #[test]
    fn test_text_validation_no_data() {
        let text = Text::default();
        assert!(text.validate().is_err());
    }

    #[test]
    fn test_text_validation_valid() {
        let text = Text::new("Hello");
        assert!(text.validate().is_ok());
    }

    #[test]
    fn test_text_validation_invalid_max_lines() {
        let text = Text {
            data: Some("Hello".to_string()),
            max_lines: Some(0),
            ..Default::default()
        };

        assert!(text.validate().is_err());
    }

    #[test]
    fn test_text_alignment() {
        let text = Text {
            data: Some("Hello".to_string()),
            text_align: TextAlign::Center,
            ..Default::default()
        };

        assert_eq!(text.text_align, TextAlign::Center);
    }

    #[test]
    fn test_text_overflow() {
        let text = Text {
            data: Some("Hello".to_string()),
            overflow: TextOverflow::Ellipsis,
            ..Default::default()
        };

        assert_eq!(text.overflow, TextOverflow::Ellipsis);
    }

    #[test]
    fn test_text_max_lines() {
        let text = Text {
            data: Some("Hello\nWorld\nTest".to_string()),
            max_lines: Some(2),
            ..Default::default()
        };

        assert_eq!(text.max_lines, Some(2));
        assert!(text.validate().is_ok());
    }

    #[test]
    fn test_text_soft_wrap() {
        let text = Text {
            data: Some("Hello World".to_string()),
            soft_wrap: false,
            ..Default::default()
        };

        assert!(!text.soft_wrap);
    }

    #[test]
    fn test_text_scaler() {
        let text = Text {
            data: Some("Hello".to_string()),
            text_scaler: Some(TextScaler::new(2.0)),
            ..Default::default()
        };

        let scaler = text.get_text_scaler();
        assert_eq!(scaler.scale_factor, 2.0);
    }

    #[test]
    fn test_text_direction_alignment() {
        let text = Text {
            data: Some("Hello".to_string()),
            text_align: TextAlign::Start,
            text_direction: Some(TextDirection::Rtl),
            ..Default::default()
        };

        // Start should resolve to Right for RTL
        let resolved = text.text_align.resolve(text.text_direction.unwrap());
        assert_eq!(resolved, TextAlign::Right);
    }

}
