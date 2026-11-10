//! `docs/platform-parity.md`, rendered from `crates/flui-native/platform-parity.toml`.
//!
//! The source holds every cell, note and paragraph of the document; the
//! summary, the scores and the quality bar's tally are computed from it. The
//! test `parity_document_matches_its_source` holds the committed document to
//! the render, and `FLUI_OVERWRITE=1` restates it (ADR-0303 §2). A host that
//! fills a position with `Absent<position::X>` claims no support in a row of
//! that position: `absent_positions_claim_no_support` checks each host this
//! target builds.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Deserialize;

/// The source, beside this crate's manifest.
const SOURCE: &str = include_str!("../../platform-parity.toml");

/// The committed render.
const DOCUMENT: &str = include_str!("../../../../docs/platform-parity.md");

/// The parsed `platform-parity.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parity {
    /// The title, the note and the paragraphs above the legend.
    preamble: String,
    /// The paragraphs between the overall score and the summary.
    rules: String,
    /// The paragraph under the summary's heading.
    summary: String,
    /// The meaning of each mark, in the legend's order.
    legend: Vec<LegendEntry>,
    /// The environment columns, in the table's order.
    host: Vec<Host>,
    /// The row groups (`Core`, `System services`, `Distribution`).
    group: Vec<Group>,
    /// The qualities every environment must have at once.
    quality: Quality,
    /// The capability packages that do not decide readiness.
    capabilities: Capabilities,
}

/// A cell's mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mark {
    /// Implemented and passed an executed run on that environment.
    Yes,
    /// Implemented but never run there, or partial.
    Partial,
    /// Missing: no code path, a stub, a mock, or a default that pretends.
    Missing,
    /// Not applicable: the environment has no such concept.
    Na,
    /// Not audited yet.
    Unaudited,
}

impl Mark {
    const ALL: [Self; 5] = [
        Self::Yes,
        Self::Partial,
        Self::Missing,
        Self::Na,
        Self::Unaudited,
    ];

    fn glyph(self) -> &'static str {
        match self {
            Self::Yes => "✅",
            Self::Partial => "🟡",
            Self::Missing => "❌",
            Self::Na => "—",
            Self::Unaudited => "❔",
        }
    }
}

/// A host position a row is the parity of (`crate::host::position`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Position {
    Window,
    Displays,
    Input,
    Text,
    Accessibility,
    Clipboard,
    PrimarySelection,
    Drag,
    Haptics,
    Preferences,
    Lifecycle,
    Chrome,
    Storage,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegendEntry {
    mark: Mark,
    meaning: String,
}

/// One environment column.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Host {
    /// The key of the column in each row's `status`.
    id: String,
    /// The column header.
    environment: String,
    /// The host that runs there.
    runner: String,
    /// The test double: no user runs there, so it has no readiness and no
    /// share of the overall score.
    #[serde(default)]
    test_double: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    name: String,
    /// Whether a ready platform needs every row of the group.
    #[serde(default)]
    decides_readiness: bool,
    #[serde(default)]
    intro: Option<String>,
    section: Vec<Section>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Section {
    /// The section's heading; a group's only table has none.
    #[serde(default)]
    name: Option<String>,
    row: Vec<Row>,
}

/// One feature row.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    /// The row's stable identifier.
    id: String,
    feature: String,
    /// The host position the row is the parity of, where it is one.
    #[serde(default)]
    position: Option<Position>,
    /// The mark on each host, by its id.
    status: BTreeMap<String, Mark>,
    #[serde(default)]
    note: String,
    /// The run evidence under the section's table.
    #[serde(default)]
    evidence: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Quality {
    intro: String,
    row: Vec<QualityRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualityRow {
    quality: String,
    /// How the summary names the quality.
    #[serde(default)]
    short: Option<String>,
    status: Mark,
    enforced_by: String,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capabilities {
    intro: String,
    row: Vec<CapabilityRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityRow {
    capability: String,
    environments: String,
}

/// How many cells of a column carry each mark, in [`Mark::ALL`]'s order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Tally([usize; 5]);

impl Tally {
    fn slot(mark: Mark) -> usize {
        Mark::ALL
            .iter()
            .position(|m| *m == mark)
            .expect("BUG: Mark::ALL lists every mark")
    }

    fn add(&mut self, mark: Mark) {
        self.0[Self::slot(mark)] += 1;
    }

    fn get(self, mark: Mark) -> usize {
        self.0[Self::slot(mark)]
    }

    fn total(self) -> usize {
        self.0.iter().sum()
    }

    /// ✅ counted as 2 and 🟡 as 1, with the scored cells: neither — nor ❔.
    fn halves_and_applicable(self) -> (usize, usize) {
        let halves = 2 * self.get(Mark::Yes) + self.get(Mark::Partial);
        let scored = self.total() - self.get(Mark::Na) - self.get(Mark::Unaudited);
        (halves, scored)
    }
}

/// `halves / 2` out of `applicable`, as a percentage rounded half up.
fn percent(halves: usize, applicable: usize) -> usize {
    if applicable == 0 {
        return 0;
    }
    (100 * halves + applicable) / (2 * applicable)
}

impl Parity {
    fn parse(source: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(source)
    }

    fn rows(&self) -> impl Iterator<Item = (&Group, &Row)> {
        self.group
            .iter()
            .flat_map(|group| group.section.iter().map(move |section| (group, section)))
            .flat_map(|(group, section)| section.row.iter().map(move |row| (group, row)))
    }

    /// The mark `row` claims on `host`.
    fn mark(row: &Row, host: &Host) -> Mark {
        *row.status
            .get(&host.id)
            .expect("BUG: check_shape proved every row marks every host")
    }

    /// Every row marks exactly the declared hosts, row ids are unique, and
    /// every quality the summary names has its short name.
    fn check_shape(&self) -> Result<(), String> {
        let mut hosts: Vec<&str> = self.host.iter().map(|h| h.id.as_str()).collect();
        hosts.sort_unstable();
        let mut ids = BTreeMap::new();
        for (_, row) in self.rows() {
            let marked: Vec<&str> = row.status.keys().map(String::as_str).collect();
            if marked != hosts {
                return Err(format!(
                    "row `{}` marks {marked:?}, the hosts are {hosts:?}",
                    row.id
                ));
            }
            if let Some(earlier) = ids.insert(row.id.as_str(), row.feature.as_str()) {
                return Err(format!(
                    "row `{}` ({}) reuses the id of `{earlier}`",
                    row.id, row.feature
                ));
            }
        }
        for row in &self.quality.row {
            let named = matches!(row.status, Mark::Yes | Mark::Missing | Mark::Unaudited);
            if named && row.short.is_none() {
                return Err(format!(
                    "quality `{}` is named in the summary and has no `short`",
                    row.quality
                ));
            }
        }
        Ok(())
    }

    fn tally(&self, host: &Host, readiness_only: bool) -> Tally {
        let mut tally = Tally::default();
        for (group, row) in self.rows() {
            if !readiness_only || group.decides_readiness {
                tally.add(Self::mark(row, host));
            }
        }
        tally
    }

    /// The document `docs/platform-parity.md` is kept equal to.
    fn render(&self) -> String {
        let mut out = String::new();
        self.render_head(&mut out);
        self.render_summary(&mut out);
        for group in &self.group {
            self.render_group(&mut out, group);
        }
        self.render_quality(&mut out);
        self.render_capabilities(&mut out);
        out
    }

    fn render_head(&self, out: &mut String) {
        line(out, format_args!("{}\n", self.preamble.trim_end()));
        out.push_str("| Mark | Meaning |\n|:-:|:--|\n");
        for entry in &self.legend {
            line(
                out,
                format_args!("| {} | {} |", entry.mark.glyph(), entry.meaning),
            );
        }
        let users: Vec<&Host> = self.host.iter().filter(|host| !host.test_double).collect();
        let (halves, applicable) = users
            .iter()
            .map(|host| self.tally(host, false).halves_and_applicable())
            .fold((0, 0), |(h, a), (dh, da)| (h + dh, a + da));
        line(
            out,
            format_args!(
                "\n**Overall: ~{}% (✅ = 1, 🟡 = ½, over applicable cells of the {} user \
                 environments).**\n",
                percent(halves, applicable),
                number_word(users.len())
            ),
        );
        line(out, format_args!("{}\n", self.rules.trim_end()));
    }

    fn render_summary(&self, out: &mut String) {
        line(
            out,
            format_args!("## Summary\n\n{}\n", self.summary.trim_end()),
        );
        out.push_str("| Environment | Host | ✅ | 🟡 | ❌ | — | ❔ | Score | Ready |\n");
        out.push_str("|:--|:--|--:|--:|--:|--:|--:|--:|:-:|\n");
        for host in &self.host {
            let tally = self.tally(host, false);
            let (halves, applicable) = tally.halves_and_applicable();
            let needed = self.tally(host, true);
            let ready = if host.test_double {
                "—"
            } else if needed.get(Mark::Yes) + needed.get(Mark::Na) == needed.total() {
                "yes"
            } else {
                "no"
            };
            line(
                out,
                format_args!(
                    "| {} | {} | {} | {} | {} | {} | {} | {}% | {ready} |",
                    host.environment,
                    host.runner,
                    tally.get(Mark::Yes),
                    tally.get(Mark::Partial),
                    tally.get(Mark::Missing),
                    tally.get(Mark::Na),
                    tally.get(Mark::Unaudited),
                    percent(halves, applicable),
                ),
            );
        }
        let qualities = &self.quality.row;
        let named = |mark: Mark| -> (usize, String) {
            let shorts: Vec<&str> = qualities
                .iter()
                .filter(|row| row.status == mark)
                .filter_map(|row| row.short.as_deref())
                .collect();
            (shorts.len(), shorts.join(", "))
        };
        let partly = qualities
            .iter()
            .filter(|row| row.status == Mark::Partial)
            .count();
        let (met, met_names) = named(Mark::Yes);
        let (unmet, unmet_names) = named(Mark::Missing);
        let (unaudited, unaudited_names) = named(Mark::Unaudited);
        line(
            out,
            format_args!(
                "\nQuality bar: {met} of {} met ({met_names}); {partly} partly; {unmet} not met \
                 ({unmet_names}); {unaudited} not audited ({unaudited_names}).\n",
                qualities.len()
            ),
        );
    }

    fn render_group(&self, out: &mut String, group: &Group) {
        line(out, format_args!("## {}\n", group.name));
        if let Some(intro) = &group.intro {
            line(out, format_args!("{}\n", intro.trim_end()));
        }
        for section in &group.section {
            if let Some(name) = &section.name {
                line(out, format_args!("### {name}\n"));
            }
            out.push_str("| Feature |");
            for host in &self.host {
                let _ = write!(out, " {} |", host.environment);
            }
            out.push_str(" Notes |\n|:--|");
            for _ in &self.host {
                out.push_str(":-:|");
            }
            out.push_str(":--|\n");
            for row in &section.row {
                let _ = write!(out, "| {} |", row.feature);
                for host in &self.host {
                    let _ = write!(out, " {} |", Self::mark(row, host).glyph());
                }
                line(out, format_args!(" {} |", row.note));
            }
            out.push('\n');
            let evidence: Vec<(&str, &str)> = section
                .row
                .iter()
                .filter_map(|row| Some((row.feature.as_str(), row.evidence.as_deref()?)))
                .collect();
            if !evidence.is_empty() {
                out.push_str("<details>\n<summary>Run evidence</summary>\n\n");
                for (feature, text) in evidence {
                    line(out, format_args!("- **{feature}** — {}", text.trim_end()));
                }
                out.push_str("\n</details>\n\n");
            }
        }
    }

    fn render_quality(&self, out: &mut String) {
        line(
            out,
            format_args!("## Quality bar\n\n{}\n", self.quality.intro.trim_end()),
        );
        out.push_str("| Quality | Status | Enforced by | Notes |\n|---|---|---|---|\n");
        for row in &self.quality.row {
            line(
                out,
                format_args!(
                    "| {} | {} | {} | {} |",
                    row.quality,
                    row.status.glyph(),
                    row.enforced_by,
                    row.note
                ),
            );
        }
        out.push('\n');
    }

    fn render_capabilities(&self, out: &mut String) {
        line(
            out,
            format_args!(
                "## Capabilities\n\n{}\n",
                self.capabilities.intro.trim_end()
            ),
        );
        out.push_str("| Capability | Environments |\n|---|---|\n");
        for row in &self.capabilities.row {
            line(
                out,
                format_args!("| {} | {} |", row.capability, row.environments),
            );
        }
    }
}

/// Append `args` and a newline.
fn line(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _ = out.write_fmt(args);
    out.push('\n');
}

/// The count as the document spells it.
fn number_word(n: usize) -> String {
    const WORDS: [&str; 11] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    WORDS
        .get(n)
        .map_or_else(|| n.to_string(), |word| (*word).to_owned())
}

/// The positions host `H` fills with [`Absent`](crate::host::Absent).
fn absent_positions<H: crate::host::Host>() -> Vec<Position> {
    use std::any::TypeId;

    use crate::host::{Absent, position};

    fn same<A: 'static, B: 'static>() -> bool {
        TypeId::of::<A>() == TypeId::of::<B>()
    }

    [
        (
            same::<H::Window, Absent<position::Window>>(),
            Position::Window,
        ),
        (
            same::<H::Displays, Absent<position::Displays>>(),
            Position::Displays,
        ),
        (same::<H::Input, Absent<position::Input>>(), Position::Input),
        (same::<H::Text, Absent<position::Text>>(), Position::Text),
        (
            same::<H::Accessibility, Absent<position::Accessibility>>(),
            Position::Accessibility,
        ),
        (
            same::<H::Clipboard, Absent<position::Clipboard>>(),
            Position::Clipboard,
        ),
        (
            same::<H::PrimarySelection, Absent<position::PrimarySelection>>(),
            Position::PrimarySelection,
        ),
        (same::<H::Drag, Absent<position::Drag>>(), Position::Drag),
        (
            same::<H::Haptics, Absent<position::Haptics>>(),
            Position::Haptics,
        ),
        (
            same::<H::Preferences, Absent<position::Preferences>>(),
            Position::Preferences,
        ),
        (
            same::<H::Lifecycle, Absent<position::Lifecycle>>(),
            Position::Lifecycle,
        ),
        (
            same::<H::Chrome, Absent<position::Chrome>>(),
            Position::Chrome,
        ),
        (
            same::<H::Storage, Absent<position::Storage>>(),
            Position::Storage,
        ),
    ]
    .into_iter()
    .filter_map(|(absent, position)| absent.then_some(position))
    .collect()
}

/// The rows that claim ✅ or 🟡 in `column` for a position `absent` names.
fn absent_claims(parity: &Parity, column: &str, absent: &[Position]) -> Vec<String> {
    let host = parity
        .host
        .iter()
        .find(|host| host.id == column)
        .unwrap_or_else(|| panic!("the parity table has no host `{column}`"));
    parity
        .rows()
        .filter_map(|(_, row)| {
            let position = row.position.filter(|p| absent.contains(p))?;
            let mark = Parity::mark(row, host);
            matches!(mark, Mark::Yes | Mark::Partial).then(|| {
                format!(
                    "row `{}` claims {} on `{column}`, whose host fills {position:?} with Absent",
                    row.id,
                    mark.glyph()
                )
            })
        })
        .collect()
}

fn parse_checked(source: &str) -> Parity {
    let parity = Parity::parse(source).expect("platform-parity.toml parses");
    if let Err(shape) = parity.check_shape() {
        panic!("platform-parity.toml: {shape}");
    }
    parity
}

#[test]
fn parity_document_matches_its_source() {
    let rendered = parse_checked(SOURCE).render();
    if std::env::var_os("FLUI_OVERWRITE").is_some_and(|v| v == "1") {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/platform-parity.md");
        std::fs::write(path, &rendered).expect("write docs/platform-parity.md");
        return;
    }
    assert!(
        rendered == DOCUMENT,
        "docs/platform-parity.md is not the render of crates/flui-native/platform-parity.toml: \
         edit the source, then restate the document with \
         `FLUI_OVERWRITE=1 cargo test -p flui-native --lib parity_document_matches_its_source`"
    );
}

#[test]
fn absent_positions_claim_no_support() {
    let parity = parse_checked(SOURCE);
    let mut claims = absent_claims(
        &parity,
        "headless",
        &absent_positions::<crate::hosts::headless::Headless>(),
    );
    #[cfg(windows)]
    claims.extend(absent_claims(
        &parity,
        "windows",
        &absent_positions::<crate::hosts::windows::Windows>(),
    ));
    #[cfg(target_os = "macos")]
    claims.extend(absent_claims(
        &parity,
        "macos",
        &absent_positions::<crate::hosts::apple::Apple>(),
    ));
    #[cfg(target_os = "ios")]
    claims.extend(absent_claims(
        &parity,
        "ios",
        &absent_positions::<crate::hosts::apple::Apple>(),
    ));
    #[cfg(target_arch = "wasm32")]
    claims.extend(absent_claims(
        &parity,
        "web",
        &absent_positions::<crate::hosts::web::Web>(),
    ));
    #[cfg(target_os = "android")]
    claims.extend(absent_claims(
        &parity,
        "android",
        &absent_positions::<crate::hosts::android::Android>(),
    ));
    #[cfg(wayland_host)]
    claims.extend(absent_claims(
        &parity,
        "wayland",
        &absent_positions::<crate::hosts::linux::Wayland>(),
    ));
    #[cfg(x11_host)]
    claims.extend(absent_claims(
        &parity,
        "x11",
        &absent_positions::<crate::hosts::linux::X11>(),
    ));
    assert!(claims.is_empty(), "{claims:#?}");
}

/// A source with one host and one row, for the checks' own cases.
fn synthetic(position: &str, mark: &str, extra_status: &str) -> String {
    format!(
        r##"
preamble = "# P"
rules = "R"
summary = "S"
legend = []
quality = {{ intro = "Q", row = [] }}
capabilities = {{ intro = "C", row = [] }}

[[host]]
id = "headless"
environment = "Headless"
runner = "test double"
test_double = true

[[group]]
name = "Core"
[[group.section]]
[[group.section.row]]
id = "a-row"
feature = "A row"
position = "{position}"
status = {{ headless = "{mark}"{extra_status} }}
"##
    )
}

#[test]
fn the_source_checks_catch_their_violations() {
    let absent = [Position::PrimarySelection];
    // a supported cell in an absent position is a claim; an absent cell is not
    let claims = absent_claims(
        &parse_checked(&synthetic("primary-selection", "partial", "")),
        "headless",
        &absent,
    );
    assert_eq!(claims.len(), 1, "{claims:?}");
    let missing = parse_checked(&synthetic("primary-selection", "missing", ""));
    assert_eq!(
        absent_claims(&missing, "headless", &absent),
        Vec::<String>::new()
    );
    // a row marking a host the table does not declare is refused
    let stray =
        Parity::parse(&synthetic("drag", "yes", ", web = \"yes\"")).expect("the source parses");
    assert!(stray.check_shape().is_err());
}
