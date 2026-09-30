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

/// The shell a piece of code is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Dialect {
    /// sh, bash, zsh: the default.
    Posix,
    /// PowerShell: a backtick escapes (and continues a line), `\` is a
    /// plain character, and there is no `$'…'` or here-document.
    PowerShell,
}

impl Dialect {
    /// The character that escapes the next one: `\`, or PowerShell's backtick.
    fn escape(self) -> char {
        match self {
            Self::Posix => '\\',
            Self::PowerShell => '`',
        }
    }
}

/// The commands of `code`, written for `dialect`, empty ones left out.
pub(super) fn commands_in(code: &str, dialect: Dialect) -> Vec<Command> {
    let mut lexer = Lexer {
        dialect,
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
    dialect: Dialect,
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
                '\\' if self.dialect == Dialect::PowerShell => self.push('\\'),
                '`' if self.dialect == Dialect::PowerShell => self.powershell_escape(),
                ' ' | '\t' | '\r' => self.flush(),
                '\n' => {
                    self.flush();
                    self.end();
                    self.line += 1;
                    self.skip_heredoc_bodies();
                }
                '<' if self.chars.peek() == Some(&'<') => {
                    self.chars.next();
                    self.drop_descriptor();
                    self.flush();
                    if self.chars.next_if_eq(&'<').is_some() {
                        // `<<< word`: a here-string, a redirection like `<`
                        self.redirect();
                    } else {
                        self.open_heredoc();
                    }
                }
                // a doc's `<crate>` is a placeholder word, not a redirection
                // PowerShell's `<# … #>` block comment
                '<' if self.dialect == Dialect::PowerShell
                    && self.chars.next_if_eq(&'#').is_some() =>
                {
                    let mut last = ' ';
                    for c in self.chars.by_ref() {
                        self.line += usize::from(c == '\n');
                        if last == '#' && c == '>' {
                            break;
                        }
                        last = c;
                    }
                }
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
                // `args=(…)`: an array's elements are words, not a command
                '(' if self
                    .word
                    .as_ref()
                    .is_some_and(|(_, word)| word.ends_with('=')) =>
                {
                    let start = self.line;
                    let elements = substitution_text(&mut self.chars, ')', self.dialect);
                    self.line += elements.matches('\n').count();
                    // an element's substitution runs as the array is built
                    for (offset, text) in substitutions(&elements, Quoting::Shell) {
                        self.nest(&text, start + offset);
                    }
                    self.push('(');
                    for c in elements.chars() {
                        self.push(c);
                    }
                    self.push(')');
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
                '$' if self.dialect == Dialect::Posix && self.chars.next_if_eq(&'\'').is_some() => {
                    self.ansi_c_quoted();
                }
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
                '\\' if self.dialect == Dialect::PowerShell => self.push('\\'),
                '`' if self.dialect == Dialect::PowerShell => self.powershell_escape(),
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
        let text = substitution_text(&mut self.chars, closing, self.dialect);
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
            match c {
                '\'' => break,
                '\\' => {
                    for c in ansi_c_escape(&mut self.chars).chars() {
                        self.push_counting(c);
                    }
                }
                c => self.push_counting(c),
            }
        }
    }

    /// A PowerShell backtick just read: a line continuation at a line's end,
    /// or else the escape of the next character, which it keeps.
    fn powershell_escape(&mut self) {
        if !self.continues_line()
            && let Some(escaped) = self.chars.next()
        {
            self.push_counting(escaped);
        }
    }

    /// The commands of a substitution's `text`, which starts on `line`.
    fn nest(&mut self, text: &str, line: usize) {
        for mut command in commands_in(text, self.dialect) {
            for (at, _) in &mut command {
                *at += line;
            }
            self.nested.push(command);
        }
    }

    /// Drops a redirection whose operator started with the character just
    /// read: a file-descriptor number (`2>`) or name (`{fd}>`) before it, the
    /// rest of the operator, and its target word, whose command substitutions
    /// still run (`>"$(cargo …)"`).
    fn redirect(&mut self) {
        self.drop_descriptor();
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
        let start = self.line;
        let mut target = String::new();
        self.read_raw_word(&mut target);
        let quoting = match self.dialect {
            Dialect::Posix => Quoting::Shell,
            Dialect::PowerShell => Quoting::PowerShell,
        };
        for (offset, text) in substitutions(&target, quoting) {
            self.nest(&text, start + offset);
        }
    }

    /// Drops the word being read when it is the file descriptor of the
    /// redirection that follows it: a number (`2>`) or a name (`{fd}>`).
    fn drop_descriptor(&mut self) {
        let descriptor = |word: &str| {
            word.bytes().all(|b| b.is_ascii_digit())
                || word
                    .strip_prefix('{')
                    .and_then(|rest| rest.strip_suffix('}'))
                    .is_some_and(|name| {
                        !name.is_empty()
                            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    })
        };
        if self.word.as_ref().is_some_and(|(_, word)| descriptor(word)) {
            self.word = None;
        }
    }

    /// Reads one shell word as written, quotes kept, into `raw`: its end is
    /// where the shell's is, a `$(…)` in it included.
    fn read_raw_word(&mut self, raw: &mut String) {
        let escape = self.dialect.escape();
        let mut quote: Option<char> = None;
        while let Some(&next) = self.chars.peek() {
            if let Some(open) = quote {
                self.chars.next();
                raw.push(next);
                // inside `"…"` the dialect's escape (`\`, PowerShell's backtick)
                // keeps a `"` from closing it
                if open == '"' && next == escape {
                    raw.extend(self.chars.next());
                } else if next == open {
                    quote = None;
                }
                continue;
            }
            if next.is_whitespace() || WORD_ENDS.contains(next) {
                break;
            }
            self.chars.next();
            raw.push(next);
            match next {
                '\'' | '"' => quote = Some(next),
                c if c == escape => raw.extend(self.chars.next()),
                '$' if self.chars.next_if_eq(&'(').is_some() => {
                    raw.push('(');
                    raw.push_str(&substitution_text(&mut self.chars, ')', self.dialect));
                    raw.push(')');
                }
                _ => {}
            }
        }
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

    /// Reads one shell word, quotes removed (`'…'`, `"…"`, `$'…'` with its
    /// escapes), handing each character to `each`; whether any of it was quoted
    /// or escaped.
    fn read_word(&mut self, mut each: impl FnMut(char)) -> bool {
        let mut quoted = false;
        let mut quote: Option<char> = None;
        while let Some(&next) = self.chars.peek() {
            match quote {
                Some(open) => {
                    self.chars.next();
                    if next == open {
                        quote = None;
                    } else if open == '"'
                        && next == '\\'
                        && let Some(escaped) =
                            self.chars.next_if(|c| matches!(c, '"' | '\\' | '$' | '`'))
                    {
                        each(escaped);
                    } else {
                        each(next);
                    }
                }
                None if next.is_whitespace() || WORD_ENDS.contains(next) => break,
                None => {
                    self.chars.next();
                    match next {
                        '$' if self.chars.next_if_eq(&'\'').is_some() => {
                            quoted = true;
                            while let Some(c) = self.chars.next() {
                                match c {
                                    '\'' => break,
                                    '\\' => {
                                        ansi_c_escape(&mut self.chars).chars().for_each(&mut each);
                                    }
                                    c => each(c),
                                }
                            }
                        }
                        '\'' | '"' => {
                            quoted = true;
                            quote = Some(next);
                        }
                        // a backslash-newline continues the word, adding nothing
                        '\\' if self.chars.next_if_eq(&'\n').is_some() => self.line += 1,
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
                let ended = loop {
                    while let Some(next) = self.chars.next_if(|&next| next != '\n') {
                        body_line.push(next);
                    }
                    let ended = self.chars.next().is_none();
                    self.line += usize::from(!ended);
                    // an unquoted body's backslash-newline joins the next line
                    if ended || heredoc.quoted || !body_line.ends_with('\\') {
                        break ended;
                    }
                    body_line.pop();
                };
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
                for (offset, text) in substitutions(&body, Quoting::HeredocBody) {
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
/// inside quotes, a comment, or a `case` pattern does not close it.
fn substitution_text(chars: &mut Peekable<Chars<'_>>, closing: char, dialect: Dialect) -> String {
    let escape = dialect.escape();
    let mut text = String::new();
    let mut depth = 0_usize;
    // the open quote, and whether `\` escapes inside it
    let mut quote: Option<(char, bool)> = None;
    // `case … esac` blocks open: in one, an unmatched `)` ends a pattern. A
    // keyword counts only as a whole word at a command's start.
    let mut cases = 0_usize;
    let mut word = String::new();
    let mut command_start = true;
    while let Some(c) = chars.next() {
        if let Some((open, escapes)) = quote {
            text.push(c);
            if escapes && c == escape {
                text.extend(chars.next());
            } else if c == open {
                quote = None;
            }
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            if command_start {
                match word.as_str() {
                    "case" => cases += 1,
                    "esac" => cases = cases.saturating_sub(1),
                    _ => {}
                }
            }
            if !word.is_empty() {
                // `case WORD in`: the words after `case` are not commands; the
                // one after `in`, `;;` or a pattern's `)` is
                command_start = matches!(word.as_str(), "in" | "do" | "then" | "else");
            }
            word.clear();
            if matches!(c, ';' | '\n' | '|' | '&' | '(') {
                command_start = true;
            }
        }
        match c {
            c if c == escape => {
                text.push(c);
                text.extend(chars.next());
                continue;
            }
            // a comment runs to the line's end: its words are no keywords
            // a comment starts a word: after a blank or an operator
            '#' if text.is_empty()
                || text.ends_with(|c: char| c.is_whitespace() || ";&|()".contains(c)) =>
            {
                text.push(c);
                while let Some(skipped) = chars.next_if(|&next| next != '\n') {
                    text.push(skipped);
                }
                continue;
            }
            // the escape works inside `"…"` and ANSI-C `$'…'`, not inside `'…'`
            '\'' => quote = Some(('\'', dialect == Dialect::Posix && text.ends_with('$'))),
            '"' => quote = Some(('"', true)),
            ')' if closing == ')' && depth == 0 && cases > 0 => command_start = true,
            c if c == closing && depth == 0 => break,
            '(' if closing == ')' => depth += 1,
            ')' if closing == ')' => depth -= 1,
            _ => {}
        }
        text.push(c);
    }
    text
}

/// What an ANSI-C escape (after its `\\`) stands for: `\\n`, `\\x2d`, octal
/// `\\055`, `\\u002d`, `\\U0000002d`, `\\cX`, and the rest of Bash's. An
/// escape missing its digits (`\\x-`) stays as written, as Bash keeps it.
fn ansi_c_escape(chars: &mut Peekable<Chars<'_>>) -> String {
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
    let Some(escape) = chars.next() else {
        return "\\".to_owned();
    };
    let decoded = match escape {
        'a' => Some('\u{7}'),
        'b' => Some('\u{8}'),
        'e' | 'E' => Some('\u{1b}'),
        'f' => Some('\u{c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'v' => Some('\u{b}'),
        'x' => digits(chars, 16, 2),
        'u' => digits(chars, 16, 4),
        'U' => digits(chars, 16, 8),
        'c' => chars
            .next()
            .and_then(|c| char::from_u32(u32::from(c) & 0x1f)),
        first @ '0'..='7' => {
            let mut value = first.to_digit(8).unwrap_or_default();
            for _ in 0..2 {
                let Some(digit) = chars.peek().and_then(|c| c.to_digit(8)) else {
                    break;
                };
                chars.next();
                value = value * 8 + digit;
            }
            char::from_u32(value)
        }
        other => Some(other),
    };
    decoded.map_or_else(|| format!("\\{escape}"), String::from)
}

/// How the text [`substitutions`] scans treats quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quoting {
    /// A shell word: nothing inside `'…'` is expanded.
    Shell,
    /// A here-document body: quotes are plain characters.
    HeredocBody,
    /// A PowerShell word: nothing inside `'…'` is expanded, a backtick escapes
    /// the next character (so `` `$( `` is literal), and only `$(…)` runs.
    PowerShell,
}

/// The command substitutions (`$(…)`, `` `…` ``) the shell runs in `body` (a
/// here-document body, a redirection target, array elements), each with the
/// 0-based line of the body it starts on.
fn substitutions(body: &str, quoting: Quoting) -> Vec<(usize, String)> {
    let dialect = match quoting {
        Quoting::PowerShell => Dialect::PowerShell,
        Quoting::Shell | Quoting::HeredocBody => Dialect::Posix,
    };
    let mut found = Vec::new();
    let mut chars = body.chars().peekable();
    let mut line = 0;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        match c {
            '\n' => line += 1,
            '"' if quoting != Quoting::HeredocBody => in_double = !in_double,
            '`' if quoting == Quoting::PowerShell => {
                line += usize::from(chars.next() == Some('\n'));
            }
            // a single-quoted run is inert, but not inside double quotes
            '\'' if quoting != Quoting::HeredocBody && !in_double => {
                for quoted in chars.by_ref() {
                    line += usize::from(quoted == '\n');
                    if quoted == '\'' {
                        break;
                    }
                }
            }
            '\\' if quoting != Quoting::PowerShell => {
                if chars.next() == Some('\n') {
                    line += 1;
                }
            }
            '$' if chars.next_if_eq(&'(').is_some() => {
                let text = substitution_text(&mut chars, ')', dialect);
                let start = line;
                line += text.matches('\n').count();
                found.push((start, text));
            }
            '`' => {
                let text = substitution_text(&mut chars, '`', dialect);
                let start = line;
                line += text.matches('\n').count();
                found.push((start, text));
            }
            _ => {}
        }
    }
    found
}
