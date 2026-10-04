//! Run each consumer case after ordinary panics and report named failures.

type Case = (&'static str, fn());

pub fn run_cases(cases: &[Case]) {
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            let text = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("opaque panic payload");
            failures.push(format!("{name}: {text}"));
            // Opaque payloads may contain multiple destructors that panic;
            // dropping such an aggregate can abort even inside catch_unwind.
            std::mem::forget(payload);
        }
    }
    assert!(
        failures.is_empty(),
        "consumer cases failed:\n{}",
        failures.join("\n")
    );
}
