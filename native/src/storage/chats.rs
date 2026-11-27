use crate::domain::{need, new_chat, Events};
use anyhow::anyhow;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
pub(crate) struct Chats<'db> {
    db: &'db Connection,
}
impl<'db> Chats<'db> {
    fn direct_pair(bot: &str, peer: Option<&str>) -> String {
        match peer {
            None => format!("dm:user:{bot}"),
            Some(other) if bot < other => format!("dm:bots:{bot}:{other}"),
            Some(other) => format!("dm:bots:{other}:{bot}"),
        }
    }
    pub(crate) fn direct(&self, bot: &str, peer: Option<&str>) -> Result<Option<Value>> {
        Ok(self
            .db
            .query_row(
                "SELECT data FROM chats WHERE json_extract(data,'$.directPair')=?",
                [Self::direct_pair(bot, peer)],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub(crate) fn archive_direct(&self, chat: &Value, events: &mut Events) -> Result<()> {
        let mut archived = chat.clone();
        archived["directPair"] = Value::Null;
        archived["archived"] = true.into();
        self.db
            .execute("DELETE FROM inbox WHERE chat=?", [need(chat, "id")?])?;
        self.save_changed(&archived, events)
    }

    pub(crate) fn open_direct(
        &self,
        bot: &str,
        peer: Option<&str>,
        events: &mut Events,
    ) -> Result<Value> {
        let profile = self.get(bot)?;
        anyhow::ensure!(
            profile["kind"] == "bot" && Some(bot) != peer,
            "invalid_args: expected a distinct bot"
        );
        let mut members = vec![bot];
        if let Some(other) = peer {
            anyhow::ensure!(
                self.get(other)?["kind"] == "bot",
                "invalid_args: expected bot participant"
            );
            members.push(other);
        }
        members.sort_unstable();
        if let Some(row) = self.direct(bot, peer)? {
            return Ok(row);
        }
        let id = format!("dm:{}", crate::domain::id());
        let mut row = new_chat(
            &id,
            "direct",
            &serde_json::json!({"name":profile["name"],"memberIds":members,"avatarColor":profile["avatarColor"]}),
        );
        row["directPair"] = Self::direct_pair(bot, peer).into();
        row["includesUser"] = peer.is_none().into();
        self.save(&row)?;
        if peer.is_none() {
            events.push(("chat.listChanged", serde_json::json!(self.visible_list()?)));
        }
        Ok(row)
    }
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }
    pub(crate) fn list(&self) -> Result<Vec<Value>> {
        let mut statement = self
            .db
            .prepare("SELECT data FROM bots UNION ALL SELECT data FROM chats")?;
        let rows = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
    pub(crate) fn visible_list(&self) -> Result<Vec<Value>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|chat| crate::domain::visible_to(chat, None))
            .collect())
    }
    pub(crate) fn get(&self, id: &str) -> Result<Value> {
        self.find(id)?.ok_or_else(|| anyhow!("unknown_chat: {id}"))
    }
    pub(crate) fn find(&self, id: &str) -> Result<Option<Value>> {
        Ok(self
            .db
            .query_row(
                "SELECT data FROM bots WHERE id=?1 UNION ALL SELECT data FROM chats WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }
    pub(crate) fn save(&self, chat: &Value) -> Result<()> {
        self.db.execute(
            if chat["kind"] == "bot" {
                "INSERT INTO bots VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data"
            } else {
                "INSERT INTO chats VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data"
            },
            params![need(chat, "id")?, chat],
        )?;
        Ok(())
    }
    pub(crate) fn save_changed(
        &self,
        chat: &Value,
        events: &mut crate::domain::Events,
    ) -> Result<()> {
        self.save(chat)?;
        if crate::domain::visible_to(chat, None) {
            events.push(("chat.changed", chat.clone()));
        }
        Ok(())
    }

    pub(crate) fn by_pane(&self, pane: &str) -> Result<Value> {
        self.pane_owner(pane)?
            .ok_or_else(|| anyhow!("unknown_bot: pane {pane}"))
    }
    pub(crate) fn pane_owner(&self, pane: &str) -> Result<Option<Value>> {
        Ok(self.db.query_row("SELECT data FROM bots WHERE json_extract(data, '$.bot.paneId') = ? ORDER BY rowid LIMIT 1", [pane], |row| row.get(0)).optional()?)
    }
    pub(crate) fn for_request(&self, params: &Value) -> Result<Option<Value>> {
        if let Some(request_id) = params["requestId"].as_str() {
            return Ok(self
                .db
                .query_row(
                    "SELECT c.data FROM requests r JOIN chats c ON c.id=r.chat WHERE r.key=?1 UNION ALL SELECT b.data FROM bot_requests r JOIN bots b ON b.id=r.bot WHERE r.key=?1",
                    [request_id],
                    |row| row.get(0),
                )
                .optional()?);
        }
        Ok(None)
    }
    pub(crate) fn remember(&self, params: &Value, chat: &Value) -> Result<()> {
        if let Some(request_id) = params["requestId"].as_str() {
            self.db.execute(
                if chat["kind"] == "bot" {
                    "INSERT INTO bot_requests VALUES(?,?)"
                } else {
                    "INSERT INTO requests VALUES(?,?)"
                },
                params![request_id, need(chat, "id")?],
            )?;
        }
        Ok(())
    }
    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        self.db.execute("DELETE FROM bots WHERE id=?", [id])?;
        self.db.execute("DELETE FROM chats WHERE id=?", [id])?;
        Ok(())
    }
}
