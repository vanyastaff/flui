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
    let cases: Vec<(&str, Result<&str, InvalidUri>)> = vec![
        ("https://example.com/a?b#c", Ok("https")),
        // A scheme compares ignoring ASCII case, as RFC 3986 says.
        ("HTTPS://EXAMPLE.COM", Ok("HTTPS")),
        ("hTtP://example.com", Ok("hTtP")),
        ("mailto:someone@example.com", Ok("mailto")),
        (
            "mailto:a@example.com?Subject=Hi&BODY=x&cc=b@example.com&bcc=c@example.com&to=d@example.com",
            Ok("mailto"),
        ),
        ("mailto:a@example.com?&subject=Hi#part", Ok("mailto")),
        ("tel:+15550100", Ok("tel")),
        ("my-app.v2+x:open", Ok("my-app.v2+x")),
        ("MY-APP.V2+X:open", Ok("MY-APP.V2+X")),
        // Percent-escapes are handed on as written, never decoded.
        ("https://example.com/%0Aheader%00", Ok("https")),
        ("https://example.com/%40evil.example", Ok("https")),
        ("https://example.com/a%20b%22c", Ok("https")),
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
        ("data:text/html,%3Cp%3E", Err(InvalidUri::RefusedScheme)),
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
            "ms-msdt:/id%20PCWDiagnostic",
            Err(InvalidUri::SchemeNotAllowed),
        ),
        (
            "search-ms:query=x&crumb=location:%5C%5Cevil.example",
            Err(InvalidUri::SchemeNotAllowed),
        ),
        ("my-app:open", Err(InvalidUri::SchemeNotAllowed)),
        // A scheme with an authority writes its `//`: a browser reads one
        // without it all the same.
        (
            "https:bank.example@evil.example/",
            Err(InvalidUri::MissingAuthority),
        ),
        (
            "https:/bank.example@evil.example/",
            Err(InvalidUri::MissingAuthority),
        ),
        ("HTTP:example.com", Err(InvalidUri::MissingAuthority)),
        // A user before the host reads as the host and is not one.
        (
            "https://bank.example@evil.example/",
            Err(InvalidUri::Credentials),
        ),
        (
            "https://user:secret@example.com/",
            Err(InvalidUri::Credentials),
        ),
        // A mail client may attach the local file an `attach` names.
        (
            "mailto:a@example.com?attach=/etc/passwd",
            Err(InvalidUri::MailtoField),
        ),
        (
            "mailto:a@example.com?subject=x&ATTACHMENT=/etc/passwd",
            Err(InvalidUri::MailtoField),
        ),
        (
            "mailto:a@example.com?%61ttach=/etc/passwd",
            Err(InvalidUri::MailtoField),
        ),
        (
            "mailto:a@example.com?in-reply-to=x",
            Err(InvalidUri::MailtoField),
        ),
        ("example.com/path", Err(InvalidUri::NotAbsolute)),
        ("/etc/passwd", Err(InvalidUri::NotAbsolute)),
        ("1http://example.com", Err(InvalidUri::NotAbsolute)),
        // A letter that folds to an ASCII one is not one.
        ("httpſ://example.com", Err(InvalidUri::NotAbsolute)),
        // What a shell would splice into its handler's command line.
        (
            "https://example.com/\" --flag",
            Err(InvalidUri::UnencodedCharacter),
        ),
        (
            "https://example.com/ a b",
            Err(InvalidUri::UnencodedCharacter),
        ),
        ("my-app:open\"%1", Err(InvalidUri::UnencodedCharacter)),
        (
            "https:\\\\bank.example@evil.example",
            Err(InvalidUri::UnencodedCharacter),
        ),
        (
            "https://example.com/<a>",
            Err(InvalidUri::UnencodedCharacter),
        ),
        (
            "https://example.com/`x`",
            Err(InvalidUri::UnencodedCharacter),
        ),
        (
            "https://example.com/{x}",
            Err(InvalidUri::UnencodedCharacter),
        ),
        (
            "https://example.com/a|b",
            Err(InvalidUri::UnencodedCharacter),
        ),
        (
            "https://example.com/a^b",
            Err(InvalidUri::UnencodedCharacter),
        ),
        // What changes how the URI reads without being seen.
        (
            "https://example.com/\nheader",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://example.com/\u{85}",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://example.com/\u{202E}fdp.exe",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://bank\u{200B}.example/",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://example.com/\u{2060}",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://example.com/\u{FEFF}",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://exam\u{AD}ple.com/",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://example.com/\u{2028}",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            "https://example.com/\u{A0}a",
            Err(InvalidUri::HiddenCharacter),
        ),
        (
            too_long.as_str(),
            Err(InvalidUri::TooLong {
                limit: MAX_EXTERNAL_URI_LEN,
            }),
        ),
    ];
    // Every row is checked, and every row that answers otherwise is named.
    let wrong = cases
        .into_iter()
        .filter_map(|(uri, expected)| {
            let parsed = ExternalUri::try_new(uri, &allowed);
            let answered = parsed
                .as_ref()
                .map(|parsed| (parsed.scheme(), parsed.as_str()))
                .map_err(|error| *error);
            let wanted = expected.map(|scheme| (scheme, uri));
            (answered != wanted).then(|| format!("{uri:?}: {answered:?}, not {wanted:?}"))
        })
        .collect::<Vec<String>>();
    assert!(wrong.is_empty(), "{wrong:#?}");
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
    let allowed = std::collections::HashSet::from([
        Scheme::HTTPS,
        Scheme::try_new("HTTPS").expect("https"),
        Scheme::MAILTO,
    ]);
    assert_eq!(allowed.len(), 2, "a scheme is a set key, one per spelling");
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
        "tool.py",
        "tool.pyw",
        "app.jar",
        "help.chm",
        "site.website",
        "look.theme",
        "box.wsb",
        "page.mht",
        "folder.fileloc",
        "site.inetloc",
        // A Windows class id opens what the class names.
        "readme.txt.{3050f4d8-98b5-11cf-bb82-00aa00bdce0b}",
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

#[cfg(windows)]
#[test]
fn only_a_drive_letter_is_a_local_path() {
    use std::path::{Component, Prefix};

    let launcher = ScriptedLauncher::new();
    let plugin = ScriptedSystem::new(launcher.clone(), ScriptedFileDialogs::new());
    let (mut host, handle) = acquire::<LauncherCapability>(&plugin);
    let handle = handle.expect("the scripted plugin provides a launcher");

    // One row per `Prefix` kind: the drive letters are local, every other
    // prefix is not.
    let rows = [
        (r"C:\Users\me\report.pdf", true),
        (r"\\?\C:\Users\me\report.pdf", true),
        (r"\\evil.example\share\report.pdf", false),
        (r"\\?\UNC\evil.example\share\report.pdf", false),
        (
            r"\\?\GLOBALROOT\Device\Mup\evil.example\share\report.pdf",
            false,
        ),
        (r"\\.\pipe\name", false),
    ];
    let kinds = rows.map(|(path, _)| match Path::new(path).components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => "Disk",
            Prefix::VerbatimDisk(_) => "VerbatimDisk",
            Prefix::UNC(..) => "UNC",
            Prefix::VerbatimUNC(..) => "VerbatimUNC",
            Prefix::Verbatim(_) => "Verbatim",
            Prefix::DeviceNS(_) => "DeviceNS",
        },
        _ => "none",
    });
    assert_eq!(
        kinds,
        [
            "Disk",
            "VerbatimDisk",
            "UNC",
            "VerbatimUNC",
            "Verbatim",
            "DeviceNS"
        ],
        "the table covers each prefix kind"
    );

    let answers = rows.map(|(path, local)| {
        if local {
            for _ in 0..3 {
                launcher.answer_next(Ok(()));
            }
        }
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
    assert_eq!(
        launcher.requests().len(),
        6,
        "only the two local paths reached the system, once per verb"
    );
    for ((path, local), slots) in rows.iter().zip(answers) {
        let expected = if *local {
            Ok(())
        } else {
            Err(LaunchError::NotLocal)
        };
        for slot in slots {
            assert_eq!(slot.borrow_mut().take(), Some(Ok(expected)), "{path}");
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
