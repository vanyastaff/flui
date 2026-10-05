### Fixed

- The Win32 backend now honours the exit policy: closing the last window ends
  the message loop (so `cargo run --example counter` exits when its window
  closes), an installed exit-policy hook can veto that, and
  `request_exit_policy_reevaluation` re-asks the hook from any thread.
