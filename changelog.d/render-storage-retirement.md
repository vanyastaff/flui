### Fixed

- Preserve independent render-object and parent-data ownership during physical storage destruction when a destructor panics or destruction begins during unwinding.
- Preserve independent pipeline event callbacks during exceptional destruction and retire replaced callback captures after releasing the notifier lock.
