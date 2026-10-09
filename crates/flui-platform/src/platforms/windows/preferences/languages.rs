//! Owned UI-language sampling; shares the host's existing invalidation lifetime.

use flui_platform_api::Locale;
use windows::Win32::{
    Foundation::ERROR_INSUFFICIENT_BUFFER,
    Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME},
};
use windows::core::PWSTR;

use crate::PlatformError;

// Bound work on the synchronous owner lane. Two MiB holds far more installed
// language names than a normal host; excess observations fail instead of making
// unbounded allocation/initialization part of a frame wake.
const MAX_LANGUAGE_UNITS: u32 = 1_048_576;

pub(super) fn sample() -> Result<Vec<Locale>, PlatformError> {
    read_with(|buffer, count, units| {
        // SAFETY: read_with supplies either no buffer with size zero, or an
        // exclusively borrowed initialized buffer of exactly `units` u16s.
        // The count and size outputs are live locals for this synchronous call.
        unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                count,
                buffer.map(|value| PWSTR(value.as_mut_ptr())),
                units,
            )
        }
    })
}

fn invalid(message: &'static str) -> PlatformError {
    PlatformError::Preferences {
        message: message.into(),
    }
}

fn read_with(
    mut query: impl FnMut(Option<&mut [u16]>, &mut u32, &mut u32) -> windows::core::Result<()>,
) -> Result<Vec<Locale>, PlatformError> {
    for _ in 0..3 {
        let mut count = 0;
        let mut units = 0;
        query(None, &mut count, &mut units).map_err(super::native_error)?;
        if !(2..=MAX_LANGUAGE_UNITS).contains(&units) {
            return Err(invalid("invalid preferred-language buffer size"));
        }
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(units as usize)
            .map_err(|_| invalid("cannot allocate preferred-language buffer"))?;
        buffer.resize(units as usize, 0);
        match query(Some(&mut buffer), &mut count, &mut units) {
            Ok(()) => {
                let used = buffer
                    .get(..units as usize)
                    .ok_or_else(|| invalid("preferred-language result exceeds buffer"))?;
                return decode(used, count);
            }
            Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {
                // Re-query capacity: failed output counts are not an observation.
            }
            Err(error) => return Err(super::native_error(error)),
        }
    }
    Err(invalid(
        "preferred-language list kept changing during sampling",
    ))
}

fn decode(buffer: &[u16], count: u32) -> Result<Vec<Locale>, PlatformError> {
    let body = buffer
        .strip_suffix(&[0, 0])
        .ok_or_else(|| invalid("preferred-language list lacks its terminator"))?;
    let mut locales = Vec::new();
    if !body.is_empty() {
        for language in body.split(|unit| *unit == 0) {
            let tag = String::from_utf16(language)
                .map_err(|_| invalid("preferred-language list is not valid UTF-16"))?;
            locales.push(
                tag.parse()
                    .map_err(|_| invalid("invalid preferred-language tag"))?,
            );
        }
    }
    if locales.len() != count as usize {
        return Err(invalid(
            "preferred-language count does not match its buffer",
        ));
    }
    Ok(locales)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    fn complete_ordered_languages_survive_decoding() {
        let values = decode(&wire("ca-ES-valencia\0en-US-u-hc-h12\0\0"), 2).expect("complete list");
        assert_eq!(
            values
                .iter()
                .map(Locale::to_language_tag)
                .collect::<Vec<_>>(),
            ["ca-ES-valencia", "en-US-u-hc-h12"]
        );
        assert_eq!(
            decode(&[0, 0], 0).expect("empty observation"),
            Vec::<Locale>::new()
        );
        for (buffer, count) in [
            (wire("en-US\0"), 1),
            (wire("en-US\0\0"), 2),
            (wire("en-US\0\0fr-FR\0\0"), 3),
            (vec![0xd800, 0, 0], 1),
            (wire("en--US\0\0"), 1),
        ] {
            assert!(decode(&buffer, count).is_err());
        }
    }

    fn changing_capacity_retries_without_accepting_failed_outputs() {
        let expected = wire("en-US\0ca-ES-valencia\0\0");
        let mut fills = 0;
        let observed = read_with(|buffer, count, units| {
            let Some(buffer) = buffer else {
                assert_eq!(*units, 0);
                *units = expected.len() as u32;
                return Ok(());
            };
            fills += 1;
            if fills == 1 {
                *count = u32::MAX;
                *units = u32::MAX;
                return Err(ERROR_INSUFFICIENT_BUFFER.into());
            }
            buffer.copy_from_slice(&expected);
            *count = 2;
            Ok(())
        })
        .expect("retry after native growth");
        assert_eq!(observed[1].to_language_tag(), "ca-ES-valencia");
        assert_eq!(fills, 2);
        let mut attempts = 0;
        assert!(
            read_with(|buffer, _, units| {
                if buffer.is_none() {
                    *units = 2;
                    return Ok(());
                }
                attempts += 1;
                Err(ERROR_INSUFFICIENT_BUFFER.into())
            })
            .is_err()
        );
        assert_eq!(attempts, 3, "one owner turn must not spin indefinitely");
    }

    #[test]
    fn preferred_language_buffer_contract() {
        crate::table_test::run_table(
            "preferred_language_buffer_contract",
            &[
                (
                    "complete_ordered_languages_survive_decoding",
                    complete_ordered_languages_survive_decoding as fn(),
                ),
                (
                    "changing_capacity_retries_without_accepting_failed_outputs",
                    changing_capacity_retries_without_accepting_failed_outputs as fn(),
                ),
            ],
        );
    }
}
