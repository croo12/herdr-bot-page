use crate::{
    domain::{need, strings},
    Engine,
};
use anyhow::{bail, Result};
use serde_json::{json, Value as V};

pub(crate) fn delete_chats(engine: &Engine, p: &V) -> Result<V> {
    let _guard = engine.roster.lock().unwrap();
    let ids = strings(&p["chatIds"]);
    let mut deleted = vec![];
    for id in ids {
        let v = engine.tx(|c, _| c.chats.get(&id))?;
        if engine
            .active
            .lock()
            .unwrap()
            .iter()
            .any(|(bot, t)| bot == &id || t.chat == id)
        {
            bail!("bot_not_ready: {id} has an active turn")
        }
        if v["kind"] == "bot" {
            let agents = engine.herdr.agents()?;
            if agents
                .iter()
                .any(|a| a["name"] == id && a["pane_id"] == v["bot"]["paneId"])
            {
                let pane = need(&v["bot"], "paneId")?;
                if v["bot"]["adopted"] == true {
                    engine.herdr.run(&["agent", "rename", pane, "--clear"])?;
                } else {
                    engine.herdr.run(&["pane", "close", pane])?;
                }
            }
        }
        engine.tx(|c, ev| {
            for mut room in c.chats.list()? {
                let mut members = strings(&room["memberIds"]);
                if members.contains(&id) {
                    members.retain(|x| x != &id);
                    room["memberIds"] = json!(members);
                    crate::modules::reviews::revoke_removed_members(c, ev, &room)?;
                    c.chats.save(&room)?;
                }
            }
            c.chats.delete(&id)?;
            ev.push(("chat.listChanged", json!(c.chats.list()?)));
            Ok(())
        })?;
        deleted.push(id);
    }
    Ok(json!({"deletedIds":deleted}))
}
