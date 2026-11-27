use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

pub(crate) struct Prompts<'db> {
    db: &'db Connection,
}
impl<'db> Prompts<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }

    pub(crate) fn pending(&self, bot: &str) -> Result<Option<(String, Value)>> {
        Ok(self
            .db
            .query_row(
                "SELECT chat,data FROM entries WHERE json_extract(data,'$.kind')='prompt'
             AND json_extract(data,'$.author.id')=? AND json_extract(data,'$.status')='pending'
             ORDER BY rowid DESC LIMIT 1",
                [bot],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }
    pub(crate) fn pending_for_delegation(&self, tracker: &str) -> Result<Option<(String, String)>> {
        Ok(self.db.query_row(
            "SELECT chat,json_extract(data,'$.id') FROM entries WHERE json_extract(data,'$.kind')='prompt'
             AND json_extract(data,'$.status')='pending' AND json_extract(data,'$.delegationId')=? ORDER BY rowid LIMIT 1",
            [tracker], |row| Ok((row.get(0)?,row.get(1)?))
        ).optional()?)
    }
    pub(crate) fn matching(&self, bot: &str, signature: &str) -> Result<Option<(String, Value)>> {
        Ok(self.db.query_row(
            "SELECT chat,data FROM entries WHERE json_extract(data,'$.author.id')=?
             AND json_extract(data,'$.status')='pending' AND json_extract(data,'$.prompt.signature')=?
             ORDER BY rowid DESC LIMIT 1",
            [bot, signature], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?)
    }
}
