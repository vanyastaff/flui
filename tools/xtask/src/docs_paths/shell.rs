//! Shell code split into commands and words the way a POSIX shell splits
//! them, as far as finding the commands a doc runs needs:
//!
//! - quotes (`'…'`, `"…"`, ANSI-C `$'…'`) join and are removed, `\` escapes a character and
//!   continues a line, `#` at the start of a word comments out the line;
//! - `&&`, `||`, `|`, `&`, `;`, `(`, `)` and a newline end a command,
//!   attached to a word or not;
//! - a redirection (`>log`, `2>&1`, `<in`) and its target are dropped, so
//!   `>log cargo test` is cargo's command;
//! - a here-document's body (`<<EOF` … `EOF`) is data, but for the command
//!   substitutions of one whose delimiter is unquoted, which the shell runs;
//! - a command substitution (`$(…)`, `` `…` ``), bare or inside double
//!   quotes, is a command of its own, and stands for a `$` in its word.
//!
//! No expansion: `$VAR` stays as written.

use std::iter::Peekable;
use std::str::Chars;

/// One command: its words, each with the 0-based line of the code it starts on.
pub(super) type Command = Vec<(usize, String)>;

/// The commands of `code`, empty ones left out.
pub(super) fn commands(code: &str) -> Vec<Command> {
    let mut lexer = Lexer {
        chars: code.chars().peekable(),
        line: 0,
        commands: vec![Command::new()],
        word: None,
        heredocs: Vec::new(),
        nested: Vec::new(),
    };
    lexer.run();
    let mut commands = lexer.commands;
    commands.extend(lexer.nested);
    commands.retain(|command| !command.is_empty());
    commands
}

/// The characters that end an unquoted word.
const WORD_ENDS: &str = ";&|()<>";

/// A here-document opened on the current line.
struct Heredoc {
    delimiter: String,
    /// `<<-`: leading tabs are stripped from the body and the delimiter line.
    strip_tabs: bool,
    /// A quoted delimiter makes the body inert; an unquoted one leaves its
    /// command substitutions to run.
    quoted: bool,
}

/// The state of one pass over the code.
struct Lexer<'a> {
    chars: Peekable<Chars<'a>>,
    /// The 0-based line the next character is on.
    line: usize,
    /// The commands so far; the last is the one being read.
    commands: Vec<Command>,
    /// The word being read, with the line it starts on.
    word: Option<(usize, String)>,
    /// The here-documents opened on this line: each delimiter, and whether
    /// `<<-` strips leading tabs. Their bodies follow the newline.
    heredocs: Vec<Heredoc>,
    /// The commands of the substitutions met so far.
    nested: Vec<Command>,
}

impl Lexer<'_> {
    fn run(&mut self) {
        while let Some(c) = self.chars.next() {
            match c {
                ' ' | '\t' | '\r' => self.flush(),
                '\n' => {
                    self.flush();
                    self.end();
                    self.line += 1;
                    self.skip_heredoc_bodies();
                }
                '<' if self.chars.peek() == Some(&'<') => {
                    self.chars.next();
                    self.flush();
                    if self.chars.next_if_eq(&'<').is_some() {
                        // `<<< word`: a here-string, a redirection like `<`
                        self.redirect();
                    } else {
                        self.open_heredoc();
                    }
                }
                // a doc's `<crate>` is a placeholder word, not a redirection
                '<' if self.placeholder_ahead() => {
                    self.push('<');
                    while let Some(c) = self.chars.next() {
                        self.push(c);
                        if c == '>' {
                            break;
                        }
                    }
                }
                '<' | '>' => self.redirect(),
                '&' if self.chars.peek() == Some(&'>') => {
                    // `&>file`: stdout and stderr both
                    self.chars.next();
                    self.redirect();
                }
                '&' | '|' | ';' | '(' | ')' => {
                    self.flush();
                    if matches!(c, '&' | '|') && self.chars.peek() == Some(&c) {
                        self.chars.next();
                    }
                    self.end();
                }
                '#' if self.word.is_none() => {
                    while self.chars.next_if(|&next| next != '\n').is_some() {}
                }
                '\\' => match self.chars.next() {
                    Some('\n') => self.line += 1,
                    Some(escaped) => self.push(escaped),
                    None => {}
                },
                '\'' => {
                    self.start_word();
                    while let Some(quoted) = self.chars.next() {
                        if quoted == '\'' {
                            break;
                        }
                        self.push_counting(quoted);
                    }
                }
                '"' => self.double_quoted(),
                '$' if self.chars.peek() == Some(&'(') => {
                    self.chars.next();
                    self.substitution(')');
                }
                '$' if self.chars.next_if_eq(&'\'').is_some() => self.ansi_c_quoted(),
                // PowerShell's line continuation: a backtick ending the line
                '`' if self.continues_line() => {}
                '`' => self.substitution('`'),
                c => self.push(c),
            }
        }
        self.flush();
    }

    /// The rest of a `"…"` string, its substitutions included.
    fn double_quoted(&mut self) {
        self.start_word();
        while let Some(quoted) = self.chars.next() {
            match quoted {
                '"' => break,
                // a line continuation inside quotes too
                '\\' if self.chars.next_if_eq(&'\n').is_some() => self.line += 1,
                '\\' if self
                    .chars
                    .peek()
                    .is_some_and(|next| matches!(next, '"' | '\\' | '$' | '`')) =>
                {
                    let escaped = self.chars.next().expect("BUG: peeked");
                    self.push(escaped);
                }
                '$' if self.chars.peek() == Some(&'(') => {
                    self.chars.next();
                    self.substitution(')');
                }
                '`' => self.substitution('`'),
                quoted => self.push_counting(quoted),
            }
        }
    }

    /// A command substitution up to `closing` (`)` of `$(`, or `` ` ``): its
    /// text is lexed as commands of its own, at the lines it spans, and it
    /// stands for a `$` in the word around it.
    fn substitution(&mut self, closing: char) {
        let start = self.line;
        let text = substitution_text(&mut self.chars, closing);
        self.line += text.matches('\n').count();
        self.nest(&text, start);
        self.push('$');
    }

    /// Whether a backtick just read ends its line, as PowerShell continues a
    /// line; the newline is taken.
    fn continues_line(&mut self) -> bool {
        let mut ahead = self.chars.clone();
        while ahead
            .next_if(|&c| c == ' ' || c == '\t' || c == '\r')
            .is_some()
        {}
        if ahead.next() != Some('\n') {
            return false;
        }
        self.chars = ahead;
        self.line += 1;
        true
    }

    /// The rest of an ANSI-C `$'…'` string, its escapes applied.
    fn ansi_c_quoted(&mut self) {
        self.start_word();
        while let Some(c) = self.chars.next() {
            let c = match c {
                '\'' => break,
                '\\' => match ansi_c_escape(&mut self.chars) {
                    Some(c) => c,
                    None => continue,
                },
                c => c,
            };
            self.push_counting(c);
        }
    }

    /// The commands of a substitution's `text`, which starts on `line`.
    fn nest(&mut self, text: &str, line: usize) {
        for mut command in commands(text) {
            for (at, _) in &mut command {
                *at += line;
            }
            self.nested.push(command);
        }
    }

    /// Drops a redirection whose operator started with the character just
    /// read: a file-descriptor number before it, the rest of the operator,
    /// and its target word.
    fn redirect(&mut self) {
        if self
            .word
            .as_ref()
            .is_some_and(|(_, word)| word.bytes().all(|b| b.is_ascii_digit()))
        {
            self.word = None;
        }
        self.flush();
        while self
            .chars
            .next_if(|&next| matches!(next, '>' | '&' | '|'))
            .is_some()
        {}
        while self
            .chars
            .next_if(|&next| next == ' ' || next == '\t')
            .is_some()
        {}
        self.read_word(|_| {});
    }

    /// Whether the `<` just read opens a `<name>` placeholder: a run of name
    /// characters closed by `>`.
    fn placeholder_ahead(&self) -> bool {
        let mut ahead = self.chars.clone();
        let mut name = 0;
        while ahead
            .next_if(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .is_some()
        {
            name += 1;
        }
        name > 0 && ahead.next() == Some('>')
    }

    /// Records the here-document `<<` or `<<-` (just read) opens.
    fn open_heredoc(&mut self) {
        let strip_tabs = self.chars.next_if_eq(&'-').is_some();
        while self
            .chars
            .next_if(|&next| next == ' ' || next == '\t')
            .is_some()
        {}
        let mut delimiter = String::new();
        let quoted = self.read_word(|c| delimiter.push(c));
        if !delimiter.is_empty() {
            self.heredocs.push(Heredoc {
                delimiter,
                strip_tabs,
                quoted,
            });
        }
    }

    /// Reads one shell word, quotes removed, handing each character to `each`;
    /// whether any of it was quoted or escaped.
    fn read_word(&mut self, mut each: impl FnMut(char)) -> bool {
        let mut quoted = false;
        let mut quote: Option<char> = None;
        while let Some(&next) = self.chars.peek() {
            match quote {
                Some(open) => {
                    self.chars.next();
                    if next == open {
                        quote = None;
                    } else {
                        each(next);
                    }
                }
                None if next.is_whitespace() || WORD_ENDS.contains(next) => break,
                None => {
                    self.chars.next();
                    match next {
                        '\'' | '"' => {
                            quoted = true;
                            quote = Some(next);
                        }
                        '\\' => {
                            quoted = true;
                            each(self.chars.next().unwrap_or_default());
                        }
                        next => each(next),
                    }
                }
            }
        }
        quoted
    }

    /// Skips the bodies of the here-documents opened on the line just ended.
    fn skip_heredoc_bodies(&mut self) {
        for heredoc in std::mem::take(&mut self.heredocs) {
            let start = self.line;
            let mut body = String::new();
            loop {
                let mut body_line = String::new();
                while let Some(next) = self.chars.next_if(|&next| next != '\n') {
                    body_line.push(next);
                }
                let ended = self.chars.next().is_none();
                self.line += usize::from(!ended);
                let body_line = body_line.trim_end_matches('\r');
                let body_line = if heredoc.strip_tabs {
                    body_line.trim_start_matches('\t')
                } else {
                    body_line
                };
                if body_line == heredoc.delimiter {
                    break;
                }
                body.push_str(body_line);
                body.push('\n');
                if ended {
                    break;
                }
            }
            if !heredoc.quoted {
                for (offset, text) in substitutions(&body) {
                    self.nest(&text, start + offset);
                }
            }
        }
    }

    fn start_word(&mut self) {
        let line = self.line;
        self.word.get_or_insert_with(|| (line, String::new()));
    }

    fn push(&mut self, c: char) {
        self.start_word();
        if let Some((_, word)) = &mut self.word {
            word.push(c);
        }
    }

    /// [`Self::push`] for a quoted character, which may be a newline.
    fn push_counting(&mut self, c: char) {
        self.push(c);
        self.line += usize::from(c == '\n');
    }

    fn flush(&mut self) {
        if let Some(done) = self.word.take() {
            self.commands
                .last_mut()
                .expect("BUG: there is always a current command")
                .push(done);
        }
    }

    fn end(&mut self) {
        if self
            .commands
            .last()
            .is_some_and(|command| !command.is_empty())
        {
            self.commands.push(Command::new());
        }
    }
}

/// The text of a command substitution up to its `closing` (`)` of `$(`, or
/// `` ` ``), quotes and escapes honoured and kept for the nested lexing: a `)`
/// inside quotes does not close it.
fn substitution_text(chars: &mut Peekable<Chars<'_>>, closing: char) -> String {
    let mut text = String::new();
    let mut depth = 0_usize;
    let mut quote: Option<char> = None;
    // `case … esac` blocks open: in one, an unmatched `)` ends a pattern
    let mut cases = 0_usize;
    let mut word = String::new();
    while let Some(c) = chars.next() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
            text.push(c);
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            match word.as_str() {
                "case" => cases += 1,
                "esac" => cases = cases.saturating_sub(1),
                _ => {}
            }
            word.clear();
        }
        match c {
            '\\' => {
                text.push(c);
                text.extend(chars.next());
                continue;
            }
            '\'' | '"' => quote = Some(c),
            ')' if closing == ')' && depth == 0 && cases > 0 => {}
            c if c == closing && depth == 0 => break,
            '(' if closing == ')' => depth += 1,
            ')' if closing == ')' => depth -= 1,
            _ => {}
        }
        text.push(c);
    }
    text
}

/// The character an ANSI-C escape (after its `\\`) stands for: `\\n`,
/// `\\x2d`, octal `\\055`, `\\u002d`, `\\U0000002d`, `\\cX`, and the
/// rest of Bash's; `None` for one that stands for nothing.
fn ansi_c_escape(chars: &mut Peekable<Chars<'_>>) -> Option<char> {
    let digits = |chars: &mut Peekable<Chars<'_>>, radix: u32, most: usize| {
        let mut value = 0_u32;
        let mut read = 0;
        while read < most {
            let Some(digit) = chars.peek().and_then(|c| c.to_digit(radix)) else {
                break;
            };
            chars.next();
            value = value * radix + digit;
            read += 1;
        }
        (read > 0).then(|| char::from_u32(value)).flatten()
    };
    Some(match chars.next()? {
        'a' => '\u{7}',
        'b' => '\u{8}',
        'e' | 'E' => '\u{1b}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'v' => '\u{b}',
        'x' => return digits(chars, 16, 2),
        'u' => return digits(chars, 16, 4),
        'U' => return digits(chars, 16, 8),
        'c' => {
            return chars
                .next()
                .and_then(|c| char::from_u32(u32::from(c) & 0x1f));
        }
        first @ '0'..='7' => {
            let mut value = first.to_digit(8)?;
            for _ in 0..2 {
                let Some(digit) = chars.peek().and_then(|c| c.to_digit(8)) else {
                    break;
                };
                chars.next();
                value = value * 8 + digit;
            }
            return char::from_u32(value);
        }
        other => other,
    })
}

/// The command substitutions (`$(…)`, `` `…` ``) of a here-document body the
/// shell expands, each with the 0-based line of the body it starts on.
fn substitutions(body: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut chars = body.chars().peekable();
    let mut line = 0;
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            '\\' => {
                if chars.next() == Some('\n') {
                    line += 1;
                }
            }
            '$' if chars.next_if_eq(&'(').is_some() => {
                let text = substitution_text(&mut chars, ')');
                let start = line;
                line += text.matches('\n').count();
                found.push((start, text));
            }
            '`' => {
                let text = substitution_text(&mut chars, '`');
                let start = line;
                line += text.matches('\n').count();
                found.push((start, text));
            }
            _ => {}
        }
    }
    found
}
