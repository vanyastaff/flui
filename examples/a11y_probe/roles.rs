//! The role-family fixtures: one node for every role of the semantics
//! vocabulary, and nodes declaring each family's states and properties, for
//! `cargo xtask device windows-a11y` to read back through UI Automation.
//!
//! Every fixture is a bare `Semantics` node with a fixed name, so the check
//! can find it and hold UI Automation's control type, ARIA role, patterns and
//! their states to what the AccessKit adapter is expected to report. The
//! names are the contract with the check: renaming one here fails it.

use flui::prelude::*;
use flui::widgets::{HeadingLevel, NumericRange, Toggled, Wrap};

/// The variable that selects this page instead of the counter.
pub(super) const FIXTURE_ENV: &str = "FLUI_PROBE_FIXTURE";
/// [`FIXTURE_ENV`]'s value for this page.
pub(super) const ROLES: &str = "roles";

/// Every role, then every family's states, in one wrapped run of small boxes.
#[derive(Clone, StatelessView)]
pub(super) struct RoleFixtures;

impl StatelessView for RoleFixtures {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut fixtures: Vec<BoxedView> = SemanticsRole::ALL
            .iter()
            .copied()
            .filter(|role| *role != SemanticsRole::None)
            .map(|role| {
                fixture(
                    format!("{} fixture", role.name()),
                    Semantics::new().role(role),
                )
            })
            .collect();
        fixtures.extend(states());
        Wrap::new(fixtures).spacing(2.0).run_spacing(2.0)
    }
}

/// A node of its own, named `label`, that declares what `semantics` does.
fn fixture(label: impl Into<String>, semantics: Semantics) -> BoxedView {
    semantics
        .container(true)
        .label(label.into())
        .child(SizedBox::new(8.0, 8.0))
        .boxed()
}

fn range(value: f64, min: f64, max: f64, step: f64) -> NumericRange {
    NumericRange::new(value, min, max, step).expect("BUG: fixture ranges are finite and ordered")
}

/// The states and properties each family carries, one node per value.
fn states() -> Vec<BoxedView> {
    vec![
        // Toggles: the Toggle pattern, or SelectionItem for a radio.
        fixture("checked check box", Semantics::new().checkbox(Toggled::On)),
        fixture("mixed check box", Semantics::new().checkbox(Toggled::Mixed)),
        fixture("switch on", Semantics::new().switch(true)),
        fixture(
            "pressed toggle button",
            Semantics::new()
                .role(SemanticsRole::Button)
                .toggled(Toggled::On),
        ),
        fixture("checked radio", Semantics::new().radio(true)),
        fixture(
            "unchecked menu radio",
            Semantics::new()
                .role(SemanticsRole::MenuItemRadio)
                .toggled(Toggled::Off),
        ),
        // Selection: SelectionItem's selected state.
        fixture(
            "selected option",
            Semantics::new()
                .role(SemanticsRole::ListBoxOption)
                .selected(true),
        ),
        fixture(
            "unselected tab",
            Semantics::new().role(SemanticsRole::Tab).selected(false),
        ),
        // Disclosure: ExpandCollapse's state.
        fixture(
            "collapsed combo box",
            Semantics::new()
                .role(SemanticsRole::ComboBox)
                .expanded(false),
        ),
        fixture(
            "expanded tree item",
            Semantics::new()
                .role(SemanticsRole::TreeItem)
                .expanded(true),
        ),
        // Ranges: RangeValue, read-only where nothing can write it.
        fixture(
            "progress at 30",
            Semantics::new()
                .role(SemanticsRole::ProgressBar)
                .numeric_range(range(30.0, 0.0, 100.0, 1.0)),
        ),
        fixture(
            "splitter at 40",
            Semantics::new()
                .role(SemanticsRole::Separator)
                .focusable(true)
                .numeric_range(range(40.0, 0.0, 100.0, 10.0)),
        ),
        // A static separator carries no range: publication leaves it out.
        fixture(
            "static separator",
            Semantics::new()
                .role(SemanticsRole::Separator)
                .numeric_range(range(40.0, 0.0, 100.0, 10.0)),
        ),
        // Text input: Value, its read-only state, and the form properties.
        fixture(
            "draft field",
            Semantics::new()
                .role(SemanticsRole::TextInput)
                .value("draft"),
        ),
        fixture(
            "read-only field",
            Semantics::new()
                .role(SemanticsRole::TextInput)
                .value("fixed")
                .read_only(true),
        ),
        fixture(
            "required field",
            Semantics::new()
                .role(SemanticsRole::TextInput)
                .required(true),
        ),
        fixture(
            "placeholder field",
            Semantics::new()
                .role(SemanticsRole::TextInput)
                .placeholder("Type here"),
        ),
        fixture(
            "secret field",
            Semantics::new().role(SemanticsRole::PasswordInput),
        ),
        // Structure: an outline level.
        fixture(
            "level three heading",
            Semantics::new()
                .role(SemanticsRole::Heading)
                .heading_level(HeadingLevel::H3),
        ),
        // Windows: a modal dialog's Window pattern.
        fixture(
            "modal dialog",
            Semantics::new().role(SemanticsRole::Dialog).modal(true),
        ),
        // Any role: a disabled control.
        fixture(
            "disabled button",
            Semantics::new().role(SemanticsRole::Button).enabled(false),
        ),
    ]
}
