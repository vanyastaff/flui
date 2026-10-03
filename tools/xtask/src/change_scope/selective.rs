//! Finite native/GPU checks selected in addition to the ordinary PR lane.
use std::collections::BTreeSet;

use super::classify::{Mode, Repo, Scope};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ExtraJob {
    GpuTest,
    PlatformWindows,
    PlatformMacos,
    CliWindows,
    CliMacos,
}

impl ExtraJob {
    pub(super) const ALL: [Self; 5] = [
        Self::GpuTest,
        Self::PlatformWindows,
        Self::PlatformMacos,
        Self::CliWindows,
        Self::CliMacos,
    ];

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::GpuTest => "gpu-test",
            Self::PlatformWindows => "platform-windows",
            Self::PlatformMacos => "platform-macos",
            Self::CliWindows => "cli-windows",
            Self::CliMacos => "cli-macos",
        }
    }
}

pub(super) fn to_json(jobs: &BTreeSet<ExtraJob>) -> String {
    serde_json::to_string(jobs).expect("BUG: finite job names serialize")
}

pub(super) fn parse(value: &str) -> anyhow::Result<BTreeSet<ExtraJob>> {
    Ok(serde_json::from_str(value)?)
}

pub(super) fn select(repo: &Repo, scope: &Scope) -> anyhow::Result<BTreeSet<ExtraJob>> {
    if matches!(scope.mode, Mode::Docs | Mode::None) {
        return Ok(BTreeSet::new());
    }
    let paths = &scope.changed_paths;
    // Shader inputs widen Linux checks. Reclassify the remaining source inputs
    // so that this shortcut does not imply unrelated native host checks.
    if scope.mode == Mode::Full {
        let shader = |p: &str| {
            p.starts_with("crates/flui-engine/")
                && std::path::Path::new(p)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wgsl"))
        };
        if paths.iter().any(|p| shader(p)) {
            let rest: Vec<String> = paths.iter().filter(|p| !shader(p)).cloned().collect();
            let mut jobs = select(repo, &super::classify::classify(repo, &rest)?)?;
            jobs.insert(ExtraJob::GpuTest);
            return Ok(jobs);
        }
        return Ok(ExtraJob::ALL.into());
    }
    let ws = repo.workspace()?;
    let mut jobs = BTreeSet::new();
    let affected: BTreeSet<&str> = scope.packages.iter().map(String::as_str).collect();
    if affected.contains("flui-engine") {
        jobs.insert(ExtraJob::GpuTest);
    }
    if affected.contains("flui-cli") {
        jobs.extend([ExtraJob::CliWindows, ExtraJob::CliMacos]);
    }
    if affected.contains("flui-platform") {
        // Narrow only when every source input affecting this package is one
        // backend. A dependency or shared file can change both native hosts.
        let inputs: Vec<&str> = paths
            .iter()
            .filter_map(|p| {
                let owner = ws.owning_package(p)?;
                let seeds = [owner.to_owned()].into();
                ws.affected(&seeds)
                    .contains("flui-platform")
                    .then_some(p.as_str())
            })
            .collect();
        let backend_only = |backend: &str| {
            !inputs.is_empty()
                && inputs.iter().all(|p| {
                    p.starts_with(&format!("crates/flui-platform/src/platforms/{backend}/"))
                })
        };
        if !backend_only("macos") {
            jobs.insert(ExtraJob::PlatformWindows);
        }
        if !backend_only("windows") {
            jobs.insert(ExtraJob::PlatformMacos);
        }
    }
    Ok(jobs)
}
