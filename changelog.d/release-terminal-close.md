### Fixed
- Terminal presentation cleanup now withdraws input registrations, focus, IME, GlobalKey claims, agent access, and rebuild admission even after an earlier callback or destructor failure. Exceptional teardown retains opaque outgoing values and skips optional disposal so the original failure remains authoritative; healthy teardown still destroys them normally.
- Signals owned by a closed presentation refuse creation, reads, and writes with `SignalError::OwnerClosed`; its writer context reports `EventContextError::Detached`.
