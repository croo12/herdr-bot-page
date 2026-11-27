use crate::{
    domain::{need, now},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

fn room(repos: &Repositories, chat_id: &str) -> Result<Value> {
    let chat = repos.chats.get(chat_id)?;
    if chat["kind"] != "room" {
        bail!("invalid_args: tasks belong to a room");
    }
    Ok(chat)
}

pub(crate) fn list(store: &Store, params: &Value) -> Result<Value> {
    let chat_id = need(params, "chatId")?;
    store.tx(|repos, _| {
        room(repos, chat_id)?;
        Ok(json!(repos.tasks.list(chat_id)?))
    })
}

fn validate_assignee(room: &Value, assignee: &Value) -> Result<()> {
    if assignee.is_null()
        || (assignee.is_string()
            && room["memberIds"]
                .as_array()
                .is_some_and(|members| members.contains(assignee)))
    {
        return Ok(());
    }
    bail!("invalid_args: choose a bot in this room")
}

fn set_assignee(
    repos: &Repositories,
    events: &mut crate::domain::Events,
    room_id: &str,
    entry: &mut Value,
    assignee: &Value,
) -> Result<()> {
    let task = &mut entry["task"];
    task["assigneeId"] = assignee.clone();
    if task["execution"].is_object() {
        let checkpoint = task["execution"]["checkpoint"].clone();
        task["execution"] = json!({"state":"paused"});
        if !checkpoint.is_null() {
            task["execution"]["checkpoint"] = checkpoint;
        }
        super::execution::save_execution(repos, events, room_id, entry)
    } else {
        task["updatedAt"] = now().into();
        repos.update_entry(room_id, entry, events)
    }
}

pub(crate) fn release_removed_members(
    repos: &Repositories,
    events: &mut crate::domain::Events,
    room: &Value,
) -> Result<()> {
    let room_id = need(room, "id")?;
    for mut entry in repos.tasks.list(room_id)? {
        let task = &entry["task"];
        if task["status"] != "completed" && validate_assignee(room, &task["assigneeId"]).is_err() {
            set_assignee(repos, events, room_id, &mut entry, &Value::Null)?;
        }
    }
    Ok(())
}

pub(crate) fn assign(store: &Store, params: &Value) -> Result<Value> {
    let room_id = need(params, "chatId")?;
    let entry_id = need(params, "entryId")?;
    let assignee = params
        .get("assigneeId")
        .ok_or_else(|| anyhow::anyhow!("invalid_args: missing assigneeId"))?;
    store.tx(|repos, events| {
        let mut entry = repos.transcript.by_id(room_id, entry_id)?;
        let task = &mut entry["task"];
        if !task.is_object() {
            bail!("invalid_args: message has no task");
        }
        if &task["assigneeId"] == assignee {
            return Ok(entry);
        }
        let room = room(repos, room_id)?;
        validate_assignee(&room, assignee)?;
        if task["assigneeId"] != params["previousAssigneeId"]
            || task["execution"]["version"] != params["version"]
        {
            bail!("invalid_args: task has changed");
        }
        if task["execution"]["state"] == "running" || task["status"] == "completed" {
            bail!("invalid_args: pause the unfinished task before changing its assignee");
        }
        set_assignee(repos, events, room_id, &mut entry, assignee)?;
        Ok(entry)
    })
}

pub(crate) fn track(store: &Store, params: &Value) -> Result<Value> {
    let chat_id = need(params, "chatId")?;
    let entry_id = need(params, "entryId")?;
    let assignee = &params["assigneeId"];
    let criteria = need(params, "criteria")?.trim();
    store.tx(|repos, events| {
        let room = room(repos, chat_id)?;
        let mut entry = repos.transcript.by_id(chat_id, entry_id)?;
        if entry["kind"] != "message" || entry["content"].as_str().unwrap_or("").trim().is_empty() {
            bail!("invalid_args: choose a user message with a goal");
        }
        if let Some(existing) = entry.get("task") {
            if &existing["assigneeId"] == assignee && existing["criteria"] == criteria {
                return Ok(entry);
            }
            bail!("invalid_args: this message already tracks a task");
        }
        validate_assignee(&room, assignee)?;
        entry["task"] =
            json!({"assigneeId":assignee, "criteria":criteria, "status":"open", "updatedAt":now()});
        repos.update_entry(chat_id, &entry, events)?;
        Ok(entry)
    })
}

pub(crate) fn set_status(store: &Store, params: &Value) -> Result<Value> {
    let chat_id = need(params, "chatId")?;
    let entry_id = need(params, "entryId")?;
    let status = need(params, "status")?;
    let previous = need(params, "previousStatus")?;
    if !matches!(status, "open" | "in_progress" | "completed") {
        bail!("invalid_args: unknown task status");
    }
    store.tx(|repos, events| {
        let mut entry = repos.transcript.by_id(chat_id, entry_id)?;
        if !entry["task"].is_object() {
            bail!("invalid_args: message has no task");
        }
        if entry["task"]["execution"].is_object() {
            bail!("invalid_args: the task runner manages this status");
        }
        if entry["task"]["status"] == status {
            return Ok(entry);
        }
        if entry["task"]["status"] != previous {
            bail!("invalid_args: task status has changed");
        }
        entry["task"]["status"] = status.into();
        entry["task"]["updatedAt"] = now().into();
        repos.update_entry(chat_id, &entry, events)?;
        Ok(entry)
    })
}

pub(crate) fn runnable(store: &Store) -> Result<Vec<(String, String, String)>> {
    store.tx(|repos, _| repos.tasks.runnable())
}
