//! The role-family fixtures (`examples/a11y_probe/roles.rs`) read back
//! through UI Automation: every role's control type and ARIA role, and each
//! family's patterns, states and properties as a screen reader queries them.
//!
//! The roles' expected values, the family fixture names and the adapter
//! version they were read from live in `uia_role_table`, whose guards run on
//! every host; the family expectations here are transcribed from the same
//! `accesskit_windows` ([`ADAPTER_CHECKED_AGAINST`]) pattern and property
//! providers.
//!
//! Every mismatch is reported, not just the first, so one run shows the
//! whole difference.

use std::path::Path;

use windows::Win32::UI::Accessibility::{
    Assertive, ExpandCollapseState_Collapsed, ExpandCollapseState_Expanded, IUIAutomationElement,
    IUIAutomationElement6, IUIAutomationExpandCollapsePattern, IUIAutomationRangeValuePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, IUIAutomationValuePattern,
    IUIAutomationWindowPattern, LiveSetting, Polite, ToggleState_Indeterminate, ToggleState_On,
    UIA_ButtonControlTypeId, UIA_CONTROLTYPE_ID, UIA_CheckBoxControlTypeId,
    UIA_ComboBoxControlTypeId, UIA_DataGridControlTypeId, UIA_DataItemControlTypeId,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_ExpandCollapsePatternId,
    UIA_GroupControlTypeId, UIA_HyperlinkControlTypeId, UIA_ImageControlTypeId,
    UIA_IsRangeValuePatternAvailablePropertyId, UIA_IsSelectionItemPatternAvailablePropertyId,
    UIA_IsTogglePatternAvailablePropertyId, UIA_LevelPropertyId, UIA_ListControlTypeId,
    UIA_ListItemControlTypeId, UIA_LiveSettingPropertyId, UIA_MenuBarControlTypeId,
    UIA_MenuControlTypeId, UIA_MenuItemControlTypeId, UIA_PaneControlTypeId,
    UIA_ProgressBarControlTypeId, UIA_RadioButtonControlTypeId, UIA_RangeValuePatternId,
    UIA_ScrollBarControlTypeId, UIA_SelectionItemPatternId, UIA_SeparatorControlTypeId,
    UIA_SliderControlTypeId, UIA_SpinnerControlTypeId, UIA_StatusBarControlTypeId,
    UIA_TabControlTypeId, UIA_TabItemControlTypeId, UIA_TableControlTypeId, UIA_TextControlTypeId,
    UIA_TogglePatternId, UIA_ToolBarControlTypeId, UIA_ToolTipControlTypeId, UIA_TreeControlTypeId,
    UIA_TreeItemControlTypeId, UIA_ValuePatternId, UIA_WindowControlTypeId, UIA_WindowPatternId,
};
use windows::core::Interface;

use super::super::uia::{self, Session, Start};
use super::super::uia_role_table::{
    ADAPTER_CHECKED_AGAINST, ControlType, FAMILY_FIXTURES, NAME_PROHIBITED, ROLES, ROLES_PAGE,
    role_row,
};

/// The fixture the window is recognised by once its tree is published.
const FIRST: &str = "button fixture";

/// How long the check runs at most: the window's appearance, then one walk.
const CHECK_DEADLINE: std::time::Duration = uia::APPEAR_WITHIN;

/// Runs the role-family check against the built probe and returns its exit
/// code: 0 pass, 1 fail, 2 cannot verify on this host.
pub(super) fn run(probe: &Path) -> anyhow::Result<u8> {
    let mut session = match Session::start_page(probe, CHECK_DEADLINE * 2, ROLES_PAGE) {
        Ok(session) => session,
        Err(Start::CannotVerify(why)) => {
            println!("CANNOT_VERIFY: {why}");
            return Ok(2);
        }
        Err(Start::Failed(error)) => return Err(error),
    };
    let Some(window) = session.window_with_button(FIRST)? else {
        return Ok(1);
    };
    let nodes = session.walk(&window);
    let mut failures = Vec::new();
    for &(role, control, aria_role) in ROLES {
        let name = format!("{role} fixture");
        let prohibited = NAME_PROHIBITED.contains(&role);
        match fixture(&nodes, &name, prohibited) {
            Ok(node) => {
                if let Err(error) = check_role(node, uia_control(control), aria_role) {
                    failures.push(format!("{name}: {error:#}"));
                }
            }
            Err(error) => failures.push(format!("{name}: {error:#}")),
        }
    }
    for &(name, role) in FAMILY_FIXTURES {
        // The node is held to its role's row as well as its own state: a
        // helper that assigned another role would otherwise pass on state alone.
        let outcome = family(name)
            .ok_or_else(|| anyhow::anyhow!("no expectation is written for it"))
            .and_then(|check| {
                let (control, aria_role) = role_row(role)
                    .ok_or_else(|| anyhow::anyhow!("its role {role:?} has no row"))?;
                let node = fixture(&nodes, name, NAME_PROHIBITED.contains(&role))?;
                check_role(node, uia_control(control), aria_role)?;
                check(node)
            });
        if let Err(error) = outcome {
            failures.push(format!("{name}: {error:#}"));
        }
    }
    drop(session);
    if failures.is_empty() {
        println!(
            "UIA role families: {} roles and {} family fixtures as expected \
             (table checked against accesskit_windows {ADAPTER_CHECKED_AGAINST})",
            ROLES.len(),
            FAMILY_FIXTURES.len()
        );
        return Ok(0);
    }
    for failure in &failures {
        println!("FAIL: {failure}");
    }
    uia::dump(&nodes);
    Ok(1)
}

/// The fixture node named `name`: by its UIA name, or by its description for
/// a role ARIA forbids a name (whose UIA name must then be empty).
fn fixture<'a>(
    nodes: &'a [uia::Node],
    name: &str,
    prohibited: bool,
) -> anyhow::Result<&'a IUIAutomationElement> {
    let mut matches = nodes.iter().filter(|node| {
        if prohibited {
            node.name.is_empty() && description(&node.element).is_ok_and(|text| text == name)
        } else {
            node.name == name
        }
    });
    let found = matches
        .next()
        .ok_or_else(|| anyhow::anyhow!("no UIA element carries it"))?;
    anyhow::ensure!(
        matches.next().is_none(),
        "more than one UIA element carries it"
    );
    Ok(&found.element)
}

fn check_role(
    element: &IUIAutomationElement,
    control: UIA_CONTROLTYPE_ID,
    aria_role: &str,
) -> anyhow::Result<()> {
    // SAFETY: live elements on the session's COM thread.
    let (actual_control, actual_role) =
        unsafe { (element.CurrentControlType()?, element.CurrentAriaRole()?) };
    anyhow::ensure!(
        actual_control == control,
        "control type {} where {} was expected",
        actual_control.0,
        control.0
    );
    anyhow::ensure!(
        actual_role == aria_role,
        "ARIA role {:?} where {aria_role:?} was expected",
        actual_role.to_string()
    );
    Ok(())
}

fn description(element: &IUIAutomationElement) -> anyhow::Result<String> {
    // SAFETY: a plain property read on the session's COM thread;
    // `FullDescription` is on the Windows 10 element interface.
    Ok(unsafe {
        element
            .cast::<IUIAutomationElement6>()?
            .CurrentFullDescription()?
    }
    .to_string())
}

fn pattern<T: Interface>(
    element: &IUIAutomationElement,
    id: windows::Win32::UI::Accessibility::UIA_PATTERN_ID,
) -> anyhow::Result<T> {
    // SAFETY: a live element on the session's COM thread.
    Ok(unsafe { element.GetCurrentPatternAs::<T>(id)? })
}

fn flag(
    element: &IUIAutomationElement,
    id: windows::Win32::UI::Accessibility::UIA_PROPERTY_ID,
) -> anyhow::Result<bool> {
    // SAFETY: a plain property read on the session's COM thread.
    let value = unsafe { element.GetCurrentPropertyValue(id)? };
    Ok(bool::try_from(&value)?)
}

fn int(
    element: &IUIAutomationElement,
    id: windows::Win32::UI::Accessibility::UIA_PROPERTY_ID,
) -> anyhow::Result<i32> {
    // SAFETY: a plain property read on the session's COM thread.
    let value = unsafe { element.GetCurrentPropertyValue(id)? };
    Ok(i32::try_from(&value)?)
}

type Check = fn(&IUIAutomationElement) -> anyhow::Result<()>;

/// What UI Automation must report for a family fixture, or `None` for a name
/// no expectation is written for.
fn family(name: &str) -> Option<Check> {
    Some(match name {
        "checked check box" | "switch on" | "pressed toggle button" => {
            |e| toggle(e, ToggleState_On.0)
        }
        "mixed check box" => |e| toggle(e, ToggleState_Indeterminate.0),
        "checked radio" | "selected option" => |e| selected(e, true),
        "unchecked menu radio" | "unselected tab" => |e| selected(e, false),
        "collapsed combo box" => |e| expanded(e, false),
        "expanded tree item" => |e| expanded(e, true),
        "progress at 30" => |e| read_only_range(e, 30.0),
        "splitter at 40" => |e| read_only_range(e, 40.0),
        "static separator" => no_range,
        "draft field" => |e| value(e, "draft", false),
        "read-only field" => |e| value(e, "fixed", true),
        "required field" => required,
        "placeholder field" => placeholder,
        "secret field" => password,
        "level three heading" => |e| {
            level(e, 3)?;
            // The adapter writes AccessKit's zero-based level here, one less
            // than `Level` (flui-semantics ARCHITECTURE.md, heading levels).
            aria_properties(e, "level=2")
        },
        "multilineTextInput fixture" => |e| aria_properties(e, "multiline=true"),
        "modal dialog" => modal,
        "disabled button" => disabled,
        "status fixture" | "log fixture" => |e| live(e, Polite),
        "alert fixture" => |e| live(e, Assertive),
        _ => return None,
    })
}

/// The UI Automation id of a control type the table names.
fn uia_control(control: ControlType) -> UIA_CONTROLTYPE_ID {
    match control {
        ControlType::Button => UIA_ButtonControlTypeId,
        ControlType::CheckBox => UIA_CheckBoxControlTypeId,
        ControlType::ComboBox => UIA_ComboBoxControlTypeId,
        ControlType::DataGrid => UIA_DataGridControlTypeId,
        ControlType::DataItem => UIA_DataItemControlTypeId,
        ControlType::Document => UIA_DocumentControlTypeId,
        ControlType::Edit => UIA_EditControlTypeId,
        ControlType::Group => UIA_GroupControlTypeId,
        ControlType::Hyperlink => UIA_HyperlinkControlTypeId,
        ControlType::Image => UIA_ImageControlTypeId,
        ControlType::List => UIA_ListControlTypeId,
        ControlType::ListItem => UIA_ListItemControlTypeId,
        ControlType::Menu => UIA_MenuControlTypeId,
        ControlType::MenuBar => UIA_MenuBarControlTypeId,
        ControlType::MenuItem => UIA_MenuItemControlTypeId,
        ControlType::Pane => UIA_PaneControlTypeId,
        ControlType::ProgressBar => UIA_ProgressBarControlTypeId,
        ControlType::RadioButton => UIA_RadioButtonControlTypeId,
        ControlType::ScrollBar => UIA_ScrollBarControlTypeId,
        ControlType::Separator => UIA_SeparatorControlTypeId,
        ControlType::Slider => UIA_SliderControlTypeId,
        ControlType::Spinner => UIA_SpinnerControlTypeId,
        ControlType::StatusBar => UIA_StatusBarControlTypeId,
        ControlType::Tab => UIA_TabControlTypeId,
        ControlType::TabItem => UIA_TabItemControlTypeId,
        ControlType::Table => UIA_TableControlTypeId,
        ControlType::Text => UIA_TextControlTypeId,
        ControlType::ToolBar => UIA_ToolBarControlTypeId,
        ControlType::ToolTip => UIA_ToolTipControlTypeId,
        ControlType::Tree => UIA_TreeControlTypeId,
        ControlType::TreeItem => UIA_TreeItemControlTypeId,
        ControlType::Window => UIA_WindowControlTypeId,
    }
}

/// Toggled: the Toggle pattern in `state`, and no SelectionItem.
fn toggle(element: &IUIAutomationElement, state: i32) -> anyhow::Result<()> {
    let toggle: IUIAutomationTogglePattern = pattern(element, UIA_TogglePatternId)?;
    // SAFETY: a live pattern on the session's COM thread.
    let actual = unsafe { toggle.CurrentToggleState()? }.0;
    anyhow::ensure!(
        actual == state,
        "toggle state {actual} where {state} was expected"
    );
    anyhow::ensure!(
        !flag(element, UIA_IsSelectionItemPatternAvailablePropertyId)?,
        "a toggle also offers the SelectionItem pattern"
    );
    Ok(())
}

/// Selectable: the SelectionItem pattern, selected or not, and no Toggle.
fn selected(element: &IUIAutomationElement, expected: bool) -> anyhow::Result<()> {
    let item: IUIAutomationSelectionItemPattern = pattern(element, UIA_SelectionItemPatternId)?;
    // SAFETY: a live pattern on the session's COM thread.
    let actual = unsafe { item.CurrentIsSelected()? }.as_bool();
    anyhow::ensure!(actual == expected, "selected is {actual}, not {expected}");
    anyhow::ensure!(
        !flag(element, UIA_IsTogglePatternAvailablePropertyId)?,
        "a selectable item also offers the Toggle pattern"
    );
    Ok(())
}

fn expanded(element: &IUIAutomationElement, expected: bool) -> anyhow::Result<()> {
    let disclosure: IUIAutomationExpandCollapsePattern =
        pattern(element, UIA_ExpandCollapsePatternId)?;
    // SAFETY: a live pattern on the session's COM thread.
    let actual = unsafe { disclosure.CurrentExpandCollapseState()? };
    let wanted = if expected {
        ExpandCollapseState_Expanded
    } else {
        ExpandCollapseState_Collapsed
    };
    anyhow::ensure!(
        actual == wanted,
        "expand state {} where {} was expected",
        actual.0,
        wanted.0
    );
    Ok(())
}

/// A range nothing can write: RangeValue at `value`, read-only.
fn read_only_range(element: &IUIAutomationElement, value: f64) -> anyhow::Result<()> {
    let range: IUIAutomationRangeValuePattern = pattern(element, UIA_RangeValuePatternId)?;
    // SAFETY: a live pattern on the session's COM thread.
    let (actual, read_only) = unsafe { (range.CurrentValue()?, range.CurrentIsReadOnly()?) };
    anyhow::ensure!(
        actual.to_bits() == value.to_bits(),
        "range value {actual} where {value} was expected"
    );
    anyhow::ensure!(
        read_only.as_bool(),
        "a range nothing can write is not read-only"
    );
    Ok(())
}

/// A static separator: its declared range is left out of publication.
fn no_range(element: &IUIAutomationElement) -> anyhow::Result<()> {
    anyhow::ensure!(
        !flag(element, UIA_IsRangeValuePatternAvailablePropertyId)?,
        "a static separator offers RangeValue"
    );
    Ok(())
}

fn value(element: &IUIAutomationElement, expected: &str, read_only: bool) -> anyhow::Result<()> {
    let value: IUIAutomationValuePattern = pattern(element, UIA_ValuePatternId)?;
    // SAFETY: a live pattern on the session's COM thread.
    let (actual, actual_read_only) = unsafe {
        (
            value.CurrentValue()?.to_string(),
            value.CurrentIsReadOnly()?.as_bool(),
        )
    };
    anyhow::ensure!(
        actual == expected,
        "value {actual:?} where {expected:?} was expected"
    );
    anyhow::ensure!(
        actual_read_only == read_only,
        "value read-only is {actual_read_only}, not {read_only}"
    );
    Ok(())
}

fn required(element: &IUIAutomationElement) -> anyhow::Result<()> {
    // SAFETY: a plain property read on the session's COM thread.
    let required = unsafe { element.CurrentIsRequiredForForm()? }.as_bool();
    anyhow::ensure!(required, "not required for its form");
    Ok(())
}

fn placeholder(element: &IUIAutomationElement) -> anyhow::Result<()> {
    // SAFETY: a plain property read on the session's COM thread.
    let help = unsafe { element.CurrentHelpText()? }.to_string();
    anyhow::ensure!(
        help == "Type here",
        "help text {help:?} where the placeholder was expected"
    );
    Ok(())
}

fn password(element: &IUIAutomationElement) -> anyhow::Result<()> {
    // SAFETY: a plain property read on the session's COM thread.
    let password = unsafe { element.CurrentIsPassword()? }.as_bool();
    anyhow::ensure!(password, "not a password field");
    Ok(())
}

fn aria_properties(element: &IUIAutomationElement, expected: &str) -> anyhow::Result<()> {
    // SAFETY: a plain property read on the session's COM thread.
    let actual = unsafe { element.CurrentAriaProperties()? }.to_string();
    anyhow::ensure!(
        actual == expected,
        "ARIA properties {actual:?} where {expected:?} was expected"
    );
    Ok(())
}

fn level(element: &IUIAutomationElement, expected: i32) -> anyhow::Result<()> {
    let actual = int(element, UIA_LevelPropertyId)?;
    anyhow::ensure!(
        actual == expected,
        "level {actual} where {expected} was expected"
    );
    Ok(())
}

fn modal(element: &IUIAutomationElement) -> anyhow::Result<()> {
    let window: IUIAutomationWindowPattern = pattern(element, UIA_WindowPatternId)?;
    // SAFETY: a live pattern on the session's COM thread.
    let modal = unsafe { window.CurrentIsModal()? }.as_bool();
    anyhow::ensure!(modal, "a modal dialog's Window pattern is not modal");
    Ok(())
}

fn disabled(element: &IUIAutomationElement) -> anyhow::Result<()> {
    // SAFETY: a plain property read on the session's COM thread.
    let enabled = unsafe { element.CurrentIsEnabled()? }.as_bool();
    anyhow::ensure!(!enabled, "enabled");
    Ok(())
}

fn live(element: &IUIAutomationElement, expected: LiveSetting) -> anyhow::Result<()> {
    let actual = int(element, UIA_LiveSettingPropertyId)?;
    anyhow::ensure!(
        actual == expected.0,
        "live setting {actual} where {} was expected",
        expected.0
    );
    Ok(())
}
