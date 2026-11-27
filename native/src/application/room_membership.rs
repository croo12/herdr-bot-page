use crate::{
    domain::{need, strings, Events},
    infrastructure::AgentSessions,
    modules::{reviews, rooms, tasks},
    storage::{Repositories, Store},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub(crate) fn delete_chats(
    store: &Store,
    sessions: &AgentSessions,
    params: &Value,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    let ids = strings(&params["chatIds"]);
    let mut deleted = vec![];
    for id in ids {
        let chat = store.tx(|repos, _| repos.chats.get(&id))?;
        anyhow::ensure!(
            crate::domain::visible_to(&chat, None),
            "invalid_args: not a participant"
        );
        let related_directs = store.tx(|repos, _| {
            Ok(repos
                .chats
                .list()?
                .into_iter()
                .filter(|chat| {
                    chat["kind"] == "direct" && strings(&chat["memberIds"]).contains(&id)
                })
                .collect::<Vec<_>>())
        })?;
        if sessions.active.lock().unwrap().iter().any(|(bot, turn)| {
            bot == &id || turn.chat == id || related_directs.iter().any(|dm| dm["id"] == turn.chat)
        }) {
            bail!("bot_not_ready: {id} has an active turn")
        }
        if chat["kind"] == "bot" {
            let agents = sessions.herdr.agents()?;
            if agents
                .iter()
                .any(|agent| agent["name"] == id && agent["pane_id"] == chat["bot"]["paneId"])
            {
                let pane = need(&chat["bot"], "paneId")?;
                if chat["bot"]["adopted"] == true {
                    sessions.herdr.run(&["agent", "rename", pane, "--clear"])?;
                } else {
                    sessions.herdr.run(&["pane", "close", pane])?;
                }
            }
        }
        store.tx(|repos, events| {
            for mut room in repos.chats.list()? {
                let mut members = strings(&room["memberIds"]);
                if members.contains(&id) {
                    if room["kind"] == "direct" {
                        // Keep the history when a participant is deleted. A new bot reusing
                        // the name must never inherit that participant's old conversation.
                        repos.chats.archive_direct(&room, events)?;
                        continue;
                    }
                    members.retain(|member| member != &id);
                    room["memberIds"] = json!(members);
                    release_removed_members(repos, events, &room)?;
                    repos.chats.save(&room)?;
                }
            }
            repos.chats.delete(&id)?;
            events.push(("chat.listChanged", json!(repos.chats.visible_list()?)));
            Ok(())
        })?;
        deleted.push(id);
    }
    Ok(json!({"deletedIds":deleted}))
}

pub(crate) fn set_members(
    store: &Store,
    sessions: &AgentSessions,
    params: &Value,
) -> Result<Value> {
    let _guard = sessions.roster.lock().unwrap();
    store.tx(|repos, events| {
        if let Some(existing) = repos.chats.for_request(params)? {
            return Ok(json!({"chat":existing}));
        }
        let room = rooms::room_with_members(repos, params)?;
        release_removed_members(repos, events, &room)?;
        repos.chats.save_changed(&room, events)?;
        Ok(room)
    })
}

fn release_removed_members(repos: &Repositories, events: &mut Events, room: &Value) -> Result<()> {
    repos.inbox.keep_members(room)?;
    reviews::revoke_removed_members(repos, events, room)?;
    tasks::release_removed_members(repos, events, room)
}
