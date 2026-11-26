use crate::{
    domain::{id, need, strings, Events},
    modules::chats::{change, new_chat},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::sync::Mutex;

pub(crate) fn set_activity(store: &Store, target: &str, running: bool) {
    let _ = store.tx(|repos, events| {
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

pub(crate) fn create_room(store: &Store, params: &Value) -> Result<Value> {
    store.tx(|repos, events| update_room(repos, events, true, params))
}
pub(crate) fn set_members(store: &Store, roster: &Mutex<()>, params: &Value) -> Result<Value> {
    let _guard = roster.lock().unwrap();
    store.tx(|repos, events| update_room(repos, events, false, params))
}
fn update_room(
    repos: &Repositories,
    events: &mut Events,
    create: bool,
    params: &Value,
) -> Result<Value> {
    if let Some(existing) = repos.chats.for_request(params)? {
        return Ok(json!({"chat":existing}));
    }
    let raw = params["memberIds"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("invalid_args: memberIds must be an array"))?;
    let members = strings(&params["memberIds"]);
    if members.len() != raw.len() {
        bail!("invalid_args: invalid member ID")
    }
    if members.len() > 6 {
        bail!("too_many_members: maximum six bots")
    }
    let mut seen = std::collections::HashSet::new();
    for member in &members {
        if repos.chats.get(member)?["kind"] != "bot" || !seen.insert(member) {
            bail!("invalid_args: invalid room member {member}")
        }
    }
    if create {
        need(params, "name")?;
        let room = new_chat(&format!("room-{}", id()), "room", params);
        repos.chats.save(&room)?;
        repos.chats.remember(params, &room)?;
        events.push(("chat.listChanged", json!(repos.chats.list()?)));
        Ok(json!({"chat":room}))
    } else {
        let mut room = repos.chats.get(need(params, "roomId")?)?;
        if room["kind"] != "room" {
            bail!("unknown_room: expected a room")
        }
        room["memberIds"] = json!(members);
        crate::modules::reviews::revoke_removed_members(repos, events, &room)?;
        crate::modules::tasks::release_removed_members(repos, events, &room)?;
        change(repos, &room, events)?;
        Ok(room)
    }
}
