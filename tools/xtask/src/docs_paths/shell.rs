//! Shell code split into commands and words the way a POSIX shell splits
//! them, as far as finding the commands a doc runs needs: quotes (`'…'`,
//! `"…"`) join and are removed, `\` escapes a character and continues a line,
//! `#` at the start of a word comments out the rest of the line, and `&&`,
//! `||`, `|`, `&`, `;`, `(`, `)` and a newline end a command, attached to a
//! word or not. A here-document's body (`<<EOF` … `EOF`) is skipped as data.
//! No expansion: `$VAR` stays as written.

/// One command: its words, each with the 0-based line of the code it starts on.
pub(super) type Command = Vec<(usize, String)>;

/// The commands of `code`, empty ones left out.
pub(super) fn commands(code: &str) -> Vec<Command> {
    let mut commands = vec![Command::new()];
    let mut word: Option<(usize, String)> = None;
    let mut line = 0;
    let mut chars = code.chars().peekable();
    // a word ends at a separator; a command ends at an operator or a newline
    let flush = |word: &mut Option<(usize, String)>, commands: &mut Vec<Command>| {
        if let Some(done) = word.take() {
            commands
                .last_mut()
                .expect("BUG: there is always a current command")
                .push(done);
        }
    };
    let end = |commands: &mut Vec<Command>| {
        if commands.last().is_some_and(|command| !command.is_empty()) {
            commands.push(Command::new());
        }
    };
    // the here-documents opened on this line: each delimiter, and whether
    // `<<-` strips leading tabs; their bodies follow the newline
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\r' => flush(&mut word, &mut commands),
            '\n' => {
                flush(&mut word, &mut commands);
                end(&mut commands);
                line += 1;
                // a here-document's body is data, not commands
                for (delimiter, strip_tabs) in heredocs.drain(..) {
                    loop {
                        let mut body_line = String::new();
                        while let Some(next) = chars.next_if(|&next| next != '\n') {
                            body_line.push(next);
                        }
                        let ended = chars.next().is_none();
                        line += usize::from(!ended);
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
            // `<<WORD`/`<<-WORD` opens a here-document; `<<<` is a here-string
            '<' if chars.peek() == Some(&'<') => {
                flush(&mut word, &mut commands);
                chars.next();
                if chars.next_if_eq(&'<').is_some() {
                    continue;
                }
                let strip_tabs = chars.next_if_eq(&'-').is_some();
                while chars.next_if(|&next| next == ' ' || next == '\t').is_some() {}
                let mut delimiter = String::new();
                while let Some(next) =
                    chars.next_if(|&next| !next.is_whitespace() && !";&|()<>".contains(next))
                {
                    // quoting the delimiter only turns expansion off
                    if !matches!(next, '\'' | '"' | '\\') {
                        delimiter.push(next);
                    }
                }
                if !delimiter.is_empty() {
                    heredocs.push((delimiter, strip_tabs));
                }
            }
            // `(`/`)` group commands (`(cargo test)`): a command ends there too
            '&' | '|' | ';' | '(' | ')' => {
                flush(&mut word, &mut commands);
                if matches!(c, '&' | '|') && chars.peek() == Some(&c) {
                    chars.next();
                }
                end(&mut commands);
            }
            '#' if word.is_none() => {
                while chars.peek().is_some_and(|&next| next != '\n') {
                    chars.next();
                }
            }
            '\\' => match chars.next() {
                Some('\n') => line += 1,
                Some(escaped) => push(&mut word, line, escaped),
                None => {}
            },
            '\'' => {
                word.get_or_insert_with(|| (line, String::new()));
                for quoted in chars.by_ref() {
                    if quoted == '\'' {
                        break;
                    }
                    line += usize::from(quoted == '\n');
                    push(&mut word, line, quoted);
                }
            }
            '"' => {
                word.get_or_insert_with(|| (line, String::new()));
                while let Some(quoted) = chars.next() {
                    match quoted {
                        '"' => break,
                        // a line continuation inside quotes too
                        '\\' if chars.peek() == Some(&'\n') => {
                            chars.next();
                            line += 1;
                        }
                        '\\' if chars
                            .peek()
                            .is_some_and(|next| matches!(next, '"' | '\\' | '$' | '`')) =>
                        {
                            let escaped = chars.next().expect("BUG: peeked");
                            push(&mut word, line, escaped);
                        }
                        quoted => {
                            line += usize::from(quoted == '\n');
                            push(&mut word, line, quoted);
                        }
                    }
                }
            }
            c => push(&mut word, line, c),
        }
    }
    flush(&mut word, &mut commands);
    commands.retain(|command| !command.is_empty());
    commands
}

/// `c` appended to `word`, which starts on `line` when it is new.
fn push(word: &mut Option<(usize, String)>, line: usize, c: char) {
    word.get_or_insert_with(|| (line, String::new())).1.push(c);
}
