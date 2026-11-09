//! The parity table, declared once in `platform-parity.toml` and rendered into
//! `docs/platform-parity.md` (decision 8). Hand-written source; the test
//! [`the_summary_counts_the_rows`] and the cross-checks below keep the source
//! honest, and a follow-up renders `docs/platform-parity.md` from it.

use std::collections::BTreeMap;

use serde::Deserialize;

#[cfg(test)]
use crate::host::{Absent, Host as NativeHost, position};

/// The `platform-parity.toml` at the repository root.
pub const SOURCE: &str = "platform-parity.toml";

/// A cell's mark: `yes`, `partial`, `missing`, `na` or `unaudited`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Implemented and passed an executed run on that environment.
    Yes,
    /// Implemented but never run there, or partial.
    Partial,
    /// Missing: no code path, a stub, a mock, or a default that pretends.
    Missing,
    /// Not applicable: the environment has no such concept.
    NotApplicable,
    /// Not audited yet.
    Unaudited,
}

impl Mark {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "yes" => Some(Self::Yes),
            "partial" => Some(Self::Partial),
            "missing" => Some(Self::Missing),
            "na" => Some(Self::NotApplicable),
            "unaudited" => Some(Self::Unaudited),
            _ => None,
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Self::Yes => "✅",
            Self::Partial => "🟡",
            Self::Missing => "❌",
            Self::NotApplicable => "—",
            Self::Unaudited => "❔",
        }
    }
}

/// The parsed `platform-parity.toml`.
#[derive(Debug, Deserialize)]
pub struct Parity {
    legend: BTreeMap<String, String>,
    #[serde(rename = "host", default)]
    hosts: Vec<Host>,
    #[serde(rename = "section", default)]
    sections: Vec<Section>,
}

/// One environment column of the table.
#[derive(Debug, Deserialize)]
pub struct Host {
    /// The short id a row's status map keys on (`windows`, `wayland`, …).
    pub id: String,
    /// The environment's name, the column header (`Windows`, `Wayland`, …).
    pub environment: String,
    /// The host that runs there (`Win32 (native)`, `Wayland (native)`, …).
    pub runner: String,
}

/// A group of rows (`Core`, `System services`, …).
#[derive(Debug, Deserialize)]
pub struct Section {
    /// The section's short id.
    pub id: String,
    /// The section's name, the row group header.
    pub name: String,
    /// The section's rows.
    #[serde(rename = "row", default)]
    pub rows: Vec<Row>,
}

/// One capability row.
#[derive(Debug, Deserialize)]
pub struct Row {
    /// The row's short id.
    pub id: String,
    /// The capability the row describes (`Window`, `Displays`, …).
    pub capability: String,
    /// The host position this row maps to, where one exists: the cross-check
    /// against `Absent<position::X>` keys on it.
    #[serde(default)]
    pub position: Option<String>,
    /// The prose note a cell's claim rests on.
    #[serde(default)]
    pub note: String,
    /// The mark per host id.
    pub status: BTreeMap<String, String>,
    /// The executed runs a cell's claim cites.
    #[serde(default)]
    pub evidence: Vec<Evidence>,
}

/// One executed run that a cell's claim rests on.
#[derive(Debug, Deserialize)]
pub struct Evidence {
    /// The scenario's test name, checked to exist.
    pub scenario: String,
    /// The run's date.
    #[serde(default)]
    pub date: String,
    /// The device or host the run executed on.
    #[serde(default)]
    pub device: String,
}

impl Parity {
    /// Parse the hand-written source.
    pub fn parse(source: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(source)
    }

    /// The mark a row claims on a host, or an error naming the row and the
    /// unknown mark key.
    pub fn mark(&self, row: &Row, host: &Host) -> Result<Mark, String> {
        let raw = row
            .status
            .get(host.id.as_str())
            .ok_or_else(|| format!("row `{}` names no status for host `{}`", row.id, host.id))?;
        Mark::parse(raw).ok_or_else(|| format!("row `{}` names an unknown mark `{raw}`", row.id))
    }

    /// The markdown `docs/platform-parity.md` is kept equal to.
    pub fn render(&self) -> String {
        use std::fmt::Write as _;

        let mut out = String::new();

        out.push_str("# Platform parity\n\n> [!NOTE]\n> **Last reviewed:** —\n\n");
        out.push_str("Rendered from `platform-parity.toml`; do not edit by hand.\n\n");

        out.push_str("## Legend\n\n| Mark | Meaning |\n|:-:|:--|\n");
        for (key, meaning) in &self.legend {
            let glyph = Mark::parse(key).map(Mark::glyph).unwrap_or_default();
            writeln!(out, "| {glyph} | {meaning} |").expect("writing to a String never fails");
        }
        out.push('\n');

        out.push_str("## Summary\n\n| Environment | Host | ✅ | 🟡 | ❌ | — | ❔ |\n");
        out.push_str("|:--|:--|--:|--:|--:|--:|--:|\n");
        for host in &self.hosts {
            let counts = self.counts(host);
            writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} |",
                host.environment,
                host.runner,
                counts[0],
                counts[1],
                counts[2],
                counts[3],
                counts[4],
            )
            .expect("writing to a String never fails");
        }
        out.push('\n');

        out.push_str("## Parity\n\n| Capability |");
        for host in &self.hosts {
            write!(out, " {} |", host.environment).expect("writing to a String never fails");
        }
        out.push_str("\n|:--|");
        for _ in &self.hosts {
            out.push_str(":-:|");
        }
        out.push('\n');
        for section in &self.sections {
            write!(out, "| **{}** |", section.name).expect("writing to a String never fails");
            for _ in &self.hosts {
                out.push_str(" |");
            }
            out.push('\n');
            for row in &section.rows {
                write!(out, "| {} |", row.capability).expect("writing to a String never fails");
                for host in &self.hosts {
                    let glyph = self.mark(row, host).map_or("�?", Mark::glyph);
                    write!(out, " {glyph} |").expect("writing to a String never fails");
                }
                out.push('\n');
            }
        }
        out.push('\n');

        for section in &self.sections {
            for row in &section.rows {
                writeln!(out, "### {} — {}\n", section.name, row.capability)
                    .expect("writing to a String never fails");
                writeln!(out, "{}\n", row.note).expect("writing to a String never fails");
                for evidence in &row.evidence {
                    let when = if evidence.date.is_empty() {
                        "not run".to_owned()
                    } else if evidence.device.is_empty() {
                        format!("run {}", evidence.date)
                    } else {
                        format!("run {} on {}", evidence.date, evidence.device)
                    };
                    writeln!(out, "- `{}`, {when}.", evidence.scenario)
                        .expect("writing to a String never fails");
                }
                out.push('\n');
            }
        }

        out
    }

    fn counts(&self, host: &Host) -> [usize; 5] {
        let mut counts = [0; 5];
        for section in &self.sections {
            for row in &section.rows {
                let idx = match self.mark(row, host) {
                    Ok(Mark::Yes) => 0,
                    Ok(Mark::Partial) => 1,
                    Ok(Mark::Missing) => 2,
                    Ok(Mark::NotApplicable) => 3,
                    Ok(Mark::Unaudited) => 4,
                    Err(_) => continue,
                };
                counts[idx] += 1;
            }
        }
        counts
    }
}

/// Rows whose `position` is named by `absent` but whose mark on `host` claims
/// support: the violation that a host with `Absent<position::X>` must never
/// publish `yes` or `partial`.
pub fn absent_violations(parity: &Parity, host: &Host, absent: &[&str]) -> Vec<String> {
    let mut violations = Vec::new();
    for section in &parity.sections {
        for row in &section.rows {
            let Some(position) = row.position.as_deref() else {
                continue;
            };
            if !absent.contains(&position) {
                continue;
            }
            if let Ok(Mark::Yes | Mark::Partial) = parity.mark(row, host) {
                violations.push(format!(
                    "row `{}` (position `{position}`) claims support on `{}`, whose host is `Absent<{position}>`",
                    row.id, host.id
                ));
            }
        }
    }
    violations
}

/// Scenarios a row cites as evidence but which are not in `known`.
pub fn unknown_scenarios<'a>(parity: &'a Parity, known: &[&str]) -> Vec<&'a str> {
    let mut unknown = Vec::new();
    for section in &parity.sections {
        for row in &section.rows {
            for evidence in &row.evidence {
                if !known.contains(&evidence.scenario.as_str()) {
                    unknown.push(evidence.scenario.as_str());
                }
            }
        }
    }
    unknown.dedup();
    unknown
}

/// The host positions `H` fills with [`Absent`]: the parity rows that name one
/// of these must claim `missing` or `na` on that host, never `yes` or `partial`.
#[cfg(test)]
fn absent_positions<H: NativeHost>() -> Vec<&'static str> {
    use std::any::TypeId;

    fn is_absent<A: 'static, B: 'static>() -> bool {
        TypeId::of::<A>() == TypeId::of::<B>()
    }

    let mut absent = Vec::new();
    if is_absent::<H::Window, Absent<position::Window>>() {
        absent.push("window");
    }
    if is_absent::<H::Displays, Absent<position::Displays>>() {
        absent.push("displays");
    }
    if is_absent::<H::Input, Absent<position::Input>>() {
        absent.push("input");
    }
    if is_absent::<H::Text, Absent<position::Text>>() {
        absent.push("text");
    }
    if is_absent::<H::Accessibility, Absent<position::Accessibility>>() {
        absent.push("accessibility");
    }
    if is_absent::<H::Clipboard, Absent<position::Clipboard>>() {
        absent.push("clipboard");
    }
    if is_absent::<H::PrimarySelection, Absent<position::PrimarySelection>>() {
        absent.push("primary-selection");
    }
    if is_absent::<H::Drag, Absent<position::Drag>>() {
        absent.push("drag");
    }
    if is_absent::<H::Haptics, Absent<position::Haptics>>() {
        absent.push("haptics");
    }
    if is_absent::<H::Preferences, Absent<position::Preferences>>() {
        absent.push("preferences");
    }
    if is_absent::<H::Lifecycle, Absent<position::Lifecycle>>() {
        absent.push("lifecycle");
    }
    if is_absent::<H::Chrome, Absent<position::Chrome>>() {
        absent.push("chrome");
    }
    if is_absent::<H::Storage, Absent<position::Storage>>() {
        absent.push("storage");
    }
    absent
}

/// Cross-check one host's table against its parity column: every row naming an
/// absent position must claim `missing` or `na`.
#[cfg(test)]
fn check_host<H: NativeHost>(column: &str, parity: &Parity, violations: &mut Vec<String>) {
    let host = parity
        .hosts
        .iter()
        .find(|host| host.id == column)
        .expect("the parity table names every host");
    violations.extend(absent_violations(parity, host, &absent_positions::<H>()));
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = include_str!("../../../../platform-parity.toml");

    fn repo_root() -> std::path::PathBuf {
        let mut dir = std::env::current_dir().expect("a current directory");
        loop {
            if dir.join("platform-parity.toml").exists() {
                return dir;
            }
            assert!(
                dir.pop(),
                "no platform-parity.toml found above the current directory"
            );
        }
    }

    fn parity() -> Parity {
        Parity::parse(ROOT).expect("the root file parses")
    }

    /// A small source that exercises the `position` and `evidence` cross-checks
    /// against the same types as the real table.
    const SYNTHETIC: &str = r#"
[legend]
yes = "Implemented"
partial = "Partial"
missing = "Missing"
na = "Not applicable"
unaudited = "Not audited"

[[host]]
id = "windows"
environment = "Windows"
runner = "Win32"

[[host]]
id = "headless"
environment = "Headless"
runner = "test double"

[[section]]
id = "core"
name = "Core"

[[section.row]]
id = "window"
capability = "Window"
position = "window"
note = ""
status = { windows = "yes", headless = "yes" }
evidence = [{ scenario = "window_conformance", date = "2026-10-28", device = "Windows 11" }]

[[section.row]]
id = "launch"
capability = "Launch"
position = "launch"
note = ""
status = { windows = "yes", headless = "na" }
evidence = []
"#;

    #[test]
    fn parses_and_renders_every_row_and_host() {
        let parity = parity();
        let rendered = parity.render();

        for host in &parity.hosts {
            assert!(
                rendered.contains(&format!(" {} |", host.environment)),
                "no {host:?} column"
            );
        }
        for section in &parity.sections {
            for row in &section.rows {
                assert!(
                    rendered.contains(&row.capability),
                    "row `{}` missing",
                    row.id
                );
            }
        }
    }

    #[test]
    fn the_summary_counts_every_row_once() {
        let parity = parity();
        for host in &parity.hosts {
            let counts = parity.counts(host);
            let total = counts.iter().sum::<usize>();
            let rows = parity.sections.iter().map(|s| s.rows.len()).sum::<usize>();
            assert_eq!(total, rows, "{} counts every row exactly once", host.id);
        }
    }

    #[test]
    fn the_checked_in_parity_agrees() {
        let parity = parity();
        let rendered = parity.render();
        if std::env::var("FLUI_WRITE_PARITY").is_ok() {
            let root = repo_root();
            std::fs::write(root.join("docs/platform-parity.md"), rendered)
                .expect("write docs/platform-parity.md");
            return;
        }
        let committed = std::fs::read_to_string(repo_root().join("docs/platform-parity.md"))
            .expect("docs/platform-parity.md is readable");
        assert_eq!(
            rendered, committed,
            "docs/platform-parity.md drifted from platform-parity.toml; run with FLUI_WRITE_PARITY=1 to restate it"
        );
    }

    #[test]
    fn absent_positions_never_claim_support() {
        let parity = parity();
        let mut violations = Vec::new();
        check_host::<crate::hosts::headless::Headless>("headless", &parity, &mut violations);
        #[cfg(windows)]
        check_host::<crate::hosts::windows::Windows>("windows", &parity, &mut violations);
        #[cfg(target_os = "macos")]
        check_host::<crate::hosts::apple::Apple>("macos", &parity, &mut violations);
        #[cfg(target_os = "ios")]
        check_host::<crate::hosts::apple::Apple>("ios", &parity, &mut violations);
        #[cfg(target_arch = "wasm32")]
        check_host::<crate::hosts::web::Web>("web", &parity, &mut violations);
        #[cfg(target_os = "android")]
        check_host::<crate::hosts::android::Android>("android", &parity, &mut violations);
        #[cfg(wayland_host)]
        check_host::<crate::hosts::linux::Wayland>("wayland", &parity, &mut violations);
        #[cfg(x11_host)]
        check_host::<crate::hosts::linux::X11>("x11", &parity, &mut violations);
        assert!(violations.is_empty(), "{violations:#?}");
    }

    #[test]
    fn cited_scenarios_are_known_names() {
        let parity = Parity::parse(SYNTHETIC).expect("the synthetic source parses");
        assert_eq!(
            unknown_scenarios(&parity, &["window_conformance"]),
            Vec::<&str>::new()
        );
        assert_eq!(
            unknown_scenarios(&parity, &["elsewhere"]),
            vec!["window_conformance"]
        );
    }
}
