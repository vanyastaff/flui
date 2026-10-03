### Fixed
- Frame completion keeps executor wakers alive outside invocation catches, delivers all remaining waiters after a wake or retirement failure, and preserves the first failure if error telemetry also panics.
- Scheduler teardown and cancellation of a pending frame completion during unrelated unwind retain opaque executor envelopes whose destruction could abort recovery; ordinary successful retirement remains synchronous.
