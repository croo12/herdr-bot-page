use crate::{
    domain::{id, need, new_chat, strings},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

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
            repos.chats.save_changed(&row, events)?;
        }
        Ok(())
    });
}

pub(crate) fn create_room(store: &Store, params: &Value) -> Result<Value> {
    store.tx(|repos, events| {
        if let Some(existing) = repos.chats.for_request(params)? {
            return Ok(json!({"chat":existing}));
        }
        checked_members(repos, params)?;
        need(params, "name")?;
        let room = new_chat(&format!("room-{}", id()), "room", params);
        repos.chats.save(&room)?;
        repos.chats.remember(params, &room)?;
        events.push(("chat.listChanged", json!(repos.chats.list()?)));
        Ok(json!({"chat":room}))
    })
}
pub(crate) fn room_with_members(repos: &Repositories, params: &Value) -> Result<Value> {
    let members = checked_members(repos, params)?;
    let mut room = repos.chats.get(need(params, "roomId")?)?;
    if room["kind"] != "room" {
        bail!("unknown_room: expected a room")
    }
    room["memberIds"] = json!(members);
    Ok(room)
}
fn checked_members(repos: &Repositories, params: &Value) -> Result<Vec<String>> {
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
    Ok(members)
}
