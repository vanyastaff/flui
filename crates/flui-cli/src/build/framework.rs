//! The dependency through which an application reaches FLUI's build
//! switches, and the cargo features each build mode turns on through it.

use cargo_metadata::{Dependency, DependencyKind};

use crate::build::platform::Profile;

/// The facade feature a release build turns on: `tracing`'s static maximum
/// level, so the build holds no `trace!` or `debug!` call site (ADR-0398).
pub(crate) const RELEASE_CEILING_FEATURE: &str = "release-max-level-info";

/// Which of a package's normal dependencies carries FLUI's build switches
/// (the agent's `driver`, the release ceiling), by the key the package's
/// manifest names it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FrameworkDependency {
    /// `flui` (the facade, preferred) or `flui-app`, under this key: its
    /// `rename` when the manifest gives one, else the crate's name.
    Named(String),
    /// Neither (a legacy `flui-widgets`-only package, or no FLUI at all).
    Neither,
}

impl FrameworkDependency {
    /// The facade among `dependencies`, else `flui-app`, counting normal
    /// dependencies only: a dev- or build-dependency is not in the shipped
    /// graph, so a feature named through it would do nothing there.
    pub(crate) fn of(dependencies: &[Dependency]) -> Self {
        let key = |name: &str| {
            dependencies
                .iter()
                .find(|dependency| {
                    dependency.kind == DependencyKind::Normal && dependency.name == name
                })
                .map(|dependency| {
                    dependency
                        .rename
                        .clone()
                        .unwrap_or_else(|| dependency.name.clone())
                })
        };
        key("flui")
            .or_else(|| key("flui-app"))
            .map_or(Self::Neither, Self::Named)
    }

    /// `<key>/<feature>`, as `--features` takes it, when there is a key.
    pub(crate) fn feature(&self, feature: &str) -> Option<String> {
        match self {
            Self::Named(key) => Some(format!("{key}/{feature}")),
            Self::Neither => None,
        }
    }

    /// The features a build with `profile` turns on through this dependency
    /// beyond the package's defaults: the agent's `driver` feature
    /// (`agent_driver`, as `--features` takes it) when the agent connects to
    /// the build, else for a release build the release ceiling, so it holds
    /// no `trace!` or `debug!` call site (ADR-0398 §2). An agent build keeps
    /// them: its driver reads the frame's phase spans, which are DEBUG.
    pub(crate) fn mode_features(
        &self,
        profile: Profile,
        agent_driver: Option<&str>,
    ) -> Vec<String> {
        match (profile, agent_driver) {
            (_, Some(driver)) => vec![driver.to_owned()],
            (Profile::Release, None) => self.feature(RELEASE_CEILING_FEATURE).into_iter().collect(),
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
        let rename = rename.map_or("null".to_owned(), |rename| format!(r#""{rename}""#));
        let kind = kind.map_or("null".to_owned(), |kind| format!(r#""{kind}""#));
        format!(
            r#"{{"name":"{name}","source":null,"req":"*","kind":{kind},"rename":{rename},
            "optional":false,"uses_default_features":true,"features":[],"target":null,
            "registry":null,"path":null}}"#
        )
    }

    /// The key is the one the manifest names the dependency under, the
    /// facade before `flui-app`, and a dev-dependency never counts.
    #[test]
    fn the_framework_key_is_the_normal_dependency_the_manifest_names() {
        for (deps, expected) in [
            (vec![dependency("flui", None, None)], Some("flui")),
            (vec![dependency("flui", Some("ui"), None)], Some("ui")),
            (
                vec![
                    dependency("flui-app", None, None),
                    dependency("flui", None, None),
                ],
                Some("flui"),
            ),
            (vec![dependency("flui-app", None, None)], Some("flui-app")),
            (vec![dependency("flui", None, Some("dev"))], None),
            (vec![dependency("flui-widgets", None, None)], None),
        ] {
            let found = FrameworkDependency::of(&dependencies(&deps.join(",")));
            assert_eq!(
                found.feature("driver").as_deref(),
                expected.map(|key| format!("{key}/driver")).as_deref(),
                "{deps:?}"
            );
        }
    }

    /// A release build turns the ceiling on through the key the manifest
    /// names; a debug build turns nothing on, an agent's build only its
    /// driver, whatever the profile, and a package with no key gets no
    /// ceiling.
    #[test]
    fn a_release_build_without_an_agent_is_the_one_with_the_ceiling() {
        let named = FrameworkDependency::Named("ui".to_owned());
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
