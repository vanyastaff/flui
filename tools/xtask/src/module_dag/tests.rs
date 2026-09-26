//! Each test builds a small crate in memory, declares its layers, and checks
//! the identities of what the rule reports; the last one reads the real
//! flui-widgets tree.

use std::collections::BTreeSet;

use super::source::{self, Sources};
use super::{Finding, Identity, check, declaration_from_toml};
use crate::util;

/// The findings over a crate `c` with `files` under `c/src/` and the
/// declaration `toml`; ADR-0001 exists.
fn findings(toml: &str, files: &[(&str, &str)]) -> Vec<Finding> {
    let sources = files
        .iter()
        .fold(Sources::default(), |sources, (rel, text)| {
            sources.with(&format!("c/src/{rel}"), text)
        });
    let scan = source::scan(&sources, "c/src/lib.rs").expect("the test crate scans");
    let declaration = declaration_from_toml(toml).expect("the test declaration reads");
    check(&declaration, &scan, &["ADR-0001-test.md".to_owned()]).findings
}

fn identities(toml: &str, files: &[(&str, &str)]) -> BTreeSet<Identity> {
    findings(toml, files)
        .iter()
        .map(Finding::identity)
        .collect()
}

fn set(expected: &[(&str, &str, &'static str)]) -> BTreeSet<Identity> {
    expected
        .iter()
        .map(|&(a, b, kind)| (a.to_owned(), b.to_owned(), kind))
        .collect()
}

/// Two modules, `low` below `high`, and whatever `low.rs`/`high.rs` hold.
const TWO: &str = r#"layers = [["low"], ["high"]]"#;
const TWO_LIB: (&str, &str) = ("lib.rs", "pub mod low;\npub mod high;\n");

#[test]
fn a_use_of_a_higher_module_is_refused() {
    let found = findings(
        TWO,
        &[
            TWO_LIB,
            ("low.rs", "use crate::high::High;\n"),
            ("high.rs", "pub struct High;\n"),
        ],
    );
    assert_eq!(
        found.iter().map(Finding::identity).collect::<BTreeSet<_>>(),
        set(&[("low", "high", "refused")])
    );
    let message = found[0].to_string();
    assert!(
        message.contains(
            "low (layer 0) imports high (layer 1) at c/src/low.rs:1 `crate::high::High` (1 site(s))"
        ),
        "{message}"
    );
}

#[test]
fn a_use_of_a_lower_module_is_allowed() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                ("low.rs", "pub struct Low;\n"),
                ("high.rs", "use crate::low::Low;\nuse super::low;\n"),
            ],
        ),
        set(&[])
    );
}

#[test]
fn peers_in_one_layer_may_not_import_each_other() {
    assert_eq!(
        identities(
            r#"layers = [["a", "b"]]"#,
            &[
                ("lib.rs", "pub mod a;\npub mod b;\n"),
                ("a.rs", "pub struct A;\n"),
                ("b.rs", "use crate::a::A;\n"),
            ],
        ),
        set(&[("b", "a", "refused")])
    );
}

#[test]
fn the_wildcard_layer_leaves_its_members_unchecked_among_themselves() {
    // the flui-view shape: one module below everything else
    let lib = (
        "lib.rs",
        "pub mod reactive;\npub mod owner;\npub mod context;\n",
    );
    let toml = r#"layers = [["reactive"], ["*"]]"#;
    assert_eq!(
        identities(
            toml,
            &[
                lib,
                ("reactive.rs", "pub struct Signal;\n"),
                (
                    "owner.rs",
                    "use crate::context::Ctx;\nuse crate::reactive::Signal;\n"
                ),
                ("context.rs", "pub struct Ctx;\nuse crate::owner;\n"),
            ],
        ),
        set(&[])
    );
    assert_eq!(
        identities(
            toml,
            &[
                lib,
                ("reactive.rs", "use crate::owner::Owner;\n"),
                ("owner.rs", "pub struct Owner;\n"),
                ("context.rs", ""),
            ],
        ),
        set(&[("reactive", "owner", "refused")])
    );
    // "*" shares no layer and appears once
    assert_eq!(
        identities(
            r#"layers = [["*", "reactive"], ["*"], ["*"]]"#,
            &[
                lib,
                ("reactive.rs", ""),
                ("owner.rs", ""),
                ("context.rs", "")
            ],
        ),
        set(&[("layer 0", "*", "wildcard"), ("layer 2", "*", "wildcard")])
    );
}

#[test]
fn a_path_through_a_root_reexport_counts_as_its_source_module() {
    let found = identities(
        r#"layers = [["text"], ["interaction"]]"#,
        &[
            (
                "lib.rs",
                "pub mod text;\npub mod interaction;\n\
                 pub use interaction::{Listener, GestureDetector as Detector};\n\
                 pub use flui_types::Color;\n",
            ),
            ("text.rs", "use crate::Detector;\nuse crate::Color;\n"),
            (
                "interaction.rs",
                "pub struct Listener;\npub struct GestureDetector;\n",
            ),
        ],
    );
    // Only the first segment would read `Detector`, which is no module.
    assert_eq!(found, set(&[("text", "interaction", "refused")]));
}

#[test]
fn a_path_through_a_transparent_module_counts_as_its_source() {
    let toml = r#"
layers = [["anchored_box", "scroll"], ["navigator"]]
transparent = ["__private"]
"#;
    let lib = (
        "lib.rs",
        "pub mod anchored_box;\npub mod scroll;\npub mod navigator;\npub mod __private;\n",
    );
    let files = |scroll: &'static str| {
        vec![
            lib,
            (
                "__private.rs",
                "pub use crate::anchored_box::AnchoredBox;\npub use crate::navigator::Hero;\n",
            ),
            ("anchored_box.rs", "pub struct AnchoredBox;\n"),
            (
                "navigator.rs",
                "pub struct Hero;\nuse crate::__private::AnchoredBox;\n",
            ),
            ("scroll.rs", scroll),
        ]
    };
    // navigator -> anchored_box through the seam is a legal downward edge, and
    // the seam's own `use` items are not edges
    assert_eq!(
        identities(toml, &files("use crate::__private::AnchoredBox;\n")),
        set(&[("scroll", "anchored_box", "refused")])
    );
    assert_eq!(
        identities(toml, &files("pub struct S;\nuse crate::__private::Hero;\n")),
        set(&[("scroll", "navigator", "refused")])
    );
}

#[test]
fn a_plain_path_in_a_transparent_module_follows_the_relays_own_use() {
    let found = identities(
        r#"
layers = [["layout"], ["interaction"]]
transparent = ["__private"]
"#,
        &[
            (
                "lib.rs",
                "pub mod layout;\npub mod interaction;\npub mod __private;\n",
            ),
            (
                "__private.rs",
                "use crate::interaction;\n\
                 pub use interaction::focus::install_rect_provider;\n\
                 pub use flui_types::Color;\n",
            ),
            (
                "interaction.rs",
                "pub mod focus { pub fn install_rect_provider() {} }\n",
            ),
            (
                "layout.rs",
                "use crate::__private::Color;\n\
                 pub fn f() { crate::__private::install_rect_provider(); }\n",
            ),
        ],
    );
    // `interaction` is bound by the relay's `use`; `flui_types` is bound by
    // nothing there, so it is another crate
    assert_eq!(found, set(&[("layout", "interaction", "refused")]));
    // a relay whose plain names bind each other ends, as a finding
    assert_eq!(
        identities(
            r#"
layers = [["layout"]]
transparent = ["__private"]
"#,
            &[
                ("lib.rs", "pub mod layout;\npub mod __private;\n"),
                ("__private.rs", "use a as b;\nuse b as a;\npub use a::X;\n"),
                ("layout.rs", "use crate::__private::X;\n"),
            ],
        ),
        set(&[("layout", "crate::__private::X", "unattributed")])
    );
}

#[test]
fn an_expression_or_type_path_without_a_use_is_an_edge() {
    assert_eq!(
        identities(
            r#"layers = [["text", "field"], ["interaction"]]"#,
            &[
                (
                    "lib.rs",
                    "pub mod text;\npub mod field;\npub mod interaction;\n"
                ),
                ("interaction.rs", "pub struct Listener;\n"),
                // the editable_text.rs shapes: a field type, a constructor call
                (
                    "field.rs",
                    "pub struct S { field: crate::interaction::Listener }\n",
                ),
                (
                    "text.rs",
                    "pub fn f() { let _ = crate::interaction::Listener::new(); }\n",
                ),
            ],
        ),
        set(&[
            ("field", "interaction", "refused"),
            ("text", "interaction", "refused"),
        ])
    );
}

#[test]
fn super_paths_that_leave_the_top_level_module_are_edges() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                (
                    "low/mod.rs",
                    "mod deep;\nmod inline { use super::super::high::High; }\n"
                ),
                (
                    "low/deep.rs",
                    "use super::super::high::High;\nuse super::inline;\n"
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[("low", "high", "refused")])
    );
    let found = findings(
        TWO,
        &[
            TWO_LIB,
            ("low/mod.rs", "mod deep;\n"),
            ("low/deep.rs", "use super::super::high::High;\n"),
            ("high.rs", "pub struct High;\n"),
        ],
    );
    assert!(
        found[0].to_string().contains("c/src/low/deep.rs:1"),
        "{}",
        found[0]
    );
    // `super` above the root stops the scan
    let sources = Sources::default()
        .with("c/src/lib.rs", "pub mod low;\n")
        .with("c/src/low.rs", "use super::super::x;\n");
    let error = source::scan(&sources, "c/src/lib.rs").expect_err("climbs above the root");
    assert!(
        format!("{error:#}").contains("climbs above the crate root"),
        "{error:#}"
    );
}

#[test]
fn a_path_inside_a_macro_invocation_is_an_edge() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                (
                    "low.rs",
                    "pub fn f() { let _ = vec![Some((crate::high::High::new(), 1))]; \
                     println!(\"{}\", self.x); }\n",
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[("low", "high", "refused")])
    );
}

#[test]
fn a_crate_path_in_a_macro_rules_body_is_an_edge_of_the_defining_module() {
    let found = findings(
        TWO,
        &[
            TWO_LIB,
            (
                "low.rs",
                "macro_rules! make { ($ty:ident) => { $crate::high::$ty::new() }; }\n",
            ),
            ("high.rs", "pub struct High;\n"),
        ],
    );
    assert_eq!(
        found.iter().map(Finding::identity).collect::<BTreeSet<_>>(),
        set(&[("low", "high", "refused")])
    );
    // the site reads as written, `$crate` and all
    let message = found[0].to_string();
    assert!(
        message.contains("c/src/low.rs:1 `$crate::high`"),
        "{message}"
    );
}

#[test]
fn a_macro_export_macro_belongs_to_the_module_that_defines_it() {
    // the support.rs shape: exported at the root, re-exported by the module
    // and by a seam, named through all three paths
    let toml = r#"
layers = [["support"], ["flex"]]
transparent = ["__private"]
"#;
    let files = |flex: &'static str| {
        vec![
            (
                "lib.rs",
                "mod support;\npub mod flex;\npub mod __private;\n",
            ),
            (
                "support.rs",
                "#[macro_export]\nmacro_rules! __generic { ($ty:ident) => {}; }\n\
                 pub(crate) use crate::__generic as generic;\n",
            ),
            ("__private.rs", "pub use crate::__generic as generic;\n"),
            ("flex.rs", flex),
        ]
    };
    for flex in [
        "crate::__generic!(Flex);\n",
        "crate::__private::generic!(Flex);\n",
        "use crate::support::generic;\n",
    ] {
        assert_eq!(identities(toml, &files(flex)), set(&[]), "{flex}");
    }
    // and an edge to it points at the defining module
    let toml = r#"
layers = [["flex"], ["support"]]
transparent = ["__private"]
"#;
    assert_eq!(
        identities(toml, &files("crate::__private::generic!(Flex);\n")),
        set(&[("flex", "support", "refused")])
    );
}

#[test]
fn an_extern_crate_self_alias_is_a_crate_path() {
    assert_eq!(
        identities(
            TWO,
            &[
                (
                    "lib.rs",
                    "extern crate self as flui_view;\npub mod low;\npub mod high;\n"
                ),
                (
                    "low.rs",
                    "pub fn f() -> flui_view::high::High { flui_view::high::High }\n",
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[("low", "high", "refused")])
    );
}

#[test]
fn a_leading_colon_alias_path_is_a_crate_path() {
    let run = |lib_extra: &str, low: &str| {
        let lib =
            format!("extern crate self as flui_view;\npub mod low;\npub mod high;\n{lib_extra}");
        identities(
            TWO,
            &[
                ("lib.rs", lib.as_str()),
                ("low.rs", low),
                ("high.rs", "pub struct High;\n"),
            ],
        )
    };
    let refused = set(&[("low", "high", "refused")]);
    assert_eq!(
        run("", "pub fn f() -> ::flui_view::high::High { todo() }\n"),
        refused
    );
    assert_eq!(run("", "use ::flui_view::high::High;\n"), refused);
    assert_eq!(
        run("pub use ::flui_view::high::High as H;\n", "use crate::H;\n"),
        refused
    );
    // `::` before any other name is another crate
    assert_eq!(
        run(
            "pub use ::flui_types::Color;\n",
            "use ::flui_types::Size;\nuse crate::Color;\n"
        ),
        set(&[])
    );
}

#[test]
fn cfg_test_code_is_exempt() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                (
                    "low.rs",
                    "pub struct Low;\n\
                     #[cfg(test)]\nmod tests { use crate::high::High; }\n\
                     #[cfg(all(test, feature = \"images\"))]\n\
                     fn fixture() -> crate::high::High { crate::high::High }\n\
                     impl Low {\n    #[cfg(test)]\n    fn probe(&self) -> crate::high::High { crate::high::High }\n}\n\
                     #[cfg(test)]\nmod mounted;\n\
                     mod file_level;\n",
                ),
                ("low/mounted.rs", "use crate::high::High;\n"),
                (
                    "low/file_level.rs",
                    "#![cfg(test)]\nuse crate::high::High;\n"
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[])
    );
}

#[test]
fn a_top_level_module_file_under_inner_cfg_test_is_no_module() {
    assert_eq!(
        identities(
            TWO,
            &[
                ("lib.rs", "pub mod low;\npub mod high;\nmod fixtures;\n"),
                ("low.rs", "pub struct Low;\n"),
                ("high.rs", "pub struct High;\n"),
                ("fixtures.rs", "#![cfg(test)]\nuse crate::high::High;\n"),
            ],
        ),
        set(&[])
    );
}

#[test]
fn cfg_any_test_or_feature_code_is_checked() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                (
                    "low.rs",
                    "#[cfg(any(test, feature = \"testing\"))]\npub use crate::high::High;\n\
                     #[cfg(not(test))]\nfn f() -> crate::high::High { crate::high::High }\n",
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[("low", "high", "refused")])
    );
    // the lib.rs shape: a whole module under `any(test, feature)` is scanned
    assert_eq!(
        identities(
            TWO,
            &[
                (
                    "lib.rs",
                    "pub mod high;\n#[cfg(any(test, feature = \"testing\"))]\npub mod low;\n",
                ),
                ("low.rs", "use crate::high::High;\n"),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[("low", "high", "refused")])
    );
}

#[test]
fn a_restricted_visibility_path_is_not_an_edge() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                (
                    "low.rs",
                    "pub(in crate::high) fn f() {}\npub(super) struct S;\n"
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[])
    );
}

#[test]
fn doc_links_and_string_literals_are_not_edges() {
    assert_eq!(
        identities(
            TWO,
            &[
                TWO_LIB,
                (
                    "low.rs",
                    "//! See [`crate::high::High`].\n\
                     /// Links [`crate::high::High`].\n\
                     #[doc = \"crate::high::High\"]\n\
                     pub const KEY: &str = \"flui_widgets::high::High\";\n\
                     pub fn f() { let _ = format!(\"crate::high::{}\", 1); }\n",
                ),
                ("high.rs", "pub struct High;\n"),
            ],
        ),
        set(&[])
    );
}

#[test]
fn an_undeclared_top_level_module_is_reported() {
    assert_eq!(
        identities(
            r#"layers = [["low"]]"#,
            &[
                (
                    "lib.rs",
                    "pub mod low;\nmod extra;\n#[cfg(test)]\nmod tests;\n"
                ),
                ("low.rs", ""),
                ("extra.rs", ""),
                ("tests.rs", ""),
            ],
        ),
        set(&[("extra", "", "undeclared")])
    );
}

#[test]
fn a_declared_name_that_is_not_a_module_is_reported() {
    assert_eq!(
        identities(
            r#"layers = [["low", "gone", "low::deep"], ["tests"]]"#,
            &[
                ("lib.rs", "pub mod low;\n#[cfg(test)]\nmod tests;\n"),
                ("low.rs", "mod deep;\n"),
                ("low/deep.rs", ""),
                ("tests.rs", ""),
            ],
        ),
        set(&[
            ("gone", "", "unknown"),
            ("low::deep", "", "nested"),
            // a test-only module is not a node
            ("tests", "", "unknown"),
        ])
    );
}

#[test]
fn a_module_declared_twice_is_reported() {
    assert_eq!(
        identities(
            r#"
layers = [["low"], ["low"], ["relay"]]
transparent = ["relay"]
"#,
            &[
                ("lib.rs", "pub mod low;\npub mod relay;\n"),
                ("low.rs", ""),
                ("relay.rs", ""),
            ],
        ),
        set(&[("low", "", "duplicate"), ("relay", "", "duplicate")])
    );
}

#[test]
fn an_unattributable_crate_path_is_reported() {
    let lib = (
        "lib.rs",
        "pub mod low;\npub mod high;\npub mod other;\npub fn helper() {}\n\
         pub use high::*;\npub use other::*;\n",
    );
    assert_eq!(
        identities(
            r#"layers = [["low"], ["high", "other"]]"#,
            &[
                lib,
                (
                    "low.rs",
                    "use crate::Missing;\nuse crate::*;\npub fn f() { crate::helper(); }\n\
                     use crate::Either;\n",
                ),
                ("high.rs", ""),
                ("other.rs", ""),
            ],
        ),
        set(&[
            ("low", "crate::Missing", "unattributed"),
            ("low", "crate", "unattributed"),
            ("low", "crate::helper", "unattributed"),
            ("low", "crate::Either", "unattributed"),
        ])
    );
    // the root is no node: its own code, which names `high` here, is not
    // scanned, so declaring it in a layer would pass an upward edge unseen
    assert_eq!(
        identities(
            r#"layers = [["low", "crate"], ["high", "other"]]"#,
            &[
                (
                    "lib.rs",
                    "pub mod low;\npub mod high;\npub mod other;\n\
                     pub fn helper() -> high::High { high::High }\n",
                ),
                ("low.rs", "pub fn f() { crate::helper(); }\n"),
                ("high.rs", "pub struct High;\n"),
                ("other.rs", ""),
            ],
        ),
        set(&[
            ("crate", "", "unknown"),
            ("low", "crate::helper", "unattributed"),
        ])
    );
}

#[test]
fn a_transparent_module_with_an_item_is_reported() {
    let found = findings(
        r#"
layers = [["low"]]
transparent = ["prelude", "seam"]
"#,
        &[
            (
                "lib.rs",
                "pub mod low;\npub mod seam;\npub mod prelude { pub use crate::low::Low; }\n",
            ),
            ("low.rs", "pub struct Low;\n"),
            ("seam.rs", "pub use crate::low::Low;\npub fn stray() {}\n"),
        ],
    );
    assert_eq!(
        found.iter().map(Finding::identity).collect::<BTreeSet<_>>(),
        set(&[("seam", "", "relay item")])
    );
    assert!(
        found[0].to_string().contains("c/src/seam.rs:2 `fn`"),
        "{}",
        found[0]
    );
}

/// `low` imports `high` (refused) and `high` imports `low` (allowed), under
/// the given `exceptions` entries.
fn with_exceptions(exceptions: &str) -> BTreeSet<Identity> {
    identities(
        &format!("layers = [[\"low\"], [\"high\"]]\nexceptions = [{exceptions}]"),
        &[
            TWO_LIB,
            ("low.rs", "use crate::high::High;\n"),
            ("high.rs", "pub struct High;\nuse crate::low;\n"),
        ],
    )
}

#[test]
fn an_exception_admits_one_refused_edge() {
    assert_eq!(
        with_exceptions(
            r#"{ from = "low", to = "high", exit = "ADR-0001", since = "2026-09-26", reason = "r" }"#
        ),
        set(&[])
    );
}

#[test]
fn a_stale_exception_is_reported() {
    // an edge the layers allow
    assert_eq!(
        with_exceptions(
            r#"{ from = "low", to = "high", exit = "ADR-0001", since = "2026-09-26", reason = "r" },
               { from = "high", to = "low", exit = "ADR-0001", since = "2026-09-26", reason = "r" }"#
        ),
        set(&[("high", "low", "stale exception")])
    );
    // an edge the code does not have
    assert_eq!(
        identities(
            &format!(
                "{TWO}\nexceptions = [{{ from = \"low\", to = \"high\", exit = \"ADR-0001\", \
                 since = \"2026-09-26\", reason = \"r\" }}]"
            ),
            &[TWO_LIB, ("low.rs", ""), ("high.rs", "")],
        ),
        set(&[("low", "high", "stale exception")])
    );
    // a repeated entry
    let entry =
        r#"{ from = "low", to = "high", exit = "ADR-0001", since = "2026-09-26", reason = "r" }"#;
    assert_eq!(
        with_exceptions(&format!("{entry}, {entry}")),
        set(&[("low", "high", "stale exception")])
    );
}

#[test]
fn an_exception_with_a_missing_adr_or_bad_date_is_reported() {
    let entry = |exit: &str, since: &str| {
        format!(
            r#"{{ from = "low", to = "high", exit = "{exit}", since = "{since}", reason = "r" }}"#
        )
    };
    assert_eq!(
        with_exceptions(&entry("ADR-0999", "2026-09-26")),
        set(&[("low", "high", "missing adr")])
    );
    assert_eq!(
        with_exceptions(&entry("0001", "2026-09-26")),
        set(&[("low", "high", "bad exit")])
    );
    for since in [
        "2026-02-29",
        "2026-13-01",
        "26-09-26",
        "2026-9-26",
        "yesterday",
    ] {
        assert_eq!(
            with_exceptions(&entry("ADR-0001", since)),
            set(&[("low", "high", "bad date")]),
            "{since}"
        );
    }
    assert_eq!(with_exceptions(&entry("ADR-0001", "2024-02-29")), set(&[]));
    assert_eq!(
        with_exceptions(
            r#"{ from = "low", to = "ghost", exit = "ADR-0001", since = "2026-09-26", reason = "r" }"#
        ),
        set(&[
            ("low", "high", "refused"),
            ("low", "ghost", "exception module")
        ])
    );
}

#[test]
fn a_malformed_declaration_is_an_error() {
    for (toml, needle) in [
        ("transparent = []", "needs `layers`"),
        ("layers = [\"low\"]", "each layer must be a list"),
        (
            "layers = []\nextra = 1",
            "unknown `[package.metadata.flui.modules]` key `extra`",
        ),
        (
            "layers = []\nexceptions = [{ from = \"a\", to = \"b\" }]",
            "`exceptions` must be a list of",
        ),
    ] {
        let error = declaration_from_toml(toml).expect_err(toml);
        assert!(format!("{error:#}").contains(needle), "{toml}: {error:#}");
    }
}

#[test]
fn a_missing_module_file_or_a_parse_error_stops_the_scan() {
    let sources = Sources::default().with("c/src/lib.rs", "pub mod gone;\n");
    let error = source::scan(&sources, "c/src/lib.rs").expect_err("no file");
    assert!(
        format!("{error:#}").contains("`mod gone;` has no file"),
        "{error:#}"
    );
    let sources = Sources::default()
        .with("c/src/lib.rs", "pub mod bad;\n")
        .with("c/src/bad.rs", "fn (\n");
    let error = source::scan(&sources, "c/src/lib.rs").expect_err("does not parse");
    assert!(format!("{error:#}").contains("c/src/bad.rs:1"), "{error:#}");
}

#[test]
fn the_self_test_reports_exactly_the_planted_findings() {
    let (missed, extra) = super::self_test_diff().expect("the planted crates scan");
    assert!(
        missed.is_empty() && extra.is_empty(),
        "missed {missed:#?}, false positives {extra:#?}"
    );
}

#[test]
fn the_self_test_counts_a_repeated_finding() {
    let refused = || ("low".to_owned(), "high".to_owned(), "refused");
    let (missed, extra) = super::multiset_diff(vec![refused()], vec![refused(), refused()]);
    assert_eq!((missed, extra), (vec![], vec![refused()]));
    let (missed, extra) = super::multiset_diff(vec![refused(), refused()], vec![refused()]);
    assert_eq!((missed, extra), (vec![refused()], vec![]));
}

#[test]
fn a_planted_import_in_the_real_widgets_tree_is_refused() {
    let root = util::repo_root();
    let manifest: toml::Table = toml::from_str(
        &util::read("crates/flui-widgets/Cargo.toml").expect("the widgets manifest"),
    )
    .expect("the widgets manifest parses");
    let modules = &manifest["package"]["metadata"]["flui"]["modules"];
    let declaration = super::Declaration::from_json(
        &serde_json::to_value(modules).expect("TOML converts"),
        "crates/flui-widgets/Cargo.toml",
    )
    .expect("the widgets declaration reads");
    let adrs = util::adr_files(&root);
    let lib = "crates/flui-widgets/src/lib.rs";
    let run = |sources: &Sources| -> BTreeSet<Identity> {
        let scan = source::scan(sources, lib).expect("the widgets tree scans");
        check(&declaration, &scan, &adrs)
            .findings
            .iter()
            .map(Finding::identity)
            .collect()
    };
    // a module of the lowest layer names one of the highest, whichever the
    // manifest declares today
    let named = |layer: Option<&Vec<String>>| -> String {
        layer
            .and_then(|layer| layer.iter().find(|name| *name != super::WILDCARD))
            .expect("the widgets declaration names a module in its lowest and highest layers")
            .clone()
    };
    let low = named(declaration.layers.first());
    let high = named(declaration.layers.last());
    let on_disk = Sources::on_disk(root.clone());
    let file = source::scan(&on_disk, lib)
        .expect("the widgets tree scans")
        .modules[&low]
        .clone();
    let before = run(&on_disk);
    let planted = format!(
        "{}\nuse crate::{high};\n",
        util::read(&file).expect("the low module's file")
    );
    let after = run(&Sources::on_disk(root).with(&file, &planted));
    let refused: BTreeSet<Identity> = [(low, high, "refused")].into_iter().collect();
    assert_eq!(
        after.difference(&before).cloned().collect::<BTreeSet<_>>(),
        refused,
        "before: {before:#?}"
    );
}
