use super::execution::save;
use crate::{
    domain::{id, need, string_field, strings, Turn},
    infrastructure::{herdr::Herdr, AgentSessions},
    modules::turns::run_or_resume_prompt,
    storage::Store,
};
use anyhow::{bail, Result};
use serde_json::Value;

pub(crate) fn runnable(store: &Store) -> Result<Vec<(String, String)>> {
    store.tx(|repos, _| repos.tasks.runnable())
}

struct Step {
    bot: String,
    pane: String,
    attempt: String,
    prompt: Option<String>,
}

fn instructions(herdr: &Herdr, room: &str, entry: &Value, attempt: &str) -> Result<String> {
    let task = &entry["task"];
    let bot = need(task, "assigneeId")?;
    let entry_id = need(entry, "id")?;
    let cli = herdr.cli.to_string_lossy().replace('\\', "/");
    Ok(format!("You are {bot} in herdr-bot chat {room}. Work on one concrete step toward the goal.
Goal: {}
Completion criteria: {}
Previous checkpoint: {}
Inspect existing work before acting. Read room context with the CLI if needed: read {room}.
Verify the completion criteria. At the end use the native CLI at {cli} from this pane
(in PowerShell invoke its quoted path with &). Submit exactly one report:
task-report {room} {entry_id} {attempt} <outcome> \"summary\"
Use completed with evidence of verification, continue with progress and the next step, or blocked with the input needed.
Your report is posted as a reply to the original message.",
        need(entry,"content")?, need(task,"criteria")?, string_field(&task["execution"],"checkpoint")))
}

// The room queue serializes execution with normal conversation. The roster lock reserves
// this bot across rooms and permission reviews. Never hold a database transaction during I/O.
fn prepare(
    store: &Store,
    sessions: &AgentSessions,
    room: &str,
    entry_id: &str,
) -> Result<Option<Step>> {
    let _roster = sessions.roster.lock().unwrap();
    let entry = store.tx(|repos, _| repos.transcript.by_id(room, entry_id))?;
    let task = &entry["task"];
    let execution = &task["execution"];
    let resume = match execution["state"].as_str() {
        Some("running") => true,
        Some("queued") => false,
        _ => return Ok(None),
    };
    let bot = need(task, "assigneeId")?;
    if sessions.active.lock().unwrap().contains_key(bot) {
        return Ok(None);
    }
    let (member, pending) = store.tx(|repos, _| {
        Ok((
            strings(&repos.chats.get(room)?["memberIds"])
                .iter()
                .any(|member| member == bot),
            repos.reviews.pending_for(bot)?,
        ))
    })?;
    if pending {
        return Ok(None);
    }
    let pane_result = (|| -> Result<Option<String>> {
        if !member {
            bail!("The assigned bot is no longer in this room.");
        }
        let live = if resume {
            let pane = need(execution, "paneId")?;
            let profile = store.tx(|repos, _| repos.chats.get(bot))?;
            if profile["bot"]["paneId"] != pane {
                bail!("The assigned pane changed.");
            }
            let live = sessions.herdr.run(&["agent", "get", pane])?;
            if live["agent"]["name"] != bot || live["agent"]["pane_id"] != pane {
                bail!("The assigned pane changed.");
            }
            live["agent"].clone()
        } else {
            crate::modules::bots::connect_for_turn(store, &sessions.herdr, bot)?
        };
        let pane = need(&live, "pane_id")?;
        match string_field(&live, "agent_status") {
            "idle" | "done" => {}
            "working" | "blocked" if resume => {}
            "working" | "blocked" => return Ok(None),
            status => bail!("The assigned pane is unavailable ({status})."),
        }
        Ok(Some(pane.into()))
    })();
    let step = store.tx(|repos, events| {
        let mut current = repos.transcript.by_id(room, entry_id)?;
        if current["task"]["execution"]["version"] != execution["version"] {
            return Ok(None);
        }
        let pane = match pane_result {
            Ok(Some(pane)) => pane,
            Ok(None) => return Ok(None),
            Err(error) => {
                current["task"]["execution"]["state"] = "blocked".into();
                current["task"]["execution"]["detail"] = error.to_string().into();
                save(repos, events, room, &mut current)?;
                return Ok(None);
            }
        };
        let (attempt, prompt) = if resume {
            (need(execution, "attemptId")?.to_owned(), None)
        } else {
            let attempt = id();
            let prompt = instructions(&sessions.herdr, room, &current, &attempt)?;
            current["task"]["execution"]["state"] = "running".into();
            current["task"]["execution"]["paneId"] = pane.clone().into();
            current["task"]["execution"]["attemptId"] = attempt.clone().into();
            save(repos, events, room, &mut current)?;
            (attempt, Some(prompt))
        };
        Ok(Some(Step {
            bot: bot.into(),
            pane,
            attempt,
            prompt,
        }))
    })?;
    if step.is_some() {
        let review_context = format!(
            "{}\nCompletion criteria: {}",
            need(&entry, "content")?,
            need(task, "criteria")?
        );
        sessions
            .active
            .lock()
            .unwrap()
            .insert(bot.into(), Turn::new(room, &review_context, 0));
    }
    Ok(step)
}

pub(crate) fn execute(
    store: &Store,
    sessions: &AgentSessions,
    room: &str,
    entry_id: &str,
) -> Result<()> {
    let Some(step) = prepare(store, sessions, room, entry_id)? else {
        return Ok(());
    };
    let result = run_or_resume_prompt(
        &sessions.herdr,
        &step.bot,
        &step.pane,
        step.prompt.as_deref(),
    );
    sessions.active.lock().unwrap().remove(&step.bot);
    store.tx(|repos, events| {
        let mut entry = repos.transcript.by_id(room, entry_id)?;
        let execution = &mut entry["task"]["execution"];
        if execution["state"] == "running" && execution["attemptId"] == step.attempt {
            execution["state"] = "blocked".into();
            execution["detail"] = result
                .err()
                .map(|error| format!("{error:#}"))
                .unwrap_or_else(|| "The bot ended its step without reporting the outcome.".into())
                .into();
            save(repos, events, room, &mut entry)?;
        }
        Ok(())
    })
}
