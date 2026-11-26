use crate::{
    domain::{need, strings},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};

pub(crate) fn delete_chats(engine: &Engine, params: &Value) -> Result<Value> {
    let _guard = engine.roster.lock().unwrap();
    let ids = strings(&params["chatIds"]);
    let mut deleted = vec![];
    for id in ids {
        let chat = engine.tx(|repos, _| repos.chats.get(&id))?;
        if engine
            .active
            .lock()
            .unwrap()
            .iter()
            .any(|(bot, turn)| bot == &id || turn.chat == id)
        {
            bail!("bot_not_ready: {id} has an active turn")
        }
        if chat["kind"] == "bot" {
            let agents = engine.herdr.agents()?;
            if agents
                .iter()
                .any(|agent| agent["name"] == id && agent["pane_id"] == chat["bot"]["paneId"])
            {
                let pane = need(&chat["bot"], "paneId")?;
                if chat["bot"]["adopted"] == true {
                    engine.herdr.run(&["agent", "rename", pane, "--clear"])?;
                } else {
                    engine.herdr.run(&["pane", "close", pane])?;
                }
            }
        }
        engine.tx(|repos, events| {
            for mut room in repos.chats.list()? {
                let mut members = strings(&room["memberIds"]);
                if members.contains(&id) {
                    members.retain(|member| member != &id);
                    room["memberIds"] = json!(members);
                    crate::modules::reviews::revoke_removed_members(repos, events, &room)?;
                    crate::modules::tasks::release_removed_members(repos, events, &room)?;
                    repos.chats.save(&room)?;
                }
            }
            repos.chats.delete(&id)?;
            events.push(("chat.listChanged", json!(repos.chats.list()?)));
            Ok(())
        })?;
        deleted.push(id);
    }
    Ok(json!({"deletedIds":deleted}))
}
