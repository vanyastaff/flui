//! The parity table, declared once in `platform-parity.toml` and rendered into
//! `docs/platform-parity.md` (decision 8). Hand-written source; the test
//! [`the_summary_counts_the_rows`] and the cross-checks below keep the source
//! honest, and a follow-up renders `docs/platform-parity.md` from it.

use std::collections::BTreeMap;

use serde::Deserialize;

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

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = include_str!("../../../../platform-parity.toml");

    fn parity() -> Parity {
        Parity::parse(ROOT).expect("the root file parses")
    }

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
    fn the_summary_counts_the_rows() {
        let parity = parity();
        let host = parity
            .hosts
            .iter()
            .find(|host| host.id == "headless")
            .expect("a headless host");
        // Core rows `window`/`displays`/`clipboard` are `yes`, the services
        // `launch` row is `na` for the headless host.
        let counts = parity.counts(host);
        assert_eq!(counts[0], 3, "three `yes` core rows");
        assert_eq!(counts[3], 1, "one `na` services row");
        assert_eq!(counts[1] + counts[2] + counts[4], 0);
    }

    #[test]
    fn absent_positions_never_claim_support() {
        let parity = parity();
        let host = parity
            .hosts
            .iter()
            .find(|host| host.id == "headless")
            .expect("a headless host");
        // `launch` is `na` for the headless host, so an absent `launch` position
        // is not a violation.
        assert_eq!(
            absent_violations(&parity, host, &["launch"]),
            Vec::<String>::new(),
            "`na` never violates"
        );
        // `window` is `yes` for the headless host, so an absent `window` position
        // is a violation.
        assert_ne!(
            absent_violations(&parity, host, &["window"]),
            Vec::<String>::new(),
            "`yes` on an absent position violates"
        );
    }

    #[test]
    fn cited_scenarios_are_known_names() {
        let parity = parity();
        let known = ["window_conformance", "displays", "clipboard", "portal"];
        assert_eq!(unknown_scenarios(&parity, &known), Vec::<&str>::new());
    }
}
