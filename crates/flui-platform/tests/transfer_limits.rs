//! The transport's size policy: `TransferLimits::enforce`, the one predicate
//! every delivery passes.

use flui_platform::data_transfer::{TransferError, TransferLimits};

#[test]
fn transfer_limits_admit_up_to_the_ceiling() {
    const LIMIT: u64 = 10;
    let limits = TransferLimits::default().with_max_bytes(LIMIT);
    // Each case: its name, the delivered length, and whether it is refused.
    let cases: &[(&str, u64, bool)] = &[
        ("empty is admitted", 0, false),
        ("under the ceiling is admitted", LIMIT - 1, false),
        ("exactly the ceiling is admitted", LIMIT, false),
        ("one byte past is refused", LIMIT + 1, true),
        ("a saturated length is refused", u64::MAX, true),
    ];
    for &(name, len, refused) in cases {
        let outcome = limits.enforce(len);
        if refused {
            assert!(
                matches!(
                    outcome,
                    Err(TransferError::TooLarge { actual, limit }) if actual == len && limit == LIMIT
                ),
                "{name}: {outcome:?}"
            );
        } else {
            assert!(outcome.is_ok(), "{name}: {outcome:?}");
        }
    }
    assert!(
        TransferLimits::default()
            .with_max_bytes(u64::MAX)
            .enforce(u64::MAX)
            .is_ok(),
        "the largest ceiling admits the largest length"
    );
}
