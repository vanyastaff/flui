//! Web builds: the project's `fn main` binary compiled for
//! `wasm32-unknown-unknown`, then `wasm-bindgen --target web` turning the
//! `.wasm` into `pkg/app.js` + `pkg/app_bg.wasm`, the pair the page the
//! build writes (`web_page`) imports. `run_app` dispatches to the web runner
//! on wasm32, so a generated project needs no web-specific entry point.
//!
//! `wasm-bindgen` on `PATH` must match the `wasm-bindgen` crate the project
//! resolves to; the mismatch error the tool prints otherwise names neither
//! side, so the check runs here, before anything compiles.

use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts};
use crate::build::util::{cargo, process};
use crate::build::web_page;
use crate::proc::{PROBE_TIMEOUT, probe_stdout};

const WASM_TARGET: &str = "wasm32-unknown-unknown";
use web_page::{OUT_NAME, PKG_DIR};

/// Builder for the web platform.
#[derive(Debug)]
pub(crate) struct WebBuilder {
    workspace_root: PathBuf,
}

impl WebBuilder {
    #[must_use]
    pub(crate) fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    /// The wasm target, `wasm-bindgen`, and its version against the project's.
    pub(crate) fn validate_environment(&self) -> BuildResult<()> {
        crate::build::util::environment::require_rust_target(&self.workspace_root, WASM_TARGET)?;

        let wanted = resolved_wasm_bindgen_version(&self.workspace_root)?;
        let installed = installed_wasm_bindgen_version();
        match check_wasm_bindgen(wanted.as_deref(), installed.as_deref()) {
            Ok(()) => Ok(()),
            Err(hint) => Err(BuildError::ToolNotFound {
                tool: match installed {
                    Some(version) => {
                        format!("wasm-bindgen matching the project (installed: {version})")
                    }
                    None => "wasm-bindgen".to_string(),
                },
                install_hint: hint,
            }),
        }
    }

    /// Compile the project's binary for wasm32; the artifact is the `.wasm`.
    pub(crate) async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        if matches!(
            ctx.target,
            crate::build::platform::BuildUnit::Library { .. }
        ) {
            return Err(BuildError::invalid_config(
                "build unit",
                "explicit static-library delivery is supported only on iOS",
            ));
        }
        // A page the project still keeps from before the page was generated
        // stops the build before anything compiles.
        let page = web_platform_page(ctx)?;
        page_source(
            page.project.as_deref().unwrap_or(&self.workspace_root),
            page,
        )?;
        let dir = &ctx.workspace_root;
        let target = cargo::select_target(dir, &ctx.target).await?;
        let mut args = vec!["build".to_string(), "--target".into(), WASM_TARGET.into()];
        args.extend(target.cargo_args());
        args.extend(ctx.mode_args(target.framework()));
        let package = target.built_package();
        crate::ui::debug(format!(
            "building {} {} for {WASM_TARGET}",
            package.name, package.version
        ));
        let wasm = cargo::build_artifact(dir, &args, &target, &[]).await?;
        Ok(BuildArtifacts {
            rust_libs: vec![wasm],
            executable: None,
            package: Some(package),
        })
    }

    /// `wasm-bindgen` into `<output>/pkg`; beside it the page (generated, or
    /// the project's template, [`page_source`]), its bootstrap module and its
    /// stylesheet; the project's `platforms/web/manifest.json` and icons
    /// copied, or a manifest made from `flui.toml` where the project keeps
    /// none. The output directory is the deliverable.
    pub(crate) async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        let wasm = artifacts
            .rust_libs
            .first()
            .ok_or_else(|| BuildError::Other("the wasm build produced no artifact".into()))?;
        let dist = &ctx.output_dir;
        let assets = crate::build::assets::of_project(&self.workspace_root)?;
        let pkg = dist.join(PKG_DIR);
        std::fs::create_dir_all(&pkg)?;

        process::run(
            Command::new("wasm-bindgen")
                .args(["--target", "web", "--no-typescript", "--out-name", OUT_NAME])
                .arg("--out-dir")
                .arg(&pkg)
                .arg(wasm),
        )
        .await?;

        let page = web_platform_page(ctx)?;
        // The application's name: `flui.toml`'s, else the package compiled.
        let name = page
            .title
            .clone()
            .or_else(|| {
                artifacts
                    .package
                    .as_ref()
                    .map(|package| package.name.clone())
            })
            .unwrap_or_else(|| "FLUI".to_owned());
        // The project's own files are where its `flui.toml` is.
        let project = page.project.as_deref().unwrap_or(&self.workspace_root);
        let html = match page_source(project, page)? {
            PageSource::Generated => web_page::render(&name, page),
            PageSource::Template { path, text } => web_page::render_template(&text, &name, page)
                .map_err(|error| {
                    BuildError::invalid_config(path.display().to_string(), error.to_string())
                })?,
        };
        let index = dist.join("index.html");
        std::fs::write(&index, html)?;
        std::fs::write(dist.join(web_page::BOOTSTRAP), web_page::bootstrap())?;
        std::fs::write(dist.join(web_page::STYLESHEET), web_page::stylesheet())?;

        let web_dir = project.join("platforms").join("web");
        let manifest = web_dir.join(web_page::MANIFEST);
        if manifest.is_file() {
            std::fs::copy(&manifest, dist.join(web_page::MANIFEST))?;
        } else {
            std::fs::write(
                dist.join(web_page::MANIFEST),
                web_page::manifest(&name, page),
            )?;
        }
        let icons = web_dir.join("icons");
        if icons.is_dir() {
            let dist_icons = dist.join("icons");
            std::fs::create_dir_all(&dist_icons)?;
            for entry in std::fs::read_dir(&icons)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    std::fs::copy(entry.path(), dist_icons.join(entry.file_name()))?;
                }
            }
        }
        // Beside the page, where the page-relative URL reaches them (ADR-0384 §3).
        crate::build::assets::stage(&assets, dist)?;
        let size_bytes = std::fs::read_dir(&pkg)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wasm"))
            .filter_map(|entry| entry.metadata().ok())
            .map(|metadata| metadata.len())
            .sum();
        crate::ui::debug(format!("web build in {}", dist.display()));
        Ok(FinalArtifacts {
            app_binary: index,
            size_bytes,
        })
    }
}

/// The page a web build's context states.
fn web_platform_page(ctx: &BuilderContext) -> BuildResult<&web_page::WebPage> {
    match &ctx.platform {
        crate::build::platform::Platform::Web { page } => Ok(page),
        _ => Err(BuildError::InvalidPlatform {
            reason: "expected the web platform".to_string(),
        }),
    }
}

/// The page that scaffolds wrote before the build generated it, which a
/// project may keep only as its template.
const OLD_PAGE: &str = "platforms/web/index.html";

/// Where a web build's page comes from.
#[derive(Debug, PartialEq, Eq)]
enum PageSource {
    /// The page the build generates (`web_page::render`).
    Generated,
    /// The project's template, `[web] page`, with its text.
    Template { path: PathBuf, text: String },
}

/// The page source of the project at `project` whose `flui.toml` states
/// `page`. A project's own page is never overridden in silence: one at
/// [`OLD_PAGE`] that `[web] page` does not name stops the build with the two
/// ways on.
///
/// # Errors
/// [`BuildError::InvalidConfig`] for a page at [`OLD_PAGE`] that is not the
/// template, a template that cannot be read, or one without the module's
/// placeholder.
fn page_source(project: &Path, page: &web_page::WebPage) -> BuildResult<PageSource> {
    if let Some(template) = &page.template {
        let path = project.join(template);
        let text = std::fs::read_to_string(&path).map_err(|error| {
            BuildError::invalid_config("[web] page", format!("{}: {error}", path.display()))
        })?;
        web_page::render_template(&text, "", page)
            .map_err(|error| BuildError::invalid_config("[web] page", error.to_string()))?;
        return Ok(PageSource::Template { path, text });
    }
    if project.join(OLD_PAGE).is_file() {
        return Err(BuildError::invalid_config(
            OLD_PAGE,
            format!(
                "`flui build` generates the page on every build now. Delete {OLD_PAGE}, or keep \
                 it as the page template with `page = \"{OLD_PAGE}\"` under `[web]` in \
                 flui.toml; a template holds `{}` where the app starts, and may hold `{}`, `{}` \
                 and `{}`",
                web_page::Placeholder::Module.token(),
                web_page::Placeholder::Base.token(),
                web_page::Placeholder::Manifest.token(),
                web_page::Placeholder::Stylesheet.token(),
            ),
        ));
    }
    Ok(PageSource::Generated)
}

/// The `wasm-bindgen` crate version the project resolves to for wasm32, or
/// `None` when the project does not depend on it at all.
fn resolved_wasm_bindgen_version(root: &Path) -> BuildResult<Option<String>> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .other_options(vec![
            "--filter-platform".to_string(),
            WASM_TARGET.to_string(),
        ])
        .exec()
        .map_err(|error| BuildError::Other(format!("cargo metadata: {error}")))?;
    Ok(metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "wasm-bindgen")
        .map(|package| package.version.to_string()))
}

/// `wasm-bindgen --version` is `wasm-bindgen 0.2.100`.
fn installed_wasm_bindgen_version() -> Option<String> {
    let output = probe_stdout(
        std::process::Command::new("wasm-bindgen").arg("--version"),
        PROBE_TIMEOUT,
    )?;
    output.split_whitespace().last().map(str::to_string)
}

/// `Ok` when the installed tool can process what the project compiles;
/// otherwise the exact `cargo install` that fixes it.
fn check_wasm_bindgen(wanted: Option<&str>, installed: Option<&str>) -> Result<(), String> {
    let install = |version: Option<&str>| match version {
        Some(version) => format!("cargo install wasm-bindgen-cli --version {version} --locked"),
        None => "cargo install wasm-bindgen-cli --locked".to_string(),
    };
    match (wanted, installed) {
        (_, None) => Err(install(wanted)),
        (Some(wanted), Some(installed)) if wanted != installed => Err(install(Some(wanted))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project keeps no page, keeps the old scaffolded one, or names its
    /// page as its template: generated, refused with the two ways on, or the
    /// template, which must start the app.
    #[test]
    fn a_project_page_is_never_overridden_in_silence() {
        let project = tempfile::tempdir().expect("a project");
        let plain = web_page::WebPage::default();
        assert_eq!(
            page_source(project.path(), &plain).expect("no page"),
            PageSource::Generated
        );

        std::fs::create_dir_all(project.path().join("platforms/web")).expect("web dir");
        std::fs::write(
            project.path().join(OLD_PAGE),
            "<body>{{flui_module}}</body>",
        )
        .expect("old page");
        let message = page_source(project.path(), &plain)
            .expect_err("an old page")
            .to_string();
        assert!(
            message.contains("Delete") && message.contains("page = \"platforms/web/index.html\""),
            "{message}"
        );

        let template = web_page::WebPage {
            template: Some(PathBuf::from(OLD_PAGE)),
            ..web_page::WebPage::default()
        };
        assert_eq!(
            page_source(project.path(), &template).expect("the template"),
            PageSource::Template {
                path: project.path().join(OLD_PAGE),
                text: "<body>{{flui_module}}</body>".to_owned(),
            }
        );
        std::fs::write(project.path().join(OLD_PAGE), "<body></body>").expect("no module");
        assert!(
            page_source(project.path(), &template)
                .expect_err("a template without the module")
                .to_string()
                .contains("flui_module")
        );
    }
}
