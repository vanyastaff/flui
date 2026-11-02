//! The headless host's clipboard runs the scenario every host's clipboard
//! runs: a concealed write pastes back and stays out of the history
//! (ADR-0152 §6). The in-memory clipboard keeps no history.

use flui_native::testing::clipboard::{a_concealed_write_stays_out_of_history, read_at_once};
use flui_native::{HeadlessPlatform, Platform as _};

#[test]
fn a_concealed_write_stays_out_of_the_headless_history() {
    let platform = HeadlessPlatform::new();
    let clipboard = platform.clipboard();
    assert_eq!(
        a_concealed_write_stays_out_of_history(&*clipboard, read_at_once),
        Ok(())
    );
}
