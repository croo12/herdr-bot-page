use crate::modules::transcript::content;
use crate::{
    domain::{need, string_field, strings},
    modules::{chats::change, transcript::append},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::{sync::Arc, thread};
pub(crate) struct Job {
    pub seed: String,
    pub depth: u8,
    pub exclude: Option<String>,
}
pub(crate) enum QueueItem {
    Chat(Job),
    Task(String),
}

pub(crate) struct Turn {
    pub chat: String,
    pub task: String,
    pub(super) depth: u8,
    pub(super) spoken: u8,
    pub(super) passed: bool,
    pub(super) delegated: bool,
}

impl Turn {
    pub(crate) fn new(chat: &str, task: &str, depth: u8) -> Self {
        Self {
            chat: chat.into(),
            task: task.into(),
            depth,
            spoken: 0,
            passed: false,
            delegated: false,
        }
    }
}

pub(crate) fn enqueue(engine: &Arc<Engine>, chat_id: &str, job: Job) {
    enqueue_item(engine, chat_id, QueueItem::Chat(job));
}

pub(crate) fn enqueue_item(engine: &Arc<Engine>, chat_id: &str, job: QueueItem) {
    let mut queues = engine.queues.lock().unwrap();
    let start = !queues.contains_key(chat_id);
    let queue = queues.entry(chat_id.into()).or_default();
    if let QueueItem::Task(entry_id) = &job {
        if queue
            .iter()
            .any(|item| matches!(item, QueueItem::Task(id) if id == entry_id))
        {
            return;
        }
    }
    queue.push_back(job);
    drop(queues);
    if !start {
        return;
    }
    let engine = engine.clone();
    let target = chat_id.to_owned();
    thread::spawn(move || loop {
        let job = {
            let mut queues = engine.queues.lock().unwrap();
            match queues.get_mut(&target).and_then(|queue| queue.pop_front()) {
                Some(next_job) => next_job,
                None => {
                    queues.remove(&target);
                    return;
                }
            }
        };
        room_activity(&engine, &target, true);
        let result = match job {
            QueueItem::Chat(job) => cycle(&engine, &target, job),
            QueueItem::Task(entry_id) => {
                crate::modules::tasks::execute(&engine, &target, &entry_id)
            }
        };
        if let Err(err) = result {
            notice(&engine, &target, &format!("{err:#}"));
        }
        room_activity(&engine, &target, false);
    });
}
fn room_activity(engine: &Engine, target: &str, running: bool) {
    let _ = engine.tx(|repos, events| {
        let mut row = repos.chats.get(target)?;
        if row["kind"] == "room" && row["isRunning"] != running {
            row["isRunning"] = running.into();
            row["currentActivity"] = if running {
                json!({"verb":"working"})
            } else {
                Value::Null
            };
            change(repos, &row, events)?;
        }
        Ok(())
    });
}
pub(super) fn notice(engine: &Engine, target: &str, text: &str) {
    let _ = engine.tx(|repos, events| {
        append(
            repos,
            target,
            json!({"kind":"notice","content":text}),
            events,
        )
    });
}
fn cycle(engine: &Engine, target: &str, job: Job) -> Result<()> {
    let chat = engine.tx(|repos, _| repos.chats.get(target))?;
    let room = chat["kind"] == "room";
    let mut members = if room {
        strings(&chat["memberIds"])
    } else {
        vec![target.to_owned()]
    };
    members.retain(|member_id| job.exclude.as_ref() != Some(member_id));
    let seed = job.seed.to_lowercase();
    members.sort_by_key(|member_id| seed.find(&format!("@{member_id}")).unwrap_or(usize::MAX));
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
                if engine.tx(|repos, _| repos.chats.get(bot)).is_err() {
                    continue;
                }
                let target_row = engine.tx(|repos, _| repos.chats.get(target))?;
                if room && !strings(&target_row["memberIds"]).contains(bot) {
                    continue;
                }
                if engine.tx(|repos, _| repos.reviews.pending_for(bot))? {
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
                active.insert(bot.clone(), Turn::new(target, &job.seed, job.depth));
            }
            let result = prompt(engine, bot, target, &job.seed);
            let turn = engine.active.lock().unwrap().remove(bot);
            let spoken = turn
                .as_ref()
                .is_some_and(|active_turn| active_turn.spoken > 0);
            any_spoke |= spoken;
            if let Err(error) = result {
                notice(engine, target, &format!("{bot}: {error:#}"));
            } else if !spoken && !turn.is_some_and(|active_turn| active_turn.passed) {
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
    let (profile, entries, room) = engine.tx(|repos, _| {
        Ok((
            repos.chats.get(bot)?,
            repos.transcript.page(target, i64::MAX, 24)?,
            repos.chats.get(target)?,
        ))
    })?;
    let live = {
        let _roster = engine.roster.lock().unwrap();
        crate::modules::bots::connect_for_turn(engine, bot)?
    };
    let pane = need(&live, "pane_id")?;
    if matches!(string_field(&live, "agent_status"), "working" | "blocked") {
        bail!("bot_not_ready: {}", live["agent_status"])
    }
    let history = entries
        .iter()
        .filter(|entry| {
            matches!(
                string_field(entry, "kind"),
                "message" | "send-message" | "prompt"
            )
        })
        .map(|entry| {
            let author = string_field(&entry["author"], "name");
            if entry["kind"] == "prompt" {
                format!(
                    "{author} requested permission or input: {} [{}]",
                    content(entry),
                    string_field(entry, "status")
                )
            } else if let Some(decision) = entry["permissionDecision"].as_str() {
                format!("{author} ({decision}): {}", content(entry))
            } else {
                format!("{author}: {}", content(entry))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cli = engine.herdr.cli.to_string_lossy().replace('\\', "/");
    let prompt=format!("You are {} (bot ID {bot}) in herdr-bot chat {target}. Role: {}. Members: {}.\n\
        Use the native CLI at {cli} from this pane. Run it with arguments: say {target} \"your reply\" (or pass {target} if finished). \
        Call the CLI to post; terminal output is private. In PowerShell invoke a quoted path with &. \
        To delegate, use message <bot-or-room-id> \"task\". Do not repeat earlier replies.\n\
        Current request:\n{}\nRecent conversation:\n{}",string_field(&profile,"name"),string_field(&profile,"description"),room["memberIds"],seed,history);
    run_or_resume_prompt(engine, bot, pane, Some(&prompt))
}

pub(crate) fn run_or_resume_prompt(
    engine: &Engine,
    bot: &str,
    pane: &str,
    prompt: Option<&str>,
) -> Result<()> {
    let timeout = std::env::var("HERDR_BOT_TURN_TIMEOUT_MS")
        .ok()
        .and_then(|timeout| timeout.parse::<u64>().ok())
        .unwrap_or(180000)
        .clamp(1000, 3600000);
    let limit = timeout.to_string();
    let command_timeout = timeout.div_ceil(1000) + 10;
    let mut result = match prompt {
        Some(text) => engine.herdr.run_timeout(
            &["agent", "prompt", pane, text, "--wait", "--timeout", &limit],
            command_timeout,
        ),
        None => engine.herdr.run(&["agent", "get", pane]),
    };
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
        match string_field(agent, "agent_status") {
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
