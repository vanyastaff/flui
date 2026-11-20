//! The bundled Material animated icons are generated: the SVG key frames in
//! `assets/animated_icons` hold each pair, and `src/animated_icons/data` holds one
//! `AnimatedIconData` constant per pair, built with `from_static`.
//!
//! `material_icon_constants_match_their_frames` regenerates every constant from its frames and
//! fails while a committed constant differs; with `FLUI_BLESS=1` it writes them instead. With
//! `FLUI_FLUTTER_DIR=<flutter/flutter checkout>` as well, it first rewrites the frames from that
//! checkout's generated data (`animated_icons/data/*.g.dart`, BSD-3-Clause), naming its
//! revision in each file, so the bundled pairs come from a pinned upstream revision by a
//! command anyone can rerun.
//!
//! Every frame of an icon draws the same filled paths with the same commands, so the points
//! can move from frame to frame; the frames are evenly spaced in time.

use std::fmt::Write as _;
use std::path::Path;

use kurbo::{BezPath, PathEl, Point};

/// Where the bundled icons' SVG frames live, relative to the package root.
const ASSETS: &str = "assets/animated_icons";
/// Where their generated constants live, relative to the package root.
const GENERATED: &str = "src/animated_icons/data";
/// Flutter's generated animated icon data, relative to a Flutter checkout.
const FLUTTER_DATA: &str = "packages/flutter/lib/src/material/animated_icons/data";

/// The command that rewrites the constants; the frames too when `FLUI_FLUTTER_DIR` names a
/// Flutter checkout.
const BLESS_COMMAND: &str = "FLUI_BLESS=1 cargo nextest run -p flui-material --test material_it -E 'test(material_icon_constants_match_their_frames)'";

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

macro_rules! ensure {
    ($condition:expr, $($message:tt)+) => {
        if !$condition {
            return Err(format!($($message)+));
        }
    };
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

/// An animated icon: a square view box and its paths at evenly spaced frames.
#[derive(Debug, Clone, PartialEq)]
struct Icon {
    view_box: f64,
    frames: usize,
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

/// One frame: each path's opacity and steps.
type Frame = Vec<(f64, Vec<Step>)>;

/// Every constant is what its key frames generate. `FLUI_BLESS=1` writes them, after the frames
/// themselves when `FLUI_FLUTTER_DIR` names a Flutter checkout.
#[test]
fn material_icon_constants_match_their_frames() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bless = std::env::var("FLUI_BLESS").is_ok_and(|value| value == "1");
    let flutter = std::env::var_os("FLUI_FLUTTER_DIR");
    assert!(
        bless || flutter.is_none(),
        "FLUI_FLUTTER_DIR rewrites the key frames only together with FLUI_BLESS=1"
    );
    if let Some(flutter) = flutter {
        write_frames_from_flutter(package, Path::new(&flutter));
    }
    let mut stale = Vec::new();
    for name in MATERIAL {
        let source = format!("{ASSETS}/{name}.svg");
        let frames = package.join(&source);
        let text = std::fs::read_to_string(&frames)
            .unwrap_or_else(|error| panic!("reading {}: {error}", frames.display()));
        let icon = parse_svg(&text).unwrap_or_else(|error| panic!("{}: {error}", frames.display()));
        let rust = rust_source(&icon, &source);
        let generated = format!("{GENERATED}/{name}.rs");
        let output = package.join(&generated);
        if bless {
            std::fs::write(&output, rust)
                .unwrap_or_else(|error| panic!("writing {}: {error}", output.display()));
        } else if std::fs::read_to_string(&output).ok().as_deref() != Some(rust.as_str()) {
            stale.push(generated);
        }
    }
    assert!(
        stale.is_empty(),
        "{} not what the key frames in {ASSETS} generate; run `{BLESS_COMMAND}`",
        stale.join(", ")
    );
}

/// Rewrites every bundled icon's SVG frames from a Flutter checkout's generated data.
fn write_frames_from_flutter(package: &Path, flutter: &Path) {
    let revision = git_head(flutter);
    for name in MATERIAL {
        let source = flutter.join(FLUTTER_DATA).join(format!("{name}.g.dart"));
        let text = std::fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("reading {}: {error}", source.display()));
        let icon =
            parse_flutter(&text).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
        let output = package.join(ASSETS).join(format!("{name}.svg"));
        std::fs::write(&output, write_svg(&icon, name, &revision))
            .unwrap_or_else(|error| panic!("writing {}: {error}", output.display()));
    }
}

fn git_head(checkout: &Path) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|error| panic!("running git rev-parse in {}: {error}", checkout.display()));
    assert!(
        output.status.success(),
        "{} is not a git checkout",
        checkout.display()
    );
    String::from_utf8(output.stdout)
        .expect("git prints a commit id in ASCII")
        .trim()
        .to_owned()
}

// ---------------------------------------------------------------------------------------------
// Frames to an icon
// ---------------------------------------------------------------------------------------------

/// Joins frames into one icon, checking that every frame draws the same paths with the same
/// commands.
fn assemble(view_box: f64, frames: &[Frame], mirror: bool) -> Result<Icon, String> {
    ensure!(
        frames.len() >= 2,
        "{} frames: an icon needs two",
        frames.len()
    );
    let first = &frames[0];
    let kinds = |steps: &[Step]| steps.iter().map(std::mem::discriminant).collect::<Vec<_>>();
    for (index, paths) in frames.iter().enumerate() {
        ensure!(
            paths.len() == first.len(),
            "frame {index} draws {} paths, the first frame {}",
            paths.len(),
            first.len()
        );
        for (p, ((_, steps), (_, first_steps))) in paths.iter().zip(first).enumerate() {
            ensure!(
                kinds(steps) == kinds(first_steps),
                "path {p} of frame {index} has other commands than in the first frame: \
                 the paths are not compatible"
            );
        }
    }
    let column = |path: usize, command: usize| frames.iter().map(move |f| &f[path].1[command]);
    let paths = first
        .iter()
        .enumerate()
        .map(|(p, (_, steps))| {
            let opacities: Vec<f64> = frames.iter().map(|frame| frame[p].0).collect();
            ensure!(
                opacities.iter().all(|o| (0.0..=1.0).contains(o)),
                "an opacity of path {p} is outside [0, 1]"
            );
            let commands = steps
                .iter()
                .enumerate()
                .map(|(c, step)| match step {
                    Step::MoveTo(_) => Command::MoveTo(
                        column(p, c)
                            .map(|s| match s {
                                Step::MoveTo(point) => *point,
                                _ => unreachable!("the commands were checked compatible"),
                            })
                            .collect(),
                    ),
                    Step::LineTo(_) => Command::LineTo(
                        column(p, c)
                            .map(|s| match s {
                                Step::LineTo(point) => *point,
                                _ => unreachable!("the commands were checked compatible"),
                            })
                            .collect(),
                    ),
                    Step::CubicTo(_) => Command::CubicTo(
                        column(p, c)
                            .map(|s| match s {
                                Step::CubicTo(points) => *points,
                                _ => unreachable!("the commands were checked compatible"),
                            })
                            .collect(),
                    ),
                    Step::Close => Command::Close,
                })
                .collect();
            Ok(IconPath {
                opacities,
                commands,
            })
        })
        .collect::<Result<Vec<IconPath>, String>>()?;
    Ok(Icon {
        view_box,
        frames: frames.len(),
        paths,
        mirror,
    })
}

/// The steps of SVG path data, absolute, with quadratic curves raised to cubics.
fn steps(data: &str) -> Result<Vec<Step>, String> {
    let path = BezPath::from_svg(data).map_err(|error| format!("path data: {error}"))?;
    let mut steps = Vec::new();
    let mut current = Point::ZERO;
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

fn float(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .map_err(|error| format!("{text:?} is not a number: {error}"))
}

// ---------------------------------------------------------------------------------------------
// SVG key frames
// ---------------------------------------------------------------------------------------------

/// An icon from one SVG file whose `<g id="frame-…">` groups are its frames, in order.
fn parse_svg(text: &str) -> Result<Icon, String> {
    let document = roxmltree::Document::parse(text).map_err(|error| error.to_string())?;
    let root = document.root_element();
    ensure!(root.has_tag_name("svg"), "the root is not <svg>");
    let view_box = root.attribute("viewBox").ok_or("no viewBox")?;
    let values = view_box
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(float)
        .collect::<Result<Vec<f64>, String>>()?;
    ensure!(
        values.len() == 4 && values[0] == 0.0 && values[1] == 0.0 && values[2] == values[3],
        "the viewBox {view_box:?} is not a square at the origin"
    );
    let mirror = root.attribute("data-flui-match-text-direction") == Some("true");
    let frames = root
        .children()
        .filter(|node| {
            node.has_tag_name("g")
                && node
                    .attribute("id")
                    .is_some_and(|id| id.starts_with("frame-"))
        })
        .map(|group| {
            group
                .descendants()
                .filter(|node| node.has_tag_name("path"))
                .map(|path| {
                    ensure!(
                        path.attribute("transform").is_none(),
                        "path transforms are not supported: apply them to the path data"
                    );
                    let data = path.attribute("d").ok_or("a path without d")?;
                    let opacity = path.attribute("fill-opacity").map_or(Ok(1.0), float)?;
                    Ok((opacity, steps(data)?))
                })
                .collect::<Result<Frame, String>>()
        })
        .collect::<Result<Vec<Frame>, String>>()?;
    assemble(values[2], &frames, mirror)
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
    // An XML comment cannot hold `--`, so it names the test rather than the command line.
    let _ = writeln!(
        svg,
        "<!-- {name}: key frames of Flutter's Material animated icon data \
         (flutter/flutter@{revision}, {FLUTTER_DATA}/{name}.g.dart, BSD-3-Clause; see \
         LICENSE-flutter.txt). Generated by flui-material's test \
         material_icon_constants_match_their_frames with FLUI_BLESS=1 and \
         FLUI_FLUTTER_DIR=<checkout>. -->"
    );
    for frame in 0..icon.frames {
        let _ = writeln!(svg, "<g id=\"frame-{frame:02}\">");
        for path in &icon.paths {
            let data = path
                .commands
                .iter()
                .map(|command| match command {
                    Command::MoveTo(p) => {
                        format!("M{} {}", number(p[frame][0]), number(p[frame][1]))
                    }
                    Command::LineTo(p) => {
                        format!("L{} {}", number(p[frame][0]), number(p[frame][1]))
                    }
                    Command::CubicTo(p) => format!("C{}", list(" ", &p[frame])),
                    Command::Close => "Z".to_owned(),
                })
                .collect::<Vec<String>>()
                .join(" ");
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
fn parse_flutter(text: &str) -> Result<Icon, String> {
    let number = regex::Regex::new(r"-?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?").expect("a valid regex");
    let offset =
        regex::Regex::new(r"Offset\(\s*([^,\s]+)\s*,\s*([^)\s]+)\s*\)").expect("a valid regex");
    let command =
        regex::Regex::new(r"_Path(MoveTo|LineTo|CubicTo|Close)\(").expect("a valid regex");
    let size = regex::Regex::new(r"_AnimatedIconData\(\s*Size\(\s*([^,\s]+)\s*,\s*([^)\s]+)\s*\)")
        .expect("a valid regex");
    let captures = size
        .captures(text)
        .ok_or("no _AnimatedIconData(Size(..))")?;
    let (width, height) = (float(&captures[1])?, float(&captures[2])?);
    ensure!(width == height, "the icon is not square");
    let mirror = text.contains("matchTextDirection: true");
    let mut paths = Vec::new();
    for block in text.split("_PathFrames(").skip(1) {
        let list = &block[block.find("opacities:").ok_or("a path without opacities")?..];
        let open = list.find('[').ok_or("opacities without a list")?;
        let close = list.find(']').ok_or("an unclosed opacity list")?;
        let opacities = number
            .find_iter(&list[open + 1..close])
            .map(|m| float(m.as_str()))
            .collect::<Result<Vec<f64>, String>>()?;
        let frames = opacities.len();
        let starts: Vec<usize> = command.find_iter(block).map(|m| m.start()).collect();
        let mut commands = Vec::new();
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(block.len());
            let body = &block[start..end];
            let kind = &command
                .captures(body)
                .expect("the command matched at this start")[1];
            let points = offset
                .captures_iter(body)
                .map(|c| Ok([float(&c[1])?, float(&c[2])?]))
                .collect::<Result<Vec<[f64; 2]>, String>>()?;
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
    ensure!(
        paths.iter().all(|path| path.opacities.len() == frames),
        "the paths have different numbers of frames"
    );
    Ok(Icon {
        view_box: width,
        frames,
        paths,
        mirror,
    })
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

/// `values` as numbers joined by `separator`.
fn list(separator: &str, values: &[f64]) -> String {
    values
        .iter()
        .map(|value| number(*value))
        .collect::<Vec<String>>()
        .join(separator)
}

/// The Rust source of `icon` as the constant `DATA`, generated from the frames at `source`.
fn rust_source(icon: &Icon, source: &str) -> String {
    let mut out = String::new();
    let times: Vec<f64> = (0..icon.frames)
        .map(|i| i as f64 / (icon.frames - 1) as f64)
        .collect();
    let _ = writeln!(
        out,
        "// Generated by `{BLESS_COMMAND}` from {source}; do not edit."
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "use flui_sdk::widgets::{{AnimatedIconData, IconCommand, IconPath}};"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "#[rustfmt::skip]");
    let _ = writeln!(
        out,
        "pub(crate) const DATA: AnimatedIconData = AnimatedIconData::from_static("
    );
    let _ = writeln!(out, "    {},", number(icon.view_box));
    let _ = writeln!(out, "    &[{}],", list(", ", &times));
    let _ = writeln!(out, "    &[],");
    let _ = writeln!(out, "    &[");
    for path in &icon.paths {
        let _ = writeln!(out, "        IconPath::new(");
        let _ = writeln!(out, "            &[{}],", list(", ", &path.opacities));
        let _ = writeln!(out, "            &[");
        for command in &path.commands {
            let line = match command {
                Command::MoveTo(points) | Command::LineTo(points) => {
                    let kind = if matches!(command, Command::MoveTo(_)) {
                        "move_to"
                    } else {
                        "line_to"
                    };
                    let points = points
                        .iter()
                        .map(|p| format!("[{}]", list(", ", p)))
                        .collect::<Vec<String>>()
                        .join(", ");
                    format!("IconCommand::{kind}(&[{points}]),")
                }
                Command::CubicTo(points) => {
                    let points = points
                        .iter()
                        .map(|p| format!("[{}]", list(", ", p)))
                        .collect::<Vec<String>>()
                        .join(", ");
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

// ---------------------------------------------------------------------------------------------
// The generator's own cases
// ---------------------------------------------------------------------------------------------

/// Each committed frame file is what the Flutter import writes for its icon at the revision it
/// names, so a rewrite from the same checkout changes nothing.
#[test]
fn the_frames_are_what_the_flutter_import_writes() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in MATERIAL {
        let path = package.join(ASSETS).join(format!("{name}.svg"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        let Some((_, rest)) = text.split_once("flutter/flutter@") else {
            panic!("{} names no Flutter revision", path.display());
        };
        let revision = rest.split_once(',').map_or(rest, |(revision, _)| revision);
        let icon = parse_svg(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(
            write_svg(&icon, name, revision) == text,
            "{} is not what the Flutter import writes",
            path.display()
        );
    }
}

#[test]
fn incompatible_svg_frames_are_refused() {
    let svg = r#"<svg viewBox="0 0 24 24">
        <g id="frame-00"><path d="M0 0 L10 0 L10 10 Z"/></g>
        <g id="frame-01"><path d="M0 0 C1 1 2 2 3 3 Z"/></g>
    </svg>"#;
    let error = parse_svg(svg).expect_err("the frames draw different commands");
    assert!(error.contains("not compatible"), "{error}");
}

#[test]
fn an_svg_without_two_frames_is_refused() {
    let svg = r#"<svg viewBox="0 0 24 24"><path d="M0 0 L10 0 L10 10 Z"/></svg>"#;
    let error = parse_svg(svg).expect_err("no frame groups");
    assert!(error.contains("needs two"), "{error}");
}

#[test]
fn svg_frames_become_one_constant() {
    let svg = r#"<svg viewBox="0 0 24 24" data-flui-match-text-direction="true">
        <g id="frame-00"><path d="M4 4 L20 4 Q20 8 4 8 Z" fill-opacity="0.5"/></g>
        <g id="frame-01"><path d="M4 10 L20 10 Q20 14 4 14 Z"/></g>
        <g id="frame-02"><path d="M4 16 L20 16 Q20 20 4 20 Z"/></g>
    </svg>"#;
    let icon = parse_svg(svg).expect("compatible frames");
    assert_eq!(icon.view_box, 24.0);
    assert_eq!(icon.paths[0].opacities, [0.5, 1.0, 1.0]);
    assert_eq!(
        icon.paths[0].commands[0],
        Command::MoveTo(vec![[4.0, 4.0], [4.0, 10.0], [4.0, 16.0]])
    );
    // A quadratic curve is raised to the cubic with its control points two thirds of the way
    // from each end toward the quadratic's control point.
    let Command::CubicTo(cubic) = &icon.paths[0].commands[2] else {
        panic!("the quadratic became {:?}", icon.paths[0].commands[2]);
    };
    let two_thirds = 2.0 / 3.0;
    assert_eq!(
        cubic[0],
        [
            20.0,
            4.0 + 4.0 * two_thirds,
            4.0 + 16.0 * two_thirds,
            8.0,
            4.0,
            8.0
        ]
    );
    let rust = rust_source(&icon, "test.svg");
    for line in [
        "// Generated by `FLUI_BLESS=1 cargo nextest run -p flui-material --test material_it \
         -E 'test(material_icon_constants_match_their_frames)'` from test.svg; do not edit.",
        "    &[0.0, 0.5, 1.0],",
        "            &[0.5, 1.0, 1.0],",
        "                IconCommand::move_to(&[[4.0, 4.0], [4.0, 10.0], [4.0, 16.0]]),",
        "                IconCommand::line_to(&[[20.0, 4.0], [20.0, 10.0], [20.0, 16.0]]),",
        "                IconCommand::close(),",
        "    true,",
    ] {
        assert!(
            rust.lines().any(|l| l == line),
            "{line:?} missing from\n{rust}"
        );
    }
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
    let back = parse_svg(&svg).expect("the SVG the Flutter import writes");
    assert_eq!(back, icon);
}
