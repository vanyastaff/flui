//! The method table (ADR-0238 §1, R28a): one row per method, each named `group.verb`, sorted,
//! in exactly one tier, with at most 13 in the core set MCP lists by default.

use std::collections::BTreeSet;

use flui_protocol::method::{self, MAX_CORE};
use flui_protocol::version::BREAKING;
use flui_protocol::{
    METHODS, Method, MethodEffect, MethodInfo, ProtocolVersion, ServedBy, Tier,
    method::SemanticsRead,
};

#[test]
fn every_method_is_named_group_dot_verb_once_and_listed_in_order() {
    let lower_snake = |part: &str| {
        !part.is_empty()
            && part.starts_with(|c: char| c.is_ascii_lowercase())
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    for row in METHODS {
        let (group, verb) = row
            .name
            .split_once('.')
            .unwrap_or_else(|| panic!("`{}` is not `group.verb`", row.name));
        assert!(
            lower_snake(group) && lower_snake(verb),
            "`{}` is not `group.verb` in lower snake case",
            row.name
        );
        assert_eq!((row.group(), row.verb()), (group, verb));
        assert_eq!(row.tool_name(), format!("{group}_{verb}"));
    }
    let names: Vec<&str> = METHODS.iter().map(|row| row.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        names, sorted,
        "METHODS is sorted by wire name with no name twice, the order MCP lists tools in"
    );
    let tools: BTreeSet<String> = METHODS.iter().map(MethodInfo::tool_name).collect();
    assert_eq!(
        tools.len(),
        METHODS.len(),
        "no two methods share a tool name"
    );
}

#[test]
fn the_core_set_is_the_thirteen_tools_mcp_lists_by_default() {
    let core: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.tier == Tier::Core)
        .map(|row| row.name)
        .collect();
    assert!(
        core.len() <= MAX_CORE,
        "{} core methods: {core:?}",
        core.len()
    );
    assert_eq!(
        core,
        [
            "audit.screen",
            "capture.presented",
            "element.press_keys",
            "element.scroll_into_view",
            "element.tap",
            "element.type_text",
            "semantics.act",
            "semantics.locate",
            "semantics.read",
            "session.close",
            "session.launch",
            "wait.until",
            "windows.list",
        ],
    );
    // Each row has one tier by construction; every tier but the core is a `--caps` group a
    // client can name, so none may be empty of purpose here.
    for row in METHODS {
        assert!(
            Tier::ALL.contains(&row.tier),
            "{} has a known tier",
            row.name
        );
    }
}

#[test]
fn a_methods_row_is_what_its_type_declares() {
    let row = method::find("semantics.read").expect("semantics.read is a method");
    assert_eq!(row.name, SemanticsRead::NAME);
    assert_eq!(row.served_by, SemanticsRead::SERVED_BY);
    assert_eq!(row.effect, SemanticsRead::EFFECT);
    assert_eq!(row.tier, SemanticsRead::TIER);
    assert_eq!(
        (row.served_by, row.effect, row.tier),
        (ServedBy::Agent, MethodEffect::ReadOnly, Tier::Core)
    );
    assert!(method::find("semantics.write").is_none());
}

#[test]
fn only_session_close_is_destructive_and_reads_never_act() {
    let destructive: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.effect == MethodEffect::Destructive)
        .map(|row| row.name)
        .collect();
    assert_eq!(destructive, ["session.close"]);
    for row in METHODS {
        let reads = [
            "read",
            "locate",
            "list",
            "journal",
            "placement",
            "capabilities",
        ]
        .contains(&row.verb())
            || ["inspect", "capture", "audit", "wait"].contains(&row.group());
        if reads {
            assert_eq!(
                row.effect,
                MethodEffect::ReadOnly,
                "{} only reads",
                row.name
            );
        }
    }
}

#[test]
fn the_ops_protocol_0_5_renamed_are_methods_now() {
    let break_0_5 = BREAKING
        .iter()
        .find(|change| change.version == ProtocolVersion::new(0, 5))
        .expect("0.5 is listed in BREAKING");
    assert_eq!(break_0_5.adr, "ADR-0238");
    for (old, new) in break_0_5.renamed {
        assert!(
            method::find(new).is_some(),
            "`{old}` became `{new}`, a method"
        );
        assert!(method::find(old).is_none(), "`{old}` is no method");
    }
}
