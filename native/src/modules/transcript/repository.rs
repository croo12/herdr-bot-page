use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value as V;

pub(crate) struct Transcript<'db> {
    db: &'db Connection,
}
impl<'db> Transcript<'db> {
    pub(crate) fn new(db: &'db Connection) -> Self {
        Self { db }
    }
    pub(crate) fn page(&self, chat: &str, before: i64, limit: i64) -> Result<Vec<V>> {
        let mut q = self
            .db
            .prepare("SELECT data FROM entries WHERE chat=? AND seq<? ORDER BY seq DESC LIMIT ?")?;
        let mut entries = q
            .query_map(params![chat, before, limit], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        entries.reverse();
        Ok(entries)
    }
    pub(crate) fn latest_seq(&self, chat: &str) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT coalesce(max(seq),0) FROM entries WHERE chat=?",
            [chat],
            |r| r.get(0),
        )?)
    }
    pub(crate) fn unread_after(&self, chat: &str, through: i64) -> Result<i64> {
        Ok(self.db.query_row(
            "SELECT count(*) FROM entries
             WHERE chat=? AND seq>? AND json_extract(data,'$.kind')!='message'",
            params![chat, through],
            |row| row.get(0),
        )?)
    }
    pub(crate) fn insert(&self, chat: &str, seq: i64, e: &V) -> Result<()> {
        self.db
            .execute("INSERT INTO entries VALUES(?,?,?)", params![chat, seq, e])?;
        Ok(())
    }
    pub(crate) fn update(&self, chat: &str, e: &V) -> Result<()> {
        self.db.execute(
            "UPDATE entries SET data=? WHERE chat=? AND seq=?",
            params![e, chat, e["seq"]],
        )?;
        Ok(())
    }
    pub(crate) fn by_id(&self, chat: &str, id: &str) -> Result<V> {
        Ok(self.db.query_row(
            "SELECT data FROM entries WHERE chat=? AND json_extract(data,'$.id')=?",
            [chat, id],
            |r| r.get(0),
        )?)
    }
    pub(crate) fn contains(&self, chat: &str, id: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE chat=? AND json_extract(data,'$.id')=?)",
            [chat, id],
            |r| r.get(0),
        )?)
    }
    pub(crate) fn by_nonce(&self, chat: &str, nonce: &str) -> Result<Option<V>> {
        Ok(self
            .db
            .query_row(
                "SELECT data FROM entries WHERE chat=? AND json_extract(data,'$.clientNonce')=?",
                [chat, nonce],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub(crate) fn thread(&self, chat_id: &str, root: &str) -> Result<Vec<V>> {
        let mut query = self.db.prepare(
            "SELECT data FROM entries WHERE chat=?
             AND (json_extract(data,'$.id')=? OR json_extract(data,'$.replyTo')=?) ORDER BY seq",
        )?;
        let rows = query
            .query_map(params![chat_id, root, root], |r| r.get::<_, V>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    // Stream newest first so the caller can stop after the requested number of matches.
    pub(crate) fn visit_messages(&self, mut visit: impl FnMut(V, V) -> bool) -> Result<()> {
        let mut statement = self.db.prepare(
            "SELECT c.data,e.data FROM entries e JOIN chats c ON c.id=e.chat
             WHERE json_extract(e.data,'$.kind') IN ('message','send-message')
             ORDER BY json_extract(e.data,'$.timestampMs') DESC",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            if !visit(row.get(0)?, row.get(1)?) {
                break;
            }
        }
        Ok(())
    }
}
