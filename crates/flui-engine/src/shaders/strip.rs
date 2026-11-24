//! Shader text as it is linked into an application.
//!
//! `build.rs` runs [`strip_wgsl`] over every shader before a pipeline includes
//! it, and compiles this file as a module of its own (`#[path]`); the crate
//! compiles it only for its tests.

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
pub(crate) fn strip_wgsl(source: &str) -> Result<String, BlockComment> {
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

/// A shader source opens a block comment, which [`strip_wgsl`] does not strip.
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

impl std::error::Error for BlockComment {}

#[cfg(test)]
mod tests {
    use super::{BlockComment, strip_wgsl};

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
            assert_eq!(strip_wgsl(source).as_deref(), Ok(code), "{source:?}");
        }
    }

    #[test]
    fn a_block_comment_in_the_code_is_refused_with_its_line() {
        for (source, line) in [
            ("/* a */\nlet a = 1;\n", 1),
            ("let a = 1;\nlet b = 2; /* two */\n", 2),
            ("let a = 1;\n/* opens // and goes on\n*/\n", 2),
        ] {
            assert_eq!(strip_wgsl(source), Err(BlockComment { line }), "{source:?}");
        }
    }
}
