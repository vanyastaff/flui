//! Bits that hash floats consistently with their equality.
//!
//! Float equality treats `0.0` and `-0.0` as equal while their bit patterns differ, so a
//! `Hash` over `to_bits` breaks `a == b ⇒ hash(a) == hash(b)` for any type whose `PartialEq`
//! compares the floats. Hashing these bits keeps the two in agreement. NaN never equals
//! itself, so folding every NaN payload into one costs nothing and keeps keys built from NaN
//! stable.

/// The bits a hash stores for an `f32`: `-0.0` as `+0.0`, every NaN as one NaN, every other
/// value unchanged.
#[inline]
pub fn canonical_bits(value: f32) -> u32 {
    if value.is_nan() {
        f32::NAN.to_bits()
    } else {
        // `-0.0 + 0.0` is `+0.0`; every other value is unchanged.
        (value + 0.0).to_bits()
    }
}

/// The bits a hash stores for an `f64`: `-0.0` as `+0.0`, every NaN as one NaN, every other
/// value unchanged.
#[inline]
pub fn canonical_bits_f64(value: f64) -> u64 {
    if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        (value + 0.0).to_bits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F32_VALUES: [f32; 13] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::from_bits(0x8000_0001),
        f32::MAX,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        -f32::NAN,
        f32::from_bits(0x7fc0_0001),
    ];

    #[test]
    fn f32_bits_are_equal_exactly_when_the_floats_are_equal_or_both_nan() {
        for a in F32_VALUES {
            for b in F32_VALUES {
                let same = a == b || (a.is_nan() && b.is_nan());
                assert_eq!(
                    canonical_bits(a) == canonical_bits(b),
                    same,
                    "{a:?} vs {b:?}"
                );
            }
        }
    }
}
