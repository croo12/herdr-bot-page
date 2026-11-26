use crate::modules::chats::new_chat;
use crate::{
    domain::{id, need, strings, Events},
    modules::chats::change,
    storage::Repositories,
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};

pub(crate) fn create_room(engine: &Engine, p: &V) -> Result<V> {
    engine.tx(|c, ev| update_room(c, ev, true, p))
}
pub(crate) fn set_members(engine: &Engine, p: &V) -> Result<V> {
    let _guard = engine.roster.lock().unwrap();
    engine.tx(|c, ev| update_room(c, ev, false, p))
}
fn update_room(c: &Repositories, ev: &mut Events, create: bool, p: &V) -> Result<V> {
    if let Some(existing) = c.chats.for_request(p)? {
        return Ok(json!({"chat":existing}));
    }
    let raw = p["memberIds"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("invalid_args: memberIds must be an array"))?;
    let members = strings(&p["memberIds"]);
    if members.len() != raw.len() {
        bail!("invalid_args: invalid member ID")
    }
    if members.len() > 6 {
        bail!("too_many_members: maximum six bots")
    }
    let mut seen = std::collections::HashSet::new();
    for member in &members {
        if c.chats.get(member)?["kind"] != "bot" || !seen.insert(member) {
            bail!("invalid_args: invalid room member {member}")
        }
    }
    if create {
        need(p, "name")?;
        let room = new_chat(&format!("room-{}", id()), "room", p);
        c.chats.save(&room)?;
        c.chats.remember(p, &room)?;
        ev.push(("chat.listChanged", json!(c.chats.list()?)));
        Ok(json!({"chat":room}))
    } else {
        let mut room = c.chats.get(need(p, "roomId")?)?;
        if room["kind"] != "room" {
            bail!("unknown_room: expected a room")
        }
        room["memberIds"] = json!(members);
        crate::modules::reviews::revoke_removed_members(c, ev, &room)?;
        change(c, &room, ev)?;
        Ok(room)
    }
}
