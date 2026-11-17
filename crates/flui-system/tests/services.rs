//! The handles of autostart, global shortcuts, the launcher entry and the
//! user folders over their scripted systems, on every platform: what each
//! refuses before asking, what it hands back and in which order, and what
//! dropping it ends.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::scripted::{Answer, Expect, Scenario};
use flui_platform::capability::{Abandoned, Completion};
use flui_system::autostart::{
    AutostartCapability, AutostartError, AutostartRequest, AutostartState, MalformedAutostart,
};
use flui_system::shortcuts::{
    BoundShortcut, GlobalShortcutCapability, MalformedShortcut, Shortcut, ShortcutError,
    ShortcutEvent, ShortcutFeed, ShortcutId,
};
use flui_system::taskbar::{EntryState, Fraction, Progress, TaskbarCapability};
use flui_system::testing::ScriptedSystem;
use flui_system::user_dirs::{UserDir, UserDirCapability};
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::{acquire_on, acquire_twice_on, outcome_slot, turn};

/// A host whose widgets acquire from a scripted system on `scenario`.
fn scripted(scenario: &Scenario) -> (HeadlessHost, ScriptedSystem) {
    let plugin = ScriptedSystem::on(scenario);
    let host = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), scenario, &plugin);
    (host, plugin)
}

/// `pending`'s outcome, once the host's turns settled it.
fn settled<T: 'static>(host: &mut HeadlessHost, pending: Completion<T>) -> Result<T, Abandoned> {
    let slot = outcome_slot::<Result<T, Abandoned>>();
    let into = Rc::clone(&slot);
    pending.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    for _ in 0..10 {
        turn(host);
        if slot.borrow().is_some() {
            break;
        }
    }
    slot.borrow_mut().take().expect("settled within ten turns")
}

fn id(name: &str) -> ShortcutId {
    ShortcutId::try_new(name).expect("an id")
}

#[test]
fn an_autostart_request_reaches_its_system_and_a_malformed_one_does_not() {
    let scenario = Scenario::new();
    let (mut host, plugin) = scripted(&scenario);
    let autostart =
        acquire_on::<AutostartCapability>(&mut host).expect("the scripted plugin provides it");
    plugin.autostart().script().expect(
        Expect::exactly(AutostartRequest::enable().reason("Sync")),
        Answer::value(Ok(AutostartState::Enabled)),
    );
    assert_eq!(
        settled(
            &mut host,
            autostart.request(AutostartRequest::enable().reason("Sync"))
        ),
        Ok(Ok(AutostartState::Enabled))
    );
    let long = "x".repeat(AutostartRequest::MAX_REASON_LEN + 1);
    for reason in ["", "two\nlines", long.as_str()] {
        assert_eq!(
            settled(
                &mut host,
                autostart.request(AutostartRequest::enable().reason(reason))
            ),
            Ok(Err(AutostartError::InvalidRequest(MalformedAutostart::Reason))),
            "{reason:?}"
        );
    }
    assert_eq!(
        plugin.autostart().script().received(),
        1,
        "only the well-formed request reached the system"
    );
}

#[test]
fn a_folder_lookup_reaches_its_system() {
    let scenario = Scenario::new();
    let (mut host, plugin) = scripted(&scenario);
    let dirs = acquire_on::<UserDirCapability>(&mut host).expect("the scripted plugin provides it");
    plugin.user_dirs().script().expect(
        Expect::exactly(UserDir::Downloads),
        Answer::value(Ok(Some(PathBuf::from("/home/ann/Downloads")))),
    );
    assert_eq!(
        settled(&mut host, dirs.locate(UserDir::Downloads)),
        Ok(Ok(Some(PathBuf::from("/home/ann/Downloads"))))
    );
}

/// Every handle of a runtime sets one entry; a change that changes nothing
/// is not sent again.
#[test]
fn the_launcher_entry_sends_each_change_once_and_its_handles_share_one_state() {
    let scenario = Scenario::new();
    let (mut host, plugin) = scripted(&scenario);
    let [first, second] = acquire_twice_on::<TaskbarCapability>(&mut host)
        .map(|entry| entry.expect("the scripted plugin provides it"));
    let half = Progress::Done(Fraction::try_new(0.5).expect("a fraction"));
    first.set_progress(half);
    second.set_count(Some(2));
    first.set_progress(half);
    second.set_progress(Progress::Hidden);
    assert_eq!(
        plugin.taskbar().shown(),
        [
            EntryState::new(
                half,
                None,
            ),
            EntryState::new(
                half,
                Some(2),
            ),
            EntryState::new(
                Progress::Hidden,
                Some(2),
            ),
        ],
        "each handle keeps what the other set; the repeat is not sent"
    );
    assert_eq!(first.state(), second.state());
    assert!(Fraction::try_new(f64::NAN).is_none());
    assert!(Fraction::try_new(1.5).is_none());
}

#[test]
fn malformed_shortcuts_end_the_binding_before_the_system_is_asked() {
    let scenario = Scenario::new();
    let (mut host, plugin) = scripted(&scenario);
    let shortcuts =
        acquire_on::<GlobalShortcutCapability>(&mut host).expect("the scripted plugin provides it");
    let record = Shortcut::new(id("record"), "Record");
    let too_many = (0..=flui_system::shortcuts::GlobalShortcuts::MAX_PER_BINDING)
        .map(|index| Shortcut::new(id(&format!("s{index}")), "One of many"))
        .collect::<Vec<Shortcut>>();
    let cases = [
        (Vec::new(), MalformedShortcut::Count),
        (too_many, MalformedShortcut::Count),
        (vec![record.clone(), record.clone()], MalformedShortcut::DuplicateId),
        (
            vec![Shortcut::new(id("blank"), "  ")],
            MalformedShortcut::Description,
        ),
    ];
    for (asked, malformed) in cases {
        let binding = shortcuts.bind(asked);
        assert_eq!(
            settled(&mut host, binding.next()),
            Ok(ShortcutEvent::Ended(ShortcutError::InvalidRequest(malformed))),
            "{malformed:?}"
        );
    }
    assert!(plugin.shortcuts().bindings().is_empty(), "no system was asked");
}

/// A binding hands out what the system said in order, answers its end again
/// after it, and unbinds when dropped.
#[test]
fn a_binding_hears_the_system_in_order_and_unbinds_when_dropped() {
    let scenario = Scenario::new();
    let (mut host, plugin) = scripted(&scenario);
    let shortcuts =
        acquire_on::<GlobalShortcutCapability>(&mut host).expect("the scripted plugin provides it");
    let binding = shortcuts.bind(vec![Shortcut::new(id("record"), "Record")]);
    let system = plugin.shortcuts().bindings().remove(0);
    assert_eq!(system.shortcuts().len(), 1);

    let bound = ShortcutEvent::Bound(vec![BoundShortcut::new(id("record"), None)]);
    // One waits before the system speaks, the rest are queued.
    let waiting = binding.next();
    system.send(bound.clone());
    system.send(ShortcutEvent::Pressed(id("record")));
    system.send(ShortcutEvent::Released(id("record")));
    assert_eq!(settled(&mut host, waiting), Ok(bound));
    assert_eq!(
        settled(&mut host, binding.next()),
        Ok(ShortcutEvent::Pressed(id("record")))
    );
    assert_eq!(
        settled(&mut host, binding.next()),
        Ok(ShortcutEvent::Released(id("record")))
    );
    system.send(ShortcutEvent::Ended(ShortcutError::Revoked));
    system.send(ShortcutEvent::Pressed(id("record")));
    for _ in 0..2 {
        assert_eq!(
            settled(&mut host, binding.next()),
            Ok(ShortcutEvent::Ended(ShortcutError::Revoked)),
            "the end, and nothing sent after it"
        );
    }

    drop(binding);

    let binding = shortcuts.bind(vec![Shortcut::new(id("record"), "Record")]);
    let system = plugin.shortcuts().bindings().remove(1);
    assert!(system.is_held());
    let pending = binding.next();
    drop(binding);
    assert!(!system.is_held(), "dropping the binding unbinds");
    let _ = host.pump(Duration::ZERO);
    assert_eq!(
        settled(&mut host, pending),
        Err(Abandoned::ProviderDropped),
        "what still waited ends"
    );
}

/// A binding whose events nobody takes holds at most its bound: the oldest
/// press goes first, never the binding's answer.
#[test]
fn an_untaken_binding_drops_its_oldest_presses_and_keeps_its_answer() {
    let scenario = Scenario::new();
    let (mut host, plugin) = scripted(&scenario);
    let shortcuts =
        acquire_on::<GlobalShortcutCapability>(&mut host).expect("the scripted plugin provides it");
    let binding = shortcuts.bind(vec![Shortcut::new(id("record"), "Record")]);
    let system = plugin.shortcuts().bindings().remove(0);
    let bound = ShortcutEvent::Bound(vec![BoundShortcut::new(id("record"), None)]);
    system.send(bound.clone());
    // The answer and the presses fill the bound; the release takes the
    // oldest press's place.
    for _ in 1..ShortcutFeed::MAX_QUEUED {
        system.send(ShortcutEvent::Pressed(id("record")));
    }
    system.send(ShortcutEvent::Released(id("record")));
    assert_eq!(binding.dropped(), 1);
    assert_eq!(settled(&mut host, binding.next()), Ok(bound));
}
