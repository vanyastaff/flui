//! Each case builds a small crate in memory, declares its layers, and checks
//! the identities of what the rule reports; the last case reads the real
//! flui-widgets tree.

use std::collections::BTreeSet;

use super::source::{self, Sources};
use super::{Finding, Identity, Outcome, check, declaration_from_toml};
use crate::util;

/// The evaluated graph over a crate `c` with `files` under `c/src/` and the
/// declaration `toml`; ADR-0001 exists.
fn outcome(toml: &str, files: &[(&str, &str)]) -> Outcome {
    let sources = files
        .iter()
        .fold(Sources::default(), |sources, (rel, text)| {
            sources.with(&format!("c/src/{rel}"), text)
        });
    let scan = source::scan(&sources, "c/src/lib.rs").expect("the test crate scans");
    let declaration = declaration_from_toml(toml).expect("the test declaration reads");
    check(&declaration, &scan, &["ADR-0001-test.md".to_owned()])
}

fn findings(toml: &str, files: &[(&str, &str)]) -> Vec<Finding> {
    outcome(toml, files).findings
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

fn a_path_through_a_root_reexport_counts_as_its_source_module() {
    let found = identities(
        r#"layers = [["text"], ["interaction"]]"#,
        &[
            (
                "lib.rs",
                "pub mod text;\npub mod interaction;\n\
                 pub use interaction::{Listener, GestureDetector as Detector};\n\
                 pub use flui_painting::styling::Color;\n",
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

fn assert_root_alternatives_retain_the_high_owner(aliases: &str) {
    let lib = format!("pub mod low; pub mod high;\n{aliases}");
    let found = findings(
        TWO,
        &[
            ("lib.rs", &lib),
            ("low.rs", "pub struct Low; use crate::X;"),
            ("high.rs", "pub struct High;"),
        ],
    );
    assert_eq!(
        found.iter().map(Finding::identity).collect::<BTreeSet<_>>(),
        set(&[("low", "high", "refused")]),
    );
    assert!(found[0].to_string().contains("imports high"));
}

fn conditional_root_reexports_do_not_hide_the_first_owner() {
    assert_root_alternatives_retain_the_high_owner(
        "#[cfg(windows)] pub use high::High as X;\n#[cfg(unix)] pub use low::Low as X;",
    );
}

fn conditional_root_reexports_do_not_hide_the_last_owner() {
    assert_root_alternatives_retain_the_high_owner(
        "#[cfg(unix)] pub use low::Low as X;\n#[cfg(windows)] pub use high::High as X;",
    );
}

fn conditional_external_reexports_do_not_hide_an_internal_owner() {
    assert_root_alternatives_retain_the_high_owner(
        "#[cfg(windows)] pub use high::High as X;\n#[cfg(unix)] pub use ::external::X;",
    );
}

fn conditional_internal_reexports_do_not_hide_an_external_owner() {
    assert_root_alternatives_retain_the_high_owner(
        "#[cfg(unix)] pub use ::external::X;\n#[cfg(windows)] pub use high::High as X;",
    );
}

const RELAY: &str = "layers = [[\"low\"], [\"high\"]]\ntransparent = [\"relay\"]";
const RELAY_LIB: (&str, &str) = ("lib.rs", "pub mod low; pub mod high; pub mod relay;");

fn conditional_relay_reexports_are_not_overwritten() {
    assert_eq!(
        identities(
            RELAY,
            &[
                RELAY_LIB,
                ("low.rs", "pub struct Low; use crate::relay::X;"),
                ("high.rs", "pub struct High;"),
                (
                    "relay.rs",
                    "#[cfg(windows)] pub use crate::high::High as X;\n\
                     #[cfg(unix)] pub use crate::low::Low as X;",
                ),
            ],
        ),
        set(&[("low", "high", "refused")]),
    );
}

fn conditional_reexports_with_the_same_owner_remain_attributable() {
    assert_eq!(
        identities(
            TWO,
            &[
                (
                    "lib.rs",
                    "pub mod low; pub mod high;\n\
                     pub use low::Low as A; pub use low::Other as B;\n\
                     #[cfg(windows)] pub use A as X;\n\
                     #[cfg(unix)] pub use B as X;\n\
                     #[cfg(windows)] pub use A as Y;\n\
                     #[cfg(unix)] pub use A as Y;",
                ),
                ("low.rs", "pub struct Low; pub struct Other;"),
                ("high.rs", "use crate::{X, Y};"),
            ],
        ),
        set(&[]),
    );
}

fn a_cyclic_conditional_reexport_cannot_hide_behind_a_healthy_branch() {
    assert_eq!(
        identities(
            TWO,
            &[
                (
                    "lib.rs",
                    "pub mod low; pub mod high;\n\
                     #[cfg(windows)] pub use X as X;\n\
                     #[cfg(unix)] pub use low::Low as X;",
                ),
                ("low.rs", "pub struct Low; use crate::X;"),
                ("high.rs", ""),
            ],
        ),
        set(&[("low", "crate::X", "unattributed")]),
    );
}

fn test_only_relay_items_do_not_change_production_attribution() {
    assert_eq!(
        identities(
            RELAY,
            &[
                RELAY_LIB,
                ("low.rs", "pub struct Low;"),
                ("high.rs", "pub struct High; use crate::relay::Low;"),
                (
                    "relay.rs",
                    "pub use crate::low::Low;\n\
                     #[cfg(test)] mod tests { use crate::high::High; }\n\
                     #[cfg(test)] fn test_helper() -> crate::high::High { loop {} }",
                ),
            ],
        ),
        set(&[]),
    );
}

fn inner_test_only_relay_modules_do_not_change_production_attribution() {
    assert_eq!(
        identities(
            RELAY,
            &[
                RELAY_LIB,
                ("low.rs", "pub struct Low;"),
                ("high.rs", "pub struct High; use crate::relay::Low;"),
                ("relay.rs", "pub use crate::low::Low; mod tests;"),
                ("relay/tests.rs", "#![cfg(test)]\nuse crate::high::High;"),
            ],
        ),
        set(&[]),
    );
}

fn a_feature_enabled_relay_item_is_still_production_code() {
    assert_eq!(
        identities(
            RELAY,
            &[
                RELAY_LIB,
                ("low.rs", "pub struct Low;"),
                ("high.rs", ""),
                (
                    "relay.rs",
                    "pub use crate::low::Low;\n\
                     #[cfg(any(test, feature = \"testing\"))] fn helper() {}",
                ),
            ],
        ),
        set(&[("relay", "", "relay item")]),
    );
}

fn assert_allowed_alternative_edges(aliases: &str, relay: bool) {
    let lib = if relay {
        "pub mod low_a; pub mod low_b; pub mod consumer; pub mod relay;".to_owned()
    } else {
        format!("pub mod low_a; pub mod low_b; pub mod consumer;\n{aliases}")
    };
    let declaration = if relay {
        "layers = [[\"low_a\"], [\"low_b\"], [\"consumer\"]]\ntransparent = [\"relay\"]"
    } else {
        "layers = [[\"low_a\"], [\"low_b\"], [\"consumer\"]]"
    };
    let consumer = if relay {
        "use crate::relay::X;"
    } else {
        "use crate::X;"
    };
    let mut files = vec![
        ("lib.rs", lib.as_str()),
        ("low_a.rs", "pub struct A;"),
        ("low_b.rs", "pub struct B;"),
        ("consumer.rs", consumer),
    ];
    if relay {
        files.push(("relay.rs", aliases));
    }
    let evaluated = outcome(declaration, &files);
    assert!(evaluated.findings.is_empty(), "{:#?}", evaluated.findings);
    let edges: BTreeSet<_> = evaluated
        .edges
        .keys()
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();
    assert_eq!(
        edges,
        BTreeSet::from([("consumer", "low_a"), ("consumer", "low_b")])
    );
}

fn distinct_allowed_root_owners_both_contribute_edges() {
    assert_allowed_alternative_edges(
        "#[cfg(windows)] pub use crate::low_a::A as X;\n\
         #[cfg(unix)] pub use crate::low_b::B as X;",
        false,
    );
}

fn reordered_distinct_allowed_root_owners_both_contribute_edges() {
    assert_allowed_alternative_edges(
        "#[cfg(unix)] pub use crate::low_b::B as X;\n\
         #[cfg(windows)] pub use crate::low_a::A as X;",
        false,
    );
}

fn distinct_allowed_relay_owners_both_contribute_edges() {
    assert_allowed_alternative_edges(
        "#[cfg(windows)] pub use crate::low_a::A as X;\n\
         #[cfg(unix)] pub use crate::low_b::B as X;",
        true,
    );
}

fn assert_nested_alternatives_retain_the_high_owner(aliases: &str) {
    let lib = format!("pub mod low; pub mod high;\n{aliases}");
    assert_eq!(
        identities(
            TWO,
            &[
                ("lib.rs", &lib),
                ("low.rs", "pub struct Low; use crate::Y;"),
                ("high.rs", "pub struct High;"),
            ]
        ),
        set(&[("low", "high", "refused")]),
    );
}

fn nested_conditional_reexports_retain_the_high_owner() {
    assert_nested_alternatives_retain_the_high_owner(
        "#[cfg(windows)] pub use low::Low as X;\n\
         #[cfg(unix)] pub use high::High as X;\n\
         #[cfg(unix)] pub use X as Y;\n\
         #[cfg(windows)] pub use low::Low as Y;",
    );
}

fn reordered_nested_conditional_reexports_retain_the_high_owner() {
    assert_nested_alternatives_retain_the_high_owner(
        "#[cfg(unix)] pub use high::High as X;\n\
         #[cfg(windows)] pub use low::Low as X;\n\
         #[cfg(windows)] pub use low::Low as Y;\n\
         #[cfg(unix)] pub use X as Y;",
    );
}

fn assert_cyclic_and_forbidden_alternatives_are_both_reported(aliases: &str) {
    let lib = format!("pub mod low; pub mod high;\n{aliases}");
    assert_eq!(
        identities(
            TWO,
            &[
                ("lib.rs", &lib),
                ("low.rs", "use crate::X;"),
                ("high.rs", "pub struct High;"),
            ]
        ),
        set(&[
            ("low", "high", "refused"),
            ("low", "crate::X", "unattributed")
        ]),
    );
}

fn a_cyclic_branch_does_not_erase_a_forbidden_owner() {
    assert_cyclic_and_forbidden_alternatives_are_both_reported(
        "#[cfg(windows)] pub use X as X;\n\
         #[cfg(unix)] pub use high::High as X;",
    );
}

fn a_forbidden_owner_does_not_erase_a_cyclic_branch() {
    assert_cyclic_and_forbidden_alternatives_are_both_reported(
        "#[cfg(unix)] pub use high::High as X;\n\
         #[cfg(windows)] pub use X as X;",
    );
}

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
                 pub use flui_painting::styling::Color;\n",
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
            "pub use ::flui_painting::styling::Color;\n",
            "use ::flui_foundation::geometry::Size;\nuse crate::Color;\n"
        ),
        set(&[])
    );
}

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

#[test]
fn module_dag_contract() {
    crate::table_test::run_table(
        "module_dag_contract",
        &[
            (
                "a_use_of_a_higher_module_is_refused",
                a_use_of_a_higher_module_is_refused as fn(),
            ),
            (
                "the_wildcard_layer_leaves_its_members_unchecked_among_themselves",
                the_wildcard_layer_leaves_its_members_unchecked_among_themselves as fn(),
            ),
            (
                "conditional_root_reexports_do_not_hide_the_first_owner",
                conditional_root_reexports_do_not_hide_the_first_owner as fn(),
            ),
            (
                "conditional_root_reexports_do_not_hide_the_last_owner",
                conditional_root_reexports_do_not_hide_the_last_owner as fn(),
            ),
            (
                "conditional_external_reexports_do_not_hide_an_internal_owner",
                conditional_external_reexports_do_not_hide_an_internal_owner as fn(),
            ),
            (
                "conditional_internal_reexports_do_not_hide_an_external_owner",
                conditional_internal_reexports_do_not_hide_an_external_owner as fn(),
            ),
            (
                "conditional_relay_reexports_are_not_overwritten",
                conditional_relay_reexports_are_not_overwritten as fn(),
            ),
            (
                "conditional_reexports_with_the_same_owner_remain_attributable",
                conditional_reexports_with_the_same_owner_remain_attributable as fn(),
            ),
            (
                "a_cyclic_conditional_reexport_cannot_hide_behind_a_healthy_branch",
                a_cyclic_conditional_reexport_cannot_hide_behind_a_healthy_branch as fn(),
            ),
            (
                "test_only_relay_items_do_not_change_production_attribution",
                test_only_relay_items_do_not_change_production_attribution as fn(),
            ),
            (
                "inner_test_only_relay_modules_do_not_change_production_attribution",
                inner_test_only_relay_modules_do_not_change_production_attribution as fn(),
            ),
            (
                "a_feature_enabled_relay_item_is_still_production_code",
                a_feature_enabled_relay_item_is_still_production_code as fn(),
            ),
            (
                "distinct_allowed_root_owners_both_contribute_edges",
                distinct_allowed_root_owners_both_contribute_edges as fn(),
            ),
            (
                "reordered_distinct_allowed_root_owners_both_contribute_edges",
                reordered_distinct_allowed_root_owners_both_contribute_edges as fn(),
            ),
            (
                "distinct_allowed_relay_owners_both_contribute_edges",
                distinct_allowed_relay_owners_both_contribute_edges as fn(),
            ),
            (
                "nested_conditional_reexports_retain_the_high_owner",
                nested_conditional_reexports_retain_the_high_owner as fn(),
            ),
            (
                "reordered_nested_conditional_reexports_retain_the_high_owner",
                reordered_nested_conditional_reexports_retain_the_high_owner as fn(),
            ),
            (
                "a_cyclic_branch_does_not_erase_a_forbidden_owner",
                a_cyclic_branch_does_not_erase_a_forbidden_owner as fn(),
            ),
            (
                "a_forbidden_owner_does_not_erase_a_cyclic_branch",
                a_forbidden_owner_does_not_erase_a_cyclic_branch as fn(),
            ),
            (
                "a_path_through_a_root_reexport_counts_as_its_source_module",
                a_path_through_a_root_reexport_counts_as_its_source_module as fn(),
            ),
            (
                "a_path_through_a_transparent_module_counts_as_its_source",
                a_path_through_a_transparent_module_counts_as_its_source as fn(),
            ),
            (
                "a_plain_path_in_a_transparent_module_follows_the_relays_own_use",
                a_plain_path_in_a_transparent_module_follows_the_relays_own_use as fn(),
            ),
            (
                "super_paths_that_leave_the_top_level_module_are_edges",
                super_paths_that_leave_the_top_level_module_are_edges as fn(),
            ),
            (
                "a_macro_export_macro_belongs_to_the_module_that_defines_it",
                a_macro_export_macro_belongs_to_the_module_that_defines_it as fn(),
            ),
            (
                "a_leading_colon_alias_path_is_a_crate_path",
                a_leading_colon_alias_path_is_a_crate_path as fn(),
            ),
            (
                "a_top_level_module_file_under_inner_cfg_test_is_no_module",
                a_top_level_module_file_under_inner_cfg_test_is_no_module as fn(),
            ),
            (
                "cfg_any_test_or_feature_code_is_checked",
                cfg_any_test_or_feature_code_is_checked as fn(),
            ),
            (
                "an_unattributable_crate_path_is_reported",
                an_unattributable_crate_path_is_reported as fn(),
            ),
            (
                "a_stale_exception_is_reported",
                a_stale_exception_is_reported as fn(),
            ),
            (
                "an_exception_with_a_missing_adr_or_bad_date_is_reported",
                an_exception_with_a_missing_adr_or_bad_date_is_reported as fn(),
            ),
            (
                "a_malformed_declaration_is_an_error",
                a_malformed_declaration_is_an_error as fn(),
            ),
            (
                "a_missing_module_file_or_a_parse_error_stops_the_scan",
                a_missing_module_file_or_a_parse_error_stops_the_scan as fn(),
            ),
            (
                "a_planted_import_in_the_real_widgets_tree_is_refused",
                a_planted_import_in_the_real_widgets_tree_is_refused as fn(),
            ),
        ],
    );
}
