//! The launcher: the URI policy at the seam, and a widget's requests
//! reaching the system and answering at an owner turn.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use flui_platform::UnsupportedReason;
use flui_platform::capability::Abandoned;
use flui_platform::launch::{
    ExternalUri, InvalidUri, LaunchError, LaunchResult, LauncherCapability, MAX_EXTERNAL_URI_LEN,
};
use flui_system::SystemPlugin;
use flui_system::testing::{LaunchRequest, ScriptedFileDialogs, ScriptedLauncher, ScriptedSystem};

use crate::{acquire, outcome_slot, turn};

#[test]
fn external_uri_admits_handler_schemes_and_refuses_the_rest() {
    let too_long = format!("https://example.com/{}", "a".repeat(MAX_EXTERNAL_URI_LEN));
    let cases: [(&str, Result<&str, InvalidUri>); 17] = [
        ("https://example.com/a?b#c", Ok("https")),
        ("HTTPS://EXAMPLE.COM", Ok("HTTPS")),
        ("mailto:someone@example.com", Ok("mailto")),
        ("tel:+15550100", Ok("tel")),
        ("my-app.v2+x:open", Ok("my-app.v2+x")),
        ("javascript:alert(1)", Err(InvalidUri::RefusedScheme)),
        ("JavaScript:alert(1)", Err(InvalidUri::RefusedScheme)),
        ("vbscript:msgbox", Err(InvalidUri::RefusedScheme)),
        ("data:text/html,<p>", Err(InvalidUri::RefusedScheme)),
        ("about:blank", Err(InvalidUri::RefusedScheme)),
        (
            "blob:https://example.com/id",
            Err(InvalidUri::RefusedScheme),
        ),
        ("file:///etc/passwd", Err(InvalidUri::RefusedScheme)),
        ("example.com/path", Err(InvalidUri::NotAbsolute)),
        ("/etc/passwd", Err(InvalidUri::NotAbsolute)),
        ("1http://example.com", Err(InvalidUri::NotAbsolute)),
        (
            "https://example.com/\nheader",
            Err(InvalidUri::ControlCharacter),
        ),
        (
            too_long.as_str(),
            Err(InvalidUri::TooLong {
                limit: MAX_EXTERNAL_URI_LEN,
            }),
        ),
    ];
    for (uri, expected) in cases {
        let parsed = ExternalUri::try_new(uri);
        match expected {
            Ok(scheme) => {
                let parsed = parsed.unwrap_or_else(|error| panic!("{uri:?} refused: {error}"));
                assert_eq!(parsed.scheme(), scheme, "{uri:?}");
                assert_eq!(parsed.as_str(), uri, "{uri:?}");
            }
            Err(error) => assert_eq!(parsed, Err(error), "{uri:?}"),
        }
    }
}

#[test]
fn a_widget_opens_a_uri_and_the_answer_settles_at_the_next_turn() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");
    let uri = ExternalUri::try_new("https://example.com").expect("a web URI");

    let answered = outcome_slot::<Result<LaunchResult, Abandoned>>();
    let slot = Rc::clone(&answered);
    handle
        .open_uri(&uri)
        .on_complete(move |outcome| *slot.borrow_mut() = Some(outcome));

    assert_eq!(launcher.requests(), [LaunchRequest::OpenUri(uri)]);
    assert!(
        answered.borrow().is_none(),
        "the answer settles at an owner turn, not inside the call"
    );
    turn(&mut host);
    assert_eq!(*answered.borrow(), Some(Ok(Ok(()))));
}

#[test]
fn each_path_request_reaches_the_system_and_carries_its_answer() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");
    let path = std::env::temp_dir().join("flui-system-launch.txt");
    assert!(path.is_absolute());

    launcher.answer_next(Ok(()));
    launcher.answer_next(Err(LaunchError::NotFound));
    launcher.answer_next(Err(LaunchError::Unavailable(
        UnsupportedReason::Unavailable,
    )));
    let answers: Vec<_> = [
        handle.open_path(&path),
        handle.reveal_path(&path),
        handle.trash_path(&path),
    ]
    .into_iter()
    .map(|completion| {
        let slot = outcome_slot::<Result<LaunchResult, Abandoned>>();
        let into = Rc::clone(&slot);
        completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        slot
    })
    .collect::<Vec<_>>();
    turn(&mut host);

    assert_eq!(
        launcher.requests(),
        [
            LaunchRequest::OpenPath(path.clone()),
            LaunchRequest::RevealPath(path.clone()),
            LaunchRequest::TrashPath(path),
        ]
    );
    let settled = answers
        .iter()
        .map(|slot| slot.borrow_mut().take())
        .collect::<Vec<_>>();
    assert_eq!(
        settled,
        [
            Some(Ok(Ok(()))),
            Some(Ok(Err(LaunchError::NotFound))),
            Some(Ok(Err(LaunchError::Unavailable(
                UnsupportedReason::Unavailable
            )))),
        ]
    );
}

#[test]
fn a_relative_path_is_refused_without_asking_the_system() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");

    let relative = Path::new("notes/today.txt");
    let answers = [
        handle.open_path(relative),
        handle.reveal_path(relative),
        handle.trash_path(relative),
    ]
    .map(|completion| {
        let slot = outcome_slot::<Result<LaunchResult, Abandoned>>();
        let into = Rc::clone(&slot);
        completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        slot
    });
    turn(&mut host);

    assert!(launcher.requests().is_empty(), "the system was never asked");
    for slot in answers {
        assert_eq!(
            slot.borrow_mut().take(),
            Some(Ok(Err(LaunchError::RelativePath)))
        );
    }
}

#[test]
fn a_held_request_answers_when_released() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");
    launcher.hold();

    let answered = outcome_slot::<Result<LaunchResult, Abandoned>>();
    let slot = Rc::clone(&answered);
    handle
        .open_path(&PathBuf::from("/srv/report.pdf"))
        .on_complete(move |outcome| *slot.borrow_mut() = Some(outcome));
    turn(&mut host);
    assert!(answered.borrow().is_none(), "held until released");

    assert!(launcher.release(Err(LaunchError::Denied)));
    turn(&mut host);
    assert_eq!(*answered.borrow(), Some(Ok(Err(LaunchError::Denied))));
}

#[test]
fn the_system_plugin_refuses_until_a_host_provides_the_services() {
    let (_host, launcher) = acquire::<LauncherCapability>(&SystemPlugin::new());
    let refusal = launcher.expect_err("no host provides a launcher yet");
    assert_eq!(refusal.reason, UnsupportedReason::NotYetOnThisHost);

    let (_host, dialogs) =
        acquire::<flui_system::dialog::FileDialogCapability>(&SystemPlugin::new());
    let refusal = dialogs.expect_err("no host provides file dialogs yet");
    assert_eq!(refusal.reason, UnsupportedReason::NotYetOnThisHost);
}
