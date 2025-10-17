//! Input types for form controls
//!
//! This module contains types for representing input types and modes.

/// Type of input expected from user.
///
/// Similar to HTML input types and Flutter's TextInputType.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputType {
    /// Plain text input
    Text,
    /// Multi-line text input
    Multiline,
    /// Number input
    Number,
    /// Decimal number input
    Decimal,
    /// Phone number input
    Phone,
    /// Email address input
    Email,
    /// URL input
    Url,
    /// Password input (masked)
    Password,
    /// Date input
    Date,
    /// Time input
    Time,
    /// DateTime input
    DateTime,
    /// Color picker input
    Color,
    /// File upload input
    File,
}

impl InputType {
    /// Check if this input type should be masked (like password).
    pub fn is_masked(&self) -> bool {
        matches!(self, InputType::Password)
    }

    /// Check if this input type is numeric.
    pub fn is_numeric(&self) -> bool {
        matches!(self, InputType::Number | InputType::Decimal | InputType::Phone)
    }

    /// Check if this input type is for dates/times.
    pub fn is_temporal(&self) -> bool {
        matches!(
            self,
            InputType::Date | InputType::Time | InputType::DateTime
        )
    }

    /// Check if this input type supports multiline.
    pub fn is_multiline(&self) -> bool {
        matches!(self, InputType::Multiline)
    }
}

impl Default for InputType {
    fn default() -> Self {
        InputType::Text
    }
}

/// Input mode hint for virtual keyboards.
///
/// Similar to HTML inputmode attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputMode {
    /// No specific input mode
    None,
    /// Standard text keyboard
    Text,
    /// Numeric keyboard
    Numeric,
    /// Decimal keyboard (with decimal point)
    Decimal,
    /// Telephone keyboard
    Tel,
    /// Email keyboard (with @ symbol)
    Email,
    /// URL keyboard (with / and .com)
    Url,
    /// Search keyboard (with search button)
    Search,
}

impl InputMode {
    /// Get the appropriate input mode for an input type.
    pub fn from_input_type(input_type: InputType) -> Self {
        match input_type {
            InputType::Number => InputMode::Numeric,
            InputType::Decimal => InputMode::Decimal,
            InputType::Phone => InputMode::Tel,
            InputType::Email => InputMode::Email,
            InputType::Url => InputMode::Url,
            _ => InputMode::Text,
        }
    }
}

impl Default for InputMode {
    fn default() -> Self {
        InputMode::None
    }
}

/// Text capitalization mode.
///
/// Similar to Flutter's TextCapitalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextCapitalization {
    /// No automatic capitalization
    None,
    /// Capitalize first letter of sentences
    Sentences,
    /// Capitalize first letter of each word
    Words,
    /// Capitalize all characters
    Characters,
}

impl Default for TextCapitalization {
    fn default() -> Self {
        TextCapitalization::None
    }
}

/// Autocorrect behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Autocorrect {
    /// Enable autocorrect
    Enabled,
    /// Disable autocorrect
    Disabled,
}

impl Autocorrect {
    /// Check if autocorrect is enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self, Autocorrect::Enabled)
    }
}

impl Default for Autocorrect {
    fn default() -> Self {
        Autocorrect::Enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_type_checks() {
        assert!(InputType::Password.is_masked());
        assert!(!InputType::Text.is_masked());

        assert!(InputType::Number.is_numeric());
        assert!(InputType::Decimal.is_numeric());
        assert!(!InputType::Text.is_numeric());

        assert!(InputType::Date.is_temporal());
        assert!(InputType::Time.is_temporal());
        assert!(!InputType::Text.is_temporal());

        assert!(InputType::Multiline.is_multiline());
        assert!(!InputType::Text.is_multiline());
    }

    #[test]
    fn test_input_mode_from_input_type() {
        assert_eq!(
            InputMode::from_input_type(InputType::Number),
            InputMode::Numeric
        );
        assert_eq!(
            InputMode::from_input_type(InputType::Email),
            InputMode::Email
        );
        assert_eq!(InputMode::from_input_type(InputType::Text), InputMode::Text);
    }

    #[test]
    fn test_autocorrect() {
        assert!(Autocorrect::Enabled.is_enabled());
        assert!(!Autocorrect::Disabled.is_enabled());
    }

    #[test]
    fn test_defaults() {
        assert_eq!(InputType::default(), InputType::Text);
        assert_eq!(InputMode::default(), InputMode::None);
        assert_eq!(TextCapitalization::default(), TextCapitalization::None);
        assert_eq!(Autocorrect::default(), Autocorrect::Enabled);
    }
}
