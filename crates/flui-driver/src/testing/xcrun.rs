//! A Simulator scripted for the iOS Simulator lane: [`ScriptedXcrun`].

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::error::ErrorCode;

use crate::error::{Error, Result};
use crate::lane::ios_simulator::LANE;
use crate::lane::ios_simulator::device::Udid;
use crate::lane::ios_simulator::xcrun::{Xcrun, XcrunCommand, XcrunReply};

/// How a rule answers.
#[derive(Debug, Clone)]
enum Answer {
    Reply(XcrunReply),
    Timeout,
}

#[derive(Debug, Clone)]
struct Rule {
    prefix: String,
    answer: Answer,
    once: bool,
}

#[derive(Debug, Default)]
struct Script {
    rules: Vec<Rule>,
    /// The screenshots the simulator answers with, in order; the last one lasts.
    screenshots: VecDeque<Vec<u8>>,
    commands: Vec<XcrunCommand>,
}

/// A Simulator scripted for the iOS Simulator lane, answering each command by the first rule its
/// text starts with, so the lane's every command and refusal runs without a Mac. Clones share one
/// script and one log, so a test keeps one while the lane owns another.
#[derive(Debug, Clone, Default)]
pub struct ScriptedXcrun {
    script: Arc<Mutex<Script>>,
}

fn lock(script: &Mutex<Script>) -> std::sync::MutexGuard<'_, Script> {
    script.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ScriptedXcrun {
    /// A Simulator that answers nothing yet: a command no rule names exits 1 and says so, and a
    /// screenshot fails.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every command whose text (`simctl list devices --json`) starts with `prefix` with
    /// `reply`, once the rules added before it pass.
    #[must_use]
    pub fn reply(self, prefix: impl Into<String>, reply: XcrunReply) -> Self {
        self.rule(prefix.into(), Answer::Reply(reply), false)
    }

    /// Let the next command starting with `prefix` outlive its deadline.
    #[must_use]
    pub fn timeout_once(self, prefix: impl Into<String>) -> Self {
        self.rule(prefix.into(), Answer::Timeout, true)
    }

    fn rule(self, prefix: String, answer: Answer, once: bool) -> Self {
        lock(&self.script).rules.push(Rule {
            prefix,
            answer,
            once,
        });
        self
    }

    /// Answer the next screenshot with `png`; the last one scripted answers every later one.
    #[must_use]
    pub fn screenshot(self, png: Vec<u8>) -> Self {
        lock(&self.script).screenshots.push_back(png);
        self
    }

    /// The Simulator, for the lane.
    #[must_use]
    pub fn simulator(&self) -> Box<dyn Xcrun> {
        Box::new(self.clone())
    }

    /// Every command run, screenshots included, in order.
    #[must_use]
    pub fn commands(&self) -> Vec<XcrunCommand> {
        lock(&self.script).commands.clone()
    }

    /// The text of every command run, in order.
    #[must_use]
    pub fn log(&self) -> Vec<String> {
        self.commands().iter().map(ToString::to_string).collect()
    }
}

impl Xcrun for ScriptedXcrun {
    fn run(
        &mut self,
        command: &XcrunCommand,
        limit: usize,
        timeout: Duration,
    ) -> Result<XcrunReply> {
        let text = command.to_string();
        let mut script = lock(&self.script);
        script.commands.push(command.clone());
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
            return Ok(XcrunReply::new(1, Vec::new())
                .stderr(format!("scripted xcrun: no rule answers `{text}`")));
        };
        let rule = if script.rules[index].once {
            script.rules.remove(index)
        } else {
            script.rules[index].clone()
        };
        match rule.answer {
            Answer::Timeout => Err(Error::Timeout {
                effect: Some(Effect::new(EffectKind::MayHaveRun)),
                message: format!(
                    "`xcrun {text}` did not end within {} ms",
                    timeout.as_millis()
                ),
                retry: ErrorCode::Timeout.default_retry(),
            }),
            Answer::Reply(reply) if reply.stdout.len() > limit => Err(Error::Lane {
                lane: LANE.to_owned(),
                unavailable: false,
                message: format!("`xcrun {text}` printed more than {limit} bytes"),
            }),
            Answer::Reply(reply) => Ok(reply),
        }
    }

    fn screenshot(&mut self, udid: &Udid, _timeout: Duration) -> Result<Vec<u8>> {
        let mut script = lock(&self.script);
        script.commands.push(XcrunCommand::Screenshot {
            udid: udid.clone(),
            path: "screen.png".into(),
        });
        let png = if script.screenshots.len() > 1 {
            script.screenshots.pop_front()
        } else {
            script.screenshots.front().cloned()
        };
        png.ok_or_else(|| Error::Lane {
            lane: LANE.to_owned(),
            unavailable: false,
            message: "scripted xcrun: no screenshot is scripted".to_owned(),
        })
    }
}
