//! `cargo xtask icon-import`: vector animations into `AnimatedIconData` constants.
//!
//! - `svg` reads key frames from SVG files, one frame a file or one `<g id="frame-…">` group a
//!   frame in a single file, frames evenly spaced in time. Every frame must draw the same
//!   filled paths with the same commands, so the points can move from frame to frame.
//! - `avd` reads an Android AnimatedVectorDrawable with its vector inline: `pathData` and
//!   `fillAlpha` animators become key frames at every animator's start and end.
//! - `flutter` turns the Material animated icons Flutter generated (`animated_icons/data/*.g.dart`,
//!   BSD-3-Clause) into multi-frame SVG files under `packages/flui-material/assets/animated_icons`,
//!   so the bundled pairs come from a pinned upstream revision by a command anyone can rerun.
//! - `material` checks (or `--write`s) the generated `flui-material` constants against those SVG
//!   files, so an edit to either fails `cargo xtask checks` until they agree.
//!
//! The output is Rust source: one `AnimatedIconData` constant built with `from_static`, whose
//! data `AnimatedIconData::check` accepts.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, anyhow, bail, ensure};
use kurbo::{BezPath, PathEl};

use crate::util::repo_root;

/// Where the bundled Material icons' SVG frames live.
const ASSETS: &str = "packages/flui-material/assets/animated_icons";
/// Where their generated constants live.
const GENERATED: &str = "packages/flui-material/src/animated_icons/data";
/// Flutter's generated animated icon data, relative to a Flutter checkout.
const FLUTTER_DATA: &str = "packages/flutter/lib/src/material/animated_icons/data";

/// The bundled pairs, by file stem.
const MATERIAL: [&str; 14] = [
    "add_event",
    "arrow_menu",
    "close_menu",
    "ellipsis_search",
    "event_add",
    "home_menu",
    "list_view",
    "menu_arrow",
    "menu_close",
    "menu_home",
    "pause_play",
    "play_pause",
    "search_ellipsis",
    "view_list",
];

#[derive(Debug, clap::Args)]
pub(crate) struct IconImportArgs {
    #[command(subcommand)]
    command: IconImportCommand,
}

#[derive(Debug, clap::Subcommand)]
enum IconImportCommand {
    /// Key frames from SVG files: one file a frame, or one file with `frame-…` groups.
    Svg {
        /// The frames, in order.
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Key frames from an Android AnimatedVectorDrawable with its vector inline.
    Avd {
        /// The `animated-vector` XML file.
        file: PathBuf,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Write the Material icons' SVG frames from a Flutter checkout's generated data.
    Flutter {
        /// The root of a `flutter/flutter` checkout.
        #[arg(long)]
        flutter: PathBuf,
    },
    /// Check (or `--write`) the generated Material constants against their SVG frames.
    Material {
        /// Regenerate the constants instead of checking them.
        #[arg(long)]
        write: bool,
    },
}

#[derive(Debug, clap::Args)]
struct OutputArgs {
    /// The constant's name, in SCREAMING_SNAKE_CASE.
    #[arg(long)]
    name: String,
    /// Mirror the icon under right-to-left text.
    #[arg(long)]
    mirror: bool,
    /// The path that names `AnimatedIconData`, `IconCommand` and `IconPath` in the output.
    #[arg(long, default_value = "flui::widgets")]
    widgets: String,
    /// Write here instead of standard output.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// One drawing command, with its coordinates at every frame.
#[derive(Debug, Clone, PartialEq)]
enum Command {
    MoveTo(Vec<[f64; 2]>),
    LineTo(Vec<[f64; 2]>),
    CubicTo(Vec<[f64; 6]>),
    Close,
}

/// One filled path, at every frame.
#[derive(Debug, Clone, PartialEq)]
struct IconPath {
    opacities: Vec<f64>,
    commands: Vec<Command>,
}

/// An animated icon: a square view box and its paths at evenly or explicitly spaced frames.
#[derive(Debug, Clone, PartialEq)]
struct Icon {
    view_box: f64,
    times: Vec<f64>,
    paths: Vec<IconPath>,
    mirror: bool,
}

/// A path's commands at one frame.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    MoveTo([f64; 2]),
    LineTo([f64; 2]),
    CubicTo([f64; 6]),
    Close,
}

/// `cargo xtask icon-import`.
pub(crate) fn icon_import(args: &IconImportArgs) -> anyhow::Result<ExitCode> {
    match &args.command {
        IconImportCommand::Svg { files, output } => {
            let frames = svg_frames(files)?;
            let icon = assemble(frames, None, output.mirror)?;
            emit(&icon, output)?;
        }
        IconImportCommand::Avd { file, output } => {
            let text = std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?;
            let (frames, times) = avd_frames(&text)?;
            let icon = assemble(frames, Some(times), output.mirror)?;
            emit(&icon, output)?;
        }
        IconImportCommand::Flutter { flutter } => {
            let root = repo_root();
            let revision = git_head(flutter)?;
            for name in MATERIAL {
                let source = flutter.join(FLUTTER_DATA).join(format!("{name}.g.dart"));
                let text = std::fs::read_to_string(&source)
                    .with_context(|| format!("reading {}", source.display()))?;
                let icon = parse_flutter(&text).with_context(|| format!("parsing {name}"))?;
                let svg = write_svg(&icon, name, &revision);
                let out = root.join(ASSETS).join(format!("{name}.svg"));
                std::fs::write(&out, svg).with_context(|| format!("writing {}", out.display()))?;
            }
            println!(
                "icon-import: wrote {} icons from flutter/flutter@{revision}",
                MATERIAL.len()
            );
        }
        IconImportCommand::Material { write } => return material(*write),
    }
    Ok(ExitCode::SUCCESS)
}

/// Checks or writes every bundled constant from its SVG frames.
fn material(write: bool) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let mut stale = Vec::new();
    for name in MATERIAL {
        let svg = root.join(ASSETS).join(format!("{name}.svg"));
        let frames = svg_frames(std::slice::from_ref(&svg))?;
        let text = std::fs::read_to_string(&svg)?;
        let mirror = text.contains("data-flui-match-text-direction=\"true\"");
        let icon = assemble(frames, None, mirror).with_context(|| format!("assembling {name}"))?;
        let rust = rust_source(
            &icon,
            "DATA",
            "flui_sdk::widgets",
            &format!("assets/animated_icons/{name}.svg"),
        );
        let out = root.join(GENERATED).join(format!("{name}.rs"));
        if write {
            std::fs::write(&out, rust).with_context(|| format!("writing {}", out.display()))?;
        } else if std::fs::read_to_string(&out).ok().as_deref() != Some(rust.as_str()) {
            stale.push(out);
        }
    }
    if stale.is_empty() {
        println!(
            "icon-import: {} Material icons match their frames",
            MATERIAL.len()
        );
        Ok(ExitCode::SUCCESS)
    } else {
        for path in &stale {
            eprintln!("icon-import: {} is stale", path.display());
        }
        eprintln!("icon-import: run `cargo xtask icon-import material --write`");
        Ok(ExitCode::FAILURE)
    }
}

fn git_head(dir: &Path) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("running git rev-parse")?;
    ensure!(
        output.status.success(),
        "{} is not a git checkout",
        dir.display()
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn emit(icon: &Icon, output: &OutputArgs) -> anyhow::Result<()> {
    let rust = rust_source(
        icon,
        &output.name,
        &output.widgets,
        "cargo xtask icon-import",
    );
    if let Some(path) = &output.out {
        std::fs::write(path, rust).with_context(|| format!("writing {}", path.display()))
    } else {
        print!("{rust}");
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Frames to an icon
// ---------------------------------------------------------------------------------------------

/// One frame: the view box and each path's opacity and steps.
type Frame = (f64, Vec<(f64, Vec<Step>)>);

/// Joins frames into one icon, checking that every frame draws the same paths with the same
/// commands. `times` defaults to evenly spaced frames.
fn assemble(frames: Vec<Frame>, times: Option<Vec<f64>>, mirror: bool) -> anyhow::Result<Icon> {
    ensure!(!frames.is_empty(), "no frames");
    // A single frame is a still icon: two identical frames, at 0 and at 1.
    let frames = if frames.len() == 1 {
        vec![frames[0].clone(), frames[0].clone()]
    } else {
        frames
    };
    let n = frames.len();
    let times = times.unwrap_or_else(|| (0..n).map(|i| i as f64 / (n - 1) as f64).collect());
    ensure!(times.len() == n, "{} times for {n} frames", times.len());
    let view_box = frames[0].0;
    let first = &frames[0].1;
    for (index, (box_, paths)) in frames.iter().enumerate() {
        ensure!(*box_ == view_box, "frame {index} has another view box");
        ensure!(
            paths.len() == first.len(),
            "frame {index} draws {} paths, the first frame {}",
            paths.len(),
            first.len()
        );
        for (p, ((_, steps), (_, first_steps))) in paths.iter().zip(first).enumerate() {
            let kinds =
                |steps: &[Step]| steps.iter().map(std::mem::discriminant).collect::<Vec<_>>();
            if kinds(steps) != kinds(first_steps) {
                bail!(
                    "path {p} of frame {index} has other commands than in the first frame: \
                     the paths are not compatible"
                );
            }
        }
    }
    let paths = (0..first.len())
        .map(|p| {
            let opacities = frames.iter().map(|(_, paths)| paths[p].0).collect();
            let commands = (0..first[p].1.len())
                .map(|c| match first[p].1[c] {
                    Step::MoveTo(_) => Command::MoveTo(column(&frames, p, c, |s| match s {
                        Step::MoveTo(point) => *point,
                        _ => unreachable!("BUG: commands were checked compatible"),
                    })),
                    Step::LineTo(_) => Command::LineTo(column(&frames, p, c, |s| match s {
                        Step::LineTo(point) => *point,
                        _ => unreachable!("BUG: commands were checked compatible"),
                    })),
                    Step::CubicTo(_) => Command::CubicTo(column(&frames, p, c, |s| match s {
                        Step::CubicTo(points) => *points,
                        _ => unreachable!("BUG: commands were checked compatible"),
                    })),
                    Step::Close => Command::Close,
                })
                .collect();
            IconPath {
                opacities,
                commands,
            }
        })
        .collect();
    let icon = Icon {
        view_box,
        times,
        paths,
        mirror,
    };
    for path in &icon.paths {
        ensure!(
            path.opacities.iter().all(|o| (0.0..=1.0).contains(o)),
            "an opacity is outside [0, 1]"
        );
    }
    Ok(icon)
}

fn column<T>(frames: &[Frame], path: usize, command: usize, pick: impl Fn(&Step) -> T) -> Vec<T> {
    frames
        .iter()
        .map(|(_, paths)| pick(&paths[path].1[command]))
        .collect()
}

/// The steps of SVG path data, absolute, with quadratic curves raised to cubics.
fn steps(data: &str) -> anyhow::Result<Vec<Step>> {
    let path = BezPath::from_svg(data).map_err(|error| anyhow!("path data: {error}"))?;
    let mut steps = Vec::new();
    let mut current = kurbo::Point::ZERO;
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(p) => {
                steps.push(Step::MoveTo([p.x, p.y]));
                current = p;
            }
            PathEl::LineTo(p) => {
                steps.push(Step::LineTo([p.x, p.y]));
                current = p;
            }
            PathEl::QuadTo(c, p) => {
                let c1 = current + (c - current) * (2.0 / 3.0);
                let c2 = p + (c - p) * (2.0 / 3.0);
                steps.push(Step::CubicTo([c1.x, c1.y, c2.x, c2.y, p.x, p.y]));
                current = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                steps.push(Step::CubicTo([c1.x, c1.y, c2.x, c2.y, p.x, p.y]));
                current = p;
            }
            PathEl::ClosePath => steps.push(Step::Close),
        }
    }
    Ok(steps)
}

// ---------------------------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------------------------

/// The frames of SVG files: each file's `frame-…` groups in order, or the file itself.
fn svg_frames(files: &[PathBuf]) -> anyhow::Result<Vec<Frame>> {
    let mut frames = Vec::new();
    for file in files {
        let text =
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
        frames.extend(parse_svg(&text).with_context(|| format!("parsing {}", file.display()))?);
    }
    Ok(frames)
}

fn parse_svg(text: &str) -> anyhow::Result<Vec<Frame>> {
    let document = roxmltree::Document::parse(text)?;
    let root = document.root_element();
    ensure!(root.has_tag_name("svg"), "the root is not <svg>");
    let view_box = match root.attribute("viewBox") {
        Some(view_box) => {
            let values: Vec<f64> = view_box
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(str::parse)
                .collect::<Result<_, _>>()?;
            ensure!(values.len() == 4, "viewBox needs four numbers");
            ensure!(
                values[0] == 0.0 && values[1] == 0.0 && values[2] == values[3],
                "the viewBox must be a square at the origin"
            );
            values[2]
        }
        None => root
            .attribute("width")
            .ok_or_else(|| anyhow!("no viewBox or width"))?
            .trim_end_matches("px")
            .parse()?,
    };
    let groups: Vec<_> = root
        .children()
        .filter(|node| {
            node.has_tag_name("g")
                && node
                    .attribute("id")
                    .is_some_and(|id| id.starts_with("frame-"))
        })
        .collect();
    let paths_of = |node: roxmltree::Node<'_, '_>| -> anyhow::Result<Vec<(f64, Vec<Step>)>> {
        node.descendants()
            .filter(|n| n.has_tag_name("path"))
            .map(|path| {
                ensure!(
                    path.attribute("transform").is_none(),
                    "path transforms are not supported: apply them to the path data"
                );
                let data = path
                    .attribute("d")
                    .ok_or_else(|| anyhow!("a path without d"))?;
                let opacity = path
                    .attribute("fill-opacity")
                    .or_else(|| path.attribute("opacity"))
                    .map_or(Ok(1.0), str::parse)?;
                Ok((opacity, steps(data)?))
            })
            .collect()
    };
    if groups.is_empty() {
        Ok(vec![(view_box, paths_of(root)?)])
    } else {
        groups
            .into_iter()
            .map(|group| Ok((view_box, paths_of(group)?)))
            .collect()
    }
}

/// A multi-frame SVG: one `frame-NN` group a frame.
fn write_svg(icon: &Icon, name: &str, revision: &str) -> String {
    let mut svg = String::new();
    let side = number(icon.view_box);
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {side} {side}\" \
         data-flui-match-text-direction=\"{}\">",
        icon.mirror
    );
    let _ = writeln!(
        svg,
        "<!-- {name}: key frames of Flutter's Material animated icon data \
         (flutter/flutter@{revision}, {FLUTTER_DATA}/{name}.g.dart, BSD-3-Clause; see \
         LICENSE-flutter.txt). Generated by `cargo xtask icon-import flutter`. -->"
    );
    for frame in 0..icon.times.len() {
        let _ = writeln!(svg, "<g id=\"frame-{frame:02}\">");
        for path in &icon.paths {
            let mut data = String::new();
            for command in &path.commands {
                let part = match command {
                    Command::MoveTo(p) => {
                        format!("M{} {}", number(p[frame][0]), number(p[frame][1]))
                    }
                    Command::LineTo(p) => {
                        format!("L{} {}", number(p[frame][0]), number(p[frame][1]))
                    }
                    Command::CubicTo(p) => {
                        let c = p[frame];
                        format!(
                            "C{} {} {} {} {} {}",
                            number(c[0]),
                            number(c[1]),
                            number(c[2]),
                            number(c[3]),
                            number(c[4]),
                            number(c[5])
                        )
                    }
                    Command::Close => "Z".to_owned(),
                };
                if !data.is_empty() {
                    data.push(' ');
                }
                data.push_str(&part);
            }
            let _ = writeln!(
                svg,
                "<path d=\"{data}\" fill-opacity=\"{}\"/>",
                number(path.opacities[frame])
            );
        }
        let _ = writeln!(svg, "</g>");
    }
    svg.push_str("</svg>\n");
    svg
}

// ---------------------------------------------------------------------------------------------
// Flutter's generated data
// ---------------------------------------------------------------------------------------------

/// Parses one of Flutter's `animated_icons/data/*.g.dart` files.
fn parse_flutter(text: &str) -> anyhow::Result<Icon> {
    let number =
        regex::Regex::new(r"-?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?").expect("BUG: a valid regex");
    let offset = regex::Regex::new(r"Offset\(\s*([^,\s]+)\s*,\s*([^)\s]+)\s*\)")
        .expect("BUG: a valid regex");
    let command =
        regex::Regex::new(r"_Path(MoveTo|LineTo|CubicTo|Close)\(").expect("BUG: a valid regex");
    let size = regex::Regex::new(r"_AnimatedIconData\(\s*Size\(\s*([^,\s]+)\s*,\s*([^)\s]+)\s*\)")
        .expect("BUG: a valid regex");
    let captures = size
        .captures(text)
        .ok_or_else(|| anyhow!("no _AnimatedIconData(Size(..))"))?;
    let (width, height): (f64, f64) = (captures[1].parse()?, captures[2].parse()?);
    ensure!(width == height, "the icon is not square");
    let mirror = text.contains("matchTextDirection: true");
    let mut paths = Vec::new();
    for block in text.split("_PathFrames(").skip(1) {
        let opacities_start = block
            .find("opacities:")
            .ok_or_else(|| anyhow!("a path without opacities"))?;
        let list = &block[opacities_start..];
        let open = list
            .find('[')
            .ok_or_else(|| anyhow!("opacities without a list"))?;
        let close = list
            .find(']')
            .ok_or_else(|| anyhow!("an unclosed opacity list"))?;
        let opacities = number
            .find_iter(&list[open + 1..close])
            .map(|m| m.as_str().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()?;
        let frames = opacities.len();
        let starts: Vec<_> = command
            .captures_iter(block)
            .map(|c| c.get(0).expect("BUG: a match").start())
            .collect();
        let mut commands = Vec::new();
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(block.len());
            let body = &block[start..end];
            let kind = &command
                .captures(body)
                .expect("BUG: the command matched here")[1];
            let points = offset
                .captures_iter(body)
                .map(|c| Ok::<_, anyhow::Error>([c[1].parse::<f64>()?, c[2].parse::<f64>()?]))
                .collect::<Result<Vec<_>, _>>()?;
            commands.push(match kind {
                "MoveTo" | "LineTo" => {
                    ensure!(
                        points.len() == frames,
                        "{kind} has {} points for {frames} frames",
                        points.len()
                    );
                    if kind == "MoveTo" {
                        Command::MoveTo(points)
                    } else {
                        Command::LineTo(points)
                    }
                }
                "CubicTo" => {
                    ensure!(
                        points.len() == 3 * frames,
                        "a cubic has {} points for {frames} frames",
                        points.len()
                    );
                    Command::CubicTo(
                        (0..frames)
                            .map(|f| {
                                let (c1, c2, p) =
                                    (points[f], points[frames + f], points[2 * frames + f]);
                                [c1[0], c1[1], c2[0], c2[1], p[0], p[1]]
                            })
                            .collect(),
                    )
                }
                _ => Command::Close,
            });
        }
        paths.push(IconPath {
            opacities,
            commands,
        });
    }
    ensure!(!paths.is_empty(), "no paths");
    let frames = paths[0].opacities.len();
    ensure!(frames >= 2, "fewer than two frames");
    Ok(Icon {
        view_box: width,
        times: (0..frames)
            .map(|i| i as f64 / (frames - 1) as f64)
            .collect(),
        paths,
        mirror,
    })
}

// ---------------------------------------------------------------------------------------------
// AnimatedVectorDrawable
// ---------------------------------------------------------------------------------------------

const ANDROID: &str = "http://schemas.android.com/apk/res/android";

/// One animator of an AVD target: a property from a value to a value over a time span.
#[derive(Debug, Clone)]
struct Animator {
    target: String,
    property: String,
    from: String,
    to: String,
    start: f64,
    end: f64,
}

/// The frames of an AVD: every path at the start and end of every animator, and the frame
/// times as shares of the whole animation.
fn avd_frames(text: &str) -> anyhow::Result<(Vec<Frame>, Vec<f64>)> {
    let document = roxmltree::Document::parse(text)?;
    let root = document.root_element();
    ensure!(
        root.has_tag_name("animated-vector"),
        "the root is not <animated-vector>"
    );
    let vector = root
        .descendants()
        .find(|n| n.has_tag_name("vector"))
        .ok_or_else(|| {
            anyhow!("the vector must be inline (<aapt:attr name=\"android:drawable\">)")
        })?;
    let width: f64 = attr(vector, "viewportWidth")?.parse()?;
    let height: f64 = attr(vector, "viewportHeight")?.parse()?;
    ensure!(width == height, "the viewport is not square");
    ensure!(
        !vector
            .descendants()
            .any(|n| n.has_tag_name("group") && n.attributes().any(|a| a.name() != "name")),
        "group transforms are not supported: apply them to the path data"
    );
    let paths: Vec<_> = vector
        .descendants()
        .filter(|n| n.has_tag_name("path"))
        .collect();
    let mut animators = Vec::new();
    for target in root.children().filter(|n| n.has_tag_name("target")) {
        let name = attr(target, "name")?.to_owned();
        for animator in target
            .descendants()
            .filter(|n| n.has_tag_name("objectAnimator"))
        {
            if let Some(interpolator) = animator.attribute((ANDROID, "interpolator"))
                && !interpolator.contains("linear")
            {
                eprintln!(
                    "icon-import: {name}: interpolator {interpolator} is drawn linearly between key frames"
                );
            }
            let start: f64 = animator
                .attribute((ANDROID, "startOffset"))
                .unwrap_or("0")
                .parse()?;
            let duration: f64 = animator
                .attribute((ANDROID, "duration"))
                .unwrap_or("300")
                .parse()?;
            animators.push(Animator {
                target: name.clone(),
                property: attr(animator, "propertyName")?.to_owned(),
                from: attr(animator, "valueFrom")?.to_owned(),
                to: attr(animator, "valueTo")?.to_owned(),
                start,
                end: start + duration,
            });
        }
    }
    let total = animators.iter().map(|a| a.end).fold(0.0, f64::max);
    ensure!(total > 0.0, "no animator runs for any time");
    let mut times: Vec<f64> = std::iter::once(0.0)
        .chain(animators.iter().flat_map(|a| [a.start, a.end]))
        .chain(std::iter::once(total))
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    let mut frames = Vec::new();
    for &time in &times {
        let mut frame = Vec::new();
        for path in &paths {
            let name = path.attribute((ANDROID, "name")).unwrap_or("");
            let mut data =
                steps(attr(*path, "pathData")?).with_context(|| format!("path {name}"))?;
            let mut alpha: f64 = path
                .attribute((ANDROID, "fillAlpha"))
                .unwrap_or("1")
                .parse()?;
            for animator in animators.iter().filter(|a| a.target == name) {
                // Before its start an animator holds its first value, after its end its last,
                // and in between it moves linearly.
                let share = if animator.end > animator.start {
                    ((time - animator.start) / (animator.end - animator.start)).clamp(0.0, 1.0)
                } else if time >= animator.end {
                    1.0
                } else {
                    0.0
                };
                match animator.property.as_str() {
                    "pathData" => {
                        let from =
                            steps(&animator.from).with_context(|| format!("{name} valueFrom"))?;
                        let to = steps(&animator.to).with_context(|| format!("{name} valueTo"))?;
                        data = lerp_steps(&from, &to, share).with_context(|| {
                            format!("{name}: valueFrom and valueTo are not compatible")
                        })?;
                    }
                    "fillAlpha" => {
                        let (from, to): (f64, f64) = (animator.from.parse()?, animator.to.parse()?);
                        alpha = from + (to - from) * share;
                    }
                    other => bail!("{name}: animating {other} is not supported"),
                }
            }
            frame.push((alpha, data));
        }
        frames.push((width, frame));
    }
    let times = times.iter().map(|t| t / total).collect();
    Ok((frames, times))
}

/// The steps `share` of the way from `from` to `to`, which must have the same commands.
fn lerp_steps(from: &[Step], to: &[Step], share: f64) -> anyhow::Result<Vec<Step>> {
    ensure!(from.len() == to.len(), "different numbers of commands");
    let lerp = |a: f64, b: f64| a + (b - a) * share;
    from.iter()
        .zip(to)
        .map(|pair| match pair {
            (Step::MoveTo(a), Step::MoveTo(b)) => {
                Ok(Step::MoveTo([lerp(a[0], b[0]), lerp(a[1], b[1])]))
            }
            (Step::LineTo(a), Step::LineTo(b)) => {
                Ok(Step::LineTo([lerp(a[0], b[0]), lerp(a[1], b[1])]))
            }
            (Step::CubicTo(a), Step::CubicTo(b)) => {
                Ok(Step::CubicTo(std::array::from_fn(|i| lerp(a[i], b[i]))))
            }
            (Step::Close, Step::Close) => Ok(Step::Close),
            _ => bail!("different commands"),
        })
        .collect()
}
fn attr<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> anyhow::Result<&'a str> {
    node.attribute((ANDROID, name))
        .ok_or_else(|| anyhow!("<{}> lacks android:{name}", node.tag_name().name()))
}

// ---------------------------------------------------------------------------------------------
// Rust output
// ---------------------------------------------------------------------------------------------

/// The shortest decimal that reads back as `value`, always with a decimal point.
fn number(value: f64) -> String {
    let text = format!("{value:?}");
    if text.contains('.') || text.contains('e') || text.contains("inf") || text.contains("NaN") {
        text
    } else {
        format!("{text}.0")
    }
}

/// The Rust source of `icon` as the constant `name`.
fn rust_source(icon: &Icon, name: &str, widgets: &str, source: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Generated by `cargo xtask icon-import` from {source}; do not edit."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "use {widgets}::{{AnimatedIconData, IconCommand, IconPath}};"
    );
    let _ = writeln!(out);
    let list = |values: &mut dyn Iterator<Item = String>| values.collect::<Vec<_>>().join(", ");
    let _ = writeln!(out, "#[rustfmt::skip]");
    let _ = writeln!(
        out,
        "pub(crate) const {name}: AnimatedIconData = AnimatedIconData::from_static("
    );
    let _ = writeln!(out, "    {},", number(icon.view_box));
    let _ = writeln!(
        out,
        "    &[{}],",
        list(&mut icon.times.iter().map(|t| number(*t)))
    );
    let _ = writeln!(out, "    &[],");
    let _ = writeln!(out, "    &[");
    for path in &icon.paths {
        let _ = writeln!(out, "        IconPath::new(");
        let _ = writeln!(
            out,
            "            &[{}],",
            list(&mut path.opacities.iter().map(|o| number(*o)))
        );
        let _ = writeln!(out, "            &[");
        for command in &path.commands {
            let line = match command {
                Command::MoveTo(points) | Command::LineTo(points) => {
                    let kind = if matches!(command, Command::MoveTo(_)) {
                        "move_to"
                    } else {
                        "line_to"
                    };
                    let points = list(
                        &mut points
                            .iter()
                            .map(|p| format!("[{}, {}]", number(p[0]), number(p[1]))),
                    );
                    format!("IconCommand::{kind}(&[{points}]),")
                }
                Command::CubicTo(points) => {
                    let points = list(
                        &mut points
                            .iter()
                            .map(|p| format!("[{}]", list(&mut p.iter().map(|v| number(*v))))),
                    );
                    format!("IconCommand::cubic_to(&[{points}]),")
                }
                Command::Close => "IconCommand::close(),".to_owned(),
            };
            let _ = writeln!(out, "                {line}");
        }
        let _ = writeln!(out, "            ],");
        let _ = writeln!(out, "        ),");
    }
    let _ = writeln!(out, "    ],");
    let _ = writeln!(out, "    {},", icon.mirror);
    let _ = writeln!(out, ");");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AVD: &str = r#"<animated-vector xmlns:android="http://schemas.android.com/apk/res/android"
        xmlns:aapt="http://schemas.android.com/aapt">
      <aapt:attr name="android:drawable">
        <vector android:width="24dp" android:height="24dp"
                android:viewportWidth="24" android:viewportHeight="24">
          <path android:name="bar" android:pathData="M4 4 L20 4 L20 8 L4 8 Z"/>
        </vector>
      </aapt:attr>
      <target android:name="bar">
        <aapt:attr name="android:animation">
          <objectAnimator android:propertyName="pathData" android:duration="200"
              android:valueFrom="M4 4 L20 4 L20 8 L4 8 Z"
              android:valueTo="M4 10 L20 10 L20 14 L4 14 Z" android:valueType="pathType"/>
        </aapt:attr>
      </target>
    </animated-vector>"#;

    #[test]
    fn an_avd_becomes_two_frames_of_one_path() {
        let (frames, times) = avd_frames(AVD).expect("a valid AVD");
        let icon = assemble(frames, Some(times), false).expect("compatible frames");
        assert_eq!(icon.view_box, 24.0);
        assert_eq!(icon.times, [0.0, 1.0]);
        assert_eq!(icon.paths.len(), 1);
        assert_eq!(
            icon.paths[0].commands[0],
            Command::MoveTo(vec![[4.0, 4.0], [4.0, 10.0]])
        );
        let rust = rust_source(&icon, "BAR", "flui::widgets", "test");
        assert!(
            rust.contains("IconCommand::move_to(&[[4.0, 4.0], [4.0, 10.0]]),"),
            "{rust}"
        );
    }

    #[test]
    fn incompatible_svg_frames_are_refused() {
        let a = parse_svg(r#"<svg viewBox="0 0 24 24"><path d="M0 0 L10 0 L10 10 Z"/></svg>"#)
            .expect("valid");
        let b = parse_svg(r#"<svg viewBox="0 0 24 24"><path d="M0 0 C1 1 2 2 3 3 Z"/></svg>"#)
            .expect("valid");
        let error = assemble([a, b].concat(), None, false).expect_err("different commands");
        assert!(error.to_string().contains("not compatible"), "{error}");
    }

    #[test]
    fn flutter_data_round_trips_through_svg() {
        let dart = "const _AnimatedIconData _$x = _AnimatedIconData(Size(48.0, 48.0), <_PathFrames>[\n\
            _PathFrames(opacities: <double>[1.0, 0.5], commands: <_PathCommand>[\n\
            _PathMoveTo(<Offset>[Offset(1.0, 2.0), Offset(3.0, 4.0)]),\n\
            _PathCubicTo(<Offset>[Offset(1.0, 1.0), Offset(2.0, 2.0)], <Offset>[Offset(3.0, 3.0), Offset(4.0, 4.0)], <Offset>[Offset(5.0, 5.0), Offset(6.0, 6.0)]),\n\
            _PathClose(),\n]),\n], matchTextDirection: true);";
        let icon = parse_flutter(dart).expect("valid data");
        assert!(icon.mirror);
        assert_eq!(
            icon.paths[0].commands[1],
            Command::CubicTo(vec![
                [1.0, 1.0, 3.0, 3.0, 5.0, 5.0],
                [2.0, 2.0, 4.0, 4.0, 6.0, 6.0]
            ])
        );
        let svg = write_svg(&icon, "x", "0000");
        let back = assemble(parse_svg(&svg).expect("our own SVG"), None, true).expect("compatible");
        assert_eq!(back, icon);
    }
}
