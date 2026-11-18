//! Grants on a simulated sandbox: a pick's lasting access stored as bytes,
//! resolved after a restart by a new host and a new provider over the same
//! system store, and the layer's rule that no reacquired path is one the
//! process cannot open.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use flui_platform::capability::Abandoned;
use flui_system::dialog::{
    Dialog, FileDialogCapability, OpenRequest, PickedFile, PickedForm, Picks,
};
use flui_system::grant::{
    Grant, GrantCapability, GrantError, GrantKind, GrantResult, Grants, Resolving,
};
use flui_system::sandbox::{Sandbox, SandboxKind};
use flui_system::testing::{GrantStore, ScriptedFileDialogs, ScriptedLauncher, ScriptedSystem};
use flui_testing::HeadlessHost;

use crate::{acquire, outcome_slot, turn};

/// The simulated container: the one folder the sandbox reaches.
fn container() -> PathBuf {
    std::env::temp_dir().join("flui-simulated-container")
}

/// A file outside the container, which the process reaches only through a
/// grant's scope.
fn outside(name: &str) -> PathBuf {
    std::env::temp_dir().join("flui-user-documents").join(name)
}

/// The sandbox of every test here: the container alone.
fn sandbox() -> Sandbox {
    Sandbox::try_confined(SandboxKind::AppSandbox, [container()]).expect("an absolute folder")
}

/// One run of the application: a host over a scripted system resolving from
/// `store` in [`sandbox`], and its grants handle.
fn run(store: &GrantStore) -> (HeadlessHost, ScriptedSystem, Grants) {
    let system = ScriptedSystem::new(ScriptedLauncher::new(), ScriptedFileDialogs::new());
    system.grants().set_store(store.clone());
    system.grants().set_sandbox(sandbox());
    let (host, grants) = acquire::<GrantCapability>(&system);
    (
        host,
        system,
        grants.expect("the scripted plugin provides grants"),
    )
}

/// What `resolving` settles as within ten turns of `host`.
fn settled(host: &mut HeadlessHost, resolving: Resolving) -> GrantResult {
    let slot = outcome_slot::<Result<GrantResult, Abandoned>>();
    let into = Rc::clone(&slot);
    resolving.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    for _ in 0..10 {
        turn(host);
        if slot.borrow().is_some() {
            break;
        }
    }
    slot.borrow_mut()
        .take()
        .expect("settled within ten turns")
        .expect("not abandoned")
}

/// What `dialog` answers within ten turns of `host`.
fn answered<T: 'static>(host: &mut HeadlessHost, dialog: Dialog<T>) -> T {
    let slot: Rc<RefCell<Option<Result<T, Abandoned>>>> = outcome_slot();
    let into = Rc::clone(&slot);
    dialog.on_complete(move |outcome| *into.borrow_mut() = Some(outcome));
    for _ in 0..10 {
        turn(host);
        if slot.borrow().is_some() {
            break;
        }
    }
    slot.borrow_mut()
        .take()
        .expect("answered within ten turns")
        .expect("not abandoned")
}

/// The path an access reacquired.
fn path_of(result: &GrantResult) -> Option<&Path> {
    result.as_ref().ok().and_then(|access| access.file().path())
}

/// One case of the sandbox table: how the simulated system grants the file,
/// where the file is, and whether the process is handed its path.
struct Case {
    name: &'static str,
    grant: fn(&GrantStore, PathBuf) -> Grant,
    file: PathBuf,
    handed_out: bool,
}

#[test]
fn a_reacquired_path_is_handed_out_only_where_the_process_can_open_it() {
    let cases = [
        Case {
            name: "a path inside the sandbox",
            grant: |store, path| store.grant_path(path),
            file: container().join("notes.txt"),
            handed_out: true,
        },
        Case {
            name: "a path outside the sandbox, with no scope to open it",
            grant: |store, path| store.grant_path(path),
            file: outside("notes.txt"),
            handed_out: false,
        },
        Case {
            name: "a path outside the sandbox that a scope holds open",
            grant: |store, path| store.grant(path),
            file: outside("notes.txt"),
            handed_out: true,
        },
        Case {
            name: "a path that climbs out of the sandbox",
            grant: |store, path| store.grant_path(path),
            file: container()
                .join("..")
                .join("flui-user-documents")
                .join("notes.txt"),
            handed_out: false,
        },
    ];
    for case in cases {
        let store = GrantStore::new();
        let grant = (case.grant)(&store, case.file.clone());
        let (mut host, _system, grants) = run(&store);
        let resolved = settled(&mut host, grants.resolve(&grant, 1024));
        if case.handed_out {
            assert_eq!(
                path_of(&resolved),
                Some(case.file.as_path()),
                "{}",
                case.name
            );
        } else {
            assert_eq!(
                resolved.as_ref().map(|access| access.file().path()).err(),
                Some(&GrantError::Revoked),
                "{}: the process could not open it",
                case.name
            );
        }
    }
}

#[test]
fn a_picks_access_survives_a_restart_through_the_bytes_the_application_kept() {
    let store = GrantStore::new();
    let document = outside("Report.pdf");
    let kept: Vec<u8> = {
        let dialogs = ScriptedFileDialogs::new();
        let system = ScriptedSystem::new(ScriptedLauncher::new(), dialogs.clone());
        system.grants().set_store(store.clone());
        system.grants().set_sandbox(sandbox());
        let (mut host, handle) = acquire::<FileDialogCapability>(&system);
        let handle = handle.expect("the scripted plugin provides dialogs");
        let pick = PickedFile::try_new(&document)
            .expect("an absolute path")
            .granted(store.grant(&document));
        dialogs.answer_next_open(Ok(Some(Picks::try_new(vec![pick]).expect("one pick"))));
        let picks = answered(&mut host, handle.open(OpenRequest::files().keep_access()))
            .expect("the dialog answered")
            .expect("the user picked");
        let grant = picks.first().grant().expect("the pick kept its access");
        grant.to_bytes()
        // The host, its runtime and the provider end here: the process exits.
    };

    let (mut host, _system, grants) = run(&store);
    let grant = Grant::try_from_bytes(&kept).expect("the bytes the application kept");
    let resolved = settled(&mut host, grants.resolve(&grant, 1024));
    let access = resolved.expect("the grant reacquires the file after the restart");
    assert_eq!(access.file().form(), PickedForm::Path(&document));
    assert!(
        access.is_scoped(),
        "outside the sandbox, a scope holds it open"
    );
    assert_eq!(access.renewed(), None, "the grant stays as good");
}

#[test]
fn a_grant_the_system_no_longer_honours_answers_why() {
    let store = GrantStore::new();
    let revoked = store.grant(outside("revoked.txt"));
    store.revoke(&revoked);
    let deleted = store.grant(outside("deleted.txt"));
    store.remove(&deleted);
    let stale = store.grant(outside("renamed.txt"));
    store.make_stale(&stale);
    let elsewhere = GrantStore::new().grant(outside("elsewhere.txt"));
    let foreign = Grant::try_new(GrantKind::WindowsPath, &b"C\0:\0\\\0"[..]).expect("a payload");

    let (mut host, _system, grants) = run(&store);
    assert_eq!(
        settled(&mut host, grants.resolve(&revoked, 1024)).err(),
        Some(GrantError::Revoked)
    );
    assert_eq!(
        settled(&mut host, grants.resolve(&deleted, 1024)).err(),
        Some(GrantError::Missing)
    );
    assert_eq!(
        settled(&mut host, grants.resolve(&elsewhere, 1024)).err(),
        Some(GrantError::Revoked),
        "a grant another system's store made is not this one's to honour"
    );
    assert_eq!(
        settled(&mut host, grants.resolve(&foreign, 1024)).err(),
        Some(GrantError::Foreign),
        "a grant of another kind of system is not asked about"
    );

    let renewed = settled(&mut host, grants.resolve(&stale, 1024))
        .expect("a stale grant still reaches its file");
    let fresher = renewed.renewed().cloned().expect("a fresher grant to keep");
    assert_ne!(fresher, stale);
    assert_eq!(
        path_of(&settled(&mut host, grants.resolve(&fresher, 1024))),
        Some(outside("renamed.txt").as_path())
    );
}
