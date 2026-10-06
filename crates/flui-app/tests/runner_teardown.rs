//! The runner's teardown: how an application exits after a last window
//! closes, a callback panics or the session ends, run in child processes
//! whose exit codes and ordered markers the parent checks.
