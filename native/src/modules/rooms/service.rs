use crate::modules::chats::new_chat;
use crate::{
    domain::{id, need, strings, Events},
    modules::chats::change,
    storage::Repositories,
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub(crate) fn create_room(engine: &Engine, params: &Value) -> Result<Value> {
    engine.tx(|repos, events| update_room(repos, events, true, params))
}
pub(crate) fn set_members(engine: &Engine, params: &Value) -> Result<Value> {
    let _guard = engine.roster.lock().unwrap();
    engine.tx(|repos, events| update_room(repos, events, false, params))
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
        change(repos, &room, events)?;
        Ok(room)
    }
}
