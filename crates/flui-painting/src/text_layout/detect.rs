//! RTL/LTR detection helpers.
//!
//! Mythos chain U6 extracted these from the 1,243-LOC
//! `text_layout.rs` god module. Uses Unicode codepoint ranges to
//! classify characters as strong-LTR, strong-RTL, or neutral.

use flui_types::typography::TextDirection;

/// Detects the text direction from the content.
///
/// Uses the first strong directional character to determine
/// direction. Returns `None` if no strong directional character is
/// found.
pub fn detect_text_direction(text: &str) -> Option<TextDirection> {
    for ch in text.chars() {
        if is_rtl_char(ch) {
            return Some(TextDirection::Rtl);
        }
        if is_ltr_char(ch) {
            return Some(TextDirection::Ltr);
        }
    }
    None
}

/// Checks if a character is a strong RTL character.
fn is_rtl_char(ch: char) -> bool {
    matches!(ch,
        // Arabic
        '\u{0600}'..='\u{06FF}' |
        '\u{0750}'..='\u{077F}' |
        '\u{08A0}'..='\u{08FF}' |
        '\u{FB50}'..='\u{FDFF}' |
        '\u{FE70}'..='\u{FEFF}' |
        // Hebrew
        '\u{0590}'..='\u{05FF}' |
        '\u{FB1D}'..='\u{FB4F}' |
        // Syriac
        '\u{0700}'..='\u{074F}' |
        // Thaana
        '\u{0780}'..='\u{07BF}' |
        // N'Ko
        '\u{07C0}'..='\u{07FF}'
    )
}

/// Checks if a character is a strong LTR character.
fn is_ltr_char(ch: char) -> bool {
    matches!(ch,
        // Basic Latin letters
        'A'..='Z' | 'a'..='z' |
        // Latin Extended
        '\u{00C0}'..='\u{024F}' |
        // Greek
        '\u{0370}'..='\u{03FF}' |
        // Cyrillic
        '\u{0400}'..='\u{04FF}' |
        // Georgian
        '\u{10A0}'..='\u{10FF}' |
        // CJK (treated as LTR in horizontal layout)
        '\u{4E00}'..='\u{9FFF}' |
        '\u{3040}'..='\u{309F}' | // Hiragana
        '\u{30A0}'..='\u{30FF}'   // Katakana
    )
}
