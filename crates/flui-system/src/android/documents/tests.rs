//! The Android file dialogs on every host: the rules as functions, and the
//! whole path from a request to its answer over the kit's stand-in Activity
//! and picker (`testing::android`).

use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::{
    Abandoned, CapabilityRegistrar, ProviderContext, ResultCode, ResultRefused, WindowRef,
};
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};

use super::{
    Described, LetGo, MAX_INTENT_TYPES, MAX_PICKS, NotOpened, OPEN_KEY, PickerIntent, READ_CHUNK,
    SAVE_KEY, create_intent, open_refusal, open_types, read_bounded, read_picks, request_refusal,
    write_save,
};
use crate::dialog::{
    DialogError, FileDialogCapability, FileDialogs, FileFilter, OpenRequest, OpenResult,
    PickedForm, SaveRequest, SaveResult,
};
use crate::testing::android::{
    ActivityStandIn, Document, DocumentsStandIn, PickerStandIn, Step, Steps, TestOwner, window,
};

const TYPES: [(&str, &str); 4] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("pdf", "application/pdf"),
    ("gz", "application/gzip"),
];

fn known(extension: &str) -> Option<String> {
    TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime_type)| (*mime_type).to_owned())
}

fn images() -> FileFilter {
    FileFilter::new("Images")
        .with_extension("png")
        .with_extension("jpg")
}

fn refusal(error: DialogError) -> Option<UnsupportedReason> {
    error.unsupported().map(|unsupported| unsupported.reason)
}

/// A filter is shown as the types Android maps it to, each once; a request
/// it cannot say that way shows every document instead of dropping a
/// pattern.
#[test]
fn a_filter_becomes_the_types_android_maps_it_to_or_every_document() {
    let every = vec!["*/*".to_owned()];
    assert_eq!(open_types(&[], known), every, "no filter");
    assert_eq!(
        open_types(&[images()], known),
        ["image/png", "image/jpeg"],
        "each extension's type"
    );
    let mixed = [
        FileFilter::new("Pictures")
            .with_mime_type("image/*")
            .with_extension("png"),
        FileFilter::new("Archives").with_extension("tar.gz"),
        FileFilter::new("Again").with_mime_type("image/png"),
    ];
    assert_eq!(
        open_types(&mixed, known),
        ["image/*", "image/png", "application/gzip"],
        "a stated type as it is, an extension by its last part, each type once"
    );
    let unknown = [images(), FileFilter::new("Notes").with_extension("note")];
    assert_eq!(
        open_types(&unknown, known),
        every,
        "an extension Android has no type for widens the picker; the picks are still checked"
    );
    for widening in [
        FileFilter::all_files("Everything"),
        FileFilter::new("Make").with_file_name("Makefile"),
    ] {
        assert_eq!(
            open_types(&[images(), widening.clone()], known),
            every,
            "a filter of every file, or of a file by its name, shows every document: {widening:?}"
        );
    }
    let numbered = |count: usize| {
        (0..count).fold(FileFilter::new("Many"), |filter, index| {
            filter.with_mime_type(format!("application/x-{index}"))
        })
    };
    assert_eq!(
        open_types(&[numbered(MAX_INTENT_TYPES)], known).len(),
        MAX_INTENT_TYPES
    );
    assert_eq!(
        open_types(
            &[
                numbered(MAX_INTENT_TYPES),
                FileFilter::new("One more").with_extension("png")
            ],
            known
        ),
        every,
        "more types than an Intent carries"
    );
}

/// A save proposes the suggested name with the first filter's extension and
/// creates a document of that name's type, so Android's provider adds no
/// second extension.
#[test]
fn a_save_proposes_a_name_and_a_type_after_the_first_filter() {
    let create = |mime_type: &str, name: Option<&str>| PickerIntent::Create {
        mime_type: mime_type.to_owned(),
        name: name.map(str::to_owned),
    };
    for (suggested, filters, expected) in [
        (
            Some("photo"),
            vec![images()],
            create("image/png", Some("photo.png")),
        ),
        (
            Some("scan.PDF"),
            vec![images()],
            create("application/pdf", Some("scan.PDF")),
        ),
        (
            Some("notes.note"),
            vec![FileFilter::new("Text").with_mime_type("text/plain")],
            create("text/plain", Some("notes.note")),
        ),
        (
            Some("notes"),
            vec![FileFilter::new("Notes").with_extension("note")],
            create("application/octet-stream", Some("notes.note")),
        ),
        (
            Some("any"),
            vec![FileFilter::new("Pictures").with_mime_type("image/*")],
            create("application/octet-stream", Some("any")),
        ),
        // Unnamed: the type of the first filter's extension, so the
        // provider adds that extension to the name the user types.
        (None, vec![images()], create("image/png", None)),
        (
            None,
            vec![FileFilter::new("Archives").with_extension("tar.gz")],
            create("application/gzip", None),
        ),
        (
            None,
            vec![
                FileFilter::new("Notes")
                    .with_extension("note")
                    .with_mime_type("text/plain"),
            ],
            create("text/plain", None),
        ),
        (None, Vec::new(), create("application/octet-stream", None)),
    ] {
        assert_eq!(
            create_intent(suggested, &filters, known),
            expected,
            "{suggested:?}"
        );
    }
}

fn steps_of(documents: Vec<Document>) -> (DocumentsStandIn, Steps) {
    let steps = Steps::default();
    (DocumentsStandIn::new(documents, &steps), steps)
}

fn too_large(actual: u64, limit: u64) -> OpenResult {
    Err(DialogError::TooLarge { actual, limit })
}

/// Every document is described before any is read: a pick the filters do
/// not mean, a pick with no name, more picks than one, and picks declared
/// past the limit are refused with nothing read; a document that holds more
/// than it declared is caught by the bytes read, one byte past what is
/// left.
#[test]
fn the_picks_are_held_to_the_filters_and_the_limit_before_any_is_read() {
    let malformed = || Err(DialogError::Failed(SystemFailure::MalformedAnswer));
    let png = || Document::new("a.png", "image/png", b"12345");
    let text = || Document::new("b.txt", "text/plain", b"678");
    let nameless = || Document {
        described: Described {
            name: None,
            ..png().described
        },
        ..png()
    };
    let understated = || Document {
        described: Described {
            size: Some(1),
            ..png().described
        },
        ..png()
    };
    let undeclared = || Document {
        described: Described {
            size: None,
            ..text().described
        },
        ..text()
    };
    let described_only = |count: usize| (0..count).map(Step::Described).collect::<Vec<Step>>();

    let (mut documents, steps) = steps_of(vec![png(), text()]);
    assert_eq!(
        read_picks(
            &mut documents,
            &[images()],
            true,
            100,
            &LetGo::default(),
            false
        ),
        Err(DialogError::NotInFilter)
    );
    assert_eq!(steps.taken(), described_only(2), "nothing was read");

    let (mut documents, steps) = steps_of(vec![png(), nameless()]);
    assert_eq!(
        read_picks(&mut documents, &[], true, 100, &LetGo::default(), false),
        malformed()
    );
    assert_eq!(steps.taken(), described_only(2));

    let (mut documents, steps) = steps_of(vec![png(), text()]);
    assert_eq!(
        read_picks(&mut documents, &[], false, 100, &LetGo::default(), false),
        malformed()
    );
    assert!(
        steps.taken().is_empty(),
        "several picks for one are not described"
    );

    let (mut documents, steps) = steps_of(Vec::new());
    assert_eq!(
        read_picks(&mut documents, &[], true, 100, &LetGo::default(), false),
        malformed()
    );
    assert!(
        steps.taken().is_empty(),
        "an empty answer has nothing to describe"
    );

    let (mut documents, steps) = steps_of(vec![png(), text()]);
    assert_eq!(
        read_picks(&mut documents, &[], true, 7, &LetGo::default(), false),
        too_large(8, 7)
    );
    assert_eq!(steps.taken(), described_only(2), "declared past the limit");

    let (mut documents, steps) = steps_of(vec![understated(), text()]);
    assert_eq!(
        read_picks(&mut documents, &[], true, 4, &LetGo::default(), false),
        too_large(5, 4)
    );
    assert_eq!(
        steps.taken(),
        [Step::Described(0), Step::Described(1), Step::Read(0, 5)],
        "one byte past the limit is asked for, and the next document is not read"
    );

    let (mut documents, steps) = steps_of(vec![png(), undeclared()]);
    let picks = read_picks(&mut documents, &[], true, 8, &LetGo::default(), false)
        .expect("within the limit")
        .expect("two picks");
    assert_eq!(
        picks
            .iter()
            .map(|pick| pick.form())
            .collect::<Vec<PickedForm<'_>>>(),
        [
            PickedForm::Contents {
                name: "a.png",
                contents: b"12345"
            },
            PickedForm::Contents {
                name: "b.txt",
                contents: b"678"
            },
        ]
    );
    assert_eq!(
        steps.taken()[2..],
        [Step::Read(0, 9), Step::Read(1, 4)],
        "each read asks for what is left and one byte more"
    );

    // A MIME pattern admits a pick by the type its provider states.
    let (mut documents, _) = steps_of(vec![Document::new("IMG_0001", "image/jpeg", b"1")]);
    let typed = [FileFilter::new("Pictures").with_mime_type("image/*")];
    assert!(matches!(
        read_picks(&mut documents, &typed, false, 8, &LetGo::default(), false),
        Ok(Some(_))
    ));
}

/// A save writes the one document the picker created and names it as its
/// provider does; a document that cannot be written is deleted and the
/// failure answered.
#[test]
fn a_save_writes_the_document_the_user_chose_and_deletes_one_it_could_not_write() {
    let contents: Arc<[u8]> = Arc::from(&b"report"[..]);
    let created = || Document::new("report (1).pdf", "application/pdf", b"");

    let (mut documents, steps) = steps_of(vec![created()]);
    let saved = write_save(&mut documents, &contents, Some("report.pdf".to_owned()))
        .expect("written")
        .expect("a location");
    assert_eq!(
        saved.file().form(),
        PickedForm::Contents {
            name: "report (1).pdf",
            contents: b"report"
        },
        "named as the provider named what it created"
    );
    assert_eq!(saved.filter(), None);
    assert_eq!(
        steps.taken(),
        [Step::Wrote(0, b"report".to_vec()), Step::Described(0)]
    );

    let unnamed = Document {
        described: Described::default(),
        ..created()
    };
    let (mut documents, _) = steps_of(vec![unnamed]);
    let saved = write_save(&mut documents, &contents, Some("report.pdf".to_owned()))
        .expect("written")
        .expect("a location");
    assert_eq!(
        saved.file().file_name(),
        Some(std::ffi::OsStr::new("report.pdf"))
    );

    let full = DialogError::Failed(SystemFailure::Os(28));
    let unwritable = Document {
        write_error: Some(full),
        ..created()
    };
    let (mut documents, steps) = steps_of(vec![unwritable]);
    assert_eq!(write_save(&mut documents, &contents, None), Err(full));
    assert_eq!(steps.taken(), [Step::Discarded(0)], "no empty file is left");

    let (mut documents, steps) = steps_of(vec![created(), created()]);
    assert_eq!(
        write_save(&mut documents, &contents, None),
        Err(DialogError::Failed(SystemFailure::MalformedAnswer))
    );
    assert!(
        steps.taken().is_empty(),
        "an answer of two documents is not written"
    );
}

/// A document its provider did not open answers the dialog what the
/// provider's exception means.
#[test]
fn a_document_that_was_not_opened_answers_what_its_provider_threw() {
    for (reason, expected) in [
        (
            NotOpened::GrantEnded,
            DialogError::Failed(SystemFailure::Os(13)),
        ),
        (
            NotOpened::NotFound,
            DialogError::Failed(SystemFailure::Os(2)),
        ),
        (NotOpened::Cancelled, DialogError::WindowClosed),
        (
            NotOpened::Failed,
            DialogError::Failed(SystemFailure::MalformedAnswer),
        ),
    ] {
        assert_eq!(open_refusal(reason), expected, "{reason:?}");
    }
}

/// An owner whose host offers a stand-in Activity, a window, and the file
/// dialogs acquired in it over `picker`, as the Android registration makes
/// them.
struct Bench {
    owner: TestOwner,
    activity: Rc<ActivityStandIn>,
    picker: Rc<PickerStandIn>,
    window: WindowRef,
    _closer: flui_platform::capability::WindowCloser,
}

impl Bench {
    fn new() -> Self {
        let owner = TestOwner::new();
        let activity = Rc::new(ActivityStandIn::default());
        owner.offer_activity(
            Rc::clone(&activity) as Rc<dyn flui_platform::capability::ActivityBridge>
        );
        let (window, closer) = window();
        Self {
            owner,
            activity,
            picker: Rc::new(PickerStandIn::mapping(&TYPES)),
            window,
            _closer: closer,
        }
    }

    /// The dialogs of one acquisition.
    fn dialogs(&self) -> FileDialogs {
        let picker = Rc::clone(&self.picker);
        let mut registrar = CapabilityRegistrar::new();
        registrar.provide::<FileDialogCapability>(move |ctx: &ProviderContext<'_>| {
            let activity = ctx
                .activity()
                .ok_or(Unsupported::of::<FileDialogCapability>(
                    UnsupportedReason::NotYetOnThisHost,
                ))?;
            super::provide(ctx, activity, Rc::clone(&picker))
        });
        let provider = registrar
            .into_entries()
            .pop()
            .expect("one provider was registered");
        *provider
            .provide(&self.owner.ctx(Some(self.window.clone())))
            .expect("the host offers its Activity")
            .downcast::<FileDialogs>()
            .expect("the provider hands out the dialogs")
    }

    /// `dialog`'s outcome, running the owner's turns until it settles.
    fn settle<T: 'static>(&self, mut dialog: crate::dialog::Dialog<T>) -> Result<T, Abandoned> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(outcome) = dialog.poll() {
                return outcome;
            }
            assert!(std::time::Instant::now() < deadline, "the dialog answered");
            if self.owner.turn() == 0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
}

/// An open dialog starts the picker for the types its filters map to, and
/// answers the documents the picker named as their names and contents; a
/// user who backs out answers no pick.
#[test]
fn an_open_dialog_answers_the_documents_the_picker_named_as_their_contents() {
    let bench = Bench::new();
    let dialogs = bench.dialogs();
    let opening = dialogs.open(OpenRequest::files().allow_multiple().filter(images()));
    assert_eq!(bench.activity.taken(), 1);
    let (key, filled) = bench.activity.request(0);
    assert!(filled && key.matches("file-dialog", OPEN_KEY.as_str()));
    assert_eq!(
        *bench.picker.filled.borrow(),
        [PickerIntent::Open {
            mime_types: vec!["image/png".to_owned(), "image/jpeg".to_owned()],
            multiple: true
        }]
    );

    bench.picker.will_answer(vec![
        Document::new("a.png", "image/png", b"first"),
        Document::new("b.JPG", "image/jpeg", b"second"),
    ]);
    bench.activity.answer(0, ResultCode::Ok, true);
    let picks = bench
        .settle(opening)
        .expect("not abandoned")
        .expect("an answer")
        .expect("two picks");
    assert_eq!(
        picks
            .iter()
            .map(|pick| (pick.path(), pick.contents()))
            .collect::<Vec<(Option<&std::path::Path>, Option<&[u8]>)>>(),
        [(None, Some(&b"first"[..])), (None, Some(&b"second"[..]))],
        "a document has no path: its contents are the pick"
    );
    assert_eq!(bench.activity.intents_read(), 1);

    let backed_out = dialogs.open(OpenRequest::files());
    bench.activity.answer(1, ResultCode::Canceled, false);
    assert_eq!(bench.settle(backed_out), Ok(Ok(None)));
    assert_eq!(
        bench.activity.intents_read(),
        1,
        "a cancel carries nothing to read"
    );
}

/// What the Activity refuses, ends or answers out of shape reaches the
/// dialog's caller as its reason; a folder dialog and a save without
/// contents are refused before the Activity is asked.
#[test]
fn a_request_the_activity_refuses_or_ends_answers_the_dialog_its_reason() {
    let bench = Bench::new();
    let dialogs = bench.dialogs();
    for (refused, expected) in [
        (
            ResultRefused::TooManyOpen,
            Some(UnsupportedReason::Unavailable),
        ),
        (
            ResultRefused::NotLaunched,
            Some(UnsupportedReason::NotOnThisDevice),
        ),
    ] {
        bench.activity.refuse_next(refused);
        let error = bench
            .settle(dialogs.open(OpenRequest::files()))
            .expect("not abandoned")
            .expect_err("refused");
        assert_eq!(refusal(error), expected, "{refused:?}");
    }
    assert_eq!(bench.activity.taken(), 0);

    let ended = dialogs.open(OpenRequest::files());
    bench.activity.end(0, ResultRefused::ActivityEnded);
    assert_eq!(
        bench.settle(ended),
        Ok(Err(DialogError::WindowClosed)),
        "the Activity's window went with it"
    );
    assert_eq!(
        request_refusal(ResultRefused::AnswerUnreadable),
        DialogError::Failed(SystemFailure::MalformedAnswer)
    );

    // `RESULT_OK` without an `Intent`, and more documents than are read.
    let empty = dialogs.open(OpenRequest::files());
    bench.activity.answer(1, ResultCode::Ok, false);
    assert_eq!(
        bench.settle(empty),
        Ok(Err(DialogError::Failed(SystemFailure::MalformedAnswer)))
    );
    let too_many = dialogs.open(OpenRequest::files().allow_multiple());
    bench
        .picker
        .will_answer(vec![Document::default(); MAX_PICKS + 1]);
    bench.activity.answer(2, ResultCode::Ok, true);
    assert_eq!(
        bench.settle(too_many),
        Ok(Err(DialogError::Failed(SystemFailure::MalformedAnswer)))
    );
    assert!(
        bench.picker.steps.taken().is_empty(),
        "none of them was described"
    );

    let taken = bench.activity.taken();
    let folders = bench
        .settle(dialogs.open(OpenRequest::folders()))
        .expect("not abandoned")
        .expect_err("refused");
    assert_eq!(refusal(folders), Some(UnsupportedReason::NotYetOnThisHost));
    let unwritten = bench
        .settle(dialogs.save(SaveRequest::new().suggested_name("a.pdf")))
        .expect("not abandoned")
        .expect_err("refused");
    assert_eq!(
        refusal(unwritten),
        Some(UnsupportedReason::NotOnThisPlatform)
    );
    assert_eq!(bench.activity.taken(), taken, "neither started a picker");
}

/// A dialog its caller lets go finishes its picker: the request's ticket is
/// dropped. Let go while its answer is read, it interrupts the read.
#[test]
fn a_dialog_let_go_finishes_its_picker_and_stops_the_read_of_its_answer() {
    let bench = Bench::new();
    let dialogs = bench.dialogs();
    let waiting = dialogs.open(OpenRequest::files());
    assert!(!bench.activity.is_cancelled(0));
    drop(waiting);
    assert!(bench.activity.is_cancelled(0), "the ticket went with it");

    let reading = dialogs.open(OpenRequest::files());
    bench
        .picker
        .will_answer(vec![Document::new("a.png", "image/png", b"12345")]);
    let release = bench.picker.hold_reads();
    bench.activity.answer(1, ResultCode::Ok, true);
    // The answer settles at an owner turn, which starts the worker.
    bench.owner.turn_until(|| {
        bench
            .picker
            .steps
            .taken()
            .iter()
            .any(|step| matches!(step, Step::Read(0, _)))
    });
    drop(reading);
    assert_eq!(
        bench.picker.steps.taken().last(),
        Some(&Step::Interrupted),
        "the hold's drop stops the worker's calls"
    );
    drop(release);
}

/// An open dialog let go after its picker answered, before the answer's
/// owner turn, leaves the answer unread: no worker describes or reads any
/// of its documents, and the caller hears nothing.
#[test]
fn a_dialog_let_go_before_its_answer_is_read_reads_nothing() {
    let bench = Bench::new();
    let dialogs = bench.dialogs();
    let opening = dialogs.open(OpenRequest::files());
    bench
        .picker
        .will_answer(vec![Document::new("a.png", "image/png", b"12345")]);
    bench.activity.answer(0, ResultCode::Ok, true);
    // The answer has arrived; the caller lets go before its owner turn.
    drop(opening);
    // Turns until the owner is idle: the answer's settlement runs among
    // them, and a worker it started would have taken its first step.
    for _ in 0..100 {
        if bench.owner.turn() == 0 {
            break;
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(
        bench.picker.steps.taken(),
        Vec::<Step>::new(),
        "nobody waits for the answer, so nothing of it is read"
    );
    assert_eq!(
        bench.activity.intents_read(),
        1,
        "the answer's Intent was let go"
    );
}

/// A read stops between two chunks once its dialog is let go: the reader
/// is asked for one chunk, not for the rest of the document.
#[test]
fn a_read_stops_between_chunks_once_its_dialog_is_let_go() {
    /// A reader that lets its dialog go once it has handed over a chunk.
    struct LettingGo<'a> {
        left: &'a [u8],
        let_go: LetGo,
        chunks: usize,
    }

    impl std::io::Read for LettingGo<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.left.read(buffer)?;
            self.chunks += 1;
            self.let_go.set();
            Ok(read)
        }
    }

    let document = vec![7_u8; READ_CHUNK * 3];
    let let_go = LetGo::default();
    let mut reader = LettingGo {
        left: &document,
        let_go: let_go.clone(),
        chunks: 0,
    };
    assert_eq!(
        read_bounded(&mut reader, u64::MAX, &let_go),
        Err(DialogError::WindowClosed)
    );
    assert_eq!(reader.chunks, 1, "the rest of the document is not read");

    let whole = read_bounded(&document[..], u64::MAX, &LetGo::default()).expect("read");
    assert_eq!(
        whole.len(),
        document.len(),
        "a dialog still held reads it all"
    );
    let bounded = read_bounded(&document[..], 5, &LetGo::default()).expect("read");
    assert_eq!(bounded.len(), 5, "and no more than it asks for");
}

/// A save writes its contents to the document the user chose and answers
/// it; once the user has chosen, the write completes whoever still waits.
#[test]
fn a_save_completes_once_the_user_chose_whoever_still_waits() {
    let bench = Bench::new();
    let dialogs = bench.dialogs();
    let request = || {
        SaveRequest::new()
            .suggested_name("report")
            .filter(FileFilter::new("PDF").with_extension("pdf"))
            .contents(&b"report"[..])
    };
    let saving = dialogs.save(request());
    let (key, _) = bench.activity.request(0);
    assert!(key.matches("file-dialog", SAVE_KEY.as_str()));
    assert_eq!(
        *bench.picker.filled.borrow(),
        [PickerIntent::Create {
            mime_type: "application/pdf".to_owned(),
            name: Some("report.pdf".to_owned())
        }]
    );
    bench
        .picker
        .will_answer(vec![Document::new("report.pdf", "application/pdf", b"")]);
    bench.activity.answer(0, ResultCode::Ok, true);
    let saved: SaveResult = bench.settle(saving).expect("not abandoned");
    let location = saved.expect("written").expect("a location");
    assert_eq!(location.file().contents(), Some(&b"report"[..]));

    let let_go = dialogs.save(request());
    bench
        .picker
        .will_answer(vec![Document::new("report.pdf", "application/pdf", b"")]);
    bench.activity.answer(1, ResultCode::Ok, true);
    // The answer has arrived; the caller lets go before its owner turn.
    drop(let_go);
    let wrote = Step::Wrote(0, b"report".to_vec());
    bench.owner.turn_until(|| {
        bench
            .picker
            .steps
            .taken()
            .iter()
            .filter(|step| **step == wrote)
            .count()
            == 2
    });
    assert!(
        !bench.picker.steps.taken().contains(&Step::Interrupted),
        "a save is not interrupted"
    );
}

/// The answers to an earlier Activity's requests are claimed by a runtime's
/// first dialogs, once, and hold nothing: an open's `Intent` is let go
/// unread, and the empty document a save created is deleted.
#[test]
fn an_earlier_activitys_answers_are_claimed_once_and_hold_nothing() {
    let bench = Bench::new();
    bench
        .activity
        .earlier_answer(OPEN_KEY, ResultCode::Ok, true);
    bench
        .activity
        .earlier_answer(SAVE_KEY, ResultCode::Ok, true);
    bench
        .activity
        .earlier_answer(SAVE_KEY, ResultCode::Canceled, false);
    bench
        .picker
        .will_answer(vec![Document::new("report.pdf", "application/pdf", b"")]);

    let _dialogs = bench.dialogs();
    // One claim per waiting answer, and the one that finds nothing, per key.
    assert_eq!(bench.activity.reattachments(), 5);
    bench
        .owner
        .turn_until(|| bench.picker.steps.taken() == [Step::Discarded(0)]);
    assert_eq!(
        bench.activity.intents_read(),
        1,
        "the save's Intent was read for its document; the open's was not"
    );
    assert_eq!(bench.activity.intents_held(), 0, "the host holds no Intent");

    let _again = bench.dialogs();
    assert_eq!(
        bench.activity.reattachments(),
        5,
        "claimed once per runtime"
    );
}

#[test]
fn a_kept_pick_carries_its_uri_once_every_pick_was_read() {
    let persistable = |name: &str| Document {
        persisted: true,
        ..Document::new(name, "text/plain", b"abc")
    };
    let (mut documents, steps) = steps_of(vec![
        persistable("a.txt"),
        Document::new("b.txt", "text/plain", b"def"),
    ]);
    let picks = read_picks(&mut documents, &[], true, 8, &LetGo::default(), true)
        .expect("within the limit")
        .expect("two picks");
    let grants = picks
        .iter()
        .map(|pick| pick.grant().map(|grant| grant.payload().to_vec()))
        .collect::<Vec<Option<Vec<u8>>>>();
    assert_eq!(
        grants,
        [Some(b"content://stand-in/document/0".to_vec()), None],
        "a provider that offers no persistable permission leaves its pick without a grant"
    );
    assert_eq!(
        steps.taken()[2..],
        [Step::Read(0, 9), Step::Read(1, 6), Step::Kept(0)],
        "the permission is persisted after every pick was read"
    );

    let (mut documents, steps) = steps_of(vec![persistable("a.txt"), persistable("b.txt")]);
    assert!(read_picks(&mut documents, &[], true, 5, &LetGo::default(), true).is_err());
    assert!(
        !steps.taken().contains(&Step::Kept(0)),
        "an answer refused midway persists nothing"
    );

    let (mut documents, steps) = steps_of(vec![persistable("a.txt")]);
    let picks = read_picks(&mut documents, &[], false, 8, &LetGo::default(), false)
        .expect("within the limit")
        .expect("a pick");
    assert_eq!(picks.first().grant(), None);
    assert!(
        !steps.taken().contains(&Step::Kept(0)),
        "nothing kept unasked"
    );

    let unholdable = Document {
        uri: Some(format!("content://x/{}", "a".repeat(64 * 1024))),
        ..persistable("a.txt")
    };
    let (mut documents, steps) = steps_of(vec![unholdable]);
    let picks = read_picks(&mut documents, &[], false, 8, &LetGo::default(), true)
        .expect("within the limit")
        .expect("a pick");
    assert_eq!(picks.first().grant(), None, "a URI no grant holds");
    assert_eq!(
        steps.taken()[1..],
        [Step::Read(0, 9), Step::Kept(0), Step::Released(0)],
        "a permission no grant can carry is given back"
    );
}
