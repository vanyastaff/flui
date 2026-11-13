//! `Milliseconds` and `Seconds` against `std::time::Duration`: a count is any `f64`, a
//! `Duration` is neither negative nor unbounded.

/// A millisecond or second count is any `f64`. Converting one that is no `Duration` returns an
/// error; it does not panic inside `Duration::from_secs_f64`.
#[test]
fn a_count_that_is_no_duration_is_refused_and_does_not_panic() {
    use std::time::Duration;

    use flui_scheduler::duration::{Milliseconds, Seconds};

    assert_eq!(
        Duration::try_from(Milliseconds::new(12.5)),
        Ok(Duration::from_micros(12_500))
    );
    assert_eq!(
        Duration::try_from(Seconds::new(1.5)),
        Ok(Duration::from_millis(1500))
    );
    assert_eq!(Duration::try_from(Milliseconds::ZERO), Ok(Duration::ZERO));
    for count in [
        -1.0,
        -0.001,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::MAX,
    ] {
        assert!(
            Duration::try_from(Milliseconds::new(count)).is_err(),
            "{count} ms"
        );
        assert!(
            Duration::try_from(Seconds::new(count)).is_err(),
            "{count} s"
        );
    }
    // A difference of two measurements is how a negative count arises.
    let behind = Milliseconds::new(4.0) - Milliseconds::new(9.0);
    assert!(Duration::try_from(behind).is_err());
}
