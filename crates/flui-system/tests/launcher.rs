//! The launcher: the URI policy at the seam, and a widget's requests
//! reaching the system and answering at an owner turn.

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use flui_platform::capability::Abandoned;
use flui_platform::capability::scripted::{Answer, Expect, Scenario, ScenarioFailure};
use flui_platform::launch::{
    ExternalUri, InvalidUri, LaunchError, LaunchResult, LauncherCapability, MAX_EXTERNAL_URI_LEN,
    REFUSED_SCHEMES, Scheme,
};
use flui_platform::{Unsupported, UnsupportedReason};
use flui_system::SystemPlugin;
use flui_system::testing::{LaunchRequest, ScriptedFileDialogs, ScriptedLauncher, ScriptedSystem};
use flui_testing::{HeadlessHost, HeadlessWindow};

use crate::{acquire, acquire_on, outcome_slot, turn};

/// What a link to the web and to mail allows.
fn web_and_mail() -> [Scheme; 4] {
    [Scheme::HTTPS, Scheme::HTTP, Scheme::MAILTO, Scheme::TEL]
}

#[test]
fn external_uri_admits_only_the_schemes_its_caller_names() {
    let own = Scheme::try_new("my-app.v2+x").expect("an application's scheme");
    let allowed = [
        Scheme::HTTPS,
        Scheme::HTTP,
        Scheme::MAILTO,
        Scheme::TEL,
        own,
    ];
    let too_long = format!("https://example.com/{}", "a".repeat(MAX_EXTERNAL_URI_LEN));
    let at_limit = format!(
        "https://example.com/{}",
        "a".repeat(MAX_EXTERNAL_URI_LEN - "https://example.com/".len())
    );
    let cases: [(&str, Result<&str, InvalidUri>); 38] = [
        ("https://example.com/a?b#c", Ok("https")),
        // A scheme compares ignoring ASCII case, as RFC 3986 says.
        ("HTTPS://EXAMPLE.COM", Ok("HTTPS")),
        ("hTtP://example.com", Ok("hTtP")),
        ("mailto:someone@example.com", Ok("mailto")),
        ("tel:+15550100", Ok("tel")),
        ("my-app.v2+x:open", Ok("my-app.v2+x")),
        ("MY-APP.V2+X:open", Ok("MY-APP.V2+X")),
        // Percent-escapes are handed on as written, never decoded.
        ("https://example.com/%0Aheader%00", Ok("https")),
        ("https://example.com/%40evil.example", Ok("https")),
        // An IRI's letters are not hidden characters.
        ("https://例え.jp/パス", Ok("https")),
        // An `@` after the authority is the path's or the query's.
        ("https://example.com/user@host", Ok("https")),
        ("https://example.com/?to=a@b.example", Ok("https")),
        (at_limit.as_str(), Ok("https")),
        // Schemes that run code or read local data, whoever allows them.
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
        ("FILE:///etc/passwd", Err(InvalidUri::RefusedScheme)),
        // Every other scheme passes only when the caller names it.
        (
            "smb://evil.example/share",
            Err(InvalidUri::SchemeNotAllowed),
        ),
        (
            "nfs://evil.example/export",
            Err(InvalidUri::SchemeNotAllowed),
        ),
        ("ghelp:///etc/passwd", Err(InvalidUri::SchemeNotAllowed)),
        (
            "ms-msdt:/id PCWDiagnostic",
            Err(InvalidUri::SchemeNotAllowed),
        ),
        (
            "search-ms:query=x&crumb=location:\\\\evil.example",
            Err(InvalidUri::SchemeNotAllowed),
        ),
        ("my-app:open", Err(InvalidUri::SchemeNotAllowed)),
        // A user before the host reads as the host and is not one.
        (
            "https://bank.example@evil.example/",
            Err(InvalidUri::Credentials),
        ),
        (
            "https://user:secret@example.com/",
            Err(InvalidUri::Credentials),
        ),
        (
            "https:\\\\bank.example@evil.example",
            Err(InvalidUri::Credentials),
        ),
        ("example.com/path", Err(InvalidUri::NotAbsolute)),
        ("/etc/passwd", Err(InvalidUri::NotAbsolute)),
        ("1http://example.com", Err(InvalidUri::NotAbsolute)),
        // A letter that folds to an ASCII one is not one.
        ("httpſ://example.com", Err(InvalidUri::NotAbsolute)),
        (
            "https://example.com/\nheader",
            Err(InvalidUri::ControlCharacter),
        ),
        (
            "https://example.com/\u{85}",
            Err(InvalidUri::ControlCharacter),
        ),
        (
            "https://example.com/\u{202E}fdp.exe",
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
        let parsed = ExternalUri::try_new(uri, &allowed);
        match expected {
            Ok(scheme) => {
                let parsed = parsed.unwrap_or_else(|error| panic!("{uri:?} refused: {error}"));
                assert_eq!(parsed.scheme(), scheme, "{uri:?}");
                assert_eq!(parsed.as_str(), uri, "{uri:?}");
            }
            Err(error) => assert_eq!(parsed, Err(error), "{uri:?}"),
        }
    }
    assert_eq!(
        ExternalUri::try_new("https://example.com", &[]),
        Err(InvalidUri::SchemeNotAllowed),
        "a caller that names no scheme allows none"
    );
    assert_eq!(
        ExternalUri::try_new("mailto:a@example.com", &[Scheme::HTTPS]),
        Err(InvalidUri::SchemeNotAllowed)
    );
}

#[test]
fn a_scheme_is_named_once_whatever_its_case_and_never_a_refused_one() {
    assert_eq!(Scheme::try_new("HTTPS"), Some(Scheme::HTTPS));
    assert_eq!(
        Scheme::try_new(String::from("My-App")).map(|scheme| scheme.as_str().to_owned()),
        Some("my-app".to_owned())
    );
    for refused in REFUSED_SCHEMES {
        assert_eq!(Scheme::try_new(refused), None, "{refused}");
        assert_eq!(
            Scheme::try_new(refused.to_ascii_uppercase()),
            None,
            "{refused}"
        );
    }
    for malformed in ["", "1abc", "a b", "a:b", "ſcheme", "ht%74p"] {
        assert_eq!(Scheme::try_new(malformed), None, "{malformed:?}");
    }
}

#[test]
fn opening_a_program_a_script_or_a_shortcut_is_refused_without_asking_the_system() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");
    let folder = std::env::temp_dir();

    let runnable = [
        "setup.exe",
        "SETUP.EXE",
        "run.bat",
        "notes.txt.lnk",
        "site.url",
        "app.desktop",
        "Tool.app",
        "go.command",
        "script.ps1",
        "troubleshoot.diagcab",
        // Windows drops trailing dots and spaces, and opens a file through
        // a stream name.
        "setup.exe.",
        "setup.exe ",
        "setup.exe:stream",
    ];
    let answers = runnable.map(|name| {
        let slot = outcome_slot::<Result<LaunchResult, Abandoned>>();
        let into = Rc::clone(&slot);
        handle
            .open_path(&folder.join(name))
            .on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        slot
    });
    turn(&mut host);
    assert!(launcher.requests().is_empty(), "the system was never asked");
    for (name, slot) in runnable.iter().zip(answers) {
        assert_eq!(
            slot.borrow_mut().take(),
            Some(Ok(Err(LaunchError::NotADocument))),
            "{name}"
        );
    }

    // Revealing or trashing one runs nothing, and a document opens.
    launcher.answer_next(Ok(()));
    launcher.answer_next(Ok(()));
    launcher.answer_next(Ok(()));
    let program = folder.join("setup.exe");
    let document = folder.join("setup.exe.txt");
    let asked = [
        handle.reveal_path(&program),
        handle.trash_path(&program),
        handle.open_path(&document),
    ]
    .map(|completion| {
        let slot = outcome_slot::<Result<LaunchResult, Abandoned>>();
        let into = Rc::clone(&slot);
        completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
        slot
    });
    turn(&mut host);
    assert_eq!(
        launcher.requests(),
        [
            LaunchRequest::RevealPath(program.clone()),
            LaunchRequest::TrashPath(program),
            LaunchRequest::OpenPath(document),
        ]
    );
    for slot in asked {
        assert_eq!(slot.borrow_mut().take(), Some(Ok(Ok(()))));
    }
}

#[cfg(unix)]
#[test]
fn a_link_is_judged_by_what_it_links_to() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");
    let folder = std::env::temp_dir().join(format!("flui-system-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    let entry = folder.join("evil.desktop");
    std::fs::write(&entry, b"[Desktop Entry]\nExec=true\n").expect("a desktop entry");
    let disguised = folder.join("report.pdf");
    std::os::unix::fs::symlink(&entry, &disguised).expect("a symbolic link");

    let answered = outcome_slot::<Result<LaunchResult, Abandoned>>();
    let slot = Rc::clone(&answered);
    handle
        .open_path(&disguised)
        .on_complete(move |outcome| *slot.borrow_mut() = Some(outcome));
    turn(&mut host);
    assert!(launcher.requests().is_empty(), "the system was never asked");
    assert_eq!(*answered.borrow(), Some(Ok(Err(LaunchError::NotADocument))));
    let _ = std::fs::remove_dir_all(&folder);
}

#[cfg(windows)]
#[test]
fn a_network_share_or_a_device_is_refused_without_asking_the_system() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");

    let shares = [
        r"\\evil.example\share\report.pdf",
        r"\\?\UNC\evil.example\share\report.pdf",
        r"\\.\pipe\name",
    ];
    let answers = shares.map(|path| {
        [
            handle.open_path(Path::new(path)),
            handle.reveal_path(Path::new(path)),
            handle.trash_path(Path::new(path)),
        ]
        .map(|completion| {
            let slot = outcome_slot::<Result<LaunchResult, Abandoned>>();
            let into = Rc::clone(&slot);
            completion.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
            slot
        })
    });
    turn(&mut host);
    assert!(launcher.requests().is_empty(), "the system was never asked");
    for (path, slots) in shares.iter().zip(answers) {
        for slot in slots {
            assert_eq!(
                slot.borrow_mut().take(),
                Some(Ok(Err(LaunchError::NotLocal))),
                "{path}"
            );
        }
    }
}

#[test]
fn a_widget_opens_a_uri_and_the_answer_settles_at_the_next_turn() {
    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");
    let uri = ExternalUri::try_new("https://example.com", &web_and_mail()).expect("a web URI");
    launcher.answer_next(Ok(()));

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
    launcher.answer_next(Err(LaunchError::Unsupported(Unsupported::new(
        UnsupportedReason::Unavailable,
    ))));
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
            Some(Ok(Err(LaunchError::Unsupported(Unsupported::new(
                UnsupportedReason::Unavailable
            ))))),
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
        .open_path(&std::env::temp_dir().join("flui-system-report.pdf"))
        .on_complete(move |outcome| *slot.borrow_mut() = Some(outcome));
    turn(&mut host);
    assert!(answered.borrow().is_none(), "held until released");

    assert!(launcher.release(Err(LaunchError::Denied)));
    turn(&mut host);
    assert_eq!(*answered.borrow(), Some(Ok(Err(LaunchError::Denied))));
}

#[test]
fn the_system_plugin_provides_the_portal_on_linux_and_refuses_elsewhere() {
    let (_host, launcher) = acquire::<LauncherCapability>(&SystemPlugin::new());
    let (_host, dialogs) =
        acquire::<flui_system::dialog::FileDialogCapability>(&SystemPlugin::new());
    if cfg!(target_os = "linux") {
        assert!(launcher.is_ok(), "the portal backs the launcher on Linux");
        assert!(dialogs.is_ok(), "the portal backs the dialogs on Linux");
    } else {
        let refusal = launcher.expect_err("no provider yet off Linux");
        assert_eq!(refusal.reason, UnsupportedReason::NotYetOnThisHost);
        let refusal = dialogs.expect_err("no provider yet off Linux");
        assert_eq!(refusal.reason, UnsupportedReason::NotYetOnThisHost);
    }
}

#[test]
fn the_system_doubles_answer_on_the_hosts_scenario_and_report_what_was_not_scripted() {
    let scenario = Scenario::new();
    let plugin = ScriptedSystem::on(&scenario);
    let mut host = HeadlessHost::with_scenario(HeadlessWindow::new(10, 10), &scenario, &plugin);
    let launcher = acquire_on::<LauncherCapability>(&mut host)
        .expect("the scripted plugin provides a launcher");
    let uri = ExternalUri::try_new("https://example.com", &web_and_mail()).expect("a web URI");
    plugin.launcher().script().expect(
        Expect::exactly(LaunchRequest::OpenUri(uri.clone())),
        Answer::value(Err(LaunchError::Denied)).after(Duration::from_secs(2)),
    );

    let opened = outcome_slot::<Result<LaunchResult, Abandoned>>();
    let slot = Rc::clone(&opened);
    launcher
        .open_uri(&uri)
        .on_complete(move |outcome| *slot.borrow_mut() = Some(outcome));
    let revealed = outcome_slot::<Result<LaunchResult, Abandoned>>();
    let slot = Rc::clone(&revealed);
    launcher
        .reveal_path(&std::env::temp_dir().join("flui-system-report.pdf"))
        .on_complete(move |outcome| *slot.borrow_mut() = Some(outcome));

    let _ = host.pump(Duration::from_secs(1));
    assert_eq!(*opened.borrow(), None, "the answer is due at 2 s");
    assert_eq!(
        *revealed.borrow(),
        Some(Err(Abandoned::ProviderDropped)),
        "nothing answers a reveal: no success is made up"
    );
    let _ = host.pump(Duration::from_secs(1));
    assert_eq!(*opened.borrow(), Some(Ok(Err(LaunchError::Denied))));

    let failures = scenario.verify().expect_err("the reveal was not scripted");
    assert!(
        matches!(
            failures.failures(),
            [ScenarioFailure::Unexpected {
                script: "launcher",
                ..
            }]
        ),
        "{failures}"
    );
    assert!(
        plugin.dialogs().requests().is_empty(),
        "the dialogs share the scenario, not the requests"
    );
}
