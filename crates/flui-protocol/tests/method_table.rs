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

#[test]
fn every_method_the_agent_serves_is_a_window_method_but_the_apps_own() {
    macro_rules! names {
        ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {
            [$($name),+]
        };
    }
    let window_methods: &[&str] = &flui_protocol::for_each_window_method!(names);
    let apps_own = [
        "events.subscribe",
        "events.unsubscribe",
        "record.start",
        "record.stop",
        "windows.list",
    ];
    let expected: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.served_by == ServedBy::Agent && !apps_own.contains(&row.name))
        .map(|row| row.name)
        .collect();
    assert_eq!(window_methods, expected.as_slice());
}

#[test]
fn a_window_methods_parameters_name_the_window_they_address() {
    use std::time::Duration;

    use flui_protocol::{
        ActParams, ActionName, Addressed, Condition, ElementId, ElementRef, LocateParams, Locator,
        LogicalPoint, Target, WaitParams, WindowId,
    };

    let window = WindowId::from_u64(3).expect("non-zero");
    let element = ElementRef::new(window, ElementId::from_u64(9).expect("non-zero"));
    let act = |target| ActParams::new(target, ActionName::Invoke);
    let mut named = Locator::default();
    named.window = Some(window);
    let table: &[(&str, &dyn Addressed, Option<WindowId>)] = &[
        ("an element", &act(Target::Element(element)), Some(window)),
        (
            "a point",
            &act(Target::Point {
                window,
                at: LogicalPoint::new(1.0, 2.0),
            }),
            Some(window),
        ),
        (
            "a locator naming none",
            &act(Target::Locator(Locator::default())),
            None,
        ),
        (
            "a locator naming one",
            &LocateParams::new(named),
            Some(window),
        ),
        (
            "a wait",
            &WaitParams::new(window, Condition::Settled),
            Some(window),
        ),
    ];
    for (case, params, expected) in table {
        assert_eq!(params.window(), *expected, "{case}");
    }
    assert_eq!(
        WaitParams::new(window, Condition::Settled)
            .timeout_ms(250)
            .holds_for(),
        Duration::from_millis(250),
        "a wait holds for its timeout"
    );
    assert_eq!(
        LocateParams::new(Locator::default()).holds_for(),
        Duration::ZERO,
        "a locate answers at the drain"
    );
}
