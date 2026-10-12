### Fixed

- Web keyboard input belongs to its focused canvas; unrelated HTML controls keep their input and browser defaults. Keyboard propagation and default prevention now follow independent dispatch outcomes.
- Web window focus reflects the DOM and application startup submits a fresh native snapshot after installing observers, preserving logical focus across native focus loss and restoration.
- Web windows own and retire their DOM listeners, resize observer, callbacks and frame registration. Failed installation rolls back, close/reopen cannot retain stale delivery, and a second live owner of the same canvas is refused.
- Terminal callback retirement finishes after a callback unwind; a replaced capture's destructor cannot replace the original invocation failure.
- Accepted native focus observations remain ordered and deliverable after a callback unwind, including reentrant recovery and a competing notification failure.
