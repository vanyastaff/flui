//! The copies of the shaders that are linked into an application.
//!
//! `build.rs` reads the sources with [`read_sources`] and writes their code
//! with [`write_copies`], and compiles this file as a module of its own
//! (`#[path]`); the crate compiles it only for its tests.

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

/// `source` without its line comments, indentation and blank lines: the code a
/// shader compiler reads, each kept line ending in a line feed.
///
/// WGSL has no string or character literal, so a `//` always opens a comment.
///
/// # Errors
///
/// [`BlockComment`] when the code of a line opens a block comment (`/*`): only
/// line comments are stripped, and a `//` inside a block comment would cut it
/// in half. A `/*` inside a line comment is part of that comment.
pub(crate) fn code_of(source: &str) -> Result<String, BlockComment> {
    let mut code = String::with_capacity(source.len());
    for (index, line) in source.lines().enumerate() {
        let line = line
            .split_once("//")
            .map_or(line, |(code, _comment)| code)
            .trim();
        if line.contains("/*") {
            return Err(BlockComment { line: index + 1 });
        }
        if !line.is_empty() {
            code.push_str(line);
            code.push('\n');
        }
    }
    Ok(code)
}

/// A shader source opens a block comment, which [`code_of`] does not strip.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BlockComment {
    /// The 1-based line that opens it.
    pub(crate) line: usize,
}

impl std::fmt::Display for BlockComment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "line {} opens a block comment (`/*`): the shader sources use line comments, which the build strips",
            self.line
        )
    }
}

impl Error for BlockComment {}

/// Every `.wgsl` file under `shader_root`, as its path below that root and its
/// text.
pub(crate) fn read_sources(shader_root: &Path) -> io::Result<Vec<(PathBuf, String)>> {
    fn read_into(
        directory: &Path,
        below_root: &Path,
        sources: &mut Vec<(PathBuf, String)>,
    ) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let below_root = below_root.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                read_into(&path, &below_root, sources)?;
            } else if path
                .extension()
                .is_some_and(|extension| extension == "wgsl")
            {
                sources.push((below_root, fs::read_to_string(&path)?));
            }
        }
        Ok(())
    }

    let mut sources = Vec::new();
    read_into(shader_root, Path::new(""), &mut sources)?;
    Ok(sources)
}

/// Makes `copies_root` hold the code of each of `sources`, at the source's
/// path below it, and nothing else.
///
/// The directory is emptied first: the copy of a shader that was renamed or
/// deleted would otherwise stay where an earlier build wrote it, and whatever
/// still names it would keep compiling.
///
/// # Errors
///
/// An I/O error, or the [`BlockComment`] of a source with its path.
pub(crate) fn write_copies(
    copies_root: &Path,
    sources: &[(PathBuf, String)],
) -> Result<(), Box<dyn Error>> {
    match fs::remove_dir_all(copies_root) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(copies_root)?;
    for (below_root, source) in sources {
        let code = code_of(source).map_err(|error| format!("{}: {error}", below_root.display()))?;
        let copy = copies_root.join(below_root);
        if let Some(parent) = copy.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(copy, code)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{BlockComment, code_of, read_sources, write_copies};

    #[test]
    fn a_shader_keeps_its_code_and_loses_comments_indentation_and_blank_lines() {
        for (source, code) in [
            ("fn f() {\n    return;\n}\n", "fn f() {\nreturn;\n}\n"),
            ("let a = 1; // one\n", "let a = 1;\n"),
            ("// a comment-only line\n\n\t\nlet a = 1;", "let a = 1;\n"),
            ("let a = 1;\r\n  let b = 2;\r\n", "let a = 1;\nlet b = 2;\n"),
            ("// shared by effects/*.wgsl\nlet a = 1;\n", "let a = 1;\n"),
            ("let a = 6 / 2;\n", "let a = 6 / 2;\n"),
            ("", ""),
        ] {
            assert_eq!(code_of(source).as_deref(), Ok(code), "{source:?}");
        }
    }

    #[test]
    fn a_block_comment_in_the_code_is_refused_with_its_line() {
        for (source, line) in [
            ("/* a */\nlet a = 1;\n", 1),
            ("let a = 1;\nlet b = 2; /* two */\n", 2),
            ("let a = 1;\n/* opens // and goes on\n*/\n", 2),
        ] {
            assert_eq!(code_of(source), Err(BlockComment { line }), "{source:?}");
        }
    }

    #[test]
    fn the_copies_follow_the_sources_through_a_rename_and_a_removal() {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let shader_root = scratch.path().join("shaders");
        let copies_root = scratch.path().join("out").join("wgsl");
        fs::create_dir_all(shader_root.join("effects")).expect("the source tree");
        fs::write(shader_root.join("shape.wgsl"), "  // shape\n  let a = 1;\n").expect("a source");
        fs::write(
            shader_root.join("effects/blur.wgsl"),
            "let b = 2; // blur\n",
        )
        .expect("a nested source");
        fs::write(shader_root.join("notes.md"), "not a shader").expect("another file");

        let sources = read_sources(&shader_root).expect("the sources");
        write_copies(&copies_root, &sources).expect("the copies");
        assert_eq!(
            fs::read_to_string(copies_root.join("shape.wgsl")).expect("the copy"),
            "let a = 1;\n"
        );
        assert_eq!(
            fs::read_to_string(copies_root.join("effects/blur.wgsl")).expect("the nested copy"),
            "let b = 2;\n"
        );
        assert!(!copies_root.join("notes.md").exists());

        fs::rename(
            shader_root.join("effects/blur.wgsl"),
            shader_root.join("effects/gaussian.wgsl"),
        )
        .expect("the rename");
        fs::remove_file(shader_root.join("shape.wgsl")).expect("the removal");
        let sources = read_sources(&shader_root).expect("the sources");
        write_copies(&copies_root, &sources).expect("the copies");

        let mut copies: Vec<PathBuf> = read_sources(&copies_root)
            .expect("the copies read back")
            .into_iter()
            .map(|(below_root, _code)| below_root)
            .collect();
        copies.sort();
        assert_eq!(copies, [PathBuf::from("effects/gaussian.wgsl")]);
    }

    #[test]
    fn a_block_comment_fails_the_write_with_the_path_of_its_source() {
        let scratch = tempfile::tempdir().expect("a scratch directory");
        let sources = [(PathBuf::from("effects/blit.wgsl"), "/* no */\n".to_owned())];

        let error = write_copies(&scratch.path().join("wgsl"), &sources)
            .expect_err("a block comment is refused");

        assert_eq!(
            error.to_string(),
            "effects/blit.wgsl: line 1 opens a block comment (`/*`): the shader sources use line comments, which the build strips"
        );
    }
}
