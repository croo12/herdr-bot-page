use crate::domain::need;
use anyhow::anyhow;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value as V;
pub(crate) struct Chats<'db> {
    db: &'db Connection,
}
impl<'db> Chats<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }
    pub(crate) fn list(&self) -> Result<Vec<V>> {
        let mut q = self.db.prepare("SELECT data FROM chats ORDER BY rowid")?;
        let rows = q
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
    pub(crate) fn get(&self, id: &str) -> Result<V> {
        self.db
            .query_row("SELECT data FROM chats WHERE id=?", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| anyhow!("unknown_chat: {id}"))
    }
    pub(crate) fn save(&self, v: &V) -> Result<()> {
        self.db.execute(
            "INSERT INTO chats VALUES(?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![need(v, "id")?, v],
        )?;
        Ok(())
    }
    pub(crate) fn for_request(&self, p: &V) -> Result<Option<V>> {
        if let Some(r) = p["requestId"].as_str() {
            return Ok(self
                .db
                .query_row(
                    "SELECT c.data FROM requests r JOIN chats c ON c.id=r.chat WHERE r.key=?",
                    [r],
                    |r| r.get(0),
                )
                .optional()?);
        }
        Ok(None)
    }
    pub(crate) fn remember(&self, p: &V, v: &V) -> Result<()> {
        if let Some(r) = p["requestId"].as_str() {
            self.db.execute(
                "INSERT INTO requests VALUES(?,?)",
                params![r, need(v, "id")?],
            )?;
        }
        Ok(())
    }
    pub(crate) fn delete(&self, id: &str) -> Result<()> {
        self.db.execute("DELETE FROM chats WHERE id=?", [id])?;
        Ok(())
    }
}
