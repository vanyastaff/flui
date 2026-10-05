### Fixed

- Preserve independent render-object and parent-data ownership during physical storage destruction when a destructor panics or destruction begins during unwinding.
- Preserve independent pipeline event callbacks during exceptional destruction and retire replaced callback captures after releasing the notifier lock.

### Changed

- A render object or parent data whose retirement fails, or that is destroyed while the thread is already panicking, is retained instead of dropped (ADR-0127): its destructor never runs and its resources are held until exit. The entry's own framework state is still released.
