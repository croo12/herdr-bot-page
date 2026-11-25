use crate::{store::*, Engine};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value as V};
use std::{sync::Arc, thread};

pub(crate) struct Job {
    pub seed: String,
    pub depth: u8,
    pub exclude: Option<String>,
}
pub(crate) struct Turn {
    pub chat: String,
    depth: u8,
    spoken: u8,
    passed: bool,
    delegated: bool,
}

impl Engine {
    pub(crate) fn enqueue(self: &Arc<Self>, chat_id: &str, job: Job) {
        let mut queues = self.queues.lock().unwrap();
        let start = !queues.contains_key(chat_id);
        queues.entry(chat_id.into()).or_default().push_back(job);
        drop(queues);
        if !start {
            return;
        }
        let engine = self.clone();
        let target = chat_id.to_owned();
        thread::spawn(move || loop {
            let job = {
                let mut queues = engine.queues.lock().unwrap();
                match queues.get_mut(&target).and_then(|q| q.pop_front()) {
                    Some(j) => j,
                    None => {
                        queues.remove(&target);
                        return;
                    }
                }
            };
            engine.room_activity(&target, true);
            if let Err(err) = engine.cycle(&target, job) {
                engine.notice(&target, &format!("{err:#}"));
            }
            engine.room_activity(&target, false);
        });
    }
    fn room_activity(&self, target: &str, running: bool) {
        let _ = self.tx(|c, events| {
            let mut row = chat(c, target)?;
            if row["kind"] == "room" && row["isRunning"] != running {
                row["isRunning"] = running.into();
                row["currentActivity"] = if running {
                    json!({"verb":"working"})
                } else {
                    V::Null
                };
                change(c, &row, events)?;
            }
            Ok(())
        });
    }
    fn notice(&self, target: &str, text: &str) {
        let _ = self.tx(|c, ev| append(c, target, json!({"kind":"notice","content":text}), ev));
    }
    fn cycle(&self, target: &str, job: Job) -> Result<()> {
        let v = self.tx(|c, _| chat(c, target))?;
        let room = v["kind"] == "room";
        let mut members = if room {
            strings(&v["memberIds"])
        } else {
            vec![target.to_owned()]
        };
        members.retain(|m| job.exclude.as_ref() != Some(m));
        let seed = job.seed.to_lowercase();
        members.sort_by_key(|m| seed.find(&format!("@{m}")).unwrap_or(usize::MAX));
        let mut turns = 0;
        for _ in 0..if room { 3 } else { 1 } {
            let mut any_spoke = false;
            for bot in &members {
                if turns >= 10 {
                    return Ok(());
                }
                turns += 1;
                {
                    let _roster = self.roster.lock().unwrap();
                    if self.tx(|c, _| chat(c, bot)).is_err() {
                        continue;
                    }
                    let target_row = self.tx(|c, _| chat(c, target))?;
                    if room && !strings(&target_row["memberIds"]).contains(bot) {
                        continue;
                    }
                    let mut active = self.active.lock().unwrap();
                    if active.contains_key(bot) {
                        drop(active);
                        self.notice(target, &format!("{bot} is busy in another chat."));
                        continue;
                    }
                    active.insert(
                        bot.clone(),
                        Turn {
                            chat: target.into(),
                            depth: job.depth,
                            spoken: 0,
                            passed: false,
                            delegated: false,
                        },
                    );
                }
                let result = self.prompt(bot, target, &job.seed);
                let turn = self.active.lock().unwrap().remove(bot);
                let spoken = turn.as_ref().is_some_and(|t| t.spoken > 0);
                any_spoke |= spoken;
                if let Err(e) = result {
                    self.notice(target, &format!("{bot}: {e:#}"));
                } else if !spoken && !turn.is_some_and(|t| t.passed) {
                    self.notice(target, &format!("{bot} finished without posting a reply."));
                }
            }
            if !any_spoke {
                break;
            }
        }
        Ok(())
    }
    fn prompt(&self, bot: &str, target: &str, seed: &str) -> Result<()> {
        let (profile, entries, room) = self.tx(|c, _| {
            Ok((
                chat(c, bot)?,
                page(c, target, i64::MAX, 24)?,
                chat(c, target)?,
            ))
        })?;
        let pane = need(&profile["bot"], "paneId")?;
        let live = self.herdr.run(&["agent", "get", pane])?;
        if live["agent"]["name"] != bot {
            bail!("bot_not_ready: pane has changed")
        }
        if matches!(s(&live["agent"], "agent_status"), "working" | "blocked") {
            bail!("bot_not_ready: {}", live["agent"]["agent_status"])
        }
        let history = entries
            .iter()
            .filter(|e| matches!(s(e, "kind"), "message" | "send-message"))
            .map(|e| format!("{}: {}", s(&e["author"], "name"), content(e)))
            .collect::<Vec<_>>()
            .join("\n");
        let cli = self.herdr.cli.to_string_lossy().replace('\\', "/");
        let prompt=format!("You are {} (bot ID {bot}) in herdr-bot chat {target}. Role: {}. Members: {}.\n\
            Use the native CLI at {cli} from this pane. Run it with arguments: say {target} \"your reply\" (or pass {target} if finished). \
            Call the CLI to post; terminal output is private. In PowerShell invoke a quoted path with &. \
            To delegate, use message <bot-or-room-id> \"task\". Do not repeat earlier replies.\n\
            Current request:\n{}\nRecent conversation:\n{}",s(&profile,"name"),s(&profile,"description"),room["memberIds"],seed,history);
        let timeout = std::env::var("HERDR_BOT_TURN_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(180000)
            .clamp(1000, 3600000);
        self.herdr.run_timeout(
            &[
                "agent",
                "prompt",
                pane,
                &prompt,
                "--wait",
                "--until",
                "done",
                "--until",
                "idle",
                "--timeout",
                &timeout.to_string(),
            ],
            timeout / 1000 + 10,
        )?;
        Ok(())
    }
    pub(crate) fn bot_cli(self: &Arc<Self>, method: &str, p: V) -> Result<V> {
        let pane = need(&p, "paneId")?;
        let bot = self
            .tx(|c, _| list(c))?
            .into_iter()
            .find(|x| x["bot"]["paneId"] == pane)
            .ok_or_else(|| anyhow!("unknown_bot: pane {pane}"))?;
        let bot_id = need(&bot, "id")?;
        if method == "cli.whoami" || method == "cli.profile" {
            return Ok(bot);
        }
        if method == "cli.profileUpdate" {
            let mut patch = p;
            patch["chatId"] = bot_id.into();
            return self.dispatch("chat.updateProfile", patch);
        }
        let target = need(&p, "chatId")?;
        let target_chat = self.tx(|c, _| chat(c, target))?;
        let mut active = self.active.lock().unwrap();
        let turn = active
            .get_mut(bot_id)
            .ok_or_else(|| anyhow!("bot_not_ready: no open turn for {bot_id}"))?;
        let cross = method == "cli.message";
        if !cross && target != turn.chat {
            bail!("invalid_args: wrong chat for this turn")
        }
        if cross
            && (turn.depth >= 2
                || turn.delegated
                || target == turn.chat
                || target == bot_id
                || (target_chat["kind"] == "room"
                    && !strings(&target_chat["memberIds"]).contains(&bot_id.to_owned())))
        {
            bail!("invalid_args: delegation is not available for this target or turn")
        }
        if turn.passed || turn.spoken >= 2 {
            bail!("bot_not_ready: turn has ended")
        }
        if method == "cli.pass" {
            turn.passed = true;
            return Ok(json!({"passed":true}));
        }
        let text = need(&p, "text")?;
        let e=self.tx(|c,ev|append(c,target,json!({"kind":"send-message","message":{"type":"text","content":text},"author":{"id":bot_id,"name":bot["name"]}}),ev))?;
        turn.spoken += 1;
        if cross {
            turn.delegated = true;
            let depth = turn.depth + 1;
            let from = turn.chat.clone();
            drop(active);
            self.notice(&from, &format!("{bot_id} sent a message to {target}."));
            self.enqueue(
                target,
                Job {
                    seed: text.into(),
                    depth,
                    exclude: Some(bot_id.into()),
                },
            );
        }
        Ok(json!({"entryId":e["id"]}))
    }
}
