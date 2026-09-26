//! The rules over in-memory crates: the self-test, the scan and cfg rules,
//! the counter rule and the allowlist rules.

use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use serde_json::json;

use super::fixture::{EXPECTED, Memory};
use super::scan::{Def, Target, Truth, eval_cfg, macro_statics, scan_target};
use super::*;

/// An in-memory source; the first file is the target root.
fn memory(files: &[(&str, &str)]) -> Memory {
    Memory(
        files
            .iter()
            .map(|&(rel, text)| (rel.to_owned(), text.to_owned()))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn scan(files: &[(&str, &str)]) -> anyhow::Result<Vec<Def>> {
    let target = Target {
        src: files[0].0.to_owned(),
        bin: None,
    };
    Ok(scan_target(&memory(files), &target)?.defs)
}

fn keys(files: &[(&str, &str)]) -> Vec<String> {
    let mut keys: Vec<String> = scan(files)
        .expect("the files scan")
        .into_iter()
        .map(|def| def.item)
        .collect();
    keys.sort();
    keys
}

/// The definitions of `item`, from one lib file.
fn defs_of(source: &str, item: &str) -> Vec<Def> {
    let defs: Vec<Def> = scan(&[("src/lib.rs", source)])
        .expect("the file scans")
        .into_iter()
        .filter(|def| def.item == item)
        .collect();
    assert!(!defs.is_empty(), "no `{item}` in {source}");
    defs
}

fn tokens(text: &str) -> TokenStream {
    text.parse().expect("the test text lexes")
}

// ---------------------------------------------------------------------------
// self-test

#[test]
fn self_test_reports_exactly_the_planted_findings() {
    let (missed, extra) = self_test_diff(&EXPECTED);
    assert_eq!((missed, extra), (Vec::new(), Vec::new()));
    assert_eq!(self_test(), ExitCode::SUCCESS);
}

#[test]
fn self_test_diff_names_a_missing_and_an_unexpected_finding() {
    // one planted finding no longer expected: it is a false positive
    let (missed, extra) = self_test_diff(&EXPECTED[1..]);
    assert!(missed.is_empty(), "{missed:?}");
    let (krate, item, kind) = EXPECTED[0];
    assert_eq!(
        extra,
        vec![(krate.to_owned(), item.to_owned(), kind.to_owned())]
    );
    // one expected finding nothing plants: it is missed
    let mut more = EXPECTED.to_vec();
    more.push((HOST, "NOT_PLANTED", NEW));
    let (missed, extra) = self_test_diff(&more);
    assert_eq!(
        missed,
        vec![(HOST.to_owned(), "NOT_PLANTED".to_owned(), NEW.to_owned())]
    );
    assert!(extra.is_empty(), "{extra:?}");
}

// ---------------------------------------------------------------------------
// scan and cfg

#[test]
fn cfg_truth_table() {
    for (predicate, truth) in [
        ("test", Truth::False),
        ("all(test, feature = \"x\")", Truth::False),
        ("any(test, feature = \"x\")", Truth::Unknown),
        ("any(test, test)", Truth::False),
        ("not(test)", Truth::True),
        ("all()", Truth::True),
        ("debug_assertions", Truth::Unknown),
        ("windows", Truth::Unknown),
        ("target_os = \"macos\"", Truth::Unknown),
        ("not(any(test, doc))", Truth::Unknown),
    ] {
        assert_eq!(eval_cfg(tokens(predicate)), truth, "cfg({predicate})");
    }
}

#[test]
fn fn_local_statics_are_keyed_by_enclosing_path() {
    let keys = keys(&[
        ("src/lib.rs", "mod m;"),
        (
            "src/m.rs",
            r"
            use std::sync::Mutex;
            fn f() { static Q: Mutex<u8> = Mutex::new(0); }
            fn g() { if true { static Q: Mutex<u8> = Mutex::new(0); } }
            struct Ty;
            impl Ty { fn g(&self) { let _ = || { static S: Mutex<u8> = Mutex::new(0); }; } }
            trait Tr { fn h() { static T: Mutex<u8> = Mutex::new(0); } }
            const _: () = { static C: Mutex<u8> = Mutex::new(0); };
            ",
        ),
    ]);
    assert_eq!(
        keys,
        ["m::C", "m::Tr::h::T", "m::Ty::g::S", "m::f::Q", "m::g::Q"]
    );
}

#[test]
fn thread_local_body_parses_const_blocks_expr_initializers_and_per_entry_cfg() {
    let source = r"
        thread_local! {
            /// A doc comment.
            static REGISTRY_STACK: DropFreeRegistryStack = const {
                ManuallyDrop::new(RefCell::new(Vec::new()))
            };
            #[cfg(test)]
            static TEST_ONLY: Cell<u8> = Cell::new(0);
            static DELEGATE_STATE: RefCell<DelegateState> = const { RefCell::new(DelegateState {
                on_ready: None,
                platform: None,
            }) };
            pub(crate) static DEFAULT_TRANSITION_BUILDER: Rc<dyn Fn(u8) -> u8> = Rc::new(|x| x)
        }
        fn f() {
            std::thread_local!(static IN_FN: Cell<u8> = const { Cell::new(0) });
        }
    ";
    assert_eq!(
        keys(&[("src/lib.rs", source)]),
        [
            "DEFAULT_TRANSITION_BUILDER",
            "DELEGATE_STATE",
            "REGISTRY_STACK",
            "f::IN_FN"
        ]
    );
    let error = scan(&[("src/lib.rs", "thread_local! { static : u8 = 0; }")])
        .expect_err("a malformed body is an error");
    assert!(
        format!("{error:#}").contains("`thread_local!` body"),
        "{error:#}"
    );
}

#[test]
fn lifetime_static_in_macro_tokens_is_not_an_item() {
    let found: Vec<String> = macro_statics(tokens(
        "fn f() -> &'static str { \"static X: u8\" } static REAL: u8 = 0; static mut M: u8 = 0; \
         ($name:ident) => { static $name: u8 = 0; } \
         (&'static $t:ty) => {}",
    ))
    .into_iter()
    .map(|(name, _)| name)
    .collect();
    assert_eq!(found, ["REAL", "M", "$name"]);
}

#[test]
fn quote_interpolated_static_in_macro_tokens_is_an_item() {
    let found: Vec<String> = macro_statics(tokens(
        "quote! { static #name: ::std::sync::Mutex<u8> = ::std::sync::Mutex::new(0); \
         static mut #counter: u8 = 0; #[doc = \"x\"] fn f() {} #(#items)* }",
    ))
    .into_iter()
    .map(|(name, _)| name)
    .collect();
    assert_eq!(found, ["#name", "#counter"]);
}

#[test]
fn module_walk_follows_path_attributes_and_mod_rs_rules() {
    let lock = "static X: std::sync::Mutex<u8> = std::sync::Mutex::new(0);";
    let named = |name: &str| lock.replace('X', name);
    let (a, a_child, b_child, c_child, d) = (
        format!("mod a_child; {}", named("A")),
        named("AC"),
        named("BC"),
        named("CC"),
        named("D"),
    );
    let files = [
        (
            "src/lib.rs",
            "mod a; mod b; #[path = \"elsewhere/c_impl.rs\"] mod c; mod inline { mod d; } \
             #[cfg(test)] #[path = \"missing.rs\"] mod tests;",
        ),
        ("src/a.rs", a.as_str()),
        ("src/a/a_child.rs", a_child.as_str()),
        ("src/b/mod.rs", "mod b_child;"),
        ("src/b/b_child.rs", b_child.as_str()),
        ("src/elsewhere/c_impl.rs", "mod c_child;"),
        // a `#[path]` file owns its directory, like a `mod.rs`
        ("src/elsewhere/c_child.rs", c_child.as_str()),
        ("src/inline/d.rs", d.as_str()),
    ];
    assert_eq!(
        keys(&files),
        [
            "a::A",
            "a::a_child::AC",
            "b::b_child::BC",
            "c::c_child::CC",
            "inline::d::D"
        ]
    );
    let scanned = scan_target(
        &memory(&files),
        &Target {
            src: "src/lib.rs".to_owned(),
            bin: Some("tool".to_owned()),
        },
    )
    .expect("the files scan");
    assert_eq!(
        scanned.files,
        files.len(),
        "every file is reached and parsed"
    );
    assert!(
        scanned
            .defs
            .iter()
            .all(|def| def.item.starts_with("tool::")),
        "a bin target prefixes its keys"
    );
}

#[test]
fn same_key_under_the_same_cfg_is_ambiguous() {
    let lock = "static X: std::sync::Mutex<u8> = std::sync::Mutex::new(0);";
    let two_impls = format!(
        "struct Foo<T>(T); impl Foo<u8> {{ fn new() {{ {lock} }} }} \
         impl Foo<u16> {{ fn new() {{ {lock} }} }}"
    );
    let entry = json!([{ "item": "Foo::new::X", "exit": "ADR-0094", "reason": "a test" }]);
    let files = [("crates/flui-widgets/src/lib.rs", two_impls.as_str())];
    assert_eq!(
        problems(&files, &[krate("flui-widgets", &entry)]),
        [(
            "flui-widgets".to_owned(),
            "Foo::new::X".to_owned(),
            AMBIGUOUS
        )]
    );
    let by_cfg = format!(
        "struct Foo<T>(T); #[cfg(unix)] impl Foo<u8> {{ fn new() {{ {lock} }} }} \
         #[cfg(windows)] impl Foo<u8> {{ fn new() {{ {lock} }} }}"
    );
    let files = [("crates/flui-widgets/src/lib.rs", by_cfg.as_str())];
    assert_eq!(problems(&files, &[krate("flui-widgets", &entry)]), []);
}

#[test]
fn unresolvable_mod_is_an_error() {
    let error = scan(&[("src/lib.rs", "mod gone;")]).expect_err("an unresolved mod fails");
    assert!(
        format!("{error:#}").contains("resolves to no file"),
        "{error:#}"
    );
    let error = scan(&[
        ("src/lib.rs", "mod two;"),
        ("src/two.rs", ""),
        ("src/two/mod.rs", ""),
    ])
    .expect_err("an ambiguous mod fails");
    assert!(format!("{error:#}").contains("ambiguous"), "{error:#}");
    let error = scan(&[("src/lib.rs", "fn broken( {")]).expect_err("a parse error fails");
    assert!(
        format!("{error:#}").contains("syn cannot parse"),
        "{error:#}"
    );
}

#[test]
fn a_file_the_walk_cannot_follow_is_an_error() {
    let state = "static S: std::sync::Mutex<u8> = std::sync::Mutex::new(0);";
    for (root, needle) in [
        (
            "fn init() { #[path = \"state.rs\"] mod state; }",
            "not at module level",
        ),
        (
            "struct T; impl T { fn f() { mod inner { #[path = \"state.rs\"] mod state; } } }",
            "not at module level",
        ),
        (
            "const _: () = { #[path = \"state.rs\"] mod state; };",
            "not at module level",
        ),
        ("mod m { include!(\"state.rs\"); }", "`include!`"),
        (
            "fn f() { include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/state.rs\")); }",
            "`include!`",
        ),
    ] {
        let error = scan(&[("src/lib.rs", root), ("src/state.rs", state)])
            .expect_err(&format!("{root} is not followed"));
        assert!(format!("{error:#}").contains(needle), "{root}: {error:#}");
    }
    // build-script output, a test-only include and inline modules in a body
    // are fine
    let keys = keys(&[(
        "src/lib.rs",
        "mod generated { include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\")); } \
         #[cfg(test)] mod t { include!(\"state.rs\"); } \
         fn f() { mod inline { static I: std::sync::Mutex<u8> = std::sync::Mutex::new(0); } }",
    )]);
    assert_eq!(keys, ["f::inline::I"]);
}

// ---------------------------------------------------------------------------
// the counter rule

#[test]
fn counter_rule_rejects_each_non_fetch_add_use() {
    let declaration = "use std::sync::atomic::{AtomicU64, Ordering::Relaxed};\n\
                       static C: AtomicU64 = AtomicU64::new(1);\n";
    for (uses, exempt) in [
        ("pub fn f() -> u64 { C.fetch_add(1, Relaxed) }", true),
        (
            "mod inner { use super::C; pub fn f() -> u64 { C.fetch_add(2, super::Relaxed) } }",
            true,
        ),
        ("pub fn f() -> u64 { C.load(Relaxed) }", false),
        ("pub fn f() { C.store(1, Relaxed) }", false),
        (
            "pub fn f() { let _ = C.try_update(Relaxed, Relaxed, |v| Some(v + 1)); }",
            false,
        ),
        (
            "pub fn f() -> u64 { bump(&C) } fn bump(c: &AtomicU64) -> u64 { c.fetch_add(1, Relaxed) }",
            false,
        ),
        ("pub fn f() -> u64 { C.fetch_add(0, Relaxed) }", false),
        (
            "mod inner { use super::C as D; pub fn f() -> u64 { D.fetch_add(1, super::Relaxed) } }",
            false,
        ),
        ("pub fn f() { println!(\"{:?}\", C); }", false),
        // a test-only use does not count
        (
            "pub fn f() -> u64 { C.fetch_add(1, Relaxed) }\n#[cfg(test)] mod tests { fn t() { super::C.store(1, super::Relaxed) } }",
            true,
        ),
    ] {
        let defs = defs_of(&format!("{declaration}{uses}"), "C");
        assert_eq!(
            exemption(&defs) == Some(Exempt::Counter),
            exempt,
            "{uses}: {:?}",
            defs[0].uses
        );
    }
    let public = defs_of(
        "use std::sync::atomic::{AtomicU64, Ordering::Relaxed};\n\
         pub static C: AtomicU64 = AtomicU64::new(1);\n\
         pub fn f() -> u64 { C.fetch_add(1, Relaxed) }",
        "C",
    );
    assert_eq!(exemption(&public), None, "a `pub` counter is never exempt");
    let computed = defs_of(
        "static C: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(START);\n\
         pub fn f() -> u64 { C.fetch_add(1, std::sync::atomic::Ordering::Relaxed) }",
        "C",
    );
    assert_eq!(exemption(&computed), None, "the initializer is a literal");
    let immutable = defs_of(
        "static S: &[&str] = &[\"a\"]; static F: fn() = f; fn f() {}",
        "S",
    );
    assert_eq!(exemption(&immutable), Some(Exempt::Immutable));
}

// ---------------------------------------------------------------------------
// the allowlist

/// A crate for [`check`], with its lib at `crates/<name>/src/lib.rs` unless
/// `files` names others.
fn krate(name: &str, globals: &serde_json::Value) -> Krate {
    Krate {
        name: name.to_owned(),
        manifest: format!("crates/{name}/Cargo.toml"),
        flui: json!({ "globals": globals }),
        targets: vec![Target {
            src: format!("crates/{name}/src/lib.rs"),
            bin: None,
        }],
    }
}

fn problems(files: &[(&str, &str)], crates: &[Krate]) -> Vec<(String, String, &'static str)> {
    let report = check(&memory(files), crates, &fixture::adrs()).expect("the crates check");
    report
        .problems
        .into_iter()
        .map(|problem| (problem.krate, problem.item, problem.kind))
        .collect()
}

fn trampoline(item: &str) -> serde_json::Value {
    json!({ "item": item, "grant": "ADR-0097", "class": "trampoline", "reason": "a test" })
}

#[test]
fn trampoline_limits() {
    let cell = |name: &str| {
        format!(
            "thread_local! {{ static {name}: std::cell::Cell<u8> = const {{ std::cell::Cell::new(0) }}; }}"
        )
    };
    let (host_one, host_two) = (cell("ONE"), format!("{} {}", cell("ONE"), cell("TWO")));

    // one in the host
    let files = [("crates/flui-app/src/lib.rs", host_one.as_str())];
    assert_eq!(
        problems(&files, &[krate(HOST, &json!([trampoline("ONE")]))]),
        []
    );
    // a second in the host
    let files = [("crates/flui-app/src/lib.rs", host_two.as_str())];
    assert_eq!(
        problems(
            &files,
            &[krate(HOST, &json!([trampoline("ONE"), trampoline("TWO")]))]
        ),
        [(HOST.to_owned(), "TWO".to_owned(), SECOND_TRAMPOLINE)]
    );

    // one per backend directory; `shared/` is not a backend
    let (win, mac, shared) = (cell("WIN"), cell("MAC"), cell("SHARED"));
    let files = [
        (
            "crates/flui-platform/src/lib.rs",
            "mod platforms; mod shared;",
        ),
        (
            "crates/flui-platform/src/platforms/mod.rs",
            "mod windows; mod macos;",
        ),
        (
            "crates/flui-platform/src/platforms/windows/mod.rs",
            win.as_str(),
        ),
        ("crates/flui-platform/src/platforms/macos.rs", "mod queue;"),
        (
            "crates/flui-platform/src/platforms/macos/queue.rs",
            mac.as_str(),
        ),
        ("crates/flui-platform/src/shared.rs", shared.as_str()),
    ];
    assert_eq!(
        problems(
            &files,
            &[krate(
                PLATFORM,
                &json!([
                    trampoline("platforms::macos::queue::MAC"),
                    trampoline("platforms::windows::WIN"),
                    trampoline("shared::SHARED"),
                ])
            )]
        ),
        [(PLATFORM.to_owned(), "shared::SHARED".to_owned(), PLACEMENT)]
    );

    // another crate, and a plain static claimed as a trampoline
    let other = format!(
        "{} static PLAIN: std::sync::Mutex<u8> = std::sync::Mutex::new(0);",
        cell("CELL")
    );
    let files = [("crates/flui-widgets/src/lib.rs", other.as_str())];
    let mut found = problems(
        &files,
        &[krate(
            "flui-widgets",
            &json!([trampoline("CELL"), trampoline("PLAIN")]),
        )],
    );
    found.sort();
    assert_eq!(
        found,
        [
            ("flui-widgets".to_owned(), "CELL".to_owned(), PLACEMENT),
            ("flui-widgets".to_owned(), "PLAIN".to_owned(), CLASS),
            ("flui-widgets".to_owned(), "PLAIN".to_owned(), PLACEMENT),
        ]
    );
}

#[test]
fn entry_schema_rejects_both_exit_and_grant_unknown_adr_grant_without_class_empty_reason_duplicate_item()
 {
    let adrs = fixture::adrs();
    let parse = |globals: serde_json::Value| {
        allowlist::parse(&json!({ "globals": globals }), "crates/x/Cargo.toml", &adrs)
            .map_err(|error| format!("{error:#}"))
    };
    let ok = json!({ "item": "A", "exit": "ADR-0094", "reason": "r" });
    assert!(parse(json!([ok])).is_ok());
    assert!(
        parse(json!([{ "item": "A", "grant": "ADR-0097", "class": "process", "reason": "r" }]))
            .is_ok()
    );
    for (globals, needle) in [
        (
            json!([{ "item": "A", "exit": "ADR-0094", "grant": "ADR-0097", "class": "process", "reason": "r" }]),
            "both `exit` and `grant`",
        ),
        (
            json!([{ "item": "A", "exit": "ADR-0001", "reason": "r" }]),
            "not an ADR with a file",
        ),
        (
            json!([{ "item": "A", "exit": "step 3", "reason": "r" }]),
            "not an ADR with a file",
        ),
        (
            json!([{ "item": "A", "exit": "ADR-0094", "class": "process", "reason": "r" }]),
            "only a grant has a class",
        ),
        (
            json!([{ "item": "A", "grant": "ADR-0097", "reason": "r" }]),
            "no `class`",
        ),
        (
            json!([{ "item": "A", "grant": "ADR-0094", "class": "process", "reason": "r" }]),
            "grants are made by ADR-0097",
        ),
        (
            json!([{ "item": "A", "grant": "ADR-0097", "class": "forever", "reason": "r" }]),
            "the classes are",
        ),
        (
            json!([{ "item": "A", "exit": "ADR-0094", "reason": " " }]),
            "non-empty `reason`",
        ),
        (json!([{ "item": "A", "reason": "r" }]), "needs an `exit`"),
        (json!([ok, ok]), "lists `A` twice"),
        (
            json!([{ "item": "A", "exit": "ADR-0094", "reason": "r", "step": "W1" }]),
            "unknown key `step`",
        ),
        (json!({ "item": "A" }), "must be a list"),
    ] {
        let error = parse(globals.clone()).expect_err(&globals.to_string());
        assert!(error.contains(needle), "{globals}: {error}");
    }
}

#[test]
fn immutable_class_refuses_interior_mutable_types() {
    let source = r"
        use std::sync::{Arc, LazyLock, Mutex, OnceLock, atomic::AtomicU32};
        static TABLE: LazyLock<Vec<&'static str>> = LazyLock::new(Vec::new);
        static CONFIG: Config = Config::new();
        static LOCKED: LazyLock<Mutex<u8>> = LazyLock::new(Mutex::default);
        static INNER: Arc<OnceLock<u8>> = Arc::new(OnceLock::new());
        static FLAG: AtomicU32 = AtomicU32::new(0);
        static mut RAW: Config = Config::new();
        thread_local! {
            static SHARED: std::rc::Rc<Config> = std::rc::Rc::new(Config::new());
            static CELL: std::cell::RefCell<u8> = const { std::cell::RefCell::new(0) };
        }
    ";
    for (item, fits) in [
        ("TABLE", true),
        ("CONFIG", true),
        ("SHARED", true),
        ("LOCKED", false),
        ("INNER", false),
        ("FLAG", false),
        ("RAW", false),
        ("CELL", false),
    ] {
        let problem = allowlist::class_problem(Class::Immutable, &defs_of(source, item));
        assert_eq!(problem.is_none(), fits, "{item}: {problem:?}");
    }
}

#[test]
fn counter_and_diagnostic_classes_check_their_shapes() {
    let source = r#"
        use std::sync::{Once, atomic::{AtomicBool, AtomicU64, Ordering::Relaxed}};
        static ESCAPED: AtomicU64 = AtomicU64::new(1);
        static RESET: AtomicU64 = AtomicU64::new(1);
        pub fn f() { g(&ESCAPED); RESET.store(0, Relaxed); }
        fn g(_: &AtomicU64) {}
        static WARNED: AtomicBool = AtomicBool::new(false);
        static INIT: Once = Once::new();
        #[cfg(debug_assertions)]
        static SEEN: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
        static STATE: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
        #[cfg(all(debug_assertions, unix))]
        static DEBUG_UNIX: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
        #[cfg(not(debug_assertions))]
        static RELEASE: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
        #[cfg(any(debug_assertions, feature = "x"))]
        static EITHER: std::sync::Mutex<u8> = std::sync::Mutex::new(0);
    "#;
    let fits = |class, item| allowlist::class_problem(class, &defs_of(source, item)).is_none();
    assert!(fits(Class::Diagnostic, "DEBUG_UNIX"));
    assert!(!fits(Class::Diagnostic, "RELEASE"), "release-only state");
    assert!(!fits(Class::Diagnostic, "EITHER"), "on with the feature");
    // a file's own `#![cfg(debug_assertions)]` holds for its items
    let defs = scan(&[
        ("src/lib.rs", "mod dbg;"),
        (
            "src/dbg.rs",
            "#![cfg(debug_assertions)] static D: std::sync::Mutex<u8> = std::sync::Mutex::new(0);",
        ),
    ])
    .expect("the files scan");
    assert!(allowlist::class_problem(Class::Diagnostic, &defs).is_none());
    assert!(fits(Class::Counter, "ESCAPED"));
    assert!(!fits(Class::Counter, "RESET"));
    assert!(!fits(Class::Counter, "STATE"));
    assert!(fits(Class::Diagnostic, "WARNED"));
    assert!(fits(Class::Diagnostic, "INIT"));
    assert!(fits(Class::Diagnostic, "SEEN"));
    assert!(!fits(Class::Diagnostic, "STATE"));
    assert!(fits(Class::Process, "STATE"));
}
