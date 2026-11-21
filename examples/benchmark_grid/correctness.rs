//! Off-clock oracles shared by the executable and its integration consumer.
use flui_foundation::geometry::{Offset, Size};
use flui_rendering::testing::inspect::{paragraph_text, render_snapshot};
use flui_testing::{HeadlessBinding, PointerScript};

use super::scene::{GridScene, HEIGHT, Kind, Shape, WIDTH};

pub fn verify_scene(
    binding: &HeadlessBinding,
    scene: &GridScene,
    header: &str,
) -> Result<(), String> {
    binding
        .pipeline_owner()
        .ok_or("missing pipeline")?
        .with(|owner| {
            let tree = render_snapshot(owner);
            let mut actual: Vec<String> = tree
                .find_by_kind("RenderParagraph")
                .into_iter()
                .filter_map(paragraph_text)
                .collect();
            let mut expected: Vec<String> = (0..scene.shape.sidebar)
                .map(|id| format!("Item {id}"))
                .collect();
            expected.extend(std::iter::repeat_n(
                String::from("X"),
                scene.shape.columns * scene.shape.rows,
            ));
            expected.push(header.to_owned());
            actual.sort();
            expected.sort();
            if actual != expected {
                return Err(format!(
                    "text population mismatch: {} actual / {} expected",
                    actual.len(),
                    expected.len()
                ));
            }
            let sizes: Result<Vec<Size>, _> = tree
                .find_by_kind("RenderParagraph")
                .into_iter()
                .map(|node| node.get_fact::<Size>("size"))
                .collect();
            let sizes = sizes.map_err(|error| error.to_string())?;
            for (size, count) in [
                (
                    Size::new(14.0, 14.0),
                    scene.shape.rows * scene.shape.columns,
                ),
                (Size::new(244.0, 22.0), scene.shape.sidebar),
                (Size::new(1584.0, 48.0), 1),
            ] {
                if sizes.iter().filter(|actual| **actual == size).count() != count {
                    return Err(format!(
                        "wrong geometry for {size:?}: expected {count}; found {} boxes, first {:?}",
                        sizes.len(),
                        &sizes[..sizes.len().min(5)]
                    ));
                }
            }
            if sizes.len() != scene.shape.items() + 1 {
                return Err("unexpected text boxes".into());
            }
            Ok(())
        })
}

pub fn verify_input(binding: &mut HeadlessBinding, scene: &GridScene) -> Result<(), String> {
    if scene.kind == Kind::Buttons {
        for (x, y, id) in [
            (20.0, 80.0, 0),
            (280.0, 68.0, scene.shape.sidebar),
            (
                280.0 + (scene.shape.columns - 1) as f64 * 16.0,
                68.0,
                scene.shape.sidebar + scene.shape.columns - 1,
            ),
        ] {
            binding.replay(&PointerScript::tap(Offset::new(x, y)));
            if scene.activated.get() != Some(id) {
                return Err(format!(
                    "wrong button activation at ({x},{y}), expected {id}"
                ));
            }
        }
        scene.activated.set(None);
        binding.replay(&PointerScript::tap(Offset::new(270.0, 68.0)));
        if scene.activated.get().is_some() {
            return Err("gap activated a button".into());
        }
    }
    Ok(())
}

pub fn verify_pixels(pixels: &[u8], shape: Shape) -> Result<(), String> {
    if pixels.len() != WIDTH as usize * HEIGHT as usize * 4 {
        return Err("invalid RGBA length".into());
    }
    let pixel = |x: usize, y: usize| &pixels[(y * WIDTH as usize + x) * 4..][..4];
    let near = |actual: &[u8], expected: [u8; 4]| {
        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3)
    };
    for (x, y, color) in [
        (2, 2, [20, 24, 32, 255]),
        (270, 68, [20, 24, 32, 255]),
        (289, 77, [40, 80, 120, 255]),
        (259, 93, [40, 80, 120, 255]),
        (280, 995, [20, 24, 32, 255]),
    ] {
        if !near(pixel(x, y), color) {
            return Err(format!(
                "wrong pixel at ({x},{y}): {:?}, expected {color:?}",
                pixel(x, y)
            ));
        }
    }
    for column in 0..shape.columns {
        let left = 276 + column * 16;
        let ink = (64..78).any(|y| (left..left + 14).any(|x| pixel(x, y)[0] > 160));
        if !ink {
            return Err(format!("no glyph ink in grid column {column}"));
        }
        if !near(pixel(left + 14, 77), [20, 24, 32, 255]) {
            return Err(format!("grid gap missing at column {column}"));
        }
    }
    Ok(())
}

pub fn verify_header_delta(before: &[u8], after: &[u8]) -> Result<(), String> {
    if before.len() != after.len() || before.len() != WIDTH as usize * HEIGHT as usize * 4 {
        return Err("invalid capture dimensions".into());
    }
    let split = WIDTH as usize * 64 * 4;
    if before[..split] == after[..split] {
        return Err("header change produced no pixel change".into());
    }
    if before[split..] != after[split..] {
        return Err("local header update changed body pixels".into());
    }
    Ok(())
}
