//! The dependency through which an application reaches FLUI's build
//! switches, and the cargo features each build mode turns on through it.

use cargo_metadata::{Dependency, DependencyKind};

use crate::build::platform::Profile;

/// The facade feature a release build turns on: `tracing`'s static maximum
/// level, so the build holds no `trace!` or `debug!` call site (ADR-0398).
/// CI's `size` job builds generated projects with cargo and names it by
/// hand (`size_of` in `.github/workflows/ci.yml`): a rename changes it there.
pub(crate) const RELEASE_CEILING_FEATURE: &str = "release-max-level-info";

/// Which of a package's normal dependencies carries FLUI's build switches
/// (the agent's `driver`, the release ceiling), by the key the package's
/// manifest names it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameworkDependency {
    /// `flui` (the facade, preferred) or `flui-app`.
    Named {
        /// Its `rename` when the manifest gives one, else the crate's name.
        key: String,
        /// Whether the manifest declares it `optional`: a feature named
        /// through it then turns on only where the dependency already is
        /// (`<key>?/<feature>`), so a mode never adds it to the graph.
        optional: bool,
    },
    /// Neither (a legacy `flui-widgets`-only package, or no FLUI at all).
    Neither,
}

impl FrameworkDependency {
    /// The facade among `dependencies`, else `flui-app`, counting normal
    /// dependencies only: a dev- or build-dependency is not in the shipped
    /// graph, so a feature named through it would do nothing there.
    pub(crate) fn of(dependencies: &[Dependency]) -> Self {
        let named = |name: &str| {
            dependencies
                .iter()
                .find(|dependency| {
                    dependency.kind == DependencyKind::Normal && dependency.name == name
                })
                .map(|dependency| Self::Named {
                    key: dependency
                        .rename
                        .clone()
                        .unwrap_or_else(|| dependency.name.clone()),
                    optional: dependency.optional,
                })
        };
        named("flui")
            .or_else(|| named("flui-app"))
            .unwrap_or(Self::Neither)
    }

    /// `<key>/<feature>` (`<key>?/<feature>` for an optional dependency), as
    /// `--features` takes it, when there is a key.
    pub(crate) fn feature(&self, feature: &str) -> Option<String> {
        match self {
            Self::Named {
                key,
                optional: false,
            } => Some(format!("{key}/{feature}")),
            Self::Named {
                key,
                optional: true,
            } => Some(format!("{key}?/{feature}")),
            Self::Neither => None,
        }
    }

    /// The features a build with `profile` turns on through this dependency
    /// beyond the package's defaults: the agent's `driver` feature
    /// (`agent_driver`, as `--features` takes it) when the agent connects to
    /// the build, else for a release build the release ceiling, so it holds
    /// no `trace!` or `debug!` call site (ADR-0398 §2). An agent build is a
    /// development build, not one that ships, and keeps them. A release build
    /// of a package with no key has no feature to name, which it is warned of.
    pub(crate) fn mode_features(
        &self,
        profile: Profile,
        agent_driver: Option<&str>,
    ) -> Vec<String> {
        match (profile, agent_driver) {
            (_, Some(driver)) => vec![driver.to_owned()],
            (Profile::Release, None) => {
                let ceiling = self.feature(RELEASE_CEILING_FEATURE);
                if ceiling.is_none() {
                    let _ = crate::ui::warning(
                        "this package depends on neither `flui` nor `flui-app`, whose \
                         `release-max-level-info` feature leaves trace and debug call sites \
                         out of a release build; it keeps them (ADR-0398 §2)",
                    );
                }
                ceiling.into_iter().collect()
            }
            (Profile::Debug, None) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FrameworkDependency;
    use crate::build::platform::Profile;

    fn dependencies(manifest_deps: &str) -> Vec<cargo_metadata::Dependency> {
        let json = format!("[{manifest_deps}]");
        serde_json::from_str(&json).expect("dependency fixtures")
    }

    fn dependency(name: &str, rename: Option<&str>, kind: Option<&str>) -> String {
        optional_dependency(name, rename, kind, false)
    }

    fn optional_dependency(
        name: &str,
        rename: Option<&str>,
        kind: Option<&str>,
        optional: bool,
    ) -> String {
        let rename = rename.map_or("null".to_owned(), |rename| format!(r#""{rename}""#));
        let kind = kind.map_or("null".to_owned(), |kind| format!(r#""{kind}""#));
        format!(
            r#"{{"name":"{name}","source":null,"req":"*","kind":{kind},"rename":{rename},
            "optional":{optional},"uses_default_features":true,"features":[],"target":null,
            "registry":null,"path":null}}"#
        )
    }

    /// The key is the one the manifest names the dependency under, the
    /// facade before `flui-app`, and a dev-dependency never counts; a
    /// feature named through an optional one turns on only where it is.
    #[test]
    fn the_framework_key_is_the_normal_dependency_the_manifest_names() {
        for (deps, expected) in [
            (vec![dependency("flui", None, None)], Some("flui/driver")),
            (
                vec![dependency("flui", Some("ui"), None)],
                Some("ui/driver"),
            ),
            (
                vec![optional_dependency("flui", None, None, true)],
                Some("flui?/driver"),
            ),
            (
                vec![
                    dependency("flui-app", None, None),
                    dependency("flui", None, None),
                ],
                Some("flui/driver"),
            ),
            (
                vec![dependency("flui-app", None, None)],
                Some("flui-app/driver"),
            ),
            (vec![dependency("flui", None, Some("dev"))], None),
            (vec![dependency("flui-widgets", None, None)], None),
        ] {
            let found = FrameworkDependency::of(&dependencies(&deps.join(",")));
            assert_eq!(found.feature("driver").as_deref(), expected, "{deps:?}");
        }
    }

    /// A release build turns the ceiling on through the key the manifest
    /// names; a debug build turns nothing on, an agent's build only its
    /// driver, whatever the profile, and a package with no key gets no
    /// ceiling.
    #[test]
    fn a_release_build_without_an_agent_is_the_one_with_the_ceiling() {
        let named = FrameworkDependency::Named {
            key: "ui".to_owned(),
            optional: false,
        };
        for (framework, profile, driver, expected) in [
            (
                &named,
                Profile::Release,
                None,
                vec!["ui/release-max-level-info"],
            ),
            (&named, Profile::Debug, None, vec![]),
            (
                &named,
                Profile::Release,
                Some("ui/driver"),
                vec!["ui/driver"],
            ),
            (&named, Profile::Debug, Some("ui/driver"), vec!["ui/driver"]),
            (
                &FrameworkDependency::Neither,
                Profile::Release,
                None,
                vec![],
            ),
        ] {
            assert_eq!(
                framework.mode_features(profile, driver),
                expected,
                "{framework:?} {profile:?} {driver:?}"
            );
        }
    }
}
