//! API admission and absence/error handling for Android's native text-weight query.

use flui_platform_api::TextWeightPreference;

/// The native field is absent before API31. A failed available query remains an
/// error; Android's undefined sentinel remains an unavailable observation.
pub(crate) fn observe<E>(
    sdk: i32,
    query: impl FnOnce() -> Result<i32, E>,
) -> Result<Option<TextWeightPreference>, E> {
    if sdk < 31 {
        return Ok(None);
    }
    let value = query()?;
    Ok((value != i32::MAX).then(|| TextWeightPreference::from_adjustment(value)))
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, num::NonZeroI32};

    use super::{TextWeightPreference, observe};

    #[test]
    fn android_text_weight_query_contract() {
        let adjustment = |value| {
            Some(TextWeightPreference::Adjustment(
                NonZeroI32::new(value).expect("nonzero fixture"),
            ))
        };
        for (sdk, raw, expected) in [
            (30, 237, None),
            (31, i32::MAX, None),
            (31, 0, Some(TextWeightPreference::NoPreference)),
            (31, 237, adjustment(237)),
            (31, -137, adjustment(-137)),
            (35, i32::MIN, adjustment(i32::MIN)),
            (35, i32::MAX - 1, adjustment(i32::MAX - 1)),
        ] {
            let calls = Cell::new(0);
            let result = observe(sdk, || {
                calls.set(calls.get() + 1);
                Ok::<_, &str>(raw)
            });
            assert_eq!(result, Ok(expected), "SDK {sdk}, native value {raw}");
            assert_eq!(
                calls.get(),
                usize::from(sdk >= 31),
                "native query admission"
            );
        }

        for sdk in [30, 31] {
            let calls = Cell::new(0);
            let failed = observe(sdk, || {
                calls.set(calls.get() + 1);
                Err::<i32, _>("native field failure")
            });
            let recovered = observe(sdk, || {
                calls.set(calls.get() + 1);
                Ok::<_, &str>(237)
            });
            if sdk == 30 {
                assert_eq!(failed, Ok(None));
                assert_eq!(recovered, Ok(None));
                assert_eq!(calls.get(), 0, "unsupported field must never be queried");
            } else {
                assert_eq!(failed, Err("native field failure"));
                assert_eq!(recovered, Ok(adjustment(237)));
                assert_eq!(calls.get(), 2, "failure cannot fabricate a preference");
            }
        }
    }
}
