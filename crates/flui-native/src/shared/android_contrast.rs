//! What Android's two contrast observations mean together.

/// High contrast from the system contrast level (`UiModeManager.getContrast`,
/// API 34: `-1` to `1`, above zero asks for more) and the high-contrast-text
/// setting. Either asking for more is high contrast; neither readable is no
/// observation.
pub(crate) fn high_contrast(level: Option<f32>, high_text: Option<bool>) -> Option<bool> {
    match (level, high_text) {
        (None, None) => None,
        (level, high_text) => {
            Some(level.is_some_and(|level| level > 0.0) || high_text == Some(true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::high_contrast;

    #[test]
    fn android_contrast_contract() {
        for (level, high_text, expected) in [
            (None, None, None),
            (Some(0.0), None, Some(false)),
            (Some(-1.0), None, Some(false)),
            (Some(0.5), None, Some(true)),
            (Some(1.0), Some(false), Some(true)),
            (None, Some(true), Some(true)),
            (None, Some(false), Some(false)),
            (Some(0.0), Some(true), Some(true)),
            (Some(f32::NAN), Some(false), Some(false)),
        ] {
            assert_eq!(
                high_contrast(level, high_text),
                expected,
                "level {level:?}, high text {high_text:?}"
            );
        }
    }
}
