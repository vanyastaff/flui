### Fixed
- Navigator, router and overlay owners preserve the first terminal destructor failure, retain competing outgoing ownership during unwind, and retire healthy values normally. Installed routes no longer retain themselves through navigator registry bindings; surviving local handles retain their own state.
