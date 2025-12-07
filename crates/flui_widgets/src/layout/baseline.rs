//! Baseline widget - positions child based on baseline
//!
//! A widget that positions its child according to the child's baseline.
//! Similar to Flutter's Baseline widget.

use bon::Builder;
use flui_core::element::Element;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::objects::RenderBaseline;
use flui_types::typography::TextBaseline;

/// A widget that positions its child according to the child's baseline.
///
/// Baseline is used to align text and other widgets along a common baseline.
/// This is particularly useful in Row layouts where you want to align text
/// of different sizes.
///
/// ## Layout Behavior
///
/// The baseline is measured as the distance from the top of the widget.
/// The widget's height will be at least `baseline + child_height`.
///
/// ## TextBaseline Types
///
/// - **Alphabetic**: Standard baseline for Latin/Cyrillic/Greek scripts
/// - **Ideographic**: Baseline for CJK (Chinese/Japanese/Korean) characters
///
/// ## Common Use Cases
///
/// ### Align different text sizes in Row
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Baseline::builder()
///             .baseline(20.0)
///             .baseline_type(TextBaseline::Alphabetic)
///             .child(Text::new("Small").font_size(12.0))
///             .build(),
///         Baseline::builder()
///             .baseline(20.0)
///             .baseline_type(TextBaseline::Alphabetic)
///             .child(Text::new("Large").font_size(24.0))
///             .build(),
///     ])
/// ```
///
/// ### Align text with icon
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Baseline::alphabetic(16.0, Icon::new("star")),
///         Baseline::alphabetic(16.0, Text::new("Rating")),
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Alphabetic baseline at 20px from top
/// Baseline::alphabetic(20.0, Text::new("Hello"))
///
/// // Ideographic baseline for CJK text
/// Baseline::ideographic(18.0, Text::new("你好"))
///
/// // Using builder
/// Baseline::builder()
///     .baseline(24.0)
///     .baseline_type(TextBaseline::Alphabetic)
///     .child(widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), on(TextBaseline, into), finish_fn(name = build_internal, vis = ""))]
pub struct Baseline {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Distance from top to baseline in logical pixels.
    /// If None, defaults to 0.0 (no offset).
    pub baseline: Option<f32>,

    /// Type of baseline to use.
    /// Default: TextBaseline::Alphabetic
    #[builder(default = TextBaseline::Alphabetic)]
    pub baseline_type: TextBaseline,

    /// The child widget to position.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Element>,
}

impl std::fmt::Debug for Baseline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Baseline")
            .field("key", &self.key)
            .field("baseline", &self.baseline)
            .field("baseline_type", &self.baseline_type)
            .field("child", &if self.child.is_some() { "<>" } else { "None" })
            .finish()
    }
}

impl Baseline {
    /// Creates a new Baseline with the given parameters.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Baseline::new(20.0, TextBaseline::Alphabetic, child);
    /// ```
    pub fn new(baseline: f32, baseline_type: TextBaseline, child: impl IntoElement) -> Self {
        Self {
            key: None,
            baseline: Some(baseline),
            baseline_type,
            child: Some(child.into_element()),
        }
    }

    /// Creates a Baseline with alphabetic baseline.
    ///
    /// This is the most common baseline for Latin scripts.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Baseline::alphabetic(20.0, Text::new("Hello"));
    /// ```
    pub fn alphabetic(baseline: f32, child: impl IntoElement) -> Self {
        Self::new(baseline, TextBaseline::Alphabetic, child)
    }

    /// Creates a Baseline with ideographic baseline.
    ///
    /// This is used for CJK characters.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Baseline::ideographic(18.0, Text::new("你好"));
    /// ```
    pub fn ideographic(baseline: f32, child: impl IntoElement) -> Self {
        Self::new(baseline, TextBaseline::Ideographic, child)
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl IntoElement) {
        self.child = Some(child.into_element());
    }

    /// Validates Baseline configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(baseline) = self.baseline {
            if baseline < 0.0 || baseline.is_nan() {
                return Err(format!(
                    "Invalid baseline: {}. Must be non-negative.",
                    baseline
                ));
            }
        }
        Ok(())
    }
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            key: None,
            baseline: None,
            baseline_type: TextBaseline::Alphabetic,
            child: None,
        }
    }
}

// Implement View for Baseline - New architecture
impl IntoElement for Baseline {
    fn into_element(self) -> Element {
        let baseline = self.baseline.unwrap_or(0.0);

        RenderBaseline::new(baseline, self.baseline_type).child_opt(self.child)
    }
}

// bon Builder Extensions
use baseline_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> BaselineBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Baseline::builder()
    ///     .baseline(20.0)
    ///     .child(Text::new("Hello"))
    ///     .build()
    /// ```
    pub fn child(self, child: impl IntoElement) -> BaselineBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

// Public build() wrapper with validation
impl<S: State> BaselineBuilder<S> {
    /// Builds the Baseline widget with automatic validation in debug mode.
    pub fn build(self) -> Baseline {
        let baseline = self.build_internal();

        #[cfg(debug_assertions)]
        if let Err(e) = baseline.validate() {
            tracing::warn!("Baseline validation warning: {}", e);
        }

        baseline
    }
}

// Baseline now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::objects::RenderEmpty;

    // Mock view for testing
    #[derive()]
    struct MockView;

    impl IntoElement for MockView {
        fn into_element(self) -> Element {
            RenderEmpty.leaf()
        }
    }

    #[test]
    fn test_baseline_new() {
        let widget = Baseline::new(20.0, TextBaseline::Alphabetic, MockView);
        assert_eq!(widget.baseline, Some(20.0));
        assert_eq!(widget.baseline_type, TextBaseline::Alphabetic);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_baseline_alphabetic() {
        let widget = Baseline::alphabetic(15.0, MockView);
        assert_eq!(widget.baseline, Some(15.0));
        assert_eq!(widget.baseline_type, TextBaseline::Alphabetic);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_baseline_ideographic() {
        let widget = Baseline::ideographic(18.0, MockView);
        assert_eq!(widget.baseline, Some(18.0));
        assert_eq!(widget.baseline_type, TextBaseline::Ideographic);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_baseline_builder() {
        let widget = Baseline::builder()
            .baseline(25.0)
            .baseline_type(TextBaseline::Ideographic)
            .child(MockView)
            .build();
        assert_eq!(widget.baseline, Some(25.0));
        assert_eq!(widget.baseline_type, TextBaseline::Ideographic);
    }

    #[test]
    fn test_baseline_default() {
        let widget = Baseline::default();
        assert_eq!(widget.baseline, None);
        assert_eq!(widget.baseline_type, TextBaseline::Alphabetic);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_baseline_validate() {
        let widget = Baseline::alphabetic(20.0, MockView);
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_baseline_validate_negative() {
        let widget = Baseline::alphabetic(-1.0, MockView);
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_baseline_set_child() {
        let mut widget = Baseline::default();
        assert!(widget.child.is_none());

        widget.set_child(MockView);
        assert!(widget.child.is_some());
    }
}
