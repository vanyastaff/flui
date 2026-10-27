use flui_platform::{AdmittedCommand, WindowCommand, WindowRevision};

fn main() {
    let _ = AdmittedCommand {
        command: WindowCommand::Show,
        revision: WindowRevision::ZERO,
    };
}
