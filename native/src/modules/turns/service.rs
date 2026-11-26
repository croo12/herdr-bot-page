use crate::modules::transcript::content;
use crate::{
    domain::{need, s, strings},
    modules::{chats::change, transcript::append},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};
use std::{sync::Arc, thread};
pub(crate) struct Job {
    pub seed: String,
    pub depth: u8,
    pub exclude: Option<String>,
}
pub(crate) struct Turn {
    pub chat: String,
    pub task: String,
    pub(super) depth: u8,
    pub(super) spoken: u8,
    pub(super) passed: bool,
    pub(super) delegated: bool,
}

pub(crate) fn enqueue(engine: &Arc<Engine>, chat_id: &str, job: Job) {
    let mut queues = engine.queues.lock().unwrap();
    let start = !queues.contains_key(chat_id);
    queues.entry(chat_id.into()).or_default().push_back(job);
    drop(queues);
    if !start {
        return;
    }
    let engine = engine.clone();
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
        room_activity(&engine, &target, true);
        if let Err(err) = cycle(&engine, &target, job) {
            notice(&engine, &target, &format!("{err:#}"));
        }
        room_activity(&engine, &target, false);
    });
}
fn room_activity(engine: &Engine, target: &str, running: bool) {
    let _ = engine.tx(|c, events| {
        let mut row = c.chats.get(target)?;
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
pub(super) fn notice(engine: &Engine, target: &str, text: &str) {
    let _ = engine.tx(|c, ev| append(c, target, json!({"kind":"notice","content":text}), ev));
}
fn cycle(engine: &Engine, target: &str, job: Job) -> Result<()> {
    let v = engine.tx(|c, _| c.chats.get(target))?;
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
                let _roster = engine.roster.lock().unwrap();
                if engine.tx(|c, _| c.chats.get(bot)).is_err() {
                    continue;
                }
                let target_row = engine.tx(|c, _| c.chats.get(target))?;
                if room && !strings(&target_row["memberIds"]).contains(bot) {
                    continue;
                }
                if engine.tx(|c, _| c.reviews.pending_for(bot))? {
                    notice(
                        engine,
                        target,
                        &format!("{bot} is reviewing a permission request."),
                    );
                    continue;
                }
                let mut active = engine.active.lock().unwrap();
                if active.contains_key(bot) {
                    drop(active);
                    notice(engine, target, &format!("{bot} is busy in another chat."));
                    continue;
                }
                active.insert(
                    bot.clone(),
                    Turn {
                        chat: target.into(),
                        task: job.seed.clone(),
                        depth: job.depth,
                        spoken: 0,
                        passed: false,
                        delegated: false,
                    },
                );
            }
            let result = prompt(engine, bot, target, &job.seed);
            let turn = engine.active.lock().unwrap().remove(bot);
            let spoken = turn.as_ref().is_some_and(|t| t.spoken > 0);
            any_spoke |= spoken;
            if let Err(e) = result {
                notice(engine, target, &format!("{bot}: {e:#}"));
            } else if !spoken && !turn.is_some_and(|t| t.passed) {
                notice(
                    engine,
                    target,
                    &format!("{bot} finished without posting a reply."),
                );
            }
        }
        if !any_spoke {
            break;
        }
    }
    Ok(())
}
fn prompt(engine: &Engine, bot: &str, target: &str, seed: &str) -> Result<()> {
    let (profile, entries, room) = engine.tx(|c, _| {
        Ok((
            c.chats.get(bot)?,
            c.transcript.page(target, i64::MAX, 24)?,
            c.chats.get(target)?,
        ))
    })?;
    let pane = need(&profile["bot"], "paneId")?;
    let live = engine.herdr.run(&["agent", "get", pane])?;
    if live["agent"]["name"] != bot {
        bail!("bot_not_ready: pane has changed")
    }
    if matches!(s(&live["agent"], "agent_status"), "working" | "blocked") {
        bail!("bot_not_ready: {}", live["agent"]["agent_status"])
    }
    let history = entries
        .iter()
        .filter(|e| matches!(s(e, "kind"), "message" | "send-message" | "prompt"))
        .map(|e| {
            let author = s(&e["author"], "name");
            if e["kind"] == "prompt" {
                format!(
                    "{author} requested permission or input: {} [{}]",
                    content(e),
                    s(e, "status")
                )
            } else if let Some(decision) = e["permissionDecision"].as_str() {
                format!("{author} ({decision}): {}", content(e))
            } else {
                format!("{author}: {}", content(e))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cli = engine.herdr.cli.to_string_lossy().replace('\\', "/");
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
    let limit = timeout.to_string();
    let command_timeout = timeout.div_ceil(1000) + 10;
    let mut result = engine.herdr.run_timeout(
        &[
            "agent",
            "prompt",
            pane,
            &prompt,
            "--wait",
            "--timeout",
            &limit,
        ],
        command_timeout,
    );
    loop {
        // Once submitted, a slow turn and an approval both retain their original
        // chat and CLI reply channel. Only the status wait may be repeated.
        let snapshot = match result {
            Ok(snapshot) => snapshot,
            Err(error) if crate::infrastructure::herdr::is_herdr_timeout(&error) => {
                let snapshot = engine.herdr.run(&["agent", "get", pane])?;
                if snapshot["agent"]["agent_status"] == "unknown" {
                    return Err(error);
                }
                snapshot
            }
            Err(error) => return Err(error),
        };
        let agent = &snapshot["agent"];
        if agent["name"] != bot || agent["pane_id"] != pane {
            bail!("bot_not_ready: pane has changed");
        }
        match s(agent, "agent_status") {
            "done" | "idle" => return Ok(()),
            "working" | "blocked" => {}
            status => bail!("bot_not_ready: agent status is {status}"),
        }
        result = engine.herdr.run_timeout(
            &[
                "agent",
                "wait",
                pane,
                "--until",
                "done",
                "--until",
                "idle",
                "--timeout",
                &limit,
            ],
            command_timeout,
        );
    }
}
