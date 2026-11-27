use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::{INCLUDE_DEPTH, cargo_home_with, layers, tables_by_precedence};

/// Writes `file` under `root` as a table named `name` that includes
/// `includes`.
fn write(root: &Path, file: &str, name: &str, includes: &[&str]) -> PathBuf {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().expect("a fixture file has a directory"))
        .expect("a fixture directory");
    let includes: Vec<toml::Value> = includes
        .iter()
        .map(|include| toml::Value::String((*include).to_owned()))
        .collect();
    let text = format!(
        "include = {}\n[probe]\nname = \"{name}\"\n",
        toml::Value::Array(includes)
    );
    std::fs::write(&path, text).expect("a fixture file");
    path
}

fn names(tables: &[toml::Table]) -> Vec<&str> {
    tables
        .iter()
        .map(|table| {
            table["probe"]["name"]
                .as_str()
                .expect("a fixture table names itself")
        })
        .collect()
}

#[test]
fn a_file_is_layered_after_its_includes_in_their_order() {
    let root = tempfile::tempdir().expect("a temporary directory");
    write(root.path(), "b.toml", "b", &["d.toml"]);
    write(root.path(), "c.toml", "c", &[]);
    write(root.path(), "d.toml", "d", &[]);
    // Neither a missing file nor a path not ending in `.toml` adds a layer.
    let file = write(
        root.path(),
        "a.toml",
        "a",
        &["b.toml", "missing.toml", "c.conf", "c.toml"],
    );
    assert_eq!(names(&layers(&file)), ["d", "b", "c", "a"]);
}

/// A cycle through a path that names the same file differently, which a
/// comparison of the paths as written would follow down to the depth cap.
#[test]
fn a_cycle_through_another_spelling_of_the_file_adds_nothing() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let file = write(
        root.path(),
        ".cargo/config.toml",
        "own",
        &["../.cargo/config.toml"],
    );
    assert_eq!(names(&layers(&file)), ["own"]);
}

#[test]
fn includes_nest_to_the_depth_cap() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let depth = INCLUDE_DEPTH + 4;
    for level in 0..depth {
        let next = format!("{}.toml", level + 1);
        write(
            root.path(),
            &format!("{level}.toml"),
            &level.to_string(),
            &[&next],
        );
    }
    let read = layers(&root.path().join("0.toml"));
    assert_eq!(read.len(), INCLUDE_DEPTH + 1);
    assert_eq!(names(&read).last(), Some(&"0"));
}

/// The nearest file's own table, its includes from the last, a farther file,
/// then the cargo home's; in one directory `config` and not `config.toml`.
#[test]
fn the_tables_come_highest_precedence_first() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let project = root.path().join("outer/project");
    write(
        &project,
        ".cargo/config.toml",
        "near",
        &["first.toml", "second.toml"],
    );
    write(&project, ".cargo/first.toml", "near-first", &[]);
    write(&project, ".cargo/second.toml", "near-second", &[]);
    write(root.path(), "outer/.cargo/config", "outer-config", &[]);
    write(
        root.path(),
        "outer/.cargo/config.toml",
        "outer-config-toml",
        &[],
    );
    let home = root.path().join("home");
    write(&home, "config.toml", "home", &[]);
    assert_eq!(
        names(&tables_by_precedence(&project, Some(home))),
        ["near", "near-second", "near-first", "outer-config", "home"],
    );
}

#[test]
fn the_cargo_home_is_the_one_cargo_reads() {
    let dir = Path::new("/work/app");
    let home = Some(PathBuf::from("/home/author"));
    let read = |cargo_home: Option<&str>, home: Option<PathBuf>| {
        cargo_home_with(dir, cargo_home.map(OsString::from), home)
    };
    let in_home = Some(PathBuf::from("/home/author/.cargo"));
    assert_eq!(read(None, home.clone()), in_home, "unset");
    assert_eq!(read(Some(""), home.clone()), in_home, "empty");
    assert_eq!(
        read(Some("tools/cargo"), home.clone()),
        Some(PathBuf::from("/work/app/tools/cargo")),
        "relative",
    );
    assert_eq!(
        read(Some("/opt/cargo"), home),
        Some(PathBuf::from("/opt/cargo")),
        "absolute",
    );
    assert_eq!(read(None, None), None, "no home");
}
