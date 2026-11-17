//! The Linux providers of the services beyond the launcher and the dialogs,
//! end to end against the stand-in portal: the window a request names and
//! the export it holds, the application's id, the portal's versions,
//! autostart, global shortcuts, the launcher entry's signal, the user's
//! folders, and what the UI runtime's retirement closes.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use flui_platform::capability::Abandoned;
use flui_platform::keyboard::{Key, Modifiers};
use flui_platform::launch::{ExternalUri, LaunchError, LauncherCapability, Scheme};
use flui_platform::{SystemFailure, UnsupportedReason};
use flui_system::autostart::{
    AutostartCapability, AutostartError, AutostartRequest, AutostartState,
};
use flui_system::dialog::{DialogError, FileDialogCapability, OpenRequest, OpenResult};
use flui_system::shortcuts::{
    BoundShortcut, GlobalShortcutCapability, Shortcut, ShortcutError, ShortcutEvent, ShortcutId,
    Trigger,
};
use flui_system::taskbar::{Fraction, Progress, TaskbarCapability};
use flui_system::user_dirs::{UserDir, UserDirCapability};
use flui_testing::{HeadlessHost, HeadlessWindow};
use zbus::{MatchRule, MessageStream, OwnedVariant, Variant};

use crate::portal::{Call, Results, StandIn, pump_until, settle};
use crate::{acquire, acquire_on, outcome_slot};

/// A host whose window plays a Wayland window exported as `handle`, its
/// widgets acquiring what `portal` provides, and the window's exports.
fn exported_host(
    portal: &StandIn,
    handle: &str,
) -> (HeadlessHost, flui_testing::HeadlessPortalExports) {
    let window = HeadlessWindow::new(10, 10).with_portal_export(handle);
    let exports = window
        .portal_exports()
        .expect("the window plays an exported window");
    (HeadlessHost::with_plugin(window, &portal.plugin()), exports)
}

fn variant(value: impl Into<Variant<'static>>) -> OwnedVariant {
    OwnedVariant::try_from(value.into()).expect("an owned value")
}

/// A request shows its dialog attached to the window by the export the
/// window made for it, holds the export while the dialog is up, and
/// releases it when the dialog ends: answered, or dropped.
#[test]
fn a_request_names_the_window_by_its_export_and_releases_it_when_it_ends() {
    let portal = StandIn::serve();
    let (mut host, exports) = exported_host(&portal, "h1");
    let dialogs =
        acquire_on::<FileDialogCapability>(&mut host).expect("the portal provides dialogs");
    portal.hold();

    let slot = outcome_slot::<Result<OpenResult, Abandoned>>();
    let into = Rc::clone(&slot);
    dialogs
        .open(OpenRequest::files())
        .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    pump_until(&mut host, "the dialog reached the portal", || {
        !portal.held().is_empty()
    });
    assert!(
        matches!(&portal.calls()[..], [Call::Open { parent, .. }] if parent == "wayland:h1"),
        "{:?}",
        portal.calls()
    );
    assert_eq!(exports.live(), 1, "the request holds its export");
    let (_, path) = portal.held().remove(0);
    portal.release(&path, 1, Results::new());
    pump_until(&mut host, "the dialog answered", || slot.borrow().is_some());
    pump_until(&mut host, "the export was released", || exports.live() == 0);

    let dialog = dialogs.open(OpenRequest::files());
    pump_until(&mut host, "the second dialog reached the portal", || {
        portal.held().len() == 2
    });
    assert_eq!(exports.live(), 1);
    drop(dialog);
    pump_until(
        &mut host,
        "the dropped dialog's export was released",
        || exports.live() == 0,
    );
}

/// A launch is attached to the window the same way, and its export lasts
/// until the portal answered.
#[test]
fn a_launch_names_the_window_by_its_export_until_the_portal_answers() {
    let portal = StandIn::serve();
    let (mut host, exports) = exported_host(&portal, "h2");
    let launcher =
        acquire_on::<LauncherCapability>(&mut host).expect("the portal provides a launcher");
    let uri = ExternalUri::try_new("https://example.com/", &[Scheme::HTTPS]).expect("a web URI");
    portal.answer(0, Results::new());
    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));
    assert_eq!(
        portal.calls(),
        [Call::OpenUri {
            parent: "wayland:h2".to_owned(),
            uri: "https://example.com/".to_owned(),
        }]
    );
    pump_until(&mut host, "the export was released", || exports.live() == 0);
}

/// An unsandboxed application is named to the portal by its id before its
/// first portal call, once per portal instance: a portal that restarted is
/// told again. One with no id names itself to no one.
#[test]
fn the_application_is_named_to_each_portal_once_before_its_first_call() {
    let mut portal = StandIn::serve();
    let plugin = portal.plugin().app_id("org.example.Notes");
    let (mut host, launcher) = acquire::<LauncherCapability>(&plugin);
    let launcher = launcher.expect("the portal provides a launcher");
    let uri = ExternalUri::try_new("https://example.com/", &[Scheme::HTTPS]).expect("a web URI");
    let open = Call::OpenUri {
        parent: String::new(),
        uri: "https://example.com/".to_owned(),
    };
    let register = Call::Register {
        app_id: "org.example.Notes".to_owned(),
    };

    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));
    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));
    assert_eq!(
        portal.calls(),
        [register.clone(), open.clone(), open.clone()]
    );

    portal.depart();
    portal.return_to_bus();
    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));
    assert_eq!(
        portal.calls()[3..],
        [register, open.clone()],
        "the restarted portal is told before its first call"
    );

    let anonymous = StandIn::serve();
    let (mut host, launcher) = acquire::<LauncherCapability>(&anonymous.plugin());
    let launcher = launcher.expect("the portal provides a launcher");
    assert_eq!(settle(&mut host, launcher.open_uri(&uri)), Ok(Ok(())));
    assert_eq!(anonymous.calls(), [open]);
}

/// A portal older than the version a request needs, or one without the
/// interface, answers `NotOnThisDevice` before the request is made: never a
/// call that hangs or fails as though the portal broke.
#[test]
fn a_portal_older_than_a_request_needs_answers_not_on_this_device() {
    let portal = StandIn::serve();
    // `OpenDirectory` came with version 3, picking folders with 3.
    portal.set_version("OpenURI", 2);
    portal.set_version("FileChooser", 2);
    portal.set_version("GlobalShortcuts", 0);
    let too_old = UnsupportedReason::NotOnThisDevice;

    let (mut host, launcher) = acquire::<LauncherCapability>(&portal.plugin());
    let launcher = launcher.expect("the portal provides a launcher");
    let file = std::env::temp_dir().join(format!("flui-system-old-{}", std::process::id()));
    std::fs::write(&file, b"x").expect("a scratch file");
    let revealed = settle(&mut host, launcher.reveal_path(&file));
    assert!(
        matches!(revealed, Ok(Err(LaunchError::Unsupported(refusal))) if refusal.reason == too_old),
        "{revealed:?}"
    );
    assert_eq!(settle(&mut host, launcher.open_path(&file)), Ok(Ok(())));

    let (mut host, dialogs) = acquire::<FileDialogCapability>(&portal.plugin());
    let dialogs = dialogs.expect("the portal provides dialogs");
    let folders = settle(&mut host, dialogs.open(OpenRequest::folders()));
    assert!(
        matches!(folders, Ok(Err(DialogError::Unsupported(refusal))) if refusal.reason == too_old),
        "{folders:?}"
    );

    let (mut host, shortcuts) = acquire::<GlobalShortcutCapability>(&portal.plugin());
    let shortcuts = shortcuts.expect("the portal provides shortcuts");
    let binding = shortcuts.bind(vec![record()]);
    let ended = settle(&mut host, binding.next());
    assert!(
        matches!(ended, Ok(ShortcutEvent::Ended(ShortcutError::Unsupported(refusal))) if refusal.reason == too_old),
        "{ended:?}"
    );
    assert!(
        !portal
            .calls()
            .iter()
            .any(|call| matches!(call, Call::OpenDirectory { .. } | Call::CreateSession)),
        "no request the portal is too old for was made: {:?}",
        portal.calls()
    );
    let _ = std::fs::remove_file(&file);
}

/// Autostart asks the `Background` portal to start this program at login,
/// with the reason, and answers what the portal granted: started, left off,
/// or refused.
#[test]
fn autostart_asks_the_background_portal_and_answers_what_it_granted() {
    let portal = StandIn::serve();
    let (mut host, autostart) = acquire::<AutostartCapability>(&portal.plugin());
    let autostart = autostart.expect("the portal provides autostart");
    let granted = |background: bool, autostart: bool| {
        HashMap::from([
            ("background".to_owned(), variant(background)),
            ("autostart".to_owned(), variant(autostart)),
        ])
    };
    let program = std::env::current_exe()
        .expect("the test knows its program")
        .to_str()
        .expect("a UTF-8 path")
        .to_owned();

    portal.answer(0, granted(true, true));
    assert_eq!(
        settle(
            &mut host,
            autostart.request(AutostartRequest::enable().reason("Sync at login"))
        ),
        Ok(Ok(AutostartState::Enabled))
    );
    portal.answer(0, granted(true, false));
    assert_eq!(
        settle(&mut host, autostart.request(AutostartRequest::enable())),
        Ok(Ok(AutostartState::Disabled)),
        "the system left it off"
    );
    portal.answer(0, granted(false, false));
    assert_eq!(
        settle(&mut host, autostart.request(AutostartRequest::enable())),
        Ok(Err(AutostartError::Denied))
    );
    portal.answer(1, Results::new());
    assert_eq!(
        settle(&mut host, autostart.request(AutostartRequest::enable())),
        Ok(Err(AutostartError::Denied))
    );
    // A host application the portal knows no id of is refused this way.
    portal.answer(2, Results::new());
    assert_eq!(
        settle(&mut host, autostart.request(AutostartRequest::disable())),
        Ok(Err(SystemFailure::PortalResponse(2).into()))
    );
    assert_eq!(
        settle(
            &mut host,
            autostart.request(AutostartRequest::enable().reason(" "))
        ),
        Ok(Err(AutostartError::InvalidRequest(
            flui_system::autostart::MalformedAutostart::Reason
        ))),
        "a malformed request reaches no portal"
    );

    let calls = portal.calls();
    assert_eq!(calls.len(), 5, "{calls:?}");
    assert_eq!(
        calls[0],
        Call::Background {
            parent: String::new(),
            autostart: true,
            reason: Some("Sync at login".to_owned()),
            commandline: Some(vec![program]),
        }
    );
    assert_eq!(
        calls[4],
        Call::Background {
            parent: String::new(),
            autostart: false,
            reason: None,
            commandline: None,
        },
        "stopping names no program"
    );
}

fn record() -> Shortcut {
    let id = ShortcutId::try_new("record").expect("an id");
    let trigger = Trigger::try_new(
        Modifiers::CONTROL.union(Modifiers::SHIFT),
        &Key::character("r"),
    )
    .expect("a trigger");
    Shortcut::new(id, "Start recording").preferred(trigger)
}

fn record_id() -> ShortcutId {
    ShortcutId::try_new("record").expect("an id")
}

/// A binding creates a portal session, binds its shortcuts with their
/// preferred triggers, hears the triggers the portal chose, then each press
/// and release of its own session's shortcuts; dropping it closes the
/// session.
#[test]
fn global_shortcuts_bind_hear_their_presses_and_unbind_when_dropped() {
    let portal = StandIn::serve();
    portal.set_trigger("record", "Ctrl+Shift+R");
    let (mut host, shortcuts) = acquire::<GlobalShortcutCapability>(&portal.plugin());
    let shortcuts = shortcuts.expect("the portal provides shortcuts");

    let binding = shortcuts.bind(vec![record()]);
    assert_eq!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Bound(vec![BoundShortcut::new(
            record_id(),
            Some("Ctrl+Shift+R".to_owned())
        )]))
    );
    assert_eq!(
        portal.calls(),
        [
            Call::CreateSession,
            Call::BindShortcuts {
                parent: String::new(),
                shortcuts: vec![("record".to_owned(), Some("CTRL+SHIFT+r".to_owned()))],
            },
        ]
    );
    let session = portal.sessions().remove(0);

    // Another session's press is not this binding's.
    portal.shortcut(
        "Activated",
        "/org/freedesktop/portal/desktop/session/other/x",
        "record",
    );
    portal.shortcut("Activated", &session, "record");
    assert_eq!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Pressed(record_id()))
    );
    portal.shortcut("Deactivated", &session, "record");
    assert_eq!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Released(record_id()))
    );

    drop(binding);
    pump_until(&mut host, "the session was closed", || {
        portal.sessions_closed() == [session.clone()]
    });
}

/// A portal that restarts takes the session with it; the binding is renewed
/// on the new portal and hears `Bound` again. A session the portal closes
/// ends the binding as revoked.
#[test]
fn a_restarted_portal_renews_the_binding_and_a_closed_session_ends_it() {
    let mut portal = StandIn::serve();
    let (mut host, shortcuts) = acquire::<GlobalShortcutCapability>(&portal.plugin());
    let shortcuts = shortcuts.expect("the portal provides shortcuts");
    let binding = shortcuts.bind(vec![record()]);
    assert!(matches!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Bound(_))
    ));

    portal.depart();
    portal.return_to_bus();
    assert!(
        matches!(
            settle(&mut host, binding.next()),
            Ok(ShortcutEvent::Bound(_))
        ),
        "the binding is renewed"
    );
    let sessions = portal.sessions();
    assert_eq!(sessions.len(), 2, "a new session on the new portal");

    portal.close_session(&sessions[1]);
    assert_eq!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Ended(ShortcutError::Revoked))
    );
    assert_eq!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Ended(ShortcutError::Revoked)),
        "the end is answered again"
    );
}

/// A binding the user refuses ends as denied, and its session is closed.
#[test]
fn a_refused_binding_ends_denied_and_closes_its_session() {
    let portal = StandIn::serve();
    let (mut host, shortcuts) = acquire::<GlobalShortcutCapability>(&portal.plugin());
    let shortcuts = shortcuts.expect("the portal provides shortcuts");
    portal.answer(1, Results::new());
    let binding = shortcuts.bind(vec![record()]);
    assert_eq!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Ended(ShortcutError::Denied))
    );
    let session = portal.sessions().remove(0);
    pump_until(&mut host, "the session was closed", || {
        portal.sessions_closed() == [session.clone()]
    });
}

/// The launcher entry broadcasts its whole state on the session bus under
/// the application's desktop entry; an application with no id has no entry
/// to name.
#[test]
fn the_launcher_entry_broadcasts_its_whole_state_under_the_applications_entry() {
    let portal = StandIn::serve();
    let (sender, heard) = mpsc::channel::<(String, HashMap<String, OwnedVariant>)>();
    let address = portal.address().to_owned();
    let (ready, listening) = mpsc::channel::<()>();
    let listener = std::thread::spawn(move || {
        zruntime::block_on(async move {
            use futures_util::StreamExt as _;
            let connection = zbus::connection::Builder::address(address.as_str())
                .build()
                .await
                .expect("the listener connects");
            let rule = MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("com.canonical.Unity.LauncherEntry")
                .member("Update")
                .build()
                .expect("a rule");
            let mut updates = MessageStream::for_match_rule(rule, &connection, None)
                .await
                .expect("the listener subscribes");
            let _ = ready.send(());
            for _ in 0..2 {
                let Some(Ok(message)) = updates.next().await else {
                    return;
                };
                let update = message
                    .body()
                    .deserialize::<(String, HashMap<String, OwnedVariant>)>()
                    .expect("an update");
                let _ = sender.send(update);
            }
        });
    });
    listening
        .recv_timeout(Duration::from_secs(10))
        .expect("the listener subscribed");

    let plugin = portal.plugin().app_id("org.example.Notes");
    let (_host, entry) = acquire::<TaskbarCapability>(&plugin);
    let entry = entry.expect("an application with an id has an entry");
    assert!(
        !entry.honours().indeterminate && !entry.honours().confirmed,
        "a signal shows no indeterminate progress, and no dock confirms it"
    );
    entry.set_progress(Progress::Done(Fraction::try_new(0.5).expect("a fraction")));
    let (uri, first) = heard
        .recv_timeout(Duration::from_secs(10))
        .expect("the first update");
    assert_eq!(uri, "application://org.example.Notes.desktop");
    assert_eq!(first["progress"], variant(0.5));
    assert_eq!(first["progress-visible"], variant(true));
    assert_eq!(first["count-visible"], variant(false));
    entry.set_count(Some(3));
    let (_, second) = heard
        .recv_timeout(Duration::from_secs(10))
        .expect("the second update");
    assert_eq!(
        second["progress"],
        variant(0.5),
        "the whole state each time"
    );
    assert_eq!(second["count"], variant(3_i64));
    assert_eq!(second["count-visible"], variant(true));
    listener.join().expect("the listener ends");

    let (_host, anonymous) = acquire::<TaskbarCapability>(&portal.plugin());
    assert!(
        matches!(
            anonymous,
            Err(refusal) if matches!(refusal.reason, UnsupportedReason::NotDeclared { .. })
        ),
        "no id, no entry"
    );
}

/// The home folder is the process's, answered off the owner thread.
#[test]
fn the_home_folder_is_the_one_the_process_names() {
    let portal = StandIn::serve();
    let (mut host, dirs) = acquire::<UserDirCapability>(&portal.plugin());
    let dirs = dirs.expect("user folders are provided");
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    assert_eq!(settle(&mut host, dirs.locate(UserDir::Home)), Ok(Ok(home)));
}

/// A UI runtime that ends takes its portal with it, and the portal closes
/// what its requests still hold: a binding kept past the runtime's end is
/// unbound, not left registered with the desktop.
#[test]
fn a_runtime_that_ends_closes_the_sessions_its_portal_still_holds() {
    let portal = StandIn::serve();
    let (mut host, shortcuts) = acquire::<GlobalShortcutCapability>(&portal.plugin());
    let shortcuts = shortcuts.expect("the portal provides shortcuts");
    let binding = shortcuts.bind(vec![record()]);
    assert!(matches!(
        settle(&mut host, binding.next()),
        Ok(ShortcutEvent::Bound(_))
    ));
    let session = portal.sessions().remove(0);
    drop(shortcuts);
    drop(host);
    let deadline = Instant::now() + Duration::from_secs(10);
    while portal.sessions_closed().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the session was closed within 10 s"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(portal.sessions_closed(), [session]);
    drop(binding);
}
