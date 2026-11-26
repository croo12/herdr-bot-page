use crate::domain::need;
use anyhow::anyhow;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
pub(crate) struct Chats<'db> {
    db: &'db Connection,
}
impl<'db> Chats<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }
    pub(crate) fn list(&self) -> Result<Vec<Value>> {
        let mut statement = self.db.prepare("SELECT data FROM chats ORDER BY rowid")?;
        let rows = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
    pub(crate) fn get(&self, id: &str) -> Result<Value> {
        self.db
            .query_row("SELECT data FROM chats WHERE id=?", [id], |row| row.get(0))
            .optional()?
            .ok_or_else(|| anyhow!("unknown_chat: {id}"))
    }
    pub(crate) fn save(&self, chat: &Value) -> Result<()> {
        self.db.execute(
            "INSERT INTO chats VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
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
        events.push(("chat.changed", chat.clone()));
        Ok(())
    }

    pub(crate) fn by_pane(&self, pane: &str) -> Result<Value> {
        self.db.query_row("SELECT data FROM chats WHERE json_extract(data, '$.bot.paneId') = ? ORDER BY rowid LIMIT 1", [pane], |row| row.get(0))
            .optional()?.ok_or_else(|| anyhow!("unknown_bot: pane {pane}"))
    }
    pub(crate) fn for_request(&self, params: &Value) -> Result<Option<Value>> {
        if let Some(request_id) = params["requestId"].as_str() {
            return Ok(self
                .db
                .query_row(
                    "SELECT c.data FROM requests r JOIN chats c ON c.id=r.chat WHERE r.key=?",
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
                "INSERT INTO requests VALUES(?,?)",
                params![request_id, need(chat, "id")?],
            )?;
        }
        Ok(())
    }
    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        self.db.execute("DELETE FROM chats WHERE id=?", [id])?;
        Ok(())
    }
}
