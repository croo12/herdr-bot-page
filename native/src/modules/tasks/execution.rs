use crate::{
    domain::{id, need, now, strings, Events},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

/// Every transition replaces the version so a delayed click cannot restart a newer attempt.
pub(crate) fn save_execution(
    repos: &Repositories,
    events: &mut Events,
    room: &str,
    entry: &mut Value,
) -> Result<()> {
    entry["task"]["updatedAt"] = now().into();
    entry["task"]["execution"]["version"] = id().into();
    repos.update_entry(room, entry, events)
}

pub(crate) fn control(store: &Store, params: &Value, pause: bool) -> Result<Value> {
    let room = need(params, "chatId")?;
    let entry_id = need(params, "entryId")?;
    store.tx(|repos, events| {
        let mut entry = repos.transcript.by_id(room, entry_id)?;
        let task = &mut entry["task"];
        if !task.is_object() || task["status"] == "completed" {
            bail!("invalid_args: choose an unfinished task");
        }
        if !pause && (task["assigneeId"].is_null() || task["assigneeId"] != params["assigneeId"]) {
            bail!("invalid_args: select the current assigned bot");
        }
        let execution = &mut task["execution"];
        if execution["version"] != params["version"] {
            bail!("invalid_args: execution has changed");
        }
        if pause {
            match execution["state"].as_str() {
                Some("queued") => execution["state"] = "paused".into(),
                Some("running") => execution["pauseAfterStep"] = true.into(),
                _ => bail!("invalid_args: task is not executing"),
            }
        } else {
            if !matches!(
                execution["state"].as_str(),
                None | Some("paused" | "blocked")
            ) {
                bail!("invalid_args: task is already executing");
            }
            execution["state"] = "queued".into();
            execution["detail"] = Value::Null;
            execution["pauseAfterStep"] = false.into();
            if !strings(&repos.chats.get(room)?["memberIds"])
                .contains(&need(task, "assigneeId")?.to_owned())
            {
                bail!("invalid_args: the assigned bot is no longer in the room");
            }
            task["status"] = "in_progress".into();
        }
        save_execution(repos, events, room, &mut entry)?;
        Ok(entry)
    })
}

pub(crate) fn report(store: &Store, params: &Value) -> Result<Value> {
    let room = need(params, "chatId")?;
    let entry_id = need(params, "entryId")?;
    let attempt = need(params, "attemptId")?;
    let pane = need(params, "paneId")?;
    let outcome = need(params, "outcome")?;
    let summary = need(params, "summary")?.trim();
    store.tx(|repos, events| {
        let mut entry = repos.transcript.by_id(room, entry_id)?;
        let task = &mut entry["task"];
        let assignee = need(task, "assigneeId")?.to_owned();
        let bot = repos.chats.get(&assignee)?;
        let execution = &mut task["execution"];
        if execution["paneId"] != pane
            || bot["bot"]["paneId"] != pane
            || !strings(&repos.chats.get(room)?["memberIds"]).contains(&assignee)
        {
            bail!("invalid_args: only the assigned room bot can report this step");
        }
        if execution["lastReportAttempt"] == attempt
            && execution["outcome"] == outcome
            && execution["checkpoint"] == summary
        {
            return Ok(entry);
        }
        if execution["state"] != "running" || execution["attemptId"] != attempt {
            bail!("invalid_args: no matching active task step");
        }
        let state = match outcome {
            "completed" => "completed",
            "blocked" => "blocked",
            "continue" if execution["checkpoint"] == summary => {
                execution["detail"] = "The bot repeated its previous checkpoint.".into();
                "blocked"
            }
            "continue" if execution["pauseAfterStep"] == true => "paused",
            "continue" => "queued",
            _ => bail!("invalid_args: unknown report outcome"),
        };
        execution["state"] = state.into();
        execution["checkpoint"] = summary.into();
        execution["lastReportAttempt"] = attempt.into();
        execution["outcome"] = outcome.into();
        execution["pauseAfterStep"] = false.into();
        if state == "completed" {
            task["status"] = "completed".into();
        }
        repos.append_entry(
            room,
            json!({
                "kind":"send-message", "message":{"type":"text","content":summary},
                "author":{"id":assignee,"name":bot["name"]}, "replyTo":entry_id
            }),
            events,
        )?;
        save_execution(repos, events, room, &mut entry)?;
        Ok(entry)
    })
}
