//! What the app publishes against what assistive technology reads of it: the window's semantics
//! tree and the OS accessibility tree a lane reads, each as its normalized outline (ADR-0095 §4,
//! [`normalized_outline`]), and the lines on which they differ.
//!
//! The two outlines are equal when the OS shows what the app published; a difference is a line
//! the app published that the OS does not show, or one the OS shows that the app did not publish,
//! in the order of the outlines, matched by their longest common run of lines.

use flui_protocol::tree::{Tree, normalized_outline};

/// The most lines of each outline [`Projection::differences`] matches, which bounds its table to
/// four bytes for each pair of lines; past it, the lines left on either side are reported
/// unmatched.
pub const MAX_MATCHED_LINES: usize = 1024;

/// The two projections of one window: see the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Projection {
    /// The app's semantics tree, projected.
    pub app: String,
    /// The OS accessibility tree, projected.
    pub os: String,
}

/// A line one projection has and the other lacks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectionDifference {
    /// The app published it and the OS does not show it.
    MissingFromOs(String),
    /// The OS shows it and the app did not publish it.
    OnlyInOs(String),
}

impl Projection {
    /// The projections of `app` and `os`.
    #[must_use]
    pub fn of(app: &Tree, os: &Tree) -> Self {
        Self {
            app: normalized_outline(&app.roots),
            os: normalized_outline(&os.roots),
        }
    }

    /// Whether the OS shows exactly what the app published.
    #[must_use]
    pub fn is_equal(&self) -> bool {
        self.app == self.os
    }

    /// The lines on which the projections differ, in outline order.
    #[must_use]
    pub fn differences(&self) -> Vec<ProjectionDifference> {
        let app_lines: Vec<&str> = self.app.lines().collect();
        let os_lines: Vec<&str> = self.os.lines().collect();
        let app = &app_lines[..app_lines.len().min(MAX_MATCHED_LINES)];
        let os = &os_lines[..os_lines.len().min(MAX_MATCHED_LINES)];
        // `common[i][j]`: the longest common run of `app[i..]` and `os[j..]`.
        let mut common = vec![vec![0_u32; os.len() + 1]; app.len() + 1];
        for i in (0..app.len()).rev() {
            for j in (0..os.len()).rev() {
                common[i][j] = if app[i] == os[j] {
                    common[i + 1][j + 1] + 1
                } else {
                    common[i + 1][j].max(common[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        let mut differences = Vec::new();
        while i < app.len() || j < os.len() {
            if i < app.len() && j < os.len() && app[i] == os[j] {
                i += 1;
                j += 1;
            } else if j < os.len() && (i == app.len() || common[i][j + 1] >= common[i + 1][j]) {
                differences.push(ProjectionDifference::OnlyInOs(os[j].to_owned()));
                j += 1;
            } else {
                differences.push(ProjectionDifference::MissingFromOs(app[i].to_owned()));
                i += 1;
            }
        }
        differences.extend(
            app_lines[app.len()..]
                .iter()
                .map(|line| ProjectionDifference::MissingFromOs((*line).to_owned())),
        );
        differences.extend(
            os_lines[os.len()..]
                .iter()
                .map(|line| ProjectionDifference::OnlyInOs((*line).to_owned())),
        );
        differences
    }
}
