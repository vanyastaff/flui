//! A device scripted for the Android lane: [`ScriptedAdb`].

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::lane::android::adb::{
    Adb, AdbCommand, AdbReply, AdbStream, Framing, over_limit, timed_out,
};

/// How a rule answers.
#[derive(Debug, Clone)]
enum Answer {
    Reply(AdbReply),
    Timeout,
    /// The command unwinds, as a device binding the driver does not own may.
    Unwind,
}

#[derive(Debug, Clone)]
struct Rule {
    prefix: String,
    answer: Answer,
    /// How long the command runs before it answers: a slow device. One that runs to its
    /// deadline or past it is ended there, as the host ends it.
    taking: Duration,
    once: bool,
}

/// What a stream answers each line written to it with.
pub type Responder = fn(&str) -> Vec<String>;

#[derive(Debug, Clone)]
struct StreamRule {
    prefix: String,
    records: Vec<String>,
    /// Records that come only to a read that waits: what arrives while the lane waits.
    late: Vec<String>,
    responder: Option<Responder>,
    refuse: bool,
    /// Whether the program has ended once its records are read.
    ends: bool,
}

/// A record pushed into an open stream when a command runs.
#[derive(Debug, Clone)]
struct Push {
    command: String,
    stream: String,
    record: String,
}

#[derive(Debug, Default)]
struct StreamState {
    records: VecDeque<String>,
    late: VecDeque<String>,
    /// Records that arrive at a time: a reader refilling the channel from a pipe after a pause.
    timed: VecDeque<(std::time::Instant, String)>,
    ended: Option<String>,
    responder: Option<Responder>,
}

impl StreamState {
    /// Move the timed records that are due into the records.
    fn arrive(&mut self) {
        let now = std::time::Instant::now();
        while self.timed.front().is_some_and(|(at, _)| *at <= now) {
            if let Some((_, record)) = self.timed.pop_front() {
                self.records.push_back(record);
            }
        }
    }
}

#[derive(Debug, Default)]
struct Script {
    rules: Vec<Rule>,
    streams: Vec<StreamRule>,
    pushes: Vec<Push>,
    open: Vec<(String, Arc<Mutex<StreamState>>)>,
    log: Vec<String>,
}

/// A device scripted for the Android lane, answering each command by the first rule its text
/// starts with, so the lane's every command, refusal and release runs without a device or an
/// `adb`. Clones share one script and one log, so a test keeps one while the lane owns another.
#[derive(Debug, Clone, Default)]
pub struct ScriptedAdb {
    script: Arc<Mutex<Script>>,
}

fn lock(script: &Mutex<Script>) -> std::sync::MutexGuard<'_, Script> {
    script.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScriptedAdb {
    /// A device that answers nothing yet: a command no rule names exits 1 and says so.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every command whose text (`shell input keyevent KEYCODE_ENTER`) starts with
    /// `prefix` with `reply`, once the rules added before it pass.
    #[must_use]
    pub fn reply(self, prefix: impl Into<String>, reply: AdbReply) -> Self {
        self.rule(prefix.into(), Answer::Reply(reply), Duration::ZERO, false)
    }

    /// Answer the next command starting with `prefix` with `reply`, before any lasting rule.
    #[must_use]
    pub fn reply_once(self, prefix: impl Into<String>, reply: AdbReply) -> Self {
        self.rule(prefix.into(), Answer::Reply(reply), Duration::ZERO, true)
    }

    /// Answer the next command starting with `prefix` with `reply` once it has run for `taking`,
    /// as a device whose command is slow does, before any lasting rule. A command whose deadline
    /// comes first is ended at it, and times out as the host's does.
    #[must_use]
    pub fn reply_once_after(
        self,
        prefix: impl Into<String>,
        reply: AdbReply,
        taking: Duration,
    ) -> Self {
        self.rule(prefix.into(), Answer::Reply(reply), taking, true)
    }

    /// Let the next command starting with `prefix` outlive its deadline.
    #[must_use]
    pub fn timeout_once(self, prefix: impl Into<String>) -> Self {
        self.rule(prefix.into(), Answer::Timeout, Duration::ZERO, true)
    }

    /// Let the next command starting with `prefix` unwind, as an [`Adb`] the driver does not own
    /// may; the payload is the command's text.
    #[must_use]
    pub fn unwind_once(self, prefix: impl Into<String>) -> Self {
        self.rule(prefix.into(), Answer::Unwind, Duration::ZERO, true)
    }

    fn rule(self, prefix: String, answer: Answer, taking: Duration, once: bool) -> Self {
        lock(&self.script).rules.push(Rule {
            prefix,
            answer,
            taking,
            once,
        });
        self
    }

    /// Open a stream for a command starting with `prefix` that prints `records`, then waits, and
    /// answers each line written to it with what `responder` makes of it.
    #[must_use]
    pub fn stream(
        self,
        prefix: impl Into<String>,
        records: Vec<String>,
        responder: Option<Responder>,
    ) -> Self {
        lock(&self.script).streams.push(StreamRule {
            prefix: prefix.into(),
            records,
            late: Vec::new(),
            responder,
            refuse: false,
            ends: false,
        });
        self
    }

    /// The stream for a command starting with `prefix` also prints `records`, each only to a
    /// read that waits: lines that arrive while the lane waits for them.
    #[must_use]
    pub fn late_records(self, prefix: &str, records: Vec<String>) -> Self {
        if let Some(rule) = lock(&self.script)
            .streams
            .iter_mut()
            .rev()
            .find(|rule| rule.prefix == prefix)
        {
            rule.late.extend(records);
        }
        self
    }

    /// Open a stream for a command starting with `prefix` that prints `records` and ends, as a
    /// program that exits at once does.
    #[must_use]
    pub fn ending_stream(self, prefix: impl Into<String>, records: Vec<String>) -> Self {
        lock(&self.script).streams.push(StreamRule {
            prefix: prefix.into(),
            records,
            late: Vec::new(),
            responder: None,
            refuse: false,
            ends: true,
        });
        self
    }

    /// Refuse to start a stream for a command starting with `prefix`, as a device without the
    /// program does.
    #[must_use]
    pub fn refuse_stream(self, prefix: impl Into<String>) -> Self {
        lock(&self.script).streams.push(StreamRule {
            prefix: prefix.into(),
            records: Vec::new(),
            late: Vec::new(),
            responder: None,
            refuse: true,
            ends: false,
        });
        self
    }

    /// Push `record` into every open stream whose command starts with `stream`, now.
    pub fn push(&self, stream: &str, record: &str) {
        let script = lock(&self.script);
        for (command, state) in &script.open {
            if command.starts_with(stream) {
                state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .records
                    .push_back(record.to_owned());
            }
        }
    }

    /// Push `records` into every open stream whose command starts with `stream`, arriving `after`
    /// from now, in order: what a reader hands over once it refilled from the pipe.
    pub fn push_after(&self, stream: &str, records: &[String], after: Duration) {
        let at = std::time::Instant::now() + after;
        let script = lock(&self.script);
        for (command, state) in &script.open {
            if command.starts_with(stream) {
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                state
                    .timed
                    .extend(records.iter().map(|record| (at, record.clone())));
            }
        }
    }

    /// When a command starting with `command` runs, push `record` into the open stream whose
    /// command starts with `stream`, once.
    #[must_use]
    pub fn on_command(
        self,
        command: impl Into<String>,
        stream: impl Into<String>,
        record: impl Into<String>,
    ) -> Self {
        lock(&self.script).pushes.push(Push {
            command: command.into(),
            stream: stream.into(),
            record: record.into(),
        });
        self
    }

    /// End every open stream whose command starts with `prefix`, as a program that exits does.
    pub fn end_stream(&self, prefix: &str) {
        let script = lock(&self.script);
        for (command, state) in &script.open {
            if command.starts_with(prefix) {
                state.lock().unwrap_or_else(PoisonError::into_inner).ended =
                    Some(format!("`adb {command}` ended"));
            }
        }
    }

    /// The device, for the lane.
    #[must_use]
    pub fn device(&self) -> Box<dyn Adb> {
        Box::new(self.clone())
    }

    /// Every command run and stream opened, as its text, and every line written to a stream, as
    /// `<command> <- <line>`, in order.
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        lock(&self.script).log.clone()
    }

    /// The commands run whose text starts with `prefix`, in order.
    #[must_use]
    pub fn commands(&self, prefix: &str) -> Vec<String> {
        self.log()
            .into_iter()
            .filter(|line| line.starts_with(prefix))
            .collect()
    }
}

impl Adb for ScriptedAdb {
    fn run(&mut self, command: &AdbCommand, limit: usize, timeout: Duration) -> Result<AdbReply> {
        let text = command.to_string();
        let mut script = lock(&self.script);
        script.log.push(text.clone());
        let pushes: Vec<Push> = script
            .pushes
            .iter()
            .filter(|push| text.starts_with(&push.command))
            .cloned()
            .collect();
        script
            .pushes
            .retain(|push| !text.starts_with(&push.command));
        for push in pushes {
            if let Some((_, state)) = script
                .open
                .iter()
                .find(|(command, _)| command.starts_with(&push.stream))
            {
                state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .records
                    .push_back(push.record);
            }
        }
        let found = script
            .rules
            .iter()
            .position(|rule| rule.once && text.starts_with(&rule.prefix))
            .or_else(|| {
                script
                    .rules
                    .iter()
                    .position(|rule| !rule.once && text.starts_with(&rule.prefix))
            });
        let Some(index) = found else {
            return Ok(AdbReply::new(1, Vec::new())
                .stderr(format!("scripted adb: no rule answers `{text}`")));
        };
        let rule = if script.rules[index].once {
            script.rules.remove(index)
        } else {
            script.rules[index].clone()
        };
        // A slow command runs with the script let go of, as the device's command runs apart from
        // the host's other commands, and no longer than its deadline, where the host ends it.
        drop(script);
        if !rule.taking.is_zero() {
            std::thread::sleep(rule.taking.min(timeout));
        }
        if !rule.taking.is_zero() && rule.taking >= timeout {
            return Err(timed_out(command, timeout));
        }
        match rule.answer {
            Answer::Unwind => std::panic::resume_unwind(Box::new(text)),
            Answer::Timeout => Err(timed_out(command, timeout)),
            Answer::Reply(reply) if reply.stdout.len() > limit => Err(over_limit(command, limit)),
            Answer::Reply(reply) => Ok(reply),
        }
    }

    fn open(&mut self, command: &AdbCommand, framing: Framing) -> Result<Box<dyn AdbStream>> {
        let _ = framing;
        let text = command.to_string();
        let mut script = lock(&self.script);
        script.log.push(text.clone());
        let rule = script
            .streams
            .iter()
            .find(|rule| text.starts_with(&rule.prefix))
            .cloned();
        let Some(rule) = rule.filter(|rule| !rule.refuse) else {
            return Err(Error::Lane {
                lane: "android".to_owned(),
                unavailable: false,
                message: format!("scripted adb: no stream for `{text}`"),
            });
        };
        let state = Arc::new(Mutex::new(StreamState {
            records: rule.records.into_iter().collect(),
            late: rule.late.into_iter().collect(),
            timed: VecDeque::new(),
            ended: rule.ends.then(|| format!("`adb {text}` ended")),
            responder: rule.responder,
        }));
        script.open.push((text.clone(), Arc::clone(&state)));
        Ok(Box::new(ScriptedStream {
            command: text,
            state,
            script: Arc::clone(&self.script),
        }))
    }
}

/// A stream of a [`ScriptedAdb`].
struct ScriptedStream {
    command: String,
    state: Arc<Mutex<StreamState>>,
    script: Arc<Mutex<Script>>,
}

impl ScriptedStream {
    fn state(&self) -> std::sync::MutexGuard<'_, StreamState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl AdbStream for ScriptedStream {
    fn send_line(&mut self, line: &str) -> Result<()> {
        lock(&self.script)
            .log
            .push(format!("{} <- {line}", self.command));
        let mut state = self.state();
        if let Some(why) = &state.ended {
            return Err(Error::Lane {
                lane: "android".to_owned(),
                unavailable: false,
                message: why.clone(),
            });
        }
        if let Some(responder) = state.responder {
            state.records.extend(responder(line));
        }
        Ok(())
    }

    fn next_record(&mut self, wait: Duration) -> Result<Option<String>> {
        let deadline = std::time::Instant::now() + wait;
        let mut state = self.state();
        state.arrive();
        // A wait sees the timed records that arrive within it.
        while state.records.is_empty()
            && let Some(at) = state.timed.front().map(|(at, _)| *at)
            && at <= deadline
        {
            drop(state);
            std::thread::sleep(at.saturating_duration_since(std::time::Instant::now()));
            state = self.state();
            state.arrive();
        }
        if let Some(record) = state.records.pop_front() {
            return Ok(Some(record));
        }
        if !wait.is_zero()
            && let Some(record) = state.late.pop_front()
        {
            return Ok(Some(record));
        }
        if let Some(why) = &state.ended {
            return Err(Error::Lane {
                lane: "android".to_owned(),
                unavailable: false,
                message: why.clone(),
            });
        }
        drop(state);
        if !wait.is_zero() {
            std::thread::sleep(wait.min(Duration::from_millis(2)));
        }
        Ok(None)
    }
}

impl Drop for ScriptedStream {
    fn drop(&mut self) {
        let mut script = lock(&self.script);
        script.log.push(format!("{} ended", self.command));
        let state = Arc::clone(&self.state);
        script.open.retain(|(_, open)| !Arc::ptr_eq(open, &state));
    }
}
