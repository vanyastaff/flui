//! Shell code split into commands and words the way a POSIX shell splits
//! them, as far as finding the commands a doc runs needs:
//!
//! - quotes (`'…'`, `"…"`) join and are removed, `\` escapes a character and
//!   continues a line, `#` at the start of a word comments out the line;
//! - `&&`, `||`, `|`, `&`, `;`, `(`, `)` and a newline end a command,
//!   attached to a word or not;
//! - a redirection (`>log`, `2>&1`, `<in`) and its target are dropped, so
//!   `>log cargo test` is cargo's command;
//! - a here-document's body (`<<EOF` … `EOF`, the delimiter quoted or not) is
//!   skipped as data;
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
    heredocs: Vec<(String, bool)>,
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
        let mut text = String::new();
        let mut depth = 0_usize;
        let mut quote: Option<char> = None;
        // the text keeps its quotes: the nested lexing reads them
        while let Some(c) = self.chars.next() {
            if let Some(open) = quote {
                if c == open {
                    quote = None;
                }
                text.push(c);
                continue;
            }
            match c {
                '\\' => {
                    text.push(c);
                    text.extend(self.chars.next());
                    continue;
                }
                '\'' | '"' => quote = Some(c),
                c if c == closing && depth == 0 => break,
                '(' if closing == ')' => depth += 1,
                ')' if closing == ')' => depth -= 1,
                _ => {}
            }
            text.push(c);
        }
        self.line += text.matches('\n').count();
        for mut command in commands(&text) {
            for (line, _) in &mut command {
                *line += start;
            }
            self.nested.push(command);
        }
        self.push('$');
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
        self.read_word(|c| delimiter.push(c));
        if !delimiter.is_empty() {
            self.heredocs.push((delimiter, strip_tabs));
        }
    }

    /// Reads one shell word, quotes removed, handing each character to `each`.
    fn read_word(&mut self, mut each: impl FnMut(char)) {
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
                        '\'' | '"' => quote = Some(next),
                        '\\' => each(self.chars.next().unwrap_or_default()),
                        next => each(next),
                    }
                }
            }
        }
    }

    /// Skips the bodies of the here-documents opened on the line just ended.
    fn skip_heredoc_bodies(&mut self) {
        for (delimiter, strip_tabs) in std::mem::take(&mut self.heredocs) {
            loop {
                let mut body_line = String::new();
                while let Some(next) = self.chars.next_if(|&next| next != '\n') {
                    body_line.push(next);
                }
                let ended = self.chars.next().is_none();
                self.line += usize::from(!ended);
                let body_line = body_line.trim_end_matches('\r');
                let body_line = if strip_tabs {
                    body_line.trim_start_matches('\t')
                } else {
                    body_line
                };
                if ended || body_line == delimiter {
                    break;
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
